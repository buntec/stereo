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

fn install(settings: &Settings) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let home_env = std::env::var("HOME").map(PathBuf::from)?;
    let log_path = home_env.join("Library/Logs/stereo.log");
    let plist = plist_path()?;

    let mut args = format!(
        "        <string>{}</string>\n        <string>--port</string>\n        <string>{}</string>\n        <string>--host</string>\n        <string>{}</string>",
        exe.display(),
        settings.port,
        settings.host,
    );
    if let Some(ref h) = settings.home {
        args.push_str(&format!(
            "\n        <string>--home</string>\n        <string>{}</string>",
            h.display()
        ));
    }

    let plist_content = format!(
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
    <key>RunAtLoad</key>
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
        log = log_path.display(),
    );

    std::fs::create_dir_all(plist.parent().unwrap())?;
    std::fs::write(&plist, &plist_content)?;
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
