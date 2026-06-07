use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "stereo", about = "A browser-based music curation app")]
pub struct Settings {
    /// Directory for app data (default: ~/.local/share/stereo)
    #[arg(long, env = "STEREO_HOME")]
    pub home: Option<PathBuf>,

    /// Bind host
    #[arg(long, default_value = "localhost", env = "STEREO_HOST")]
    pub host: String,

    /// Bind port
    #[arg(long, short = 'p', default_value_t = 8005, env = "STEREO_PORT")]
    pub port: u16,

    /// Increase verbosity (repeat for more detail)
    #[arg(long = "verbose", short = 'v', action = clap::ArgAction::Count)]
    pub verbosity: u8,

    /// Run in dev mode (no static file serving)
    #[arg(long, env = "STEREO_DEV")]
    pub dev: bool,

    /// Path to the built frontend (served in non-dev mode)
    #[arg(long, default_value = "src/stereo/statics", env = "STEREO_STATIC_DIR")]
    pub static_dir: PathBuf,
}

impl Settings {
    pub fn home_dir(&self) -> PathBuf {
        self.home.clone().unwrap_or_else(|| {
            let base = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."));
            base.join(".local").join("share").join("stereo")
        })
    }
}
