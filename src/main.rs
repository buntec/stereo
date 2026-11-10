use std::{path::PathBuf, sync::Arc};

use axum::{routing::get, Router};
use axum_embed::{FallbackBehavior, ServeEmbed};
use clap::{Parser, Subcommand};
use rust_embed::RustEmbed;
use stereo_backend::{server, settings::Settings};

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
    if settings.verbosity > 0 {
        argv.push(format!("-{}", "v".repeat(settings.verbosity.into())));
    }
    let args = argv
        .iter()
        .map(|arg| format!("        <string>{}</string>", xml_escape(arg)))
        .collect::<Vec<_>>()
        .join("\n");

    // The token goes in the environment rather than argv so it doesn't show up in `ps`; the
    // plist itself is made owner-only below.
    let env = match &settings.ws_token {
        Some(token) => format!(
            "    <key>EnvironmentVariables</key>\n    <dict>\n        <key>STEREO_WS_TOKEN</key>\n        <string>{}</string>\n    </dict>\n",
            xml_escape(token)
        ),
        None => String::new(),
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

    let settings = cli.settings;
    settings.validate_websocket_security()?;

    let level = match settings.verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(format!("stereo={level},warn"))
                .unwrap_or_default(),
        )
        .init();

    let home = settings.home_dir();
    std::fs::create_dir_all(&home)?;
    let repos_dir = home.join("gitrepos");
    stereo_backend::utils::ensure_default_repo(&repos_dir).await?;
    stereo_backend::git::recover_pending_repositories(&repos_dir).await;
    stereo_backend::beatport::init(&home).await;

    let settings = Arc::new(settings);

    let mut router = Router::new()
        .route("/ws", get(server::ws_handler))
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
    let mode = if settings.dev { "dev" } else { "release" };
    eprintln!("stereo listening on http://{addr}  [{mode}]");
    eprintln!("data dir: {}", settings.home_dir().display());
    axum::serve(listener, router).await?;

    Ok(())
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
