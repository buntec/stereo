//! Stereo's desktop app: a window onto Stereo's web UI, served by a `stereo` backend on loopback.
//!
//! On launch the app looks for a Stereo already running on the CLI's default port (e.g. the
//! launchd service from `stereo install`) and opens that one: two backends on the same data
//! directory would race each other's git syncs. Otherwise it starts its bundled backend, on the
//! same port when it's free, and stops it again when the app quits.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::webview::{NewWindowResponse, WebviewWindowBuilder};
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindow};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

/// The CLI's default port. Besides finding a `stereo` started there, the app's own backend uses
/// it when free: the page's origin, and with it everything the UI keeps in localStorage (grid
/// state, settings), then stays the same across launches, and a browser tab can share it.
const PORT: u16 = 8005;

/// `PORT`, or `STEREO_PORT` like the CLI, e.g. to try the bundled backend in development while
/// another Stereo runs on the default port. (Apps started from the Finder don't see it.)
fn preferred_port() -> u16 {
    std::env::var("STEREO_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(PORT)
}
/// Startup recovers interrupted git syncs and checks yt-dlp, which can take a while.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
/// How many of the backend's last stderr lines an error page shows.
const STDERR_LINES: usize = 30;

/// The bundled backend, while it runs.
#[derive(Default)]
struct Backend {
    child: Mutex<Option<CommandChild>>,
    /// Set as the app quits, when the backend's exit is no error.
    quitting: std::sync::atomic::AtomicBool,
}

/// What listens on a loopback port.
#[derive(Debug, PartialEq)]
enum Probe {
    Stereo,
    Free,
    Taken,
}

fn probe(port: u16) -> Probe {
    // `stereo` binds `localhost`, which may be either address family.
    let Ok(addrs) = ("localhost", port).to_socket_addrs() else {
        return Probe::Free;
    };
    let mut taken = false;
    for addr in addrs {
        let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) else {
            continue;
        };
        if is_stereo(&mut stream, port) {
            return Probe::Stereo;
        }
        taken = true;
    }
    if taken {
        Probe::Taken
    } else {
        Probe::Free
    }
}

/// Whether `stream` is connected to Stereo: its `/info`, or, from versions before that route,
/// the web UI it serves for any path.
fn is_stereo(stream: &mut TcpStream, port: u16) -> bool {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let request =
        format!("GET /info HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = Vec::new();
    let _ = stream.take(256 * 1024).read_to_end(&mut response);
    looks_like_stereo(&String::from_utf8_lossy(&response))
}

fn looks_like_stereo(response: &str) -> bool {
    response.starts_with("HTTP/1.1 200")
        && (response.contains(r#""app":"stereo""#) || response.contains("<title>Stereo</title>"))
}

fn unused_loopback_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// The UI's URL on `port`. `localhost`, like the backend's bind address and the URL the CLI
/// prints, so that it's the same origin a browser tab would have.
fn ui_url(port: u16) -> Url {
    format!("http://localhost:{port}/")
        .parse()
        .expect("a valid URL")
}

/// Stereo's own pages, and the app's bundled start page; anything else opens in the browser.
fn is_app_url(url: &Url) -> bool {
    !matches!(url.scheme(), "http" | "https")
        || matches!(
            url.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]" | "tauri.localhost")
        )
}

fn open_in_browser(url: &Url) {
    if matches!(url.scheme(), "http" | "https" | "mailto") {
        if let Err(error) = tauri_plugin_opener::open_url(url.as_str(), None::<&str>) {
            eprintln!("could not open {url}: {error}");
        }
    }
}

/// Shows the start page with an error, for good: the app has to be restarted to try again.
fn show_error(window: &WebviewWindow, start_page: &Url, message: &str, details: &str) {
    let mut url = start_page.clone();
    let error = serde_json::json!({ "message": message, "details": details }).to_string();
    url.set_fragment(Some(&error));
    if let Err(error) = window.navigate(url) {
        eprintln!("could not show the error page: {error}");
    }
}

/// The `PATH` of the user's login shell. Apps started from the Finder or Dock get launchd's
/// minimal one, without e.g. Homebrew, where yt-dlp and the Deno it needs usually live.
#[cfg(target_os = "macos")]
fn login_path() -> Option<String> {
    use std::ffi::CStr;
    use std::process::{Command, Stdio};

    let user = unsafe { libc::getpwuid(libc::getuid()) };
    if user.is_null() || unsafe { (*user).pw_shell.is_null() } {
        return None;
    }
    let shell = unsafe { CStr::from_ptr((*user).pw_shell) }.to_str().ok()?;
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", "exec /usr/bin/printenv PATH"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Don't let a shell startup file that waits for something hold up the app.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut stdout = String::new();
    child.stdout.take()?.read_to_string(&mut stdout).ok()?;
    // Interactive shells may print something of their own first.
    stdout
        .lines()
        .last()
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
}

/// Starts the bundled backend on `port`. Its stderr's last lines are kept in `stderr` and it
/// reports its exit to `exited`.
fn start_backend(
    app: &AppHandle,
    port: u16,
    stderr: Arc<Mutex<VecDeque<String>>>,
    exited: Arc<Mutex<Option<String>>>,
) -> Result<(), String> {
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut command = app
        .shell()
        .sidecar("stereo")
        .map_err(|error| format!("Could not find Stereo's backend: {error}"))?
        .args(["--port", &port.to_string(), "--exit-with-stdin"])
        // Also ends the yt-dlp downloads it started when the app quits.
        .set_process_group(true)
        .kill_on_drop(true);
    #[cfg(target_os = "macos")]
    if let Some(path) = login_path() {
        command = command.env("PATH", path);
    }
    let (mut events, child) = command
        .spawn()
        .map_err(|error| format!("Could not start Stereo's backend: {error}"))?;
    *app.state::<Backend>().child.lock().unwrap() = Some(child);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                CommandEvent::Stdout(line) => print!("{}", String::from_utf8_lossy(&line)),
                CommandEvent::Stderr(line) => {
                    let line = String::from_utf8_lossy(&line);
                    eprint!("{line}");
                    let mut stderr = stderr.lock().unwrap();
                    if stderr.len() == STDERR_LINES {
                        stderr.pop_front();
                    }
                    stderr.push_back(line.trim_end().to_string());
                }
                CommandEvent::Terminated(payload) => {
                    let status = match (payload.code, payload.signal) {
                        (Some(code), _) => format!("exited with status {code}"),
                        (None, Some(signal)) => format!("was killed by signal {signal}"),
                        (None, None) => "exited".to_string(),
                    };
                    eprintln!("stereo backend {status}");
                    *exited.lock().unwrap() = Some(status);
                    break;
                }
                CommandEvent::Error(error) => eprintln!("stereo backend: {error}"),
                _ => {}
            }
        }
        app.state::<Backend>().child.lock().unwrap().take();
    });
    Ok(())
}

/// Opens a running Stereo, or starts the bundled one and opens it once it's up. Runs on its own
/// thread, since probing and waiting block.
fn connect(app: AppHandle, window: WebviewWindow, start_page: Url) {
    let preferred = preferred_port();
    let port = match probe(preferred) {
        Probe::Stereo => {
            let _ = window.navigate(ui_url(preferred));
            return;
        }
        Probe::Free => preferred,
        // Rather than fail: Stereo works on any port, only localStorage starts out empty.
        Probe::Taken => match unused_loopback_port() {
            Ok(port) => port,
            Err(error) => {
                let message = "Could not find a free port for Stereo.";
                return show_error(&window, &start_page, message, &error.to_string());
            }
        },
    };

    let stderr = Arc::new(Mutex::new(VecDeque::new()));
    let exited = Arc::new(Mutex::new(None));
    if let Err(error) = start_backend(&app, port, stderr.clone(), exited.clone()) {
        return show_error(&window, &start_page, &error, "");
    }

    let details = || {
        stderr
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    };
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if let Some(status) = exited.lock().unwrap().clone() {
            let message = format!("Stereo's backend {status} while starting.");
            return show_error(&window, &start_page, &message, &details());
        }
        if probe(port) == Probe::Stereo {
            break;
        }
        if Instant::now() > deadline {
            let message = "Stereo's backend did not start in time.";
            return show_error(&window, &start_page, message, &details());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if let Err(error) = window.navigate(ui_url(port)) {
        let message = "Could not open Stereo.";
        return show_error(&window, &start_page, message, &error.to_string());
    }

    // A backend that goes away later leaves the UI reconnecting forever: say so instead.
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let Some(status) = exited.lock().unwrap().clone() else {
            continue;
        };
        if !app
            .state::<Backend>()
            .quitting
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            let message = format!("Stereo's backend {status}.");
            show_error(&window, &start_page, &message, &details());
        }
        return;
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Backend::default())
        .setup(|app| {
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("Stereo")
                    .inner_size(1400.0, 900.0)
                    .min_inner_size(480.0, 480.0)
                    // Lets the UI adapt where the desktop app differs from a browser tab.
                    .initialization_script("window.__STEREO_DESKTOP__ = true;")
                    // Links to other sites open in the default browser, as do new windows
                    // (`window.open`, `target="_blank"`).
                    .on_navigation(|url| {
                        let allowed = is_app_url(url);
                        if !allowed {
                            open_in_browser(url);
                        }
                        allowed
                    })
                    .on_new_window(|url, _| {
                        open_in_browser(&url);
                        NewWindowResponse::Deny
                    })
                    .build()?;
            let start_page = window.url()?;
            let app = app.handle().clone();
            std::thread::spawn(move || connect(app, window, start_page));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the Stereo desktop app")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                let backend = app.state::<Backend>();
                backend
                    .quitting
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                let child = backend.child.lock().unwrap().take();
                if let Some(child) = child {
                    // Like Ctrl-C would: the backend keeps nothing it needs to save first, and
                    // changes not yet synced with git are picked up by its next start.
                    let _ = child.kill();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_stereo_by_its_info_or_its_web_ui() {
        let info = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n\
                    {\"app\":\"stereo\",\"version\":\"0.1.54\"}";
        assert!(looks_like_stereo(info));
        let ui = "HTTP/1.1 200 OK\r\n\r\n<!doctype html><title>Stereo</title>";
        assert!(looks_like_stereo(ui));
        assert!(!looks_like_stereo("HTTP/1.1 404 Not Found\r\n\r\n"));
        assert!(!looks_like_stereo(
            "HTTP/1.1 200 OK\r\n\r\n<title>Something else</title>"
        ));
    }

    #[test]
    fn only_loopback_and_app_pages_stay_in_the_window() {
        let url = |s: &str| s.parse::<Url>().unwrap();
        assert!(is_app_url(&url("http://localhost:8005/")));
        assert!(is_app_url(&url("http://127.0.0.1:51234/audio/x")));
        assert!(is_app_url(&url("tauri://localhost/index.html")));
        assert!(is_app_url(&url("http://tauri.localhost/index.html")));
        assert!(!is_app_url(&url("https://www.beatport.com/")));
        assert!(!is_app_url(&url("https://www.youtube.com/watch?v=x")));
    }
}
