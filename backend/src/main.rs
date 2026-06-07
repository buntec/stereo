use std::sync::Arc;

use axum::{Router, routing::get};
use axum_embed::{FallbackBehavior, ServeEmbed};
use clap::Parser;
use rust_embed::RustEmbed;
use stereo_backend::{server, settings::Settings};
use tracing::info;

#[derive(RustEmbed, Clone)]
#[folder = "../src/stereo/statics"]
struct Assets;

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
        let serve_assets = ServeEmbed::<Assets>::with_parameters(
            Some("index.html".to_string()),
            FallbackBehavior::Ok,
            Some("index.html".to_string()),
        );
        router = router.fallback_service(serve_assets);
        info!("Serving embedded frontend assets");
    }

    let addr = format!("{}:{}", settings.host, settings.port);
    info!("Listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
