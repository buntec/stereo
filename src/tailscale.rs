//! `--tailscale`: serve over HTTPS on the tailnet through `tailscale serve`, which terminates TLS,
//! preserves the Host header and proxies to a second, loopback-only listener (see `main.rs`).
//!
//! Connections on that listener come from the tailnet and must carry the tailnet token (or a
//! one-time pairing code, which hands it out) whatever the main listener accepts; telling them
//! apart by listener means no forwarded header has to be trusted.
//!
//! The Serve config is machine-wide, so stereo never replaces another route on its port and
//! removes only the root handler it created. A lease file per port in the data dir records that
//! route and stays locked while its stereo runs, so a later run takes over the route of one that
//! crashed but never that of one still running.
use std::{
    collections::HashMap,
    fs::{File, OpenOptions, TryLockError},
    io::{Read, Seek, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex as StdMutex, OnceLock},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tokio::{
    io::AsyncReadExt,
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
// Every how many checks to also check Tailscale's own state (and the tailnet name, which rarely
// changes) rather than only the route: every 5 minutes.
const STATUS_EVERY: u32 = 10;

/// How long a pairing code stays valid, and how many may be pending at once.
pub const PAIRING_TTL: Duration = Duration::from_secs(120);
const MAX_PAIRING_CODES: usize = 16;
/// Pairing codes are typed on a phone when there's no QR code to scan (a Home Screen web app has
/// its own storage, and the camera opens links in Safari): Crockford's base32, which leaves out
/// I, L, O and U, so ten characters (50 bits) suffice for a code that works once, for two minutes.
const PAIRING_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const PAIRING_CODE_LEN: usize = 10;

const TOKEN_FILE: &str = "tailnet-token";
/// The leases, one file per tailnet port.
const LEASE_DIR: &str = "tailscale-routes";

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

/// A claim on stereo's route on one tailnet port: `<lease dir>/<port>`, exclusively locked by the
/// stereo using that port (the OS releases the lock when the process ends, even in a crash) and
/// holding the proxy target of the route it created, if any.
///
/// The files are emptied rather than deleted, so a lock is always on the file at its path. They
/// are read and written through the locked handle: on Windows the lock blocks every other one.
struct Lease {
    file: File,
}

impl Lease {
    /// The lease for `port`, unless a running stereo holds it.
    fn acquire(dir: &Path, port: u16) -> std::io::Result<Option<Self>> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(port.to_string());
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }

    /// The proxy target of the route recorded.
    fn proxy(&self) -> Option<String> {
        let mut file = &self.file;
        let mut text = String::new();
        file.rewind().ok()?;
        file.read_to_string(&mut text).ok()?;
        Some(text.trim())
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    }

    fn record(&self, proxy: Option<&str>) {
        let text = proxy.map(|proxy| format!("{proxy}\n")).unwrap_or_default();
        let mut file = &self.file;
        let result = file
            .set_len(0)
            .and_then(|()| file.rewind())
            .and_then(|()| file.write_all(text.as_bytes()));
        if let Err(error) = result {
            tracing::warn!("could not record the tailscale serve route: {error}");
        }
    }
}

#[derive(Default)]
struct State {
    stopped: bool,
    // This process may have created the route.
    owned: bool,
    host: Option<String>,
    /// Checks of the route since Tailscale's status was last read, while all succeed.
    checks: Option<u32>,
    /// This process's lease on `port`, once acquired; held until it exits.
    lease: Option<Lease>,
}

struct Tailscale {
    programs: Vec<PathBuf>,
    /// The HTTPS port on the tailnet.
    port: u16,
    /// Where `tailscale serve` proxies to: the tailnet listener.
    proxy: String,
    /// Where the leases live (see `Lease`).
    leases: Option<PathBuf>,
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
        leases: Option<PathBuf>,
        token: String,
    ) -> Self {
        Self {
            programs,
            port,
            proxy: format!("http://{target}"),
            leases,
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
            let mut child = match command.spawn() {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.to_string()),
                Ok(child) => child,
            };
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            // Read as it comes, so that a command that times out still says why: `serve` waits
            // after printing where to enable Serve for the tailnet.
            let run = async {
                let (out, err) = (child.stdout.take(), child.stderr.take());
                let (_, _, status) = tokio::join!(
                    async {
                        if let Some(mut out) = out {
                            out.read_to_end(&mut stdout).await.ok();
                        }
                    },
                    async {
                        if let Some(mut err) = err {
                            err.read_to_end(&mut stderr).await.ok();
                        }
                    },
                    child.wait(),
                );
                status
            };
            let status = tokio::time::timeout(TIMEOUT, run).await;
            let printed = |output: &[u8]| String::from_utf8_lossy(output).trim().to_string();
            let said = [printed(&stderr), printed(&stdout)]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            let command = format!("`tailscale {}`", args.join(" "));
            return match status {
                Err(_) if said.is_empty() => Err(format!("{command} timed out")),
                Err(_) => Err(format!("{command} timed out: {said}")),
                Ok(Err(e)) => Err(e.to_string()),
                Ok(Ok(status)) if status.success() => Ok(stdout),
                Ok(Ok(_)) if stderr.trim_ascii().is_empty() => Err(format!("{command} failed")),
                Ok(Ok(_)) => Err(printed(&stderr)),
            };
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

    /// Remove the root handler on `port`, stereo's route. Without `--set-path`, `off` would remove
    /// every handler on the port, other services' subpaths too.
    async fn off(&self, port: u16) -> Result<(), String> {
        self.cli(&["serve", &format!("--https={port}"), "--set-path=/", "off"])
            .await
            .map(drop)
    }

    /// Remove the routes that stereos which no longer run left on other ports.
    async fn remove_abandoned(&self, config: &ServeConfig, host: &str) -> Result<(), String> {
        let Some(dir) = &self.leases else {
            return Ok(());
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let Some(port) = entry
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<u16>().ok())
            else {
                continue;
            };
            if port == self.port {
                continue;
            }
            // Held: that stereo still runs and serves its route.
            let Ok(Some(lease)) = Lease::acquire(dir, port) else {
                continue;
            };
            if let Some(proxy) = lease.proxy() {
                if route(config, host, port) == Route::Proxy(proxy) {
                    self.off(port).await?;
                }
                lease.record(None);
            }
        }
        Ok(())
    }

    /// Serve stereo unless the route is taken. Returns its URL, or None once stopped.
    async fn attempt(&self) -> Result<Option<String>, String> {
        let mut state = self.state.lock().await;
        if state.stopped {
            return Ok(None);
        }
        let checks = state.checks.take();
        let result = self.setup(&mut state, checks).await;
        if result.is_ok() {
            state.checks = Some(checks.map_or(0, |n| (n + 1) % STATUS_EVERY));
        }
        result
    }

    /// `attempt`, `checks` (if any) since the status was last read.
    async fn setup(
        &self,
        state: &mut State,
        checks: Option<u32>,
    ) -> Result<Option<String>, String> {
        if let (None, Some(dir)) = (&state.lease, &self.leases) {
            match Lease::acquire(dir, self.port) {
                Ok(Some(lease)) => state.lease = Some(lease),
                Ok(None) => {
                    return Err(format!(
                        "another stereo with this data dir serves the tailnet on port {}; stop it or run stereo on another --port",
                        self.port
                    ))
                }
                Err(error) => return Err(format!("could not lock the tailscale serve route: {error}")),
            }
        }
        let known = state.host.clone().zip(self.origin.lock().unwrap().clone());
        let (host, url) = match known {
            // Serving so far: the route alone is checked, mostly.
            Some(known) if checks.is_some_and(|n| n + 1 < STATUS_EVERY) => known,
            _ => {
                let (host, url) = url(&self.cli(&["status", "--json"]).await?, self.port)?;
                state.host = Some(host.clone());
                *self.origin.lock().unwrap() = Some(url.clone());
                (host, url)
            }
        };
        let config = self.serve_config().await?;
        // Each start listens on a new port, so an earlier stereo's route is recognized by the lease,
        // which only this process holds now.
        let recorded = state.lease.as_ref().and_then(Lease::proxy);
        match route(&config, &host, self.port) {
            Route::Proxy(target) if target == self.proxy => {
                state.owned |= recorded.as_ref() == Some(&self.proxy);
                return Ok(Some(url));
            }
            Route::Proxy(target) if recorded.as_ref() == Some(&target) => {
                self.off(self.port).await?
            }
            Route::Proxy(target) => return Err(self.taken(&host, &format!("a proxy to {target}"))),
            Route::Taken(what) => return Err(self.taken(&host, &what)),
            Route::Free => {}
        }
        self.remove_abandoned(&config, &host).await?;
        // Before the command: the daemon may keep the route even if it fails.
        state.owned = true;
        if let Some(lease) = &state.lease {
            lease.record(Some(&self.proxy));
        }
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
        let recorded = state.lease.as_ref().and_then(Lease::proxy);
        if !state.owned && recorded.as_ref() != Some(&self.proxy) {
            return;
        }
        let host = match state.host.clone() {
            Some(host) => host,
            None => match self.host().await {
                Ok(host) => host,
                Err(error) => {
                    // Keep the record; the next start cleans up.
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
            Ok(()) => {
                if let Some(lease) = &state.lease {
                    lease.record(None);
                }
            }
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
        // 5 bits from each of a v4 UUID's random bytes, skipping those with its version (6) and
        // variant (8) bits.
        let random = uuid::Uuid::new_v4();
        let code: String = (random.as_bytes().iter().enumerate())
            .filter(|(i, _)| *i != 6 && *i != 8)
            .take(PAIRING_CODE_LEN)
            .map(|(_, byte)| PAIRING_ALPHABET[usize::from(byte & 31)] as char)
            .collect();
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
    let leases = Some(home.join(LEASE_DIR));
    let tailscale = Arc::new(Tailscale::new(programs, port, target, leases, token));
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
        assert_eq!(code.len(), PAIRING_CODE_LEN);
        assert!(code.bytes().all(|c| PAIRING_ALPHABET.contains(&c)));
        assert_ne!(tailscale.create_code().unwrap(), code);
        assert!(!tailscale.redeem_code("nope"));
        assert!(tailscale.redeem_code(&code));
        assert!(!tailscale.redeem_code(&code));
        for _ in 1..MAX_PAIRING_CODES {
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

    /// A fake `tailscale` CLI whose Serve config is a file of `<port> <path> <proxy>` handlers,
    /// removed as Tailscale does: `off` alone removes every handler on the port, `--set-path` one.
    #[cfg(unix)]
    mod fake {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        pub const STATUS: &str = r#"{"BackendState":"Running","Self":{"DNSName":"box.tail1.ts.net."},"CertDomains":["box.tail1.ts.net"]}"#;

        pub struct Cli {
            pub dir: tempfile::TempDir,
        }

        impl Cli {
            pub fn new(routes: &[&str]) -> Self {
                let dir = tempfile::tempdir().unwrap();
                std::fs::write(dir.path().join("status.json"), STATUS).unwrap();
                let cli = Self { dir };
                cli.set_routes(routes);
                // `serve --bg` adds the root handler, then waits while `pause` exists, as the
                // daemon can keep a route before the command returns.
                let script = r#"#!/bin/sh
dir=$(dirname "$0")
routes="$dir/routes"
echo "$*" >> "$dir/calls"
case "$*" in
  "status --json") cat "$dir/status.json" ;;
  "serve status --json")
    awk '
      !($1 in seen) { seen[$1] = 1; ports[++n] = $1 }
      { h[$1] = h[$1] (h[$1] == "" ? "" : ",") "\"" $2 "\":{\"Proxy\":\"" $3 "\"}" }
      END {
        printf "{\"TCP\":{"
        for (i = 1; i <= n; i++) printf "%s\"%s\":{\"HTTPS\":true}", (i > 1 ? "," : ""), ports[i]
        printf "},\"Web\":{"
        for (i = 1; i <= n; i++) printf "%s\"box.tail1.ts.net:%s\":{\"Handlers\":{%s}}", (i > 1 ? "," : ""), ports[i], h[ports[i]]
        print "}}"
      }' "$routes" ;;
  "serve --bg --yes --https="*)
    port=${4#--https=}
    awk -v p="$port" '!($1 == p && $2 == "/")' "$routes" > "$routes.new"
    echo "$port / $5" >> "$routes.new"
    mv "$routes.new" "$routes"
    while [ -e "$dir/pause" ]; do sleep 0.05; done ;;
  "serve --https="*" --set-path=/ off")
    port=${2#--https=}
    awk -v p="$port" '!($1 == p && $2 == "/")' "$routes" > "$routes.new"
    mv "$routes.new" "$routes" ;;
  "serve --https="*" off")
    port=${2#--https=}
    awk -v p="$port" '$1 != p' "$routes" > "$routes.new"
    mv "$routes.new" "$routes" ;;
  *) exit 1 ;;
esac
"#;
                let path = cli.path("tailscale");
                std::fs::write(&path, script).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
                cli
            }

            /// A stereo serving tailnet `port` from `127.0.0.1:<target>`, with leases in the
            /// fake's directory: one per process, as for processes sharing a data dir.
            pub fn tailscale(&self, port: u16, target: u16) -> Tailscale {
                Tailscale::new(
                    vec![self.path("tailscale")],
                    port,
                    SocketAddr::from(([127, 0, 0, 1], target)),
                    Some(self.path(LEASE_DIR)),
                    "t".into(),
                )
            }

            /// The handlers, sorted.
            pub fn routes(&self) -> Vec<String> {
                let text = std::fs::read_to_string(self.path("routes")).unwrap();
                let mut routes: Vec<_> = text.lines().map(str::to_string).collect();
                routes.sort();
                routes
            }

            pub fn set_routes(&self, routes: &[&str]) {
                let text: String = routes.iter().map(|route| format!("{route}\n")).collect();
                std::fs::write(self.path("routes"), text).unwrap();
            }

            pub fn calls(&self) -> String {
                std::fs::read_to_string(self.path("calls")).unwrap_or_default()
            }

            /// What the lease for `port` records, as an earlier, crashed run left it.
            pub fn record(&self, port: u16, proxy: &str) {
                std::fs::create_dir_all(self.path(LEASE_DIR)).unwrap();
                std::fs::write(self.lease(port), format!("{proxy}\n")).unwrap();
            }

            pub fn recorded(&self, port: u16) -> String {
                std::fs::read_to_string(self.lease(port))
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            }

            fn lease(&self, port: u16) -> PathBuf {
                self.path(LEASE_DIR).join(port.to_string())
            }

            pub fn path(&self, name: &str) -> PathBuf {
                self.dir.path().join(name)
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn creates_and_removes_its_own_route() {
        let cli = fake::Cli::new(&[]);
        let tailscale = cli.tailscale(8005, 9000);
        let url = tailscale.attempt().await.unwrap();
        assert_eq!(url.as_deref(), Some("https://box.tail1.ts.net:8005"));
        assert_eq!(cli.routes(), ["8005 / http://127.0.0.1:9000"]);
        assert_eq!(cli.recorded(8005), "http://127.0.0.1:9000");
        assert_eq!(
            tailscale.origin.lock().unwrap().as_deref(),
            Some("https://box.tail1.ts.net:8005")
        );
        // A periodic check finds the route it created.
        assert!(tailscale.attempt().await.unwrap().is_some());
        assert_eq!(cli.calls().matches("--bg").count(), 1);
        tailscale.stop().await;
        assert!(cli.routes().is_empty());
        assert_eq!(cli.recorded(8005), "");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn leaves_other_routes_alone() {
        let other = "8005 / http://127.0.0.1:3000";
        let cli = fake::Cli::new(&[other]);
        let tailscale = cli.tailscale(8005, 9000);
        let error = tailscale.attempt().await.unwrap_err();
        assert!(error.contains("http://127.0.0.1:3000"), "{error}");
        tailscale.stop().await;
        assert_eq!(cli.routes(), [other]);
        assert!(!cli.calls().contains("--bg"));
        assert!(!cli.calls().contains("off"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn keeps_other_handlers_on_its_port() {
        let api = "8005 /api http://127.0.0.1:3000";
        let cli = fake::Cli::new(&[api]);
        let tailscale = cli.tailscale(8005, 9000);
        assert!(tailscale.attempt().await.unwrap().is_some());
        assert_eq!(cli.routes(), ["8005 / http://127.0.0.1:9000", api]);
        tailscale.stop().await;
        assert_eq!(cli.routes(), [api]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn replaces_its_route_from_an_earlier_run() {
        let api = "8005 /api http://127.0.0.1:3000";
        let cli = fake::Cli::new(&["8005 / http://127.0.0.1:9000", api]);
        cli.record(8005, "http://127.0.0.1:9000");
        let tailscale = cli.tailscale(8005, 9001);
        assert!(tailscale.attempt().await.unwrap().is_some());
        assert!(cli.calls().contains("serve --https=8005 --set-path=/ off"));
        assert_eq!(cli.routes(), ["8005 / http://127.0.0.1:9001", api]);
        tailscale.stop().await;
        assert_eq!(cli.routes(), [api]);
        assert_eq!(cli.recorded(8005), "");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn removes_its_route_left_on_another_port() {
        let api = "9005 /api http://127.0.0.1:3000";
        let cli = fake::Cli::new(&["9005 / http://127.0.0.1:9000", api]);
        cli.record(9005, "http://127.0.0.1:9000");
        let tailscale = cli.tailscale(8005, 9001);
        assert!(tailscale.attempt().await.unwrap().is_some());
        assert!(cli.calls().contains("serve --https=9005 --set-path=/ off"));
        assert_eq!(cli.routes(), ["8005 / http://127.0.0.1:9001", api]);
        assert_eq!(cli.recorded(9005), "");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn taking_over_its_route_also_removes_those_left_on_other_ports() {
        let cli = fake::Cli::new(&[
            "8005 / http://127.0.0.1:9000",
            "9005 / http://127.0.0.1:9001",
        ]);
        cli.record(8005, "http://127.0.0.1:9000");
        cli.record(9005, "http://127.0.0.1:9001");
        let tailscale = cli.tailscale(8005, 9002);
        assert!(tailscale.attempt().await.unwrap().is_some());
        assert_eq!(cli.routes(), ["8005 / http://127.0.0.1:9002"]);
        assert_eq!(cli.recorded(9005), "");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn checks_the_tailscale_status_only_every_few_checks() {
        let cli = fake::Cli::new(&[]);
        let tailscale = cli.tailscale(8005, 9000);
        let status_reads = || {
            cli.calls()
                .lines()
                .filter(|l| *l == "status --json")
                .count()
        };
        for _ in 0..STATUS_EVERY * 2 {
            assert!(tailscale.attempt().await.unwrap().is_some());
        }
        assert_eq!(status_reads(), 2);
        // After a failure, the next check reads it again.
        cli.set_routes(&["8005 / http://127.0.0.1:3000"]);
        assert!(tailscale.attempt().await.is_err());
        cli.set_routes(&[]);
        let before = status_reads();
        assert!(tailscale.attempt().await.unwrap().is_some());
        assert_eq!(status_reads(), before + 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn leaves_the_route_of_a_stereo_still_running() {
        let cli = fake::Cli::new(&[]);
        let first = cli.tailscale(9005, 9000);
        assert!(first.attempt().await.unwrap().is_some());
        // Another port, the same data dir: the first one's route stays.
        let second = cli.tailscale(8005, 9001);
        assert!(second.attempt().await.unwrap().is_some());
        assert!(second.attempt().await.unwrap().is_some());
        let both = [
            "8005 / http://127.0.0.1:9001",
            "9005 / http://127.0.0.1:9000",
        ];
        assert_eq!(cli.routes(), both);
        assert!(!cli.calls().contains("off"));
        // The same port: refused, without touching the route.
        let third = cli.tailscale(9005, 9002);
        let error = third.attempt().await.unwrap_err();
        assert!(error.contains("another stereo"), "{error}");
        third.stop().await;
        assert_eq!(cli.routes(), both);
        assert_eq!(cli.recorded(9005), "http://127.0.0.1:9000");
        // Once the first has crashed, its route is taken over.
        drop(first);
        let restarted = cli.tailscale(9005, 9003);
        // A process spawned by another test briefly shares every open file, the first one's lease
        // included (macOS closes the spawned process's copies only once it starts running).
        let mut taken_over = restarted.attempt().await;
        for _ in 0..100 {
            if !taken_over
                .as_ref()
                .is_err_and(|e| e.contains("another stereo"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            taken_over = restarted.attempt().await;
        }
        assert!(taken_over.unwrap().is_some());
        assert_eq!(
            cli.routes(),
            [
                "8005 / http://127.0.0.1:9001",
                "9005 / http://127.0.0.1:9003"
            ]
        );
        second.stop().await;
        restarted.stop().await;
        assert!(cli.routes().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reports_and_restores_a_route_that_stops_working() {
        let cli = fake::Cli::new(&[]);
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
        cli.set_routes(&[]);
        assert!(matches!(next().await, TailscaleStatus::Serving { .. }));
        assert_eq!(cli.routes(), ["8005 / http://127.0.0.1:9000"]);
        tailscale.stop().await;
        run.await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shutdown_waits_for_setup_in_flight() {
        let cli = fake::Cli::new(&[]);
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
        assert!(cli.routes().is_empty());
        // Stopped: no later attempt serves again.
        assert_eq!(tailscale.attempt().await.unwrap(), None);
        assert!(cli.routes().is_empty());
    }
}
