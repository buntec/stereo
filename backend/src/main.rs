use std::sync::Arc;

use axum::{Router, routing::get};
use clap::Parser;
use stereo_backend::{server, settings::Settings};
use tower_http::services::ServeDir;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let settings = Settings::parse();

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
    info!("Data directory: {}", home.display());

    let settings = Arc::new(settings);

    let mut router = Router::new()
        .route("/ws", get(server::ws_handler))
        .with_state(settings.clone());

    if !settings.dev {
        let static_dir = settings.static_dir.clone();
        if static_dir.is_dir() {
            router = router.fallback_service(
                ServeDir::new(&static_dir).append_index_html_on_directories(true),
            );
            info!("Serving static files from {}", static_dir.display());
        } else {
            tracing::warn!(
                "Static directory '{}' not found — run 'bun run build' first",
                static_dir.display()
            );
        }
    }

    let addr = format!("{}:{}", settings.host, settings.port);
    info!("Listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
