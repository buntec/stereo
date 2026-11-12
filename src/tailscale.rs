//! `--tailscale`: serve over HTTPS on the tailnet through `tailscale serve`, which terminates TLS,
//! preserves the Host header and proxies to a second, loopback-only listener (see `main.rs`).
//!
//! Connections on that listener come from the tailnet and must carry the tailnet token (or a
//! one-time pairing code, which hands it out) whatever the main listener accepts; telling them
//! apart by listener means no forwarded header has to be trusted.
//!
//! The Serve config is machine-wide, so stereo never replaces another route on its port and
//! removes only a route it created. A marker file in the data dir records that route, so a restart
//! after a crash takes it over.
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex as StdMutex, OnceLock},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tokio::{
    process::Command,
    sync::{watch, Mutex},
};

#[cfg(target_os = "macos")]
const PROGRAMS: &[&str] = &[
    "tailscale",
    "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
];
#[cfg(not(target_os = "macos"))]
const PROGRAMS: &[&str] = &["tailscale"];

// Shutdown waits for a command in flight, up to this long.
const TIMEOUT: Duration = Duration::from_secs(10);
const RETRY: Duration = Duration::from_secs(10);
// While serving, how often to check that the route still works.
const CHECK: Duration = Duration::from_secs(30);

/// How long a pairing code stays valid, and how many may be pending at once.
pub const PAIRING_TTL: Duration = Duration::from_secs(120);
const MAX_PAIRING_CODES: usize = 16;

const TOKEN_FILE: &str = "tailnet-token";
const MARKER_FILE: &str = "tailscale-route";

static INSTANCE: OnceLock<Arc<Tailscale>> = OnceLock::new();

/// Whether `--tailscale` serves the tailnet URL yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum TailscaleStatus {
    Starting,
    Serving {
        url: String,
    },
    /// The last attempt failed; stereo keeps retrying.
    Error {
        message: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Status {
    backend_state: String,
    #[serde(rename = "Self")]
    node: Option<Node>,
    cert_domains: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Node {
    #[serde(rename = "DNSName")]
    dns_name: String,
}

/// `tailscale serve status --json`, as far as stereo reads it.
#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ServeConfig {
    #[serde(rename = "TCP", default)]
    tcp: HashMap<String, TcpHandler>,
    #[serde(default)]
    web: HashMap<String, WebConfig>,
    // Routes of `tailscale serve` commands running in the foreground.
    #[serde(default)]
    foreground: HashMap<String, ServeConfig>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TcpHandler {
    #[serde(rename = "HTTP", default)]
    http: bool,
    #[serde(rename = "TCPForward")]
    tcp_forward: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WebConfig {
    #[serde(default)]
    handlers: HashMap<String, Handler>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Handler {
    proxy: Option<String>,
    path: Option<String>,
}

#[derive(Debug, PartialEq)]
enum Route {
    Free,
    /// A background proxy to this target.
    Proxy(String),
    /// Serves something else, described.
    Taken(String),
}

/// What the HTTPS root of `host:port` serves.
fn route(config: &ServeConfig, host: &str, port: u16) -> Route {
    let background = std::iter::once((config, false));
    let foreground = config.foreground.values().map(|config| (config, true));
    for (config, foreground) in background.chain(foreground) {
        if let Some(tcp) = config.tcp.get(&port.to_string()) {
            if let Some(target) = &tcp.tcp_forward {
                return Route::Taken(format!("a TCP forward to {target}"));
            }
            if tcp.http {
                return Route::Taken("plain HTTP".into());
            }
        }
        let web = config
            .web
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(&format!("{host}:{port}")));
        let Some(root) = web.and_then(|(_, web)| web.handlers.get("/")) else {
            continue;
        };
        return match (&root.proxy, &root.path) {
            // A foreground route ends with its command: not one to take over.
            (Some(target), _) if !foreground => Route::Proxy(target.clone()),
            (Some(target), _) => Route::Taken(format!("a proxy to {target}")),
            (None, Some(path)) => Route::Taken(format!("files from {path}")),
            (None, None) => Route::Taken("text".into()),
        };
    }
    Route::Free
}

/// The tailnet name and HTTPS URL for `port`, from `tailscale status --json`.
fn url(status: &[u8], port: u16) -> Result<(String, String), String> {
    let status: Status = serde_json::from_slice(status).map_err(|e| e.to_string())?;
    if status.backend_state != "Running" {
        return Err(format!("Tailscale is {}", status.backend_state));
    }
    let name = status
        .node
        .map(|node| node.dns_name.trim_end_matches('.').to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .ok_or("this machine has no tailnet DNS name; enable MagicDNS")?;
    if !status
        .cert_domains
        .unwrap_or_default()
        .iter()
        .any(|domain| domain.eq_ignore_ascii_case(&name))
    {
        return Err(
            "enable HTTPS certificates for the tailnet: https://tailscale.com/kb/1153/enabling-https"
                .into(),
        );
    }
    let url = if port == 443 {
        format!("https://{name}")
    } else {
        format!("https://{name}:{port}")
    };
    Ok((name, url))
}

#[derive(Default)]
struct State {
    stopped: bool,
    // This process may have created the route.
    owned: bool,
    host: Option<String>,
}

struct Tailscale {
    programs: Vec<PathBuf>,
    /// The HTTPS port on the tailnet.
    port: u16,
    /// Where `tailscale serve` proxies to: the tailnet listener.
    proxy: String,
    /// Records the route stereo created: `<port> <proxy>`.
    marker: Option<PathBuf>,
    token: String,
    // Held across each setup attempt, so shutdown waits for one in flight.
    state: Mutex<State>,
    status: watch::Sender<TailscaleStatus>,
    /// The tailnet origin browsers connect from, once known.
    origin: StdMutex<Option<String>>,
    /// Pending pairing codes and when they expire.
    codes: StdMutex<HashMap<String, Instant>>,
}

impl Tailscale {
    fn new(
        programs: Vec<PathBuf>,
        port: u16,
        target: SocketAddr,
        marker: Option<PathBuf>,
        token: String,
    ) -> Self {
        Self {
            programs,
            port,
            proxy: format!("http://{target}"),
            marker,
            token,
            state: Mutex::default(),
            status: watch::Sender::new(TailscaleStatus::Starting),
            origin: StdMutex::default(),
            codes: StdMutex::default(),
        }
    }

    /// Record `status`. Returns whether it changed.
    fn report(&self, status: TailscaleStatus) -> bool {
        self.status.send_if_modified(|current| {
            let changed = *current != status;
            *current = status;
            changed
        })
    }

    async fn cli(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        for program in &self.programs {
            let mut command = Command::new(program);
            command
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            match tokio::time::timeout(TIMEOUT, command.output()).await {
                Err(_) => return Err(format!("`tailscale {}` timed out", args.join(" "))),
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Ok(Err(e)) => return Err(e.to_string()),
                Ok(Ok(output)) if output.status.success() => return Ok(output.stdout),
                Ok(Ok(output)) => {
                    let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    if error.is_empty() {
                        return Err(format!("`tailscale {}` failed", args.join(" ")));
                    }
                    return Err(error);
                }
            }
        }
        Err("the tailscale CLI is not installed".into())
    }

    async fn serve_config(&self) -> Result<ServeConfig, String> {
        let output = self.cli(&["serve", "status", "--json"]).await?;
        if output.trim_ascii().is_empty() {
            return Ok(ServeConfig::default());
        }
        serde_json::from_slice(&output).map_err(|e| format!("unexpected serve status: {e}"))
    }

    async fn host(&self) -> Result<String, String> {
        Ok(url(&self.cli(&["status", "--json"]).await?, self.port)?.0)
    }

    async fn off(&self, port: u16) -> Result<(), String> {
        self.cli(&["serve", &format!("--https={port}"), "off"])
            .await
            .map(drop)
    }

    fn marked(&self) -> Option<(u16, String)> {
        let text = std::fs::read_to_string(self.marker.as_ref()?).ok()?;
        let (port, proxy) = text.trim().split_once(' ')?;
        Some((port.parse().ok()?, proxy.to_string()))
    }

    fn mark(&self) {
        let Some(marker) = &self.marker else { return };
        if let Err(error) = std::fs::write(marker, format!("{} {}\n", self.port, self.proxy)) {
            tracing::warn!("could not record the tailscale serve route: {error}");
        }
    }

    fn unmark(&self) {
        if let Some(marker) = &self.marker {
            let _ = std::fs::remove_file(marker);
        }
    }

    /// Serve stereo unless the route is taken. Returns its URL, or None once stopped.
    async fn attempt(&self) -> Result<Option<String>, String> {
        let mut state = self.state.lock().await;
        if state.stopped {
            return Ok(None);
        }
        let (host, url) = url(&self.cli(&["status", "--json"]).await?, self.port)?;
        state.host = Some(host.clone());
        *self.origin.lock().unwrap() = Some(url.clone());
        let config = self.serve_config().await?;
        let marked = self.marked();
        // Each start listens on a new port, so an earlier stereo's route is recognized by the marker.
        let earlier = |port: u16, target: &str| {
            marked
                .as_ref()
                .is_some_and(|(p, proxy)| *p == port && proxy == target)
        };
        match route(&config, &host, self.port) {
            Route::Proxy(target) if target == self.proxy => {
                state.owned |= earlier(self.port, &self.proxy);
                return Ok(Some(url));
            }
            Route::Proxy(target) if earlier(self.port, &target) => self.off(self.port).await?,
            Route::Proxy(target) => return Err(self.taken(&host, &format!("a proxy to {target}"))),
            Route::Taken(what) => return Err(self.taken(&host, &what)),
            Route::Free => {
                // A route left on another port by an earlier stereo.
                if let Some((port, proxy)) = &marked {
                    if *port != self.port
                        && route(&config, &host, *port) == Route::Proxy(proxy.clone())
                    {
                        self.off(*port).await?;
                    }
                }
            }
        }
        // Before the command: the daemon may keep the route even if it fails.
        state.owned = true;
        self.mark();
        self.cli(&[
            "serve",
            "--bg",
            "--yes",
            &format!("--https={}", self.port),
            &self.proxy,
        ])
        .await?;
        Ok(Some(url))
    }

    fn taken(&self, host: &str, what: &str) -> String {
        format!(
            "{host}:{} already serves {what}; remove that route or run stereo on another --port",
            self.port
        )
    }

    /// Stop setting up, and remove the route if stereo created it.
    async fn stop(&self) {
        let mut state = self.state.lock().await;
        state.stopped = true;
        if !state.owned && self.marked() != Some((self.port, self.proxy.clone())) {
            return;
        }
        let host = match state.host.clone() {
            Some(host) => host,
            None => match self.host().await {
                Ok(host) => host,
                Err(error) => {
                    // Keep the marker; the next start cleans up.
                    tracing::warn!("tailscale serve cleanup: {error}");
                    return;
                }
            },
        };
        let result = match self.serve_config().await {
            Ok(config) if route(&config, &host, self.port) == Route::Proxy(self.proxy.clone()) => {
                self.off(self.port).await
            }
            // Gone or changed since: not stereo's to remove.
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        };
        match result {
            Ok(()) => self.unmark(),
            Err(error) => tracing::warn!("tailscale serve cleanup: {error}"),
        }
    }

    /// Set up the route, then keep checking it: Tailscale may stop, or the route be removed or
    /// replaced, while stereo runs.
    async fn run(&self, check: Duration, retry: Duration) {
        let mut last_error = String::new();
        loop {
            let wait = match self.attempt().await {
                Ok(Some(url)) => {
                    last_error.clear();
                    if self.report(TailscaleStatus::Serving { url: url.clone() }) {
                        eprintln!("stereo tailnet URL: {url}");
                    }
                    check
                }
                Ok(None) => return,
                Err(error) => {
                    if error != last_error {
                        tracing::warn!("tailscale serve: {error} (retrying)");
                        last_error = error.clone();
                    }
                    self.report(TailscaleStatus::Error { message: error });
                    retry
                }
            };
            tokio::time::sleep(wait).await;
        }
    }

    fn create_code(&self) -> Result<String, String> {
        let mut codes = self.codes.lock().unwrap();
        let now = Instant::now();
        codes.retain(|_, expires| *expires > now);
        if codes.len() >= MAX_PAIRING_CODES {
            return Err("too many pending pairing codes; try again in a few minutes".into());
        }
        let code = uuid::Uuid::new_v4().simple().to_string();
        codes.insert(code.clone(), now + PAIRING_TTL);
        Ok(code)
    }

    fn redeem_code(&self, code: &str) -> bool {
        let mut codes = self.codes.lock().unwrap();
        codes
            .remove(code)
            .is_some_and(|expires| expires > Instant::now())
    }
}

/// The token tailnet clients authenticate with: `--ws-token` if given, else one kept in the data
/// dir, so that paired devices stay signed in across restarts.
async fn load_token(home: &Path, ws_token: Option<&str>) -> anyhow::Result<String> {
    if let Some(token) = ws_token {
        return Ok(token.to_string());
    }
    let path = home.join(TOKEN_FILE);
    if let Ok(data) = crate::utils::read_secret_file(&path).await {
        let token = String::from_utf8_lossy(&data).trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    crate::utils::write_secret_file(&path, token.as_bytes()).await?;
    Ok(token)
}

/// Expose the tailnet listener at `target` on `port` of the tailnet HTTPS name, retrying until
/// Tailscale is up and the route is free, and restoring the route if it goes.
pub async fn spawn(
    home: &Path,
    ws_token: Option<&str>,
    port: u16,
    target: SocketAddr,
) -> anyhow::Result<()> {
    let token = load_token(home, ws_token).await?;
    let programs = PROGRAMS.iter().map(PathBuf::from).collect();
    let marker = Some(home.join(MARKER_FILE));
    let tailscale = Arc::new(Tailscale::new(programs, port, target, marker, token));
    if INSTANCE.set(tailscale.clone()).is_err() {
        return Ok(());
    }
    tokio::spawn(async move { tailscale.run(CHECK, RETRY).await });
    Ok(())
}

/// The tailnet status and its changes, if the server runs with `--tailscale`.
pub fn subscribe() -> Option<watch::Receiver<TailscaleStatus>> {
    INSTANCE.get().map(|tailscale| tailscale.status.subscribe())
}

/// The origin of the tailnet URL, once Tailscale has reported it.
pub fn origin() -> Option<String> {
    INSTANCE.get()?.origin.lock().unwrap().clone()
}

/// The token tailnet clients must present.
pub fn token() -> Option<String> {
    INSTANCE.get().map(|tailscale| tailscale.token.clone())
}

/// A new single-use pairing code, valid for `PAIRING_TTL`.
pub fn create_pairing_code() -> Result<String, String> {
    INSTANCE
        .get()
        .ok_or_else(|| "stereo is not running with --tailscale".to_string())?
        .create_code()
}

/// Uses up `code`; whether it was valid.
pub fn redeem_pairing_code(code: &str) -> bool {
    INSTANCE
        .get()
        .is_some_and(|tailscale| tailscale.redeem_code(code))
}

/// Remove stereo's route on shutdown.
pub async fn shutdown() {
    if let Some(tailscale) = INSTANCE.get() {
        tailscale.stop().await;
    }
}

/// Serving `origin` with `token`, for tests of the tailnet listener; returns them.
#[cfg(test)]
pub(crate) fn install_for_tests() -> (&'static str, &'static str) {
    const ORIGIN: &str = "https://box.tail1.ts.net:8005";
    const TOKEN: &str = "tailnet-secret";
    INSTANCE.get_or_init(|| {
        let tailscale = Tailscale::new(
            Vec::new(),
            8005,
            "127.0.0.1:9".parse().unwrap(),
            None,
            TOKEN.into(),
        );
        *tailscale.origin.lock().unwrap() = Some(ORIGIN.into());
        Arc::new(tailscale)
    });
    (ORIGIN, TOKEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: &str, certs: &str) -> Vec<u8> {
        format!(
            r#"{{"BackendState":"{state}","Self":{{"DNSName":"Box.tail1.ts.net."}},"CertDomains":{certs}}}"#
        )
        .into_bytes()
    }

    #[test]
    fn derives_the_tailnet_url() {
        let running = status("Running", r#"["box.tail1.ts.net"]"#);
        assert_eq!(
            url(&running, 8005).unwrap().1,
            "https://box.tail1.ts.net:8005"
        );
        assert_eq!(url(&running, 443).unwrap().1, "https://box.tail1.ts.net");
        assert_eq!(url(&running, 443).unwrap().0, "box.tail1.ts.net");
        assert!(url(&status("Stopped", r#"["box.tail1.ts.net"]"#), 8005).is_err());
        assert!(url(&status("Running", "null"), 8005)
            .unwrap_err()
            .contains("HTTPS"));
    }

    #[test]
    fn classifies_existing_routes() {
        let config = |json: &str| serde_json::from_str::<ServeConfig>(json).unwrap();
        let check = |json: &str| route(&config(json), "box.tail1.ts.net", 8005);
        let web = |handler: &str| {
            format!(
                r#"{{"TCP":{{"8005":{{"HTTPS":true}}}},"Web":{{"box.tail1.ts.net:8005":{{"Handlers":{{"/":{handler}}}}}}}}}"#
            )
        };
        assert_eq!(check("{}"), Route::Free);
        assert_eq!(
            check(&web(r#"{"Proxy":"http://127.0.0.1:3000"}"#)),
            Route::Proxy("http://127.0.0.1:3000".into())
        );
        assert_eq!(
            check(&web(r#"{"Path":"/srv"}"#)),
            Route::Taken("files from /srv".into())
        );
        assert_eq!(
            check(r#"{"TCP":{"8005":{"TCPForward":"127.0.0.1:22"}}}"#),
            Route::Taken("a TCP forward to 127.0.0.1:22".into())
        );
        // Other ports, and paths below the root, leave the root free.
        assert_eq!(
            check(
                r#"{"Web":{"box.tail1.ts.net:5173":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:5173"}}}}}"#
            ),
            Route::Free
        );
        assert_eq!(
            check(
                r#"{"Web":{"box.tail1.ts.net:8005":{"Handlers":{"/api":{"Proxy":"http://127.0.0.1:9"}}}}}"#
            ),
            Route::Free
        );
        let foreground = format!(
            r#"{{"Foreground":{{"session":{}}}}}"#,
            web(r#"{"Proxy":"http://127.0.0.1:8005"}"#)
        );
        assert!(matches!(check(&foreground), Route::Taken(_)));
    }

    #[test]
    fn reports_status_changes_only() {
        let tailscale = Tailscale::new(
            Vec::new(),
            8005,
            "127.0.0.1:9000".parse().unwrap(),
            None,
            "t".into(),
        );
        let error = || TailscaleStatus::Error {
            message: "down".into(),
        };
        assert!(!tailscale.report(TailscaleStatus::Starting));
        assert!(tailscale.report(error()));
        assert!(!tailscale.report(error()));
        assert_eq!(
            serde_json::to_value(&*tailscale.status.borrow()).unwrap(),
            serde_json::json!({"state": "error", "message": "down"})
        );
    }

    #[test]
    fn pairing_codes_work_once() {
        let tailscale = Tailscale::new(
            Vec::new(),
            8005,
            "127.0.0.1:9000".parse().unwrap(),
            None,
            "t".into(),
        );
        let code = tailscale.create_code().unwrap();
        assert!(!tailscale.redeem_code("nope"));
        assert!(tailscale.redeem_code(&code));
        assert!(!tailscale.redeem_code(&code));
        for _ in 0..MAX_PAIRING_CODES {
            tailscale.create_code().unwrap();
        }
        assert!(tailscale.create_code().is_err());
    }

    #[tokio::test]
    async fn keeps_the_generated_token() {
        let home = tempfile::tempdir().unwrap();
        let token = load_token(home.path(), None).await.unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(load_token(home.path(), None).await.unwrap(), token);
        assert_eq!(
            load_token(home.path(), Some("given")).await.unwrap(),
            "given"
        );
    }

    /// A fake `tailscale` CLI with a one-route Serve config in a file.
    #[cfg(unix)]
    mod fake {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        pub const STATUS: &str = r#"{"BackendState":"Running","Self":{"DNSName":"box.tail1.ts.net."},"CertDomains":["box.tail1.ts.net"]}"#;

        pub fn route(port: u16, proxy: &str) -> String {
            format!(
                r#"{{"TCP":{{"{port}":{{"HTTPS":true}}}},"Web":{{"box.tail1.ts.net:{port}":{{"Handlers":{{"/":{{"Proxy":"{proxy}"}}}}}}}}}}"#
            )
        }

        pub struct Cli {
            pub dir: tempfile::TempDir,
        }

        impl Cli {
            pub fn new(config: &str) -> Self {
                let dir = tempfile::tempdir().unwrap();
                std::fs::write(dir.path().join("status.json"), STATUS).unwrap();
                std::fs::write(dir.path().join("serve.json"), config).unwrap();
                // `serve --bg` writes the route, then waits while `pause` exists, as the daemon
                // can keep a route before the command returns.
                let script = r#"#!/bin/sh
dir=$(dirname "$0")
echo "$*" >> "$dir/calls"
case "$*" in
  "status --json") cat "$dir/status.json" ;;
  "serve status --json") cat "$dir/serve.json" ;;
  "serve --bg --yes --https="*)
    port=${4#--https=}
    printf '{"TCP":{"%s":{"HTTPS":true}},"Web":{"box.tail1.ts.net:%s":{"Handlers":{"/":{"Proxy":"%s"}}}}}' "$port" "$port" "$5" > "$dir/serve.json"
    while [ -e "$dir/pause" ]; do sleep 0.05; done ;;
  "serve --https="*" off") echo '{}' > "$dir/serve.json" ;;
  *) exit 1 ;;
esac
"#;
                let path = dir.path().join("tailscale");
                std::fs::write(&path, script).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
                Self { dir }
            }

            pub fn tailscale(&self, port: u16, target: u16) -> Tailscale {
                Tailscale::new(
                    vec![self.path("tailscale")],
                    port,
                    SocketAddr::from(([127, 0, 0, 1], target)),
                    Some(self.marker()),
                    "t".into(),
                )
            }

            pub fn config(&self) -> String {
                std::fs::read_to_string(self.path("serve.json"))
                    .unwrap()
                    .trim()
                    .to_string()
            }

            pub fn calls(&self) -> String {
                std::fs::read_to_string(self.path("calls")).unwrap_or_default()
            }

            pub fn marker(&self) -> PathBuf {
                self.path(MARKER_FILE)
            }

            pub fn path(&self, name: &str) -> PathBuf {
                self.dir.path().join(name)
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn creates_and_removes_its_own_route() {
        let cli = fake::Cli::new("{}");
        let tailscale = cli.tailscale(8005, 9000);
        let url = tailscale.attempt().await.unwrap();
        assert_eq!(url.as_deref(), Some("https://box.tail1.ts.net:8005"));
        assert_eq!(cli.config(), fake::route(8005, "http://127.0.0.1:9000"));
        assert!(cli.marker().exists());
        assert_eq!(
            tailscale.origin.lock().unwrap().as_deref(),
            Some("https://box.tail1.ts.net:8005")
        );
        // A periodic check finds the route it created.
        assert!(tailscale.attempt().await.unwrap().is_some());
        assert_eq!(cli.calls().matches("--bg").count(), 1);
        tailscale.stop().await;
        assert_eq!(cli.config(), "{}");
        assert!(!cli.marker().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn leaves_other_routes_alone() {
        let other = fake::route(8005, "http://127.0.0.1:3000");
        let cli = fake::Cli::new(&other);
        let tailscale = cli.tailscale(8005, 9000);
        let error = tailscale.attempt().await.unwrap_err();
        assert!(error.contains("http://127.0.0.1:3000"), "{error}");
        tailscale.stop().await;
        assert_eq!(cli.config(), other);
        assert!(!cli.calls().contains("--bg"));
        assert!(!cli.calls().contains("off"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn replaces_its_route_from_an_earlier_run() {
        let cli = fake::Cli::new(&fake::route(8005, "http://127.0.0.1:9000"));
        std::fs::write(cli.marker(), "8005 http://127.0.0.1:9000\n").unwrap();
        let tailscale = cli.tailscale(8005, 9001);
        assert!(tailscale.attempt().await.unwrap().is_some());
        assert!(cli.calls().contains("serve --https=8005 off"));
        assert_eq!(cli.config(), fake::route(8005, "http://127.0.0.1:9001"));
        tailscale.stop().await;
        assert_eq!(cli.config(), "{}");
        assert!(!cli.marker().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn removes_its_route_left_on_another_port() {
        let cli = fake::Cli::new(&fake::route(9005, "http://127.0.0.1:9000"));
        std::fs::write(cli.marker(), "9005 http://127.0.0.1:9000\n").unwrap();
        let tailscale = cli.tailscale(8005, 9001);
        assert!(tailscale.attempt().await.unwrap().is_some());
        assert!(cli.calls().contains("serve --https=9005 off"));
        assert_eq!(cli.config(), fake::route(8005, "http://127.0.0.1:9001"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reports_and_restores_a_route_that_stops_working() {
        let cli = fake::Cli::new("{}");
        let tailscale = Arc::new(cli.tailscale(8005, 9000));
        let mut status = tailscale.status.subscribe();
        let run = tokio::spawn({
            let tailscale = tailscale.clone();
            let ms = Duration::from_millis(20);
            async move { tailscale.run(ms, ms).await }
        });
        let mut next = async || {
            status.changed().await.unwrap();
            let state = status.borrow_and_update().clone();
            state
        };
        assert!(matches!(next().await, TailscaleStatus::Serving { .. }));
        std::fs::write(
            cli.path("status.json"),
            fake::STATUS.replace("Running", "Stopped"),
        )
        .unwrap();
        assert!(matches!(next().await, TailscaleStatus::Error { .. }));
        std::fs::write(cli.path("status.json"), fake::STATUS).unwrap();
        // Removed while Tailscale was down: created again.
        std::fs::write(cli.path("serve.json"), "{}").unwrap();
        assert!(matches!(next().await, TailscaleStatus::Serving { .. }));
        assert_eq!(cli.config(), fake::route(8005, "http://127.0.0.1:9000"));
        tailscale.stop().await;
        run.await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shutdown_waits_for_setup_in_flight() {
        let cli = fake::Cli::new("{}");
        std::fs::write(cli.path("pause"), "").unwrap();
        let tailscale = Arc::new(cli.tailscale(8005, 9000));
        let setup = tokio::spawn({
            let tailscale = tailscale.clone();
            async move { tailscale.attempt().await }
        });
        // The route exists while `serve --bg` is still running.
        while !cli.calls().contains("--bg") {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let stop = tokio::spawn({
            let tailscale = tailscale.clone();
            async move { tailscale.stop().await }
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!stop.is_finished());
        std::fs::remove_file(cli.path("pause")).unwrap();
        stop.await.unwrap();
        assert!(setup.await.unwrap().unwrap().is_some());
        assert_eq!(cli.config(), "{}");
        // Stopped: no later attempt serves again.
        assert_eq!(tailscale.attempt().await.unwrap(), None);
        assert_eq!(cli.config(), "{}");
    }
}
