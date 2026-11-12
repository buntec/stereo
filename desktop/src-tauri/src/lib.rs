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
/// How long quitting waits for the backend to stop on its own: more than the 15 s it gives itself
/// to remove its `--tailscale` route.
const STOP_TIMEOUT: Duration = Duration::from_secs(20);

/// The bundled backend, while it runs.
#[derive(Default)]
struct Backend {
    child: Mutex<Option<CommandChild>>,
    /// Set as the app quits, when the backend's exit is no error.
    quitting: std::sync::atomic::AtomicBool,
}

/// While the start page asks for the WebSocket token: the port of the backend that wants it, the
/// start page, and how many tokens it rejected.
#[derive(Default)]
struct TokenPrompt(Mutex<Option<(u16, Url, u32)>>);

/// What listens on a loopback port.
#[derive(Debug, PartialEq)]
enum Probe {
    Stereo,
    Free,
    Taken,
}

/// The addresses `stereo` may listen on: it binds `localhost`, which may be either address family.
fn loopback_addrs(port: u16) -> Vec<std::net::SocketAddr> {
    ("localhost", port)
        .to_socket_addrs()
        .map(Iterator::collect)
        .unwrap_or_default()
}

fn probe(port: u16) -> Probe {
    let mut taken = false;
    for addr in loopback_addrs(port) {
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

/// What a backend makes of a WebSocket token (`--ws-token`).
#[derive(Debug, PartialEq)]
enum Auth {
    /// Offered none, and it needs none.
    NotRequired,
    /// Offered none, but it needs one.
    Required,
    Accepted,
    Rejected,
}

/// Opens a WebSocket to the backend on `port` as the UI would, offering `token`, and closes it
/// again. `None` if the backend doesn't answer as Stereo does.
fn websocket_auth(port: u16, token: Option<&str>) -> Option<Auth> {
    let mut stream = loopback_addrs(port)
        .iter()
        .find_map(|addr| TcpStream::connect_timeout(addr, Duration::from_millis(300)).ok())?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let mut request = format!(
        "GET /ws HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\n\
         Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n"
    );
    if let Some(token) = token {
        request.push_str(&format!(
            "Sec-WebSocket-Protocol: {}\r\n",
            websocket_auth_protocol(token)
        ));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).ok()?;

    // The response's head, a byte at a time so as not to read into the first frame.
    let mut head = Vec::new();
    let mut byte = [0];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 16 * 1024 {
            return None;
        }
        stream.read_exact(&mut byte).ok()?;
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head);
    match (head.split(' ').nth(1)?, token) {
        ("401", None) => Some(Auth::Required),
        ("101", None) => Some(Auth::NotRequired),
        // A rejected token is upgraded only to be closed at once (`TOKEN_REJECTED` in
        // `server.rs`), an accepted one starts a session, whose messages follow.
        ("101", Some(_)) => match stream.read_exact(&mut byte) {
            Ok(()) if byte[0] & 0x0f == 0x8 => Some(Auth::Rejected),
            _ => Some(Auth::Accepted),
        },
        _ => None,
    }
}

/// The WebSocket subprotocol that carries `token`, as `App.tsx` offers it.
fn websocket_auth_protocol(token: &str) -> String {
    let hex = token
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("stereo.auth.{hex}")
}

/// The UI's URL on `port`, with the WebSocket `token` in its fragment for the UI to pick up (and
/// keep). `localhost`, like the backend's bind address and the URL the CLI prints, so that it's
/// the same origin a browser tab would have.
fn ui_url(port: u16, token: Option<&str>) -> Url {
    let mut url: Url = format!("http://localhost:{port}/")
        .parse()
        .expect("a valid URL");
    if let Some(token) = token {
        url.set_fragment(Some(&format!("stereo-token={}", percent_encode(token))));
    }
    url
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Where the app keeps a token entered on the start page.
fn saved_token_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    Some(app.path().app_config_dir().ok()?.join("ws-token"))
}

fn save_token(app: &AppHandle, token: &str) {
    let Some(path) = saved_token_path(app) else {
        return;
    };
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let saved = std::fs::create_dir_all(path.parent().expect("a file in a directory"))
        .and_then(|()| options.open(&path))
        .and_then(|mut file| file.write_all(token.as_bytes()));
    if let Err(error) = saved {
        eprintln!("could not save the WebSocket token: {error}");
    }
}

/// The token of the launchd service `stereo install` sets up, which keeps it in its plist.
#[cfg(target_os = "macos")]
fn launchd_token() -> Option<String> {
    let plist = std::path::Path::new(&std::env::var_os("HOME")?)
        .join("Library/LaunchAgents/com.stereo.plist");
    let output = std::process::Command::new("/usr/bin/plutil")
        .args([
            "-extract",
            "EnvironmentVariables.STEREO_WS_TOKEN",
            "raw",
            "-o",
            "-",
        ])
        .arg(plist)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let token = String::from_utf8(output.stdout).ok()?;
    let token = token.trim_end_matches('\n');
    (output.status.success() && !token.is_empty()).then(|| token.to_string())
}

/// The WebSocket tokens the app knows of: the one its backend inherits, the one entered last on
/// the start page, and the launchd service's.
fn known_tokens(app: &AppHandle) -> Vec<String> {
    let mut tokens = Vec::new();
    tokens.extend(std::env::var("STEREO_WS_TOKEN").ok());
    tokens.extend(saved_token_path(app).and_then(|path| std::fs::read_to_string(path).ok()));
    #[cfg(target_os = "macos")]
    tokens.extend(launchd_token());
    tokens.retain(|token| !token.is_empty());
    tokens
}

/// Opens the UI of the backend on `port`, with a token it accepts when it wants one; failing
/// that, the start page asks for it. The webview keeps no token of its own the app could use: its
/// storage is not the browser's.
fn open_ui(app: &AppHandle, window: &WebviewWindow, start_page: &Url, port: u16) {
    let token = match websocket_auth(port, None) {
        Some(Auth::Required) => {
            let accepted = known_tokens(app)
                .into_iter()
                .find(|token| websocket_auth(port, Some(token)) == Some(Auth::Accepted));
            if accepted.is_none() {
                return ask_for_token(app, window, start_page, port, false);
            }
            accepted
        }
        _ => None,
    };
    if let Err(error) = window.navigate(ui_url(port, token.as_deref())) {
        let message = "Could not open Stereo.";
        show_error(window, start_page, message, &error.to_string());
    }
}

/// Shows the start page with a form for the WebSocket token of the backend on `port`, which it
/// submits as `?stereo-token=…` for `on_navigation` to hand to `submit_token`.
fn ask_for_token(
    app: &AppHandle,
    window: &WebviewWindow,
    start_page: &Url,
    port: u16,
    rejected: bool,
) {
    let rejections = {
        let state = app.state::<TokenPrompt>();
        let mut prompt = state.0.lock().unwrap();
        let rejections = prompt.as_ref().map_or(0, |(_, _, n)| *n) + u32::from(rejected);
        *prompt = Some((port, start_page.clone(), rejections));
        rejections
    };
    let message = if rejected {
        "Stereo did not accept that token. Try again:".to_string()
    } else {
        format!(
            "The Stereo running at http://localhost:{port} wants its WebSocket token \
             (--ws-token, or STEREO_WS_TOKEN):"
        )
    };
    let page = serde_json::json!({ "message": message, "token": true, "attempt": rejections });
    let mut url = start_page.clone();
    url.set_fragment(Some(&page.to_string()));
    if let Err(error) = window.navigate(url) {
        eprintln!("could not show the start page: {error}");
    }
}

/// The token submitted by the start page's form, if `url` is that submission.
fn submitted_token(app: &AppHandle, url: &Url) -> Option<String> {
    let prompt = app.state::<TokenPrompt>();
    let prompt = prompt.0.lock().unwrap();
    let (_, start_page, _) = prompt.as_ref()?;
    let same_page = url.scheme() == start_page.scheme()
        && url.host_str() == start_page.host_str()
        && url.path() == start_page.path();
    if !same_page {
        return None;
    }
    url.query_pairs()
        .find(|(key, _)| key == "stereo-token")
        .map(|(_, token)| token.trim().to_string())
}

/// Opens the UI with a token entered on the start page, or asks again if the backend rejects it.
fn submit_token(app: &AppHandle, token: &str) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Some((port, start_page, _)) = app.state::<TokenPrompt>().0.lock().unwrap().clone() else {
        return;
    };
    match websocket_auth(port, Some(token)) {
        Some(Auth::Accepted) => {
            app.state::<TokenPrompt>().0.lock().unwrap().take();
            save_token(app, token);
            if let Err(error) = window.navigate(ui_url(port, Some(token))) {
                let message = "Could not open Stereo.";
                show_error(&window, &start_page, message, &error.to_string());
            }
        }
        Some(_) => ask_for_token(app, &window, &start_page, port, true),
        None => {
            app.state::<TokenPrompt>().0.lock().unwrap().take();
            let message = format!("Stereo at http://localhost:{port} stopped answering.");
            show_error(&window, &start_page, &message, "");
        }
    }
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
        // Also ends the yt-dlp downloads it started when the app quits (`stop_backend`), or when
        // it exits by itself and the event loop below drops its child.
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
        Probe::Stereo => return open_ui(&app, &window, &start_page, preferred),
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
    open_ui(&app, &window, &start_page, port);

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

/// Stops the backend as Ctrl-C would, giving it time to remove its `--tailscale` route, then ends
/// what is left of its process group (yt-dlp downloads). `CommandChild::kill`, like dropping it,
/// sends `SIGKILL` to the whole group at once, which would leave the route behind.
fn stop_backend(child: CommandChild) {
    #[cfg(unix)]
    {
        // Gone once the shell plugin's waiter has reaped it. (Its `Terminated` event can come
        // later: it waits for the pipes to close, which yt-dlp downloads may hold open.)
        let pid = child.pid() as libc::pid_t;
        let running = || unsafe { libc::kill(pid, 0) } == 0;
        // Only to the backend itself: its group also holds the `tailscale` CLI it cleans up with.
        if running() && unsafe { libc::kill(pid, libc::SIGTERM) } == 0 {
            let deadline = Instant::now() + STOP_TIMEOUT;
            while running() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    let _ = child.kill();
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        // Its script would cancel `target="_blank"` link clicks for an IPC command the page may
        // not call: such links open in the browser through `on_new_window` instead.
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .manage(Backend::default())
        .manage(TokenPrompt::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("Stereo")
                    .inner_size(1400.0, 900.0)
                    .min_inner_size(480.0, 480.0)
                    // Lets the UI adapt where the desktop app differs from a browser tab.
                    .initialization_script("window.__STEREO_DESKTOP__ = true;")
                    // Links to other sites open in the default browser, as do new windows
                    // (`window.open`, `target="_blank"`).
                    .on_navigation(move |url| {
                        if let Some(token) = submitted_token(&handle, url) {
                            let app = handle.clone();
                            std::thread::spawn(move || submit_token(&app, &token));
                            return false;
                        }
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
                    // Changes not yet synced with git are picked up by its next start.
                    stop_backend(child);
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
    fn puts_the_token_in_the_ui_urls_fragment() {
        assert_eq!(ui_url(8005, None).as_str(), "http://localhost:8005/");
        assert_eq!(
            ui_url(8005, Some("a b&c=d#é")).as_str(),
            "http://localhost:8005/#stereo-token=a%20b%26c%3Dd%23%C3%A9"
        );
        assert_eq!(websocket_auth_protocol("ab"), "stereo.auth.6162");
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
