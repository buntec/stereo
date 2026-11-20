use std::path::{Path, PathBuf};

use clap::Args;

pub const DEFAULT_AUDIO_CACHE_LIMIT_GB: u64 = 10;
pub const DEFAULT_YT_DLP: &str = "yt-dlp";

#[derive(Debug, Clone, Args)]
pub struct Settings {
    /// Directory for app data (default: ~/.local/share/stereo)
    #[arg(long, env = "STEREO_HOME", global = true)]
    pub home: Option<PathBuf>,

    /// Bind host
    #[arg(long, default_value = "localhost", env = "STEREO_HOST", global = true)]
    pub host: String,

    /// Bind port
    #[arg(
        long,
        short = 'p',
        default_value_t = 8005,
        env = "STEREO_PORT",
        global = true
    )]
    pub port: u16,

    /// Additional browser origins allowed to open the WebSocket (comma-separated in the env var)
    #[arg(
        long = "allowed-origin",
        env = "STEREO_ALLOWED_ORIGINS",
        value_delimiter = ',',
        global = true
    )]
    pub allowed_origins: Vec<String>,

    /// WebSocket token required when listening on a non-loopback interface
    #[arg(long, env = "STEREO_WS_TOKEN", global = true)]
    pub ws_token: Option<String>,

    /// Directory for cached data such as audio downloads (default: ~/Library/Caches/stereo on
    /// macOS, $XDG_CACHE_HOME/stereo or ~/.cache/stereo elsewhere)
    #[arg(long, env = "STEREO_CACHE_DIR", global = true)]
    pub cache_dir: Option<PathBuf>,

    /// Size limit of the audio download cache in GB; least recently played tracks are evicted
    #[arg(
        long,
        default_value_t = DEFAULT_AUDIO_CACHE_LIMIT_GB,
        env = "STEREO_AUDIO_CACHE_LIMIT_GB",
        global = true
    )]
    pub audio_cache_limit_gb: u64,

    /// yt-dlp executable used to download audio for playback
    #[arg(long, default_value = DEFAULT_YT_DLP, env = "STEREO_YT_DLP", global = true)]
    pub yt_dlp: String,

    /// Increase verbosity (repeat for more detail)
    #[arg(long = "verbose", short = 'v', action = clap::ArgAction::Count)]
    pub verbosity: u8,

    /// Run in dev mode (frontend served by Vite, not the backend)
    #[arg(long, env = "STEREO_DEV")]
    pub dev: bool,

    /// Also serve over HTTPS on your tailnet, via `tailscale serve` on the same port
    #[arg(long, env = "STEREO_TAILSCALE", global = true)]
    pub tailscale: bool,
}

/// The user's home directory: `HOME`, else `USERPROFILE`, since Windows apps started from the
/// Start menu or Explorer have no `HOME`. Without either, the working directory, which for the
/// desktop app's backend on Windows is `C:\Windows\System32`.
fn user_home() -> PathBuf {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .find_map(|var| std::env::var_os(var).filter(|value| !value.is_empty()))
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// Moves a data directory from `legacy` to `home` if only `legacy` has one. Returns `legacy` if it
/// has to stay in use because it couldn't be moved.
#[cfg_attr(not(windows), allow(dead_code))]
fn adopt_legacy_home(legacy: &Path, home: &Path) -> Option<PathBuf> {
    if legacy == home || !legacy.join("gitrepos").is_dir() {
        return None;
    }
    if home.exists() {
        tracing::warn!(
            "Ignoring the older data directory {}: using {}. To use the older one instead, \
             stop Stereo and set STEREO_HOME to it, or move it there.",
            legacy.display(),
            home.display()
        );
        return None;
    }
    let moved = match home.parent() {
        Some(parent) => std::fs::create_dir_all(parent),
        None => Ok(()),
    }
    .and_then(|()| std::fs::rename(legacy, home));
    match moved {
        Ok(()) => {
            // A warning, which is logged by default: the data has moved.
            tracing::warn!(
                "Moved the data directory from {} to {}",
                legacy.display(),
                home.display()
            );
            None
        }
        Err(error) => {
            tracing::warn!(
                "Could not move the data directory from {} to {} ({error}), so using it where \
                 it is. Stop Stereo and move it to use the new location.",
                legacy.display(),
                home.display()
            );
            Some(legacy.to_path_buf())
        }
    }
}

impl Settings {
    /// Before `USERPROFILE` was used on Windows, the default data directory without `HOME` was
    /// relative to the working directory, which for the desktop app is `C:\Windows\System32`.
    /// Moves a data directory found there to the current default, or keeps using it if it can't.
    #[cfg(windows)]
    pub fn adopt_legacy_home(&mut self) {
        let has_home = std::env::var_os("HOME").is_some_and(|home| !home.is_empty());
        if self.home.is_some() || has_home {
            return;
        }
        let legacy = Path::new(".local").join("share").join("stereo");
        let Ok(legacy) = std::path::absolute(&legacy) else {
            return;
        };
        if let Some(legacy) = adopt_legacy_home(&legacy, &self.home_dir()) {
            self.home = Some(legacy);
        }
    }

    /// The data directory, always absolute: collection paths derived from it are sent to the
    /// client and later accepted back only as absolute file paths (see `utils::is_file_or_url`).
    pub fn home_dir(&self) -> PathBuf {
        let home = self
            .home
            .clone()
            .unwrap_or_else(|| user_home().join(".local").join("share").join("stereo"));
        std::path::absolute(&home).unwrap_or(home)
    }

    /// Where downloaded audio is cached, always absolute. Not under the data directory: it is
    /// disposable, large and per-machine, while the data directory holds the synced git repos.
    pub fn audio_cache_dir(&self) -> PathBuf {
        let cache = self.cache_dir.clone().unwrap_or_else(|| {
            let home = user_home();
            let base = if cfg!(target_os = "macos") {
                home.join("Library").join("Caches")
            } else {
                std::env::var("XDG_CACHE_HOME")
                    .ok()
                    .filter(|dir| !dir.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".cache"))
            };
            base.join("stereo")
        });
        let cache = std::path::absolute(&cache).unwrap_or(cache);
        cache.join("audio")
    }

    pub fn is_loopback_bind(&self) -> bool {
        matches!(
            self.host
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_ascii_lowercase()
                .as_str(),
            "localhost" | "127.0.0.1" | "::1"
        )
    }

    pub fn validate_websocket_security(&self) -> anyhow::Result<()> {
        if self
            .ws_token
            .as_deref()
            .is_some_and(|token| token.trim().is_empty())
        {
            anyhow::bail!("--ws-token must not be empty");
        }
        if self
            .ws_token
            .as_deref()
            .is_some_and(|token| token.len() > 256)
        {
            anyhow::bail!("--ws-token must be at most 256 bytes");
        }
        let mut has_remote_origin = false;
        for origin in &self.allowed_origins {
            let parsed = url::Url::parse(origin)
                .map_err(|_| anyhow::anyhow!("invalid --allowed-origin: {origin}"))?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.path() != "/"
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                anyhow::bail!("invalid --allowed-origin: {origin}");
            }
            let host = parsed
                .host_str()
                .expect("origin host was checked above")
                .trim_start_matches('[')
                .trim_end_matches(']');
            has_remote_origin |= !matches!(host, "localhost" | "127.0.0.1" | "::1");
        }
        if self.tailscale && !self.is_loopback_bind() {
            // `tailscale serve` listens on the tailnet address, on the same port.
            anyhow::bail!("--tailscale requires a loopback --host");
        }
        if self.tailscale && self.dev {
            anyhow::bail!("--tailscale serves the built frontend, which --dev doesn't");
        }
        if (!self.is_loopback_bind() || has_remote_origin) && self.ws_token.is_none() {
            anyhow::bail!("--ws-token (or STEREO_WS_TOKEN) is required for non-loopback access");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(host: &str) -> Settings {
        Settings {
            home: None,
            host: host.to_string(),
            port: 8005,
            allowed_origins: Vec::new(),
            ws_token: None,
            cache_dir: None,
            audio_cache_limit_gb: crate::settings::DEFAULT_AUDIO_CACHE_LIMIT_GB,
            yt_dlp: crate::settings::DEFAULT_YT_DLP.to_string(),
            verbosity: 0,
            dev: false,
            tailscale: false,
        }
    }

    #[test]
    fn remote_bind_requires_websocket_token() {
        assert!(settings("localhost").validate_websocket_security().is_ok());
        assert!(settings("127.0.0.1").validate_websocket_security().is_ok());
        assert!(settings("0.0.0.0").validate_websocket_security().is_err());

        let mut remote = settings("0.0.0.0");
        remote.ws_token = Some("secret".to_string());
        assert!(remote.validate_websocket_security().is_ok());
    }

    #[test]
    fn configured_origins_must_be_bare_http_origins() {
        let mut settings = settings("localhost");
        settings.allowed_origins = vec!["https://localhost".to_string()];
        assert!(settings.validate_websocket_security().is_ok());
        settings.allowed_origins = vec!["https://music.example/path".to_string()];
        assert!(settings.validate_websocket_security().is_err());
    }

    #[test]
    fn remote_allowed_origin_requires_websocket_token() {
        let mut settings = settings("localhost");
        settings.allowed_origins = vec!["https://music.example".to_string()];
        assert!(settings.validate_websocket_security().is_err());
        settings.ws_token = Some("secret".to_string());
        assert!(settings.validate_websocket_security().is_ok());
    }

    #[test]
    fn tailscale_needs_no_token_but_a_loopback_bind() {
        let mut settings = settings("localhost");
        settings.tailscale = true;
        assert!(settings.validate_websocket_security().is_ok());
        settings.host = "0.0.0.0".to_string();
        settings.ws_token = Some("secret".to_string());
        assert!(settings.validate_websocket_security().is_err());
    }

    /// A data directory under `dir`, at `parts`, with a collection in its default repo.
    fn data_dir(dir: &Path, parts: &[&str]) -> PathBuf {
        let home = parts
            .iter()
            .fold(dir.to_path_buf(), |path, part| path.join(part));
        let repo = home.join("gitrepos").join("stereo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("techno.db"), b"tracks").unwrap();
        home
    }

    #[test]
    fn legacy_home_moves_to_the_new_default() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = data_dir(dir.path(), &["System32", ".local", "share", "stereo"]);
        let home = dir
            .path()
            .join("profile")
            .join(".local")
            .join("share")
            .join("stereo");

        assert_eq!(adopt_legacy_home(&legacy, &home), None);
        assert!(!legacy.exists());
        let collection = home.join("gitrepos").join("stereo").join("techno.db");
        assert_eq!(std::fs::read(collection).unwrap(), b"tracks");
    }

    #[test]
    fn legacy_home_stays_when_the_new_default_exists() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = data_dir(dir.path(), &["legacy"]);
        let home = data_dir(dir.path(), &["home"]);

        assert_eq!(adopt_legacy_home(&legacy, &home), None);
        assert!(legacy
            .join("gitrepos")
            .join("stereo")
            .join("techno.db")
            .exists());
    }

    #[test]
    fn legacy_home_stays_in_use_when_it_cannot_move() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = data_dir(dir.path(), &["legacy"]);
        // A file where the new default's parent directory would go.
        std::fs::write(dir.path().join("profile"), b"").unwrap();
        let home = dir.path().join("profile").join("stereo");

        assert_eq!(adopt_legacy_home(&legacy, &home), Some(legacy.clone()));
        assert!(legacy
            .join("gitrepos")
            .join("stereo")
            .join("techno.db")
            .exists());
    }

    #[test]
    fn legacy_home_without_repositories_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        std::fs::create_dir_all(&legacy).unwrap();
        let home = dir.path().join("home");

        assert_eq!(adopt_legacy_home(&legacy, &home), None);
        assert!(legacy.exists());
        assert!(!home.exists());
        assert_eq!(adopt_legacy_home(&home, &home), None);
    }

    #[test]
    fn relative_home_yields_importable_collection_paths() {
        let rel = PathBuf::from(format!("target/test-relative-home-{}", std::process::id()));
        let repo = rel.join("gitrepos").join("example");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("b.db"), b"").unwrap();

        let mut settings = settings("localhost");
        settings.home = Some(rel.clone());
        let home = settings.home_dir();
        assert!(home.is_absolute());

        let collections =
            crate::utils::list_repo_collections(&home.join("gitrepos").join("example"));
        std::fs::remove_dir_all(&rel).unwrap();

        assert_eq!(collections.len(), 1);
        let path = collections[0].path.to_string_lossy();
        assert_eq!(
            crate::utils::is_file_or_url(&path),
            Some(crate::utils::PathKind::File)
        );
    }
}
