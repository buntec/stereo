use std::{path::PathBuf, sync::Arc};

use axum::{routing::get, Extension, Router};
use axum_embed::{FallbackBehavior, ServeEmbed};
use clap::{Parser, Subcommand};
use rust_embed::RustEmbed;
use stereo_backend::{
    audio, server,
    settings::{Settings, DEFAULT_AUDIO_CACHE_LIMIT_GB, DEFAULT_YT_DLP},
    tailscale,
};

#[derive(RustEmbed, Clone)]
#[folder = "dist"]
struct Assets;

#[derive(Parser)]
#[command(name = "stereo", about = "A browser-based music curation app")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    settings: Settings,

    /// Exit once stdin closes. The desktop app starts its backend with this, so that the backend
    /// never outlives it, even when the app crashes.
    #[arg(long, hide = true)]
    exit_with_stdin: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Print the version and exit
    Version,
    /// Install stereo as a macOS launchd service (starts on login)
    Install,
    /// Uninstall the stereo macOS launchd service
    Uninstall,
    /// Restart the running stereo launchd service
    Restart,
}

const SERVICE_LABEL: &str = "com.stereo";

fn plist_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME").map(PathBuf::from)?;
    Ok(home
        .join("Library/LaunchAgents")
        .join(format!("{SERVICE_LABEL}.plist")))
}

fn uid() -> anyhow::Result<String> {
    let out = std::process::Command::new("id").arg("-u").output()?;
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

/// The plist may hold the WebSocket token, so keep it readable by its owner only.
fn write_owner_only(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        // `mode` only applies on creation; also fix up a plist left by an older install.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(data)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, data)
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The launchd plist that runs this binary with `settings`.
fn plist_content(settings: &Settings, exe: &std::path::Path, log_path: &std::path::Path) -> String {
    let mut argv = vec![
        exe.display().to_string(),
        "--port".to_string(),
        settings.port.to_string(),
        "--host".to_string(),
        settings.host.clone(),
    ];
    if settings.home.is_some() {
        // launchd runs the service from `/`, so a relative --home must be resolved now.
        argv.extend([
            "--home".to_string(),
            settings.home_dir().display().to_string(),
        ]);
    }
    for origin in &settings.allowed_origins {
        argv.extend(["--allowed-origin".to_string(), origin.clone()]);
    }
    if settings.cache_dir.is_some() {
        argv.extend([
            "--cache-dir".to_string(),
            settings
                .audio_cache_dir()
                .parent()
                .expect("the audio cache dir is a subdirectory")
                .display()
                .to_string(),
        ]);
    }
    if settings.audio_cache_limit_gb != DEFAULT_AUDIO_CACHE_LIMIT_GB {
        argv.extend([
            "--audio-cache-limit-gb".to_string(),
            settings.audio_cache_limit_gb.to_string(),
        ]);
    }
    if settings.yt_dlp != DEFAULT_YT_DLP {
        argv.extend(["--yt-dlp".to_string(), settings.yt_dlp.clone()]);
    }
    if settings.tailscale {
        argv.push("--tailscale".to_string());
    }
    if settings.verbosity > 0 {
        argv.push(format!("-{}", "v".repeat(settings.verbosity.into())));
    }
    let args = argv
        .iter()
        .map(|arg| format!("        <string>{}</string>", xml_escape(arg)))
        .collect::<Vec<_>>()
        .join("\n");

    let mut env_vars = Vec::new();
    // launchd's default PATH lacks e.g. Homebrew, where yt-dlp lives.
    if let Ok(path) = std::env::var("PATH") {
        env_vars.push(("PATH", path));
    }
    // The token goes in the environment rather than argv so it doesn't show up in `ps`; the
    // plist itself is made owner-only below.
    if let Some(token) = &settings.ws_token {
        env_vars.push(("STEREO_WS_TOKEN", token.clone()));
    }
    let env = if env_vars.is_empty() {
        String::new()
    } else {
        let entries = env_vars
            .iter()
            .map(|(key, value)| {
                format!(
                    "        <key>{key}</key>\n        <string>{}</string>\n",
                    xml_escape(value)
                )
            })
            .collect::<String>();
        format!("    <key>EnvironmentVariables</key>\n    <dict>\n{entries}    </dict>\n")
    };

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{SERVICE_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
{args}
    </array>
{env}    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#,
        log = xml_escape(&log_path.display().to_string()),
    )
}

fn install(settings: &Settings) -> anyhow::Result<()> {
    // The service would otherwise crash-loop under KeepAlive on startup validation.
    settings.validate_websocket_security()?;

    let exe = std::env::current_exe()?;
    let home_env = std::env::var("HOME").map(PathBuf::from)?;
    let log_path = home_env.join("Library/Logs/stereo.log");
    let plist = plist_path()?;
    let plist_content = plist_content(settings, &exe, &log_path);

    std::fs::create_dir_all(plist.parent().unwrap())?;
    write_owner_only(&plist, plist_content.as_bytes())?;
    eprintln!("wrote {}", plist.display());

    let uid = uid()?;
    let status = std::process::Command::new("launchctl")
        .args(["bootstrap", &format!("gui/{uid}"), plist.to_str().unwrap()])
        .status()?;
    if !status.success() {
        anyhow::bail!("launchctl bootstrap failed (service may already be loaded)");
    }
    eprintln!("stereo service installed and started");
    eprintln!("logs: {}", log_path.display());
    Ok(())
}

fn restart() -> anyhow::Result<()> {
    let uid = uid()?;
    let status = std::process::Command::new("launchctl")
        .args(["kickstart", "-k", &format!("gui/{uid}/{SERVICE_LABEL}")])
        .status()?;
    if !status.success() {
        anyhow::bail!("launchctl kickstart failed (is the service installed?)");
    }
    eprintln!("stereo service restarted");
    Ok(())
}

fn uninstall() -> anyhow::Result<()> {
    let plist = plist_path()?;
    let uid = uid()?;

    let status = std::process::Command::new("launchctl")
        .args(["bootout", &format!("gui/{uid}"), plist.to_str().unwrap()])
        .status()?;
    if !status.success() {
        eprintln!("launchctl bootout failed (service may not be loaded)");
    }

    if plist.exists() {
        std::fs::remove_file(&plist)?;
        eprintln!("removed {}", plist.display());
    } else {
        eprintln!("plist not found: {}", plist.display());
    }
    eprintln!("stereo service uninstalled");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Version) => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some(Command::Install) => return install(&cli.settings),
        Some(Command::Uninstall) => return uninstall(),
        Some(Command::Restart) => return restart(),
        None => {}
    }

    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut settings = cli.settings;
    settings.validate_websocket_security()?;

    let level = match settings.verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    // Symphonia (waveforms) warns about harmless quirks of YouTube's files, e.g. padding before
    // the MP4 header; a file it can't decode is logged by `audio.rs`.
    let quiet = "symphonia_core=error,symphonia_format_isomp4=error,symphonia_codec_aac=error";
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(format!("stereo={level},warn,{quiet}"))
                .unwrap_or_default(),
        )
        .init();

    #[cfg(windows)]
    settings.adopt_legacy_home();
    let home = settings.home_dir();
    std::fs::create_dir_all(&home)?;
    // Before any repository is touched, and held until the process exits.
    let data_dir_lock = stereo_backend::data_dir::DataDirLock::acquire(&home)?;
    let repos_dir = home.join("gitrepos");
    stereo_backend::utils::ensure_default_repo(&repos_dir).await?;
    stereo_backend::git::recover_pending_repositories(&repos_dir).await;
    stereo_backend::beatport::init(&home).await;
    audio::init(&settings).await;

    let settings = Arc::new(settings);

    let mut router = Router::new()
        .route("/ws", get(server::ws_handler))
        .route("/audio/{id}", get(audio::handler))
        .route("/info", get(info))
        .with_state(settings.clone());

    if !settings.dev {
        let serve_assets = ServeEmbed::<Assets>::with_parameters(
            Some("index.html".to_string()),
            FallbackBehavior::Ok,
            Some("index.html".to_string()),
        );
        router = router.fallback_service(serve_assets);
    }

    let addr = format!("{}:{}", settings.host, settings.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    data_dir_lock.record_port(listener.local_addr()?.port());
    let mode = if settings.dev { "dev" } else { "release" };
    eprintln!("stereo listening on http://{addr}  [{mode}]");
    eprintln!("data dir: {}", settings.home_dir().display());

    // `tailscale serve` proxies to a listener of its own, whose WebSockets always need the token.
    let tailnet = if settings.tailscale {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let target = listener.local_addr()?;
        tailscale::spawn(&home, settings.ws_token.as_deref(), settings.port, target).await?;
        let router = router.clone().layer(Extension(server::Tailnet));
        Some(tokio::spawn(
            async move { axum::serve(listener, router).await },
        ))
    } else {
        None
    };

    let tailnet = async {
        match tailnet {
            Some(task) => task.await,
            None => std::future::pending().await,
        }
    };
    // Only --tailscale has something to clean up on a signal, its route; without it, signals keep
    // their default and end the process at once.
    let tailscale_on = settings.tailscale;
    let signal = async {
        if tailscale_on {
            shutdown_signal().await
        } else {
            std::future::pending().await
        }
    };
    let stdin_closed = async {
        if cli.exit_with_stdin {
            stdin_closed().await
        } else {
            std::future::pending().await
        }
    };
    let result: anyhow::Result<()> = tokio::select! {
        result = axum::serve(listener, router) => result.map_err(Into::into),
        result = tailnet => result.map_err(Into::into).and_then(|r| r.map_err(Into::into)),
        () = signal => {
            remove_tailnet_route().await;
            // As the signal would have, without waiting for blocking tasks as the runtime drops.
            std::process::exit(0);
        }
        () = stdin_closed => {
            if tailscale_on {
                remove_tailnet_route().await;
            }
            // The app that would have ended our yt-dlp downloads is gone, and exiting skips
            // their `kill_on_drop`.
            end_own_process_group();
            std::process::exit(0);
        }
    };
    if tailscale_on {
        remove_tailnet_route().await;
    }
    result
}

/// What the desktop app looks for to tell a running Stereo from another server on its port, and
/// what its UI supports that the app adapts to: `title-bar-overlay`, a UI that leaves the top
/// 28px clear for a transparent title bar while its root element has `data-title-bar="overlay"`
/// (`desktop/src-tauri/src/title_bar.rs`).
async fn info() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "app": "stereo",
        "version": env!("CARGO_PKG_VERSION"),
        "features": ["title-bar-overlay"],
    }))
}

/// Resolves once stdin is closed (or fails), e.g. when the process that started us exits.
/// Read on a thread of its own: tokio's stdin reads on a blocking-pool thread that can't be
/// cancelled, and the runtime waits for it as it drops, so a serve error would never exit.
async fn stdin_closed() {
    let (closed, on_closed) = tokio::sync::oneshot::channel::<()>();
    std::thread::spawn(move || {
        wait_for_stdin_eof();
        drop(closed);
    });
    let _ = on_closed.await;
}

fn read_stdin_until_eof() {
    use std::io::Read;
    let mut stdin = std::io::stdin().lock();
    let mut buf = [0; 256];
    while let Ok(1..) = stdin.read(&mut buf) {}
}

#[cfg(not(windows))]
fn wait_for_stdin_eof() {
    read_stdin_until_eof();
}

/// Polls a pipe instead of reading it. Windows serializes I/O on a pipe opened for synchronous
/// I/O, so while a read waits, a child that inherited stdin can't close it, and can't exit: git
/// would hang until `GIT_TIMEOUT`, and the session with it.
#[cfg(windows)]
fn wait_for_stdin_eof() {
    use std::ffi::c_void;
    use std::io::Read;
    use std::os::windows::io::AsRawHandle;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileType(file: *mut c_void) -> u32;
        fn PeekNamedPipe(
            pipe: *mut c_void,
            buffer: *mut c_void,
            buffer_size: u32,
            bytes_read: *mut u32,
            total_bytes_available: *mut u32,
            bytes_left_this_message: *mut u32,
        ) -> i32;
    }
    const FILE_TYPE_PIPE: u32 = 3;

    let stdin = std::io::stdin();
    let handle = stdin.as_raw_handle();
    // SAFETY: `handle` is our stdin's, open for the life of the process.
    if unsafe { GetFileType(handle) } != FILE_TYPE_PIPE {
        return read_stdin_until_eof();
    }
    let mut buf = [0; 256];
    loop {
        let mut available = 0;
        // SAFETY: as above; the other pointers may be null, and `available` outlives the call.
        let ok = unsafe {
            PeekNamedPipe(
                handle,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            // Closed (`ERROR_BROKEN_PIPE`), or failing as a read would have.
            return;
        }
        if available > 0 {
            // There's data, so this read returns at once.
            let len = buf.len().min(available as usize);
            if !matches!(stdin.lock().read(&mut buf[..len]), Ok(1..)) {
                return;
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
}

/// Ends the other processes of our process group, e.g. yt-dlp downloads, when we lead it, as we do
/// when the desktop app starts us. A group we don't lead, e.g. a shell pipeline's, isn't ours.
fn end_own_process_group() {
    #[cfg(unix)]
    unsafe {
        if libc::getpgrp() == libc::getpid() {
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
            libc::killpg(0, libc::SIGTERM);
        }
    }
}

/// Shutdown waits this long at most for the tailnet route's removal: less than launchd's 20 s
/// before it kills the process.
const CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Remove the tailnet route, without hanging on a stuck `tailscale` CLI: within
/// `CLEANUP_TIMEOUT`, and another Ctrl-C (or SIGTERM) gives up at once. A route left behind is
/// taken over by the next start (see `tailscale.rs`).
async fn remove_tailnet_route() {
    tokio::select! {
        () = tailscale::shutdown() => {}
        () = shutdown_signal() => eprintln!("left the tailscale serve route in place"),
        () = tokio::time::sleep(CLEANUP_TIMEOUT) => {
            eprintln!("timed out removing the tailscale serve route");
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("installing the SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_keeps_security_settings_and_escapes_values() {
        let cli = Cli::parse_from([
            "stereo",
            "--host",
            "0.0.0.0",
            "--allowed-origin",
            "https://music.example",
            "--ws-token",
            "a<b&c",
            "--tailscale",
            "-vv",
        ]);
        let plist = plist_content(
            &cli.settings,
            std::path::Path::new("/opt/stereo"),
            std::path::Path::new("/tmp/stereo.log"),
        );
        assert!(plist.contains("<string>--allowed-origin</string>"));
        assert!(plist.contains("<string>https://music.example</string>"));
        assert!(plist.contains("<string>-vv</string>"));
        assert!(plist.contains("<string>--tailscale</string>"));
        assert!(plist.contains("<key>STEREO_WS_TOKEN</key>"));
        assert!(plist.contains("<string>a&lt;b&amp;c</string>"));
        // The token must not be passed on the command line.
        assert!(!plist.contains("--ws-token"));

        #[cfg(target_os = "macos")]
        {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("stereo.plist");
            std::fs::write(&path, &plist).unwrap();
            let lint = std::process::Command::new("plutil")
                .arg("-lint")
                .arg(&path)
                .output()
                .unwrap();
            assert!(lint.status.success(), "{plist}");
        }
    }
}
