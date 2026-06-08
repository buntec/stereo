use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Clone, Args)]
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

    /// Run in dev mode (frontend served by Vite, not the backend)
    #[arg(long, env = "STEREO_DEV")]
    pub dev: bool,
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
