use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, LazyLock, Mutex,
    },
    time::Duration,
};

use anyhow::Context;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};

use crate::{
    beatport, db, git,
    message::{MsgClient, MsgServer, SearchKind},
    models::{Collection, Track},
    scraper,
    settings::Settings,
    spotify,
    utils::{self, PathKind},
    youtube,
};

/// How long collection changes wait before being committed and pushed, so that a burst of
/// edits/ratings/plays becomes one commit and one network round-trip instead of one per change.
const GIT_SYNC_DELAY: Duration = Duration::from_secs(5 * 60);

/// Pending debounced syncs, keyed by canonical repo path. An entry exists only while its task is
/// still waiting out `GIT_SYNC_DELAY`; the task removes it before it starts syncing.
static SCHEDULED_SYNCS: LazyLock<Mutex<HashMap<PathBuf, tokio::task::JoinHandle<()>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A collection file that a session moved or deleted, announced to all sessions: any other
/// session with it open still has a pool on the old path, which SQLite refuses to write to.
#[derive(Clone, Debug)]
struct CollectionMoved {
    /// The session that moved it, which has already updated itself.
    origin: uuid::Uuid,
    from: PathBuf,
    /// `None` if the collection was deleted.
    to: Option<PathBuf>,
}

static COLLECTION_MOVES: LazyLock<broadcast::Sender<CollectionMoved>> =
    LazyLock::new(|| broadcast::channel(64).0);

/// Tells the other sessions that a collection moved; sent while the repo lock is still held, so
/// that their reopening (which takes it too) can't race a follow-up change to the same file.
fn announce_collection_moved(origin: uuid::Uuid, from: PathBuf, to: Option<PathBuf>) {
    // An error only means there are no sessions to tell.
    let _ = COLLECTION_MOVES.send(CollectionMoved { origin, from, to });
}

/// Feeds other sessions' `CollectionMoved` announcements into this session's event queue.
async fn forward_collection_moves(session: uuid::Uuid, events: mpsc::Sender<SessionEvent>) {
    let mut moves = COLLECTION_MOVES.subscribe();
    loop {
        match moves.recv().await {
            Ok(moved) if moved.origin == session => {}
            Ok(CollectionMoved { from, to, .. }) => {
                let event = InternalEvent::CollectionMoved { from, to };
                if events.send(SessionEvent::Internal(event)).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!("Missed {missed} collection move announcements");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Work for a session's message loop: a message from the browser, or a follow-up posted by a task
/// the session spawned. Slow work (network, git) runs in spawned tasks so the loop stays
/// responsive, while any session-state changes it implies still happen in order on the loop.
enum SessionEvent {
    Client(Box<MsgClient>),
    Internal(InternalEvent),
}

enum InternalEvent {
    /// Another session moved (`to`) or deleted (`to: None`) the collection at `from`.
    CollectionMoved {
        from: PathBuf,
        to: Option<PathBuf>,
    },
    /// A sync that may have updated the repo's working tree finished; the repo's collection list
    /// is re-sent so the client reopens its collection.
    RepoSynced {
        repo: PathBuf,
    },
    CloneFinished {
        id: i64,
        repo: Option<PathBuf>,
    },
    ImportDownloaded {
        url: String,
        db_path: PathBuf,
        keep_user_data: bool,
        /// Keeps the downloaded file alive until the import has run.
        _dir: tempfile::TempDir,
    },
}

#[derive(Default)]
struct SessionState {
    pool: Option<sqlx::SqlitePool>,
    collection: Option<Collection>,
    repo: Option<std::path::PathBuf>,
    repo_sync_task: Option<tokio::task::JoinHandle<()>>,
    search_task: Option<tokio::task::JoinHandle<()>>,
    youtube_playlist_task: Option<tokio::task::JoinHandle<()>>,
    fixup_task: Option<tokio::task::JoinHandle<()>>,
}

impl SessionState {
    fn pool(&self) -> Option<&sqlx::SqlitePool> {
        self.pool.as_ref()
    }

    async fn set_collection(
        &mut self,
        path: std::path::PathBuf,
        validate: bool,
    ) -> anyhow::Result<bool> {
        if validate && !db::validate_db_schema(&path).await {
            self.pool = None;
            self.collection = None;
            return Ok(false);
        }

        if let Some(old_pool) = self.pool.take() {
            old_pool.close().await;
        }
        let pool = db::open_pool(&path).await?;
        let n = db::count_rows(&pool, &Default::default()).await?;
        self.collection = Some(Collection { path, size: n });
        self.pool = Some(pool);
        Ok(true)
    }

    async fn refresh_count(&mut self, filter_model: &crate::message::FilterModelMap) {
        if let (Some(pool), Some(col)) = (&self.pool, &mut self.collection) {
            if let Ok(n) = db::count_rows(pool, filter_model).await {
                col.size = n;
            }
        }
    }
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(settings): State<Arc<Settings>>,
) -> impl IntoResponse {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return (StatusCode::FORBIDDEN, "WebSocket Origin header required").into_response();
    };
    if !websocket_origin_allowed(origin, &settings) {
        tracing::warn!(%origin, "Rejected WebSocket from untrusted origin");
        return (StatusCode::FORBIDDEN, "Untrusted WebSocket origin").into_response();
    }

    let auth_protocol = settings.ws_token.as_deref().map(websocket_auth_protocol);
    if let Some(expected) = auth_protocol.as_deref() {
        if !websocket_auth_supplied(&headers, expected) {
            return (
                StatusCode::UNAUTHORIZED,
                "WebSocket authentication required",
            )
                .into_response();
        }
    }

    let ws = match auth_protocol {
        Some(protocol) => ws.protocols([protocol]),
        None => ws,
    };
    ws.on_upgrade(move |socket| handle_socket(socket, settings))
        .into_response()
}

#[derive(Debug, PartialEq, Eq)]
struct BrowserOrigin {
    scheme: String,
    host: String,
    port: u16,
}

fn parse_browser_origin(origin: &str) -> Option<BrowserOrigin> {
    let url = url::Url::parse(origin).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    Some(BrowserOrigin {
        scheme: url.scheme().to_string(),
        host: url
            .host_str()?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase(),
        port: url.port_or_known_default()?,
    })
}

fn websocket_origin_allowed(origin: &str, settings: &Settings) -> bool {
    let Some(origin) = parse_browser_origin(origin) else {
        return false;
    };

    let loopback = matches!(origin.host.as_str(), "localhost" | "127.0.0.1" | "::1");
    if loopback && origin.scheme == "http" && (origin.port == settings.port || settings.dev) {
        return true;
    }

    let bind_host = settings
        .host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if !matches!(bind_host.as_str(), "0.0.0.0" | "::")
        && origin.scheme == "http"
        && origin.host == bind_host
        && origin.port == settings.port
    {
        return true;
    }

    settings
        .allowed_origins
        .iter()
        .filter_map(|allowed| parse_browser_origin(allowed))
        .any(|allowed| allowed == origin)
}

fn websocket_auth_protocol(token: &str) -> String {
    let encoded = token
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("stereo.auth.{encoded}")
}

fn websocket_auth_supplied(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|protocols| {
            protocols
                .split(',')
                .any(|protocol| protocol.trim() == expected)
        })
}

async fn handle_socket(socket: WebSocket, settings: Arc<Settings>) {
    let (ws_sink, ws_stream) = socket.split();
    let uid = uuid::Uuid::new_v4();
    tracing::info!("WS connected: {uid}");

    let (server_tx, server_rx) = mpsc::channel::<MsgServer>(10_000);
    let (event_tx, event_rx) = mpsc::channel::<SessionEvent>(10_000);

    let home = settings.home_dir();
    let repos_dir = home.join("gitrepos");
    if let Err(e) = tokio::fs::create_dir_all(&repos_dir).await {
        tracing::warn!("Failed to create gitrepos dir: {e}");
    }

    // Spawn send / recv tasks
    let recv_task = tokio::spawn(recv_loop(ws_stream, event_tx.clone()));
    let moves_task = tokio::spawn(forward_collection_moves(uid, event_tx.clone()));
    let send_task = tokio::spawn(send_loop(server_rx, ws_sink));

    // Send initial messages
    let _ = server_tx
        .send(MsgServer::BackendInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            home_dir: home.to_string_lossy().to_string(),
        })
        .await;
    let _ = server_tx
        .send(MsgServer::Repos {
            repos: utils::list_repos(&repos_dir).await,
        })
        .await;

    let mut state = SessionState::default();
    let http_client = Arc::new(match scraper::create_client() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to create HTTP client: {e}");
            return;
        }
    });

    let mut event_rx = event_rx;
    while let Some(event) = event_rx.recv().await {
        let (request_id, result) = match event {
            SessionEvent::Client(msg) => {
                let request_id = msg.request_id();
                let ctx = Ctx {
                    tx: &server_tx,
                    events: &event_tx,
                    client: &http_client,
                    data_home: &home,
                    home: &repos_dir,
                    session_id: uid,
                };
                (request_id, handle_msg(&mut state, &ctx, *msg).await)
            }
            SessionEvent::Internal(event) => (
                None,
                handle_internal(&mut state, &server_tx, &event_tx, &repos_dir, event).await,
            ),
        };
        if let Err(e) = result {
            tracing::error!("Message handling error: {e:#}");
            if let Some(id) = request_id {
                reply_error(&server_tx, id, &format!("{e:#}")).await;
            }
            notify(&server_tx, &format!("{e:#}"), "error").await;
        }
    }

    // WebSocket disconnected: stop work whose only consumer was this client. A running repo
    // sync is left to finish so it can't be interrupted halfway through a merge.
    for task in [
        state.search_task.take(),
        state.youtube_playlist_task.take(),
        state.fixup_task.take(),
    ]
    .into_iter()
    .flatten()
    {
        task.abort();
    }
    if let Some(pool) = state.pool.take() {
        pool.close().await;
    }
    recv_task.abort();
    moves_task.abort();
    send_task.abort();
    tracing::info!("WS disconnected: {uid}");
}

async fn recv_loop(
    mut stream: futures::stream::SplitStream<WebSocket>,
    tx: mpsc::Sender<SessionEvent>,
) {
    while let Some(result) = stream.next().await {
        match result {
            Ok(Message::Text(text)) => {
                let message_type = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|value| value["type"].as_str().map(str::to_owned));
                tracing::debug!(
                    message_type = message_type.as_deref().unwrap_or("unknown"),
                    payload_bytes = text.len(),
                    "WebSocket message received"
                );
                match serde_json::from_str::<MsgClient>(&text) {
                    Ok(msg) => {
                        if tx.send(SessionEvent::Client(Box::new(msg))).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => tracing::warn!("Failed to decode client message: {e}"),
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
}

async fn send_loop(
    mut rx: mpsc::Receiver<MsgServer>,
    mut sink: futures::stream::SplitSink<WebSocket, Message>,
) {
    let max_chunk = 100usize;
    let max_delay = Duration::from_millis(100);
    let mut buf: Vec<MsgServer> = Vec::new();

    loop {
        match tokio::time::timeout(max_delay, rx.recv()).await {
            Ok(Some(msg)) => {
                buf.push(msg);
                // Drain additional messages without blocking
                while buf.len() < max_chunk {
                    match rx.try_recv() {
                        Ok(m) => buf.push(m),
                        Err(_) => break,
                    }
                }
            }
            Ok(None) => break, // channel closed
            Err(_) => {}       // timeout — flush below if anything buffered
        }

        if !buf.is_empty() {
            match serde_json::to_string(&buf) {
                Ok(json) => {
                    tracing::debug!(
                        message_count = buf.len(),
                        payload_bytes = json.len(),
                        "WebSocket messages sent"
                    );
                    if sink.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(e) => tracing::error!("serialize error: {e}"),
            }
            buf.clear();
        }
    }
}

// ── Message dispatch ──────────────────────────────────────────────────────────

/// Everything a message handler needs besides the session state.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    tx: &'a mpsc::Sender<MsgServer>,
    /// The session's own event queue, for spawned tasks to post follow-ups.
    events: &'a mpsc::Sender<SessionEvent>,
    client: &'a Arc<wreq::Client>,
    /// Stereo's data dir (account sessions live here).
    data_home: &'a Path,
    /// The directory holding the git repos.
    home: &'a Path,
    /// Identifies this session in announcements to the others.
    session_id: uuid::Uuid,
}

/// Fails a request/reply message so the client's callback fires instead of waiting forever.
async fn reply_error(tx: &mpsc::Sender<MsgServer>, id: i64, message: &str) {
    let _ = tx
        .send(MsgServer::ErrorReply {
            id,
            message: message.to_string(),
        })
        .await;
}

async fn notify(tx: &mpsc::Sender<MsgServer>, msg: &str, kind: &str) {
    let _ = tx
        .send(MsgServer::Notification {
            message: msg.to_string(),
            kind: kind.to_string(),
        })
        .await;
}

async fn update_collection_info(state: &mut SessionState, tx: &mpsc::Sender<MsgServer>) {
    state.refresh_count(&Default::default()).await;
    if let Some(col) = &state.collection {
        let _ = tx
            .send(MsgServer::CollectionInfo {
                id: None,
                collection: Some(col.clone()),
                error_message: None,
                path_completions: None,
            })
            .await;
    }
    send_collection_stats(state, tx).await;
}

async fn send_collection_stats(state: &SessionState, tx: &mpsc::Sender<MsgServer>) {
    if let Some(pool) = state.pool() {
        match db::get_collection_stats(pool).await {
            Ok(stats) => {
                let _ = tx
                    .send(MsgServer::CollectionStats {
                        top_artists_five_star: stats.top_artists_five_star,
                        top_artists_four_plus: stats.top_artists_four_plus,
                        top_labels_five_star: stats.top_labels_five_star,
                        top_labels_four_plus: stats.top_labels_four_plus,
                    })
                    .await;
            }
            Err(e) => tracing::warn!("Failed to compute collection stats: {e}"),
        }
    }
}

/// Merges two `ScoredName` lists into a deduplicated name list, preserving first-seen order.
/// The open collection's Beatport tracks, letting artist/label searches resolve Beatport ids
/// without Beatport's search. Empty without a collection; a failed read only costs that.
async fn known_bp_tracks(state: &SessionState) -> scraper::KnownBpTracks {
    let Some(pool) = state.pool() else {
        return Default::default();
    };
    match db::get_bp_credits(pool).await {
        Ok(tracks) => scraper::KnownBpTracks::new(tracks),
        Err(e) => {
            tracing::warn!("Reading the collection's Beatport tracks failed: {e}");
            Default::default()
        }
    }
}

async fn send_git_status(tx: &mpsc::Sender<MsgServer>, report: git::SyncReport) {
    let _ = tx
        .send(MsgServer::GitSyncStatus {
            state: report.state,
            message: report.message,
            retryable: report.retryable,
        })
        .await;
}

async fn lock_current_repo(state: &SessionState) -> Option<tokio::sync::OwnedMutexGuard<()>> {
    match &state.repo {
        Some(repo) => Some(git::lock_repository(repo).await),
        None => None,
    }
}

fn sync_key(repo: &Path) -> PathBuf {
    std::fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf())
}

/// Commits and pushes `db_path`'s repo after `GIT_SYNC_DELAY`, coalescing with an already pending
/// sync of that repo. Changes that are still uncommitted if the server stops are picked up by the
/// startup sync (`git::recover_pending_repositories`).
async fn schedule_git_sync(tx: &mpsc::Sender<MsgServer>, repo: PathBuf, db_path: PathBuf) {
    if !db_path.starts_with(&repo) {
        return;
    }
    let key = sync_key(&repo);
    let newly_scheduled = {
        let mut scheduled = SCHEDULED_SYNCS.lock().expect("sync schedule poisoned");
        if scheduled.get(&key).is_some_and(|task| !task.is_finished()) {
            false
        } else {
            let tx = tx.clone();
            let task_key = key.clone();
            let task = tokio::spawn(async move {
                tokio::time::sleep(GIT_SYNC_DELAY).await;
                SCHEDULED_SYNCS
                    .lock()
                    .expect("sync schedule poisoned")
                    .remove(&task_key);
                send_git_status(
                    &tx,
                    git::SyncReport {
                        state: crate::message::GitSyncState::Syncing,
                        message: "Saving collection changes".into(),
                        retryable: false,
                    },
                )
                .await;
                let report = git::sync_repository(repo, Some(db_path), false, None).await;
                send_git_status(&tx, report).await;
            });
            scheduled.insert(key, task);
            true
        }
    };
    if newly_scheduled {
        send_git_status(
            tx,
            git::SyncReport {
                state: crate::message::GitSyncState::LocalChanges,
                message: format!(
                    "Changes will be synced within {} minutes",
                    GIT_SYNC_DELAY.as_secs() / 60
                ),
                retryable: true,
            },
        )
        .await;
    }
}

/// Drops a pending debounced sync of `repo`, for callers about to sync it right away.
fn cancel_scheduled_sync(repo: &Path) {
    if let Some(task) = SCHEDULED_SYNCS
        .lock()
        .expect("sync schedule poisoned")
        .remove(&sync_key(repo))
    {
        task.abort();
    }
}

/// Schedules a sync for the open collection after it was written to.
async fn schedule_collection_sync(state: &SessionState, tx: &mpsc::Sender<MsgServer>) {
    if let (Some(col), Some(repo)) = (&state.collection, &state.repo) {
        schedule_git_sync(tx, repo.clone(), col.path.clone()).await;
    }
}

fn repo_sync_running(state: &SessionState) -> bool {
    state
        .repo_sync_task
        .as_ref()
        .is_some_and(|task| !task.is_finished())
}

/// Makes `repo` the session's repo and syncs it, allowing upstream changes to be applied. Its
/// collections are sent once the sync is done (`RepoSynced`), and the client reopens one then.
async fn activate_repo(
    state: &mut SessionState,
    tx: &mpsc::Sender<MsgServer>,
    events: &mpsc::Sender<SessionEvent>,
    repo: PathBuf,
    resolution: Option<crate::message::GitConflictStrategy>,
    status: &str,
) {
    if let Some(old_pool) = state.pool.take() {
        old_pool.close().await;
    }
    state.collection = None;
    state.repo = Some(repo.clone());
    // The sync below commits pending changes anyway.
    cancel_scheduled_sync(&repo);
    // Taken before spawning so that a `SetCollection` can't open this repo's databases until the
    // sync (which may replace them) is done.
    let guard = git::lock_repository(&repo).await;
    state.repo_sync_task = Some(spawn_update_sync(
        tx, events, guard, repo, resolution, status,
    ));
}

/// Repo containing a collection file, if that file lives directly in a git repo.
fn collection_repo(path: &Path) -> Option<PathBuf> {
    path.parent()
        .filter(|parent| parent.join(".git").is_dir())
        .map(Path::to_path_buf)
}

async fn handle_internal(
    state: &mut SessionState,
    tx: &mpsc::Sender<MsgServer>,
    events: &mpsc::Sender<SessionEvent>,
    home: &Path,
    event: InternalEvent,
) -> anyhow::Result<()> {
    match event {
        InternalEvent::CollectionMoved { from, to } => {
            if state.collection.as_ref().is_some_and(|c| c.path == from) {
                if let Some(pool) = state.pool.take() {
                    pool.close().await;
                }
                state.collection = None;
                if let Some(to) = to {
                    let _repo_guard = match collection_repo(&to) {
                        Some(repo) => Some(git::lock_repository(&repo).await),
                        None => None,
                    };
                    // Validated, so that a file that has since moved on again isn't recreated
                    // as an empty database.
                    if let Err(e) = state.set_collection(to, true).await {
                        tracing::warn!("Reopening a moved collection failed: {e:#}");
                    }
                }
                // Sent before the collection list, as in `RenameCollection`.
                let _ = tx
                    .send(MsgServer::CollectionInfo {
                        id: None,
                        collection: state.collection.clone(),
                        error_message: None,
                        path_completions: None,
                    })
                    .await;
                send_collection_stats(state, tx).await;
            }
            if let Some(repo) = state.repo.as_deref().filter(|r| from.parent() == Some(*r)) {
                let _ = tx
                    .send(MsgServer::Collections {
                        collections: utils::list_repo_collections(repo),
                    })
                    .await;
            }
        }

        InternalEvent::RepoSynced { repo } => {
            if state.repo.as_deref() == Some(repo.as_path()) {
                let _ = tx
                    .send(MsgServer::Collections {
                        collections: utils::list_repo_collections(&repo),
                    })
                    .await;
                let _ = tx
                    .send(MsgServer::GitRemote {
                        url: git::get_remote(&repo).await,
                    })
                    .await;
            }
        }

        InternalEvent::CloneFinished { id, repo } => {
            let path = repo.as_ref().map(|r| r.to_string_lossy().to_string());
            if let Some(repo) = repo {
                // Sent before `Repos` so the client adopts the new repo before it re-selects one
                // from the updated repo list.
                let _ = tx.send(MsgServer::CloneRepoReply { id, path }).await;
                let _ = tx
                    .send(MsgServer::Repos {
                        repos: utils::list_repos(home).await,
                    })
                    .await;
                // A reused clone may be behind its remote.
                activate_repo(
                    state,
                    tx,
                    events,
                    repo,
                    None,
                    "Checking for upstream changes",
                )
                .await;
            } else {
                let _ = tx.send(MsgServer::CloneRepoReply { id, path: None }).await;
            }
        }

        InternalEvent::ImportDownloaded {
            url,
            db_path,
            keep_user_data,
            _dir,
        } => {
            let _repo_guard = lock_current_repo(state).await;
            let Some(pool) = state.pool() else {
                notify(
                    tx,
                    &format!("Import of {url} skipped: no collection open"),
                    "error",
                )
                .await;
                return Ok(());
            };
            db::import_from_db(pool, &db_path, keep_user_data).await?;
            update_collection_info(state, tx).await;
            schedule_collection_sync(state, tx).await;
        }
    }
    Ok(())
}

async fn handle_msg(state: &mut SessionState, ctx: &Ctx<'_>, msg: MsgClient) -> anyhow::Result<()> {
    let Ctx {
        tx,
        events,
        client,
        data_home,
        home,
        session_id,
    } = *ctx;
    match msg {
        MsgClient::Heartbeat { timestamp } => {
            let _ = tx.send(MsgServer::Heartbeat { timestamp }).await;
        }

        MsgClient::SetRepo { path } => {
            let repo_path = std::path::PathBuf::from(&path);
            if !repo_path.is_dir() || !repo_path.starts_with(home) {
                notify(tx, "Invalid repo path", "error").await;
                return Ok(());
            }
            // Don't stack another update sync on one still running for the same repo (e.g. a
            // client re-sending its selection after a reconnect).
            if state.repo.as_deref() == Some(repo_path.as_path()) && repo_sync_running(state) {
                return Ok(());
            }
            activate_repo(
                state,
                tx,
                events,
                repo_path,
                None,
                "Checking for upstream changes",
            )
            .await;
        }

        MsgClient::CloneRepo { id, url } => {
            let repo_name = git::repo_name_from_url(&url);
            let repo_path = home.join(&repo_name);
            let tx = tx.clone();
            let events = events.clone();
            tokio::spawn(async move {
                let repo = clone_or_reuse_repo(&tx, &url, &repo_name, repo_path).await;
                let _ = events
                    .send(SessionEvent::Internal(InternalEvent::CloneFinished {
                        id,
                        repo,
                    }))
                    .await;
            });
        }

        MsgClient::ListRepos => {
            let _ = tx
                .send(MsgServer::Repos {
                    repos: utils::list_repos(home).await,
                })
                .await;
        }

        MsgClient::ListCollections => {
            if let Some(repo) = &state.repo {
                let _ = tx
                    .send(MsgServer::Collections {
                        collections: utils::list_repo_collections(repo),
                    })
                    .await;
            }
        }

        MsgClient::ValidateTrack { id, track } => {
            let is_valid = serde_json::from_value::<Track>(track).is_ok();
            let _ = tx
                .send(MsgServer::ValidateTrackReply { id, is_valid })
                .await;
        }

        MsgClient::CheckSpotifyId { id, sp_id } => {
            let client = Arc::clone(client);
            let tx = tx.clone();
            tokio::spawn(async move {
                let exists = spotify::track_exists(&client, &sp_id).await.unwrap_or(true);
                let _ = tx
                    .send(MsgServer::CheckSpotifyIdReply { id, sp_id, exists })
                    .await;
            });
        }

        MsgClient::GetGitRemote => {
            let git_dir = state.repo.as_deref().unwrap_or(home);
            let _ = tx
                .send(MsgServer::GitRemote {
                    url: git::get_remote(git_dir).await,
                })
                .await;
        }

        MsgClient::SetGitRemote { url } => {
            let git_dir = state.repo.as_deref().unwrap_or(home);
            match git::set_remote(git_dir, url.as_deref()).await {
                Ok(()) => {
                    let _ = tx
                        .send(MsgServer::GitRemote {
                            url: git::get_remote(git_dir).await,
                        })
                        .await;
                    let _ = tx
                        .send(MsgServer::Repos {
                            repos: utils::list_repos(home).await,
                        })
                        .await;
                    match state.repo.clone() {
                        // Pull in whatever the new remote already has.
                        Some(repo) if !repo_sync_running(state) => {
                            activate_repo(
                                state,
                                tx,
                                events,
                                repo,
                                None,
                                "Synchronizing with the new remote",
                            )
                            .await;
                        }
                        _ => {
                            let tx = tx.clone();
                            let git_dir = git_dir.to_path_buf();
                            tokio::spawn(async move {
                                let report = git::sync_repository(git_dir, None, false, None).await;
                                send_git_status(&tx, report).await;
                            });
                        }
                    }
                }
                Err(e) => notify(tx, &format!("Failed to set git remote: {e}"), "error").await,
            }
        }

        sync_msg @ (MsgClient::RetryGitSync | MsgClient::ResolveGitConflict { .. }) => {
            let Some(repo) = state.repo.clone() else {
                notify(tx, "No repo selected", "error").await;
                return Ok(());
            };
            if repo_sync_running(state) {
                notify(tx, "Repository sync already in progress", "info").await;
                return Ok(());
            }
            let resolution = match sync_msg {
                MsgClient::ResolveGitConflict { strategy } => Some(strategy),
                _ => None,
            };
            activate_repo(
                state,
                tx,
                events,
                repo,
                resolution,
                "Synchronizing repository",
            )
            .await;
        }

        MsgClient::GetSpotifySession { id } => {
            let connected = spotify::SpotifySession::load(data_home).await.is_some();
            let _ = tx.send(MsgServer::SpotifySession { id, connected }).await;
        }

        MsgClient::SetSpotifySession { id, cookie } => {
            let result = match &cookie {
                Some(cookie) => spotify::SpotifySession::save(data_home, cookie).await,
                None => spotify::SpotifySession::clear(data_home).await,
            };
            match result {
                Ok(()) => {
                    let _ = tx
                        .send(MsgServer::SpotifySession {
                            id,
                            connected: cookie.is_some(),
                        })
                        .await;
                }
                Err(e) => notify(tx, &format!("Failed to set Spotify session: {e}"), "error").await,
            }
        }

        MsgClient::GetBeatportSession { id } => {
            let browser = beatport::connected_browser().await;
            let _ = tx.send(MsgServer::BeatportSession { id, browser }).await;
        }

        MsgClient::ConnectBeatportSession { id, user_agent } => {
            // Reading the browser's cookie store can block on an OS keychain prompt.
            let tx = tx.clone();
            let client = client.clone();
            let data_home = data_home.to_path_buf();
            tokio::spawn(async move {
                match beatport::connect(&client, &data_home, user_agent).await {
                    Ok(browser) => {
                        let _ = tx.send(MsgServer::BeatportSession { id, browser }).await;
                    }
                    Err(e) => {
                        let message = format!("Could not connect Beatport: {e}");
                        notify(&tx, &message, "error").await;
                        reply_error(&tx, id, &message).await;
                    }
                }
            });
        }

        MsgClient::DisconnectBeatportSession { id } => {
            beatport::disconnect(data_home).await?;
            let _ = tx
                .send(MsgServer::BeatportSession { id, browser: None })
                .await;
        }

        MsgClient::DetectSpotifySession { id } => {
            let tx = tx.clone();
            let data_home = data_home.to_path_buf();
            tokio::spawn(async move {
                match spotify::detect_session().await {
                    Ok(session) => {
                        match spotify::SpotifySession::save(&data_home, &session.cookie).await {
                            Ok(()) => {
                                let _ = tx
                                    .send(MsgServer::SpotifySession {
                                        id,
                                        connected: true,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                notify(
                                    &tx,
                                    &format!("Failed to save Spotify session: {e}"),
                                    "error",
                                )
                                .await
                            }
                        }
                    }
                    Err(e) => {
                        notify(
                            &tx,
                            &format!("Could not auto-detect Spotify session: {e}"),
                            "error",
                        )
                        .await
                    }
                }
            });
        }

        MsgClient::GetSpotifyPlaylistInfo { id, url } => {
            let client = Arc::clone(client);
            let tx = tx.clone();
            tokio::spawn(async move {
                let name = async {
                    let playlist_id = spotify::parse_playlist_id(&url)
                        .context("not a valid Spotify playlist link")?;
                    spotify::playlist_info(&client, &playlist_id)
                        .await
                        .map(|info| info.name)
                }
                .await
                .ok();
                let _ = tx.send(MsgServer::SpotifyPlaylistInfo { id, name }).await;
            });
        }

        MsgClient::GetYouTubePlaylist { id, url } => {
            let client = Arc::clone(client);
            let tx = tx.clone();
            tokio::spawn(async move {
                match scraper::yt_import_playlist(&client, &url).await {
                    Ok(playlist) => {
                        let _ = tx
                            .send(MsgServer::YouTubePlaylistInfo {
                                id,
                                name: Some(playlist.name),
                                tracks: playlist.tracks,
                                error_message: None,
                            })
                            .await;
                    }
                    Err(error) => {
                        let _ = tx
                            .send(MsgServer::YouTubePlaylistInfo {
                                id,
                                name: None,
                                tracks: Vec::new(),
                                error_message: Some(error.to_string()),
                            })
                            .await;
                    }
                }
            });
        }

        MsgClient::DeleteTracks { ids, collection } => {
            // The ids were chosen in the client's view of a collection, which may since have been
            // switched; deleting them from another collection could remove the wrong tracks.
            let is_open = state
                .collection
                .as_ref()
                .is_some_and(|c| c.path == std::path::Path::new(&collection));
            if !is_open {
                notify(
                    tx,
                    &format!(
                        "Delete failed: {} is no longer the open collection",
                        utils::collection_label(&collection)
                    ),
                    "error",
                )
                .await;
                return Ok(());
            }
            let _repo_guard = lock_current_repo(state).await;
            if let Some(pool) = state.pool() {
                let deleted = db::delete_tracks(pool, &ids).await?;
                let _ = tx.send(MsgServer::ReloadTracks {}).await;
                update_collection_info(state, tx).await;
                if deleted > 0 {
                    schedule_collection_sync(state, tx).await;
                }
            }
        }

        MsgClient::ExportTracksToCollection {
            tracks,
            collection,
            source_collection,
        } => {
            if !tracks.is_empty() {
                let path = std::path::PathBuf::from(&collection);
                // A move deletes the tracks from the currently open collection once they're
                // safely in the target, so refuse anything that isn't a move out of the open
                // collection into a different one — otherwise "already present" tracks would be
                // deleted from their only copy.
                if let Some(source) = source_collection.as_deref() {
                    let is_open = state
                        .collection
                        .as_ref()
                        .is_some_and(|c| c.path == std::path::Path::new(source));
                    if !is_open || std::path::Path::new(source) == path {
                        notify(
                            tx,
                            &format!(
                                "Move failed: can only move from the open collection to a different one ({})",
                                utils::collection_label(source)
                            ),
                            "error",
                        )
                        .await;
                        return Ok(());
                    }
                }
                let target_repo = collection_repo(&path);
                let target_guard = match &target_repo {
                    Some(repo) => Some(git::lock_repository(repo).await),
                    None => None,
                };
                if db::validate_db_schema(&path).await {
                    let export_pool = db::open_pool(&path).await?;

                    let mut new_tracks = Vec::new();
                    let mut already_present = Vec::new();
                    for track in &tracks {
                        match db::track_exists(&export_pool, &track.yt_id).await {
                            Ok(true) => already_present.push(track.display_name()),
                            Ok(false) => new_tracks.push(track.clone()),
                            Err(e) => {
                                notify(tx, &format!("Export failed: {e}"), "error").await;
                                export_pool.close().await;
                                return Ok(());
                            }
                        }
                    }

                    if !new_tracks.is_empty() {
                        if let Err(e) = db::insert_tracks(&export_pool, &new_tracks, true).await {
                            notify(tx, &format!("Export failed: {e}"), "error").await;
                            export_pool.close().await;
                            return Ok(());
                        }
                    }
                    export_pool.close().await;
                    drop(target_guard);
                    if let Some(repo) = target_repo {
                        if !new_tracks.is_empty() {
                            schedule_git_sync(tx, repo, path.clone()).await;
                        }
                    }

                    if source_collection.is_some() {
                        let ids: Vec<String> = tracks.iter().map(|t| t.yt_id.clone()).collect();
                        let _repo_guard = lock_current_repo(state).await;
                        if let Some(pool) = state.pool() {
                            if let Err(e) = db::delete_tracks(pool, &ids).await {
                                notify(
                                    tx,
                                    &format!(
                                        "Copied to {} but failed to remove from the source collection: {e}",
                                        utils::collection_label(&collection)
                                    ),
                                    "error",
                                )
                                .await;
                                return Ok(());
                            }
                            let _ = tx.send(MsgServer::ReloadTracks {}).await;
                            update_collection_info(state, tx).await;
                            schedule_collection_sync(state, tx).await;
                        }
                    }

                    let target_label = utils::collection_label(&collection);
                    let source_label = source_collection.as_deref().map(utils::collection_label);
                    let verb = if source_label.is_some() {
                        "Moved"
                    } else {
                        "Copied"
                    };

                    let mut parts = Vec::new();
                    if !new_tracks.is_empty() {
                        let names = new_tracks
                            .iter()
                            .map(Track::display_name)
                            .collect::<Vec<_>>()
                            .join(", ");
                        parts.push(match source_label {
                            Some(src) => format!("{verb} {names} from {src} to {target_label}"),
                            None => format!("{verb} {names} to {target_label}"),
                        });
                    }
                    if !already_present.is_empty() {
                        let names = already_present.join(", ");
                        parts.push(match source_label {
                            Some(src) => format!(
                                "{names} already in {target_label}, kept existing copy there and removed from {src}"
                            ),
                            None => format!("{names} already in {target_label}, skipped"),
                        });
                    }
                    notify(tx, &parts.join(". "), "info").await;
                } else {
                    notify(
                        tx,
                        &format!("Export failed: {collection} is not a valid collection"),
                        "error",
                    )
                    .await;
                }
            }
        }

        MsgClient::CreateSpotifyPlaylist {
            id,
            name,
            tracks,
            existing_playlist,
        } => {
            let Some(session) = spotify::SpotifySession::load(data_home).await else {
                let _ = tx
                    .send(MsgServer::SpotifyPlaylist {
                        id,
                        url: None,
                        matched: 0,
                        total: tracks.len() as i64,
                        error_message: Some("Connect a Spotify account first".into()),
                    })
                    .await;
                return Ok(());
            };
            let client = Arc::clone(client);
            let tx = tx.clone();
            tokio::spawn(async move {
                let total = tracks.len() as i64;
                let checked = Arc::new(AtomicI64::new(0));
                let matched_count = Arc::new(AtomicI64::new(0));
                let matches: Vec<Option<String>> = futures::stream::iter(tracks)
                    .map(|t| {
                        let client = Arc::clone(&client);
                        let tx = tx.clone();
                        let checked = Arc::clone(&checked);
                        let matched_count = Arc::clone(&matched_count);
                        async move {
                            let uri = if let Some(sp_id) = t.sp_id {
                                Some(format!("spotify:track:{sp_id}"))
                            } else {
                                match spotify::match_track(&client, &t.artist, &t.title, None).await
                                {
                                    Ok(found) => found.map(|sp| sp.uri),
                                    Err(e) => {
                                        tracing::warn!(
                                            "Spotify match failed for {} - {}: {e}",
                                            t.artist,
                                            t.title
                                        );
                                        None
                                    }
                                }
                            };
                            if uri.is_some() {
                                matched_count.fetch_add(1, Ordering::Relaxed);
                            }
                            let checked_n = checked.fetch_add(1, Ordering::Relaxed) + 1;
                            let _ = tx
                                .send(MsgServer::SpotifyPlaylistProgress {
                                    id,
                                    checked: checked_n,
                                    matched: matched_count.load(Ordering::Relaxed),
                                    total,
                                })
                                .await;
                            uri
                        }
                    })
                    .buffer_unordered(5)
                    .collect()
                    .await;
                let uris: Vec<String> = matches.into_iter().flatten().collect();
                let matched = uris.len() as i64;

                let result: anyhow::Result<String> = async {
                    let playlist_uri = match &existing_playlist {
                        Some(existing) => {
                            let playlist_id = spotify::parse_playlist_id(existing)
                                .context("not a valid Spotify playlist link")?;
                            let playlist_uri = format!("spotify:playlist:{playlist_id}");
                            spotify::clear_playlist(&client, &session, &playlist_uri).await?;
                            playlist_uri
                        }
                        None => spotify::create_playlist(&client, &session, &name).await?,
                    };
                    spotify::add_tracks(&client, &session, &playlist_uri, &uris).await?;
                    spotify::playlist_url(&playlist_uri).context("malformed playlist uri")
                }
                .await;

                match result {
                    Ok(url) => {
                        let _ = tx
                            .send(MsgServer::SpotifyPlaylist {
                                id,
                                url: Some(url),
                                matched,
                                total,
                                error_message: None,
                            })
                            .await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(MsgServer::SpotifyPlaylist {
                                id,
                                url: None,
                                matched,
                                total,
                                error_message: Some(format!(
                                    "Failed to create Spotify playlist: {e}"
                                )),
                            })
                            .await;
                    }
                }
            });
        }

        MsgClient::BackfillSpotifyIds { id } => {
            let (Some(pool), Some(col)) = (state.pool.clone(), state.collection.clone()) else {
                let _ = tx
                    .send(MsgServer::SpotifyBackfillComplete {
                        id,
                        matched: 0,
                        total: 0,
                        error_message: Some("No collection open".into()),
                    })
                    .await;
                return Ok(());
            };
            let repo = state.repo.clone();
            let client = Arc::clone(client);
            let tx = tx.clone();
            tokio::spawn(async move {
                let tracks = match db::get_tracks_missing_sp_id(&pool).await {
                    Ok(t) => t,
                    Err(e) => {
                        let _ = tx
                            .send(MsgServer::SpotifyBackfillComplete {
                                id,
                                matched: 0,
                                total: 0,
                                error_message: Some(format!("Spotify backfill failed: {e}")),
                            })
                            .await;
                        return;
                    }
                };
                let total = tracks.len() as i64;
                let checked = Arc::new(AtomicI64::new(0));
                let matched = Arc::new(AtomicI64::new(0));

                futures::stream::iter(tracks)
                    .for_each_concurrent(5, |track| {
                        let client = Arc::clone(&client);
                        let pool = pool.clone();
                        let tx = tx.clone();
                        let checked = Arc::clone(&checked);
                        let matched = Arc::clone(&matched);
                        let write_repo = repo.clone();
                        async move {
                            let artist = track.artists.join(", ");
                            match spotify::match_track(
                                &client,
                                &artist,
                                &track.title,
                                track.mix_name.as_deref(),
                            )
                            .await
                            {
                                Ok(Some(sp)) => {
                                    let _repo_guard = match &write_repo {
                                        Some(repo) => Some(git::lock_repository(repo).await),
                                        None => None,
                                    };
                                    if let Err(e) =
                                        db::update_sp_id(&pool, &track.yt_id, sp.id()).await
                                    {
                                        tracing::warn!(
                                            "Failed to persist sp_id for {}: {e}",
                                            track.yt_id
                                        );
                                    } else {
                                        matched.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => tracing::warn!(
                                    "Spotify match failed for {}: {e}",
                                    track.display_name()
                                ),
                            }
                            let checked_n = checked.fetch_add(1, Ordering::Relaxed) + 1;
                            let _ = tx
                                .send(MsgServer::SpotifyBackfillProgress {
                                    id,
                                    checked: checked_n,
                                    matched: matched.load(Ordering::Relaxed),
                                    total,
                                })
                                .await;
                        }
                    })
                    .await;

                let matched_n = matched.load(Ordering::Relaxed);
                if matched_n > 0 {
                    let _ = tx.send(MsgServer::ReloadTracks {}).await;
                    if let Some(repo) = repo {
                        schedule_git_sync(&tx, repo, col.path).await;
                    }
                }
                let _ = tx
                    .send(MsgServer::SpotifyBackfillComplete {
                        id,
                        matched: matched_n,
                        total,
                        error_message: None,
                    })
                    .await;
            });
        }

        MsgClient::GetYouTubeSession { id } => {
            let browser = youtube::connected_browser(data_home).await;
            let _ = tx.send(MsgServer::YouTubeSession { id, browser }).await;
        }

        MsgClient::ConnectYouTubeSession { id, user_agent } => {
            // Reading the browser's cookie store can block on an OS keychain prompt.
            let tx = tx.clone();
            let client = client.clone();
            let data_home = data_home.to_path_buf();
            tokio::spawn(async move {
                match youtube::connect(&client, &data_home, user_agent).await {
                    Ok(browser) => {
                        let _ = tx.send(MsgServer::YouTubeSession { id, browser }).await;
                    }
                    Err(e) => {
                        let message = format!("Could not connect YouTube: {e}");
                        notify(&tx, &message, "error").await;
                        reply_error(&tx, id, &message).await;
                    }
                }
            });
        }

        MsgClient::DisconnectYouTubeSession { id } => {
            youtube::disconnect(data_home).await?;
            let _ = tx
                .send(MsgServer::YouTubeSession { id, browser: None })
                .await;
        }

        MsgClient::CreateYouTubePlaylist { id, name, yt_ids } => {
            if let Some(task) = state.youtube_playlist_task.take() {
                task.abort();
            }
            let client = Arc::clone(client);
            let tx = tx.clone();
            let data_home = data_home.to_path_buf();
            let handle = tokio::spawn(async move {
                let total = yt_ids.len() as i64;
                let result: anyhow::Result<String> = async {
                    // Reads the login from the browser now, so it may wait on a keychain prompt.
                    let api = youtube::Api::connected(&client, &data_home).await?;
                    let mut batches = yt_ids.chunks(youtube::BATCH_SIZE);
                    let first = batches.next().unwrap_or_default();
                    let playlist_id = api.create_playlist(&name, first).await?;
                    let mut done = first.len() as i64;
                    let progress =
                        |current| MsgServer::YouTubePlaylistProgress { id, current, total };
                    let _ = tx.send(progress(done)).await;
                    for batch in batches {
                        api.add_videos(&playlist_id, batch).await?;
                        done += batch.len() as i64;
                        let _ = tx.send(progress(done)).await;
                    }
                    Ok(youtube::playlist_url(&playlist_id))
                }
                .await;

                let (url, error_message) = match result {
                    Ok(url) => (Some(url), None),
                    Err(e) => (
                        None,
                        Some(format!("Failed to create YouTube playlist: {e:#}")),
                    ),
                };
                let _ = tx
                    .send(MsgServer::YouTubePlaylist {
                        id,
                        url,
                        error_message,
                    })
                    .await;
            });
            state.youtube_playlist_task = Some(handle);
        }

        MsgClient::CancelYouTubePlaylist { id } => {
            if let Some(task) = state.youtube_playlist_task.take() {
                task.abort();
            }
            let _ = tx
                .send(MsgServer::YouTubePlaylist {
                    id,
                    url: None,
                    error_message: None,
                })
                .await;
        }

        MsgClient::CreateYtAnonPlaylist { id, yt_ids } => {
            let client = Arc::clone(client);
            let tx = tx.clone();
            tokio::spawn(async move {
                match scraper::yt_create_anon_playlist(&client, &yt_ids).await {
                    Ok(url) => {
                        let _ = tx.send(MsgServer::YtAnonPlaylist { id, url }).await;
                    }
                    Err(e) => {
                        let message = format!("Failed to create playlist: {e}");
                        reply_error(&tx, id, &message).await;
                        notify(&tx, &message, "error").await;
                    }
                }
            });
        }

        MsgClient::UpdateTrack { old, new } => {
            let _repo_guard = lock_current_repo(state).await;
            if let Some(pool) = state.pool() {
                db::update_track_metadata(pool, &old.yt_id, &new).await?;
                update_collection_info(state, tx).await;
                schedule_collection_sync(state, tx).await;
            }
        }

        MsgClient::CollectionContainsIds { id, yt_ids } => {
            let Some(pool) = state.pool() else {
                reply_error(tx, id, "No collection open").await;
                return Ok(());
            };
            let contained = db::existing_yt_ids(pool, &yt_ids).await?;
            let _ = tx
                .send(MsgServer::CollectionContainsIdsResponse { id, contained })
                .await;
        }

        MsgClient::SearchCancelAll {} => {
            if let Some(task) = state.search_task.take() {
                task.abort();
            }
        }

        MsgClient::Search {
            query,
            query_id,
            limit,
            kind,
            sources,
        } => {
            if let Some(task) = state.search_task.take() {
                task.abort();
            }
            if !query.is_empty() {
                let tx2 = tx.clone();
                let c = client.clone();
                let limit = limit as usize;
                let known_bp_tracks = match kind {
                    SearchKind::Fuzzy => Default::default(),
                    SearchKind::ByLabel | SearchKind::ByArtist => known_bp_tracks(state).await,
                };
                let handle = match kind {
                    SearchKind::Fuzzy => tokio::spawn(scraper::search_fuzzy(
                        c, tx2, query, query_id, limit, sources,
                    )),
                    SearchKind::ByLabel => tokio::spawn(scraper::search_by_label(
                        c,
                        tx2,
                        query,
                        query_id,
                        limit,
                        sources,
                        known_bp_tracks,
                    )),
                    SearchKind::ByArtist => tokio::spawn(scraper::search_by_artist(
                        c,
                        tx2,
                        query,
                        query_id,
                        limit,
                        sources,
                        known_bp_tracks,
                    )),
                };
                state.search_task = Some(handle);
            }
        }

        MsgClient::GetNewReleasesCandidates { id } => {
            let pool = state
                .pool()
                .context("Open a collection to search for new releases")?;
            let candidates = db::get_new_releases_candidates(pool).await?;
            let _ = tx
                .send(MsgServer::NewReleasesCandidates {
                    id,
                    artists: candidates.artists,
                    labels: candidates.labels,
                })
                .await;
        }

        MsgClient::SearchNewReleases {
            query_id,
            days,
            limit,
            sources,
            artists,
            labels,
        } => {
            if let Some(task) = state.search_task.take() {
                task.abort();
            }
            if let Some(pool) = state.pool() {
                let existing_yt_ids = db::get_all_yt_ids(pool).await?;
                if artists.is_empty() && labels.is_empty() {
                    notify(
                        tx,
                        "Select at least one artist or label to search for new releases",
                        "info",
                    )
                    .await;
                    let _ = tx.send(MsgServer::SearchComplete { query_id }).await;
                } else {
                    let tx2 = tx.clone();
                    let c = client.clone();
                    let known_bp_tracks = known_bp_tracks(state).await;
                    let handle = tokio::spawn(scraper::search_new_releases(
                        c,
                        tx2,
                        scraper::NewReleasesQuery {
                            query_id,
                            days,
                            limit: limit as usize,
                            artists,
                            labels,
                            existing_yt_ids,
                            sources,
                            known_bp_tracks,
                        },
                    ));
                    state.search_task = Some(handle);
                }
            } else {
                notify(tx, "Open a collection to search for new releases", "info").await;
                let _ = tx.send(MsgServer::SearchComplete { query_id }).await;
            }
        }

        MsgClient::SearchShazamSimilar {
            query_id,
            track,
            limit,
        } => {
            if let Some(task) = state.search_task.take() {
                task.abort();
            }
            let tx2 = tx.clone();
            let c = client.clone();
            let handle = tokio::spawn(scraper::search_shazam_similar(
                c,
                tx2,
                track,
                query_id,
                limit.max(0) as usize,
            ));
            state.search_task = Some(handle);
        }

        MsgClient::SearchBeatportTop100 {
            query_id,
            genre_id,
            limit,
        } => {
            if let Some(task) = state.search_task.take() {
                task.abort();
            }
            let handle = tokio::spawn(scraper::search_beatport_top100(
                client.clone(),
                tx.clone(),
                genre_id,
                query_id,
                limit.max(0) as usize,
            ));
            state.search_task = Some(handle);
        }

        MsgClient::SearchTraxsourceTop100 { query_id, limit } => {
            if let Some(task) = state.search_task.take() {
                task.abort();
            }
            let handle = tokio::spawn(scraper::search_traxsource_top100(
                client.clone(),
                tx.clone(),
                query_id,
                limit.max(0) as usize,
            ));
            state.search_task = Some(handle);
        }

        MsgClient::FixupTrack { id, track } => {
            let client = Arc::clone(client);
            let tx = tx.clone();
            tokio::spawn(async move {
                let found = match scraper::fixup_track(client, track).await {
                    Ok(found @ Some(_)) => found,
                    Ok(None) => {
                        notify(&tx, "Fixup found no matching track", "info").await;
                        None
                    }
                    Err(e) => {
                        notify(&tx, &format!("Fixup failed: {e}"), "error").await;
                        None
                    }
                };
                let _ = tx
                    .send(MsgServer::FixupTrackReply { id, track: found })
                    .await;
            });
        }

        MsgClient::FixupTracks { id, tracks } => {
            if let Some(task) = state.fixup_task.take() {
                task.abort();
            }
            let client = Arc::clone(client);
            let tx = tx.clone();
            let total = tracks.len() as i64;
            let handle = tokio::spawn(async move {
                let checked = Arc::new(AtomicI64::new(0));
                let matched = Arc::new(AtomicI64::new(0));
                let mut fixed: Vec<(usize, Track)> =
                    futures::stream::iter(tracks.into_iter().enumerate())
                        .map(|(index, track)| {
                            let client = Arc::clone(&client);
                            let tx = tx.clone();
                            let checked = Arc::clone(&checked);
                            let matched = Arc::clone(&matched);
                            async move {
                                let fallback = track.clone();
                                let track = match scraper::fixup_track(client, track).await {
                                    Ok(Some(track)) => {
                                        matched.fetch_add(1, Ordering::Relaxed);
                                        track
                                    }
                                    Ok(None) => fallback,
                                    Err(error) => {
                                        tracing::warn!("Track fixup failed: {error}");
                                        fallback
                                    }
                                };
                                let current = checked.fetch_add(1, Ordering::Relaxed) + 1;
                                let _ = tx
                                    .send(MsgServer::FixupTracksProgress {
                                        id,
                                        current,
                                        matched: matched.load(Ordering::Relaxed),
                                        total,
                                    })
                                    .await;
                                (index, track)
                            }
                        })
                        .buffer_unordered(4)
                        .collect()
                        .await;
                fixed.sort_by_key(|(index, _)| *index);
                let matched = matched.load(Ordering::Relaxed);
                let tracks = fixed.into_iter().map(|(_, track)| track).collect();
                let _ = tx
                    .send(MsgServer::FixupTracksComplete {
                        id,
                        tracks,
                        matched,
                        total,
                    })
                    .await;
            });
            state.fixup_task = Some(handle);
        }

        MsgClient::CancelFixupTracks { id: _ } => {
            if let Some(task) = state.fixup_task.take() {
                task.abort();
            }
        }

        MsgClient::UpdateRating { yt_id, rating } => {
            let _repo_guard = lock_current_repo(state).await;
            if let Some(pool) = state.pool() {
                if db::update_rating(pool, &yt_id, rating).await? > 0 {
                    if let Some(track) = db::get_track(pool, &yt_id).await? {
                        let _ = tx.send(MsgServer::TrackUpdate { track }).await;
                    }
                    schedule_collection_sync(state, tx).await;
                }
            }
        }

        MsgClient::IncPlayCount { yt_id } => {
            let _repo_guard = lock_current_repo(state).await;
            if let Some(pool) = state.pool() {
                let last_played = chrono::Local::now().date_naive();
                // Plays of tracks that aren't in the collection (e.g. search results) are no-ops.
                if db::inc_play_count(pool, &yt_id, last_played).await? > 0 {
                    if let Some(track) = db::get_track(pool, &yt_id).await? {
                        let _ = tx.send(MsgServer::TrackUpdate { track }).await;
                    }
                    schedule_collection_sync(state, tx).await;
                }
            }
        }

        MsgClient::GetRows(content) => {
            let Some(pool) = state.pool() else {
                reply_error(tx, content.id, "No collection open").await;
                return Ok(());
            };
            let n = db::count_rows(pool, &content.filter_model).await?;
            let rows = db::get_rows(
                pool,
                content.start_row,
                content.end_row,
                &content.sort_model,
                &content.filter_model,
            )
            .await?;
            let _ = tx
                .send(MsgServer::Rows {
                    id: content.id,
                    rows,
                    last_row: Some(n),
                })
                .await;
        }

        MsgClient::GetRowIndex(content) => {
            let Some(pool) = state.pool() else {
                reply_error(tx, content.id, "No collection open").await;
                return Ok(());
            };
            let index = db::get_row_index(
                pool,
                &content.yt_id,
                &content.sort_model,
                &content.filter_model,
            )
            .await?;
            let _ = tx
                .send(MsgServer::RowIndex {
                    id: content.id,
                    index,
                })
                .await;
        }

        MsgClient::AddTrack {
            track,
            overwrite_existing,
        } => {
            let _repo_guard = lock_current_repo(state).await;
            if let Some(pool) = state.pool() {
                db::insert_track(pool, &track, !overwrite_existing).await?;
                let _ = tx.send(MsgServer::TrackUpdate { track }).await;
                update_collection_info(state, tx).await;
                schedule_collection_sync(state, tx).await;
            }
        }

        MsgClient::AddTracks {
            tracks,
            overwrite_existing,
        } => {
            let _repo_guard = lock_current_repo(state).await;
            if let Some(pool) = state.pool() {
                db::insert_tracks(pool, &tracks, !overwrite_existing).await?;
                let _ = tx.send(MsgServer::ReloadTracks {}).await;
                update_collection_info(state, tx).await;
                schedule_collection_sync(state, tx).await;
            }
        }

        MsgClient::GetTrackInfo { yt_id } => {
            if let Some(pool) = state.pool() {
                if let Some(track) = db::get_track(pool, &yt_id).await? {
                    let _ = tx.send(MsgServer::TrackInfo { track }).await;
                }
            }
        }

        MsgClient::CreateCollection { name } => {
            let _repo_guard = lock_current_repo(state).await;
            let repo = match state.repo.clone() {
                Some(r) => r,
                None => {
                    notify(tx, "No repo selected", "error").await;
                    return Ok(());
                }
            };
            if name.is_empty() || name.contains('/') || name.contains('\\') {
                notify(tx, "Invalid collection name", "error").await;
                return Ok(());
            }
            let path = repo.join(format!("{name}.db"));
            if path.exists() {
                notify(
                    tx,
                    "Cannot create collection — file already exists",
                    "error",
                )
                .await;
                return Ok(());
            }
            let pool = db::open_pool(&path).await?;
            db::init_db(&pool).await?;
            let col = Collection { path, size: 0 };
            if let Some(old_pool) = state.pool.replace(pool) {
                old_pool.close().await;
            }
            state.collection = Some(col.clone());
            let _ = tx
                .send(MsgServer::Collections {
                    collections: utils::list_repo_collections(&repo),
                })
                .await;
            let _ = tx
                .send(MsgServer::CollectionInfo {
                    id: None,
                    collection: Some(col),
                    error_message: None,
                    path_completions: None,
                })
                .await;
            schedule_collection_sync(state, tx).await;
        }

        MsgClient::DeleteCollection { path } => {
            let _repo_guard = lock_current_repo(state).await;
            let Some(repo) = state.repo.clone() else {
                notify(tx, "No repo selected", "error").await;
                return Ok(());
            };
            let path = std::path::PathBuf::from(&path);
            // Only collections listed for the current repo, never an arbitrary file.
            if !utils::list_repo_collections(&repo)
                .iter()
                .any(|c| c.path == path)
            {
                notify(tx, "Cannot delete: not a collection of this repo", "error").await;
                return Ok(());
            }
            let was_open = state.collection.as_ref().is_some_and(|c| c.path == path);
            if was_open {
                if let Some(pool) = state.pool.take() {
                    pool.close().await;
                }
                state.collection = None;
            }
            let label = path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
            if let Err(e) = utils::remove_collection_file(&path) {
                notify(tx, &format!("Could not delete {label}: {e}"), "error").await;
                return Ok(());
            }
            announce_collection_moved(session_id, path.clone(), None);
            let _ = tx
                .send(MsgServer::Collections {
                    collections: utils::list_repo_collections(&repo),
                })
                .await;
            if was_open {
                let _ = tx
                    .send(MsgServer::CollectionInfo {
                        id: None,
                        collection: None,
                        error_message: None,
                        path_completions: None,
                    })
                    .await;
            }
            notify(tx, &format!("Deleted collection {label}"), "info").await;
            schedule_git_sync(tx, repo, path).await;
        }

        MsgClient::RenameCollection { path, name } => {
            let _repo_guard = lock_current_repo(state).await;
            let Some(repo) = state.repo.clone() else {
                notify(tx, "No repo selected", "error").await;
                return Ok(());
            };
            let path = std::path::PathBuf::from(&path);
            if !utils::list_repo_collections(&repo)
                .iter()
                .any(|c| c.path == path)
            {
                notify(tx, "Cannot rename: not a collection of this repo", "error").await;
                return Ok(());
            }
            let name = name.trim();
            if name.is_empty() || name.contains('/') || name.contains('\\') {
                notify(tx, "Invalid collection name", "error").await;
                return Ok(());
            }
            let new_path = repo.join(format!("{name}.db"));
            if new_path == path {
                return Ok(());
            }
            if new_path.exists() {
                notify(
                    tx,
                    &format!("Cannot rename — a collection named {name} already exists"),
                    "error",
                )
                .await;
                return Ok(());
            }
            let label = path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
            let was_open = state.collection.as_ref().is_some_and(|c| c.path == path);
            if was_open {
                if let Some(pool) = state.pool.take() {
                    pool.close().await;
                }
                state.collection = None;
            }
            let renamed = utils::rename_collection_file(&path, &new_path);
            if let Err(e) = &renamed {
                // The helper puts everything back on failure, so the original is still there.
                notify(tx, &format!("Could not rename {label}: {e}"), "error").await;
            } else {
                announce_collection_moved(session_id, path.clone(), Some(new_path.clone()));
            }
            if was_open {
                let open_path = if renamed.is_ok() { &new_path } else { &path };
                // Validated, so that a missing file is reported rather than created empty.
                match state.set_collection(open_path.clone(), true).await {
                    Ok(true) => {}
                    Ok(false) => {
                        notify(tx, &format!("Could not reopen {label}"), "error").await;
                    }
                    Err(e) => {
                        notify(tx, &format!("Could not reopen {label}: {e:#}"), "error").await;
                    }
                }
                // Sent before the new collection list so that the client follows the rename
                // rather than falling back to another collection when the old path disappears.
                let _ = tx
                    .send(MsgServer::CollectionInfo {
                        id: None,
                        collection: state.collection.clone(),
                        error_message: None,
                        path_completions: None,
                    })
                    .await;
            }
            if renamed.is_err() {
                return Ok(());
            }
            let _ = tx
                .send(MsgServer::Collections {
                    collections: utils::list_repo_collections(&repo),
                })
                .await;
            if was_open {
                send_collection_stats(state, tx).await;
            }
            notify(tx, &format!("Renamed collection {label} to {name}"), "info").await;
            schedule_git_sync(tx, repo, new_path).await;
        }

        MsgClient::SetCollection { id, path } => {
            let p = std::path::PathBuf::from(&path);
            // Don't open a database while an update sync may still be replacing it.
            let repo_guard = match collection_repo(&p) {
                Some(repo) => Some(git::lock_repository(&repo).await),
                None => None,
            };
            let valid = state.set_collection(p.clone(), true).await?;
            drop(repo_guard);
            if valid {
                let _ = tx
                    .send(MsgServer::CollectionInfo {
                        id: Some(id),
                        collection: state.collection.clone(),
                        error_message: None,
                        path_completions: None,
                    })
                    .await;
                send_collection_stats(state, tx).await;
            } else {
                let completions = utils::get_path_completions(&path);
                let _ = tx
                    .send(MsgServer::CollectionInfo {
                        id: Some(id),
                        collection: None,
                        error_message: Some("not a valid collection".into()),
                        path_completions: Some(completions),
                    })
                    .await;
            }
        }

        MsgClient::GetPathCompletions { id, path_prefix } => {
            let paths = utils::get_path_completions(&path_prefix);
            let _ = tx.send(MsgServer::PathCompletions { id, paths }).await;
        }

        MsgClient::ImportFrom {
            path,
            keep_user_data,
        } => match utils::is_file_or_url(&path) {
            None => {
                notify(
                    tx,
                    &format!("Import source {path} is neither file path nor URL"),
                    "error",
                )
                .await;
            }
            Some(PathKind::Url) => {
                // Download off the loop; the import itself runs back on it (it needs the pool).
                let tx = tx.clone();
                let events = events.clone();
                tokio::spawn(async move {
                    match download_collection(&path).await {
                        Ok((dir, db_path)) => {
                            let _ = events
                                .send(SessionEvent::Internal(InternalEvent::ImportDownloaded {
                                    url: path,
                                    db_path,
                                    keep_user_data,
                                    _dir: dir,
                                }))
                                .await;
                        }
                        Err(e) => notify(&tx, &format!("Import failed: {e:#}"), "error").await,
                    }
                });
            }
            Some(PathKind::File) => {
                let _repo_guard = lock_current_repo(state).await;
                if let Some(pool) = state.pool() {
                    let db_path = std::path::PathBuf::from(&path);
                    if !db::validate_db_schema(&db_path).await {
                        notify(tx, &format!("Invalid collection: {path}"), "error").await;
                        return Ok(());
                    }
                    db::import_from_db(pool, &db_path, keep_user_data).await?;
                    update_collection_info(state, tx).await;
                    schedule_collection_sync(state, tx).await;
                }
            }
        },

        MsgClient::CheckImportFrom { path } => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let is_valid = match utils::is_file_or_url(&path) {
                    None => false,
                    Some(PathKind::File) => {
                        db::validate_db_schema(&std::path::PathBuf::from(&path)).await
                    }
                    Some(PathKind::Url) => download_collection(&path).await.is_ok(),
                };
                let _ = tx.send(MsgServer::ImportFromValid { path, is_valid }).await;
            });
        }
    }

    Ok(())
}

/// Downloads a collection into a fresh temp dir and checks that it is a valid collection.
async fn download_collection(url: &str) -> anyhow::Result<(tempfile::TempDir, PathBuf)> {
    let dir = tempfile::tempdir()?;
    let dest = dir.path().join("temp-collection.db");
    utils::download_file(url, &dest)
        .await
        .context("download failed")?;
    anyhow::ensure!(
        db::validate_db_schema(&dest).await,
        "{url} is not a valid collection"
    );
    Ok((dir, dest))
}

/// Runs a sync that may update the repo's working tree (the caller has closed its pool and holds
/// the repo lock), then posts `RepoSynced` so the session re-sends the repo's collections.
fn spawn_update_sync(
    tx: &mpsc::Sender<MsgServer>,
    events: &mpsc::Sender<SessionEvent>,
    guard: tokio::sync::OwnedMutexGuard<()>,
    repo: PathBuf,
    resolution: Option<crate::message::GitConflictStrategy>,
    status: &str,
) -> tokio::task::JoinHandle<()> {
    let tx = tx.clone();
    let events = events.clone();
    let status = status.to_string();
    tokio::spawn(async move {
        send_git_status(
            &tx,
            git::SyncReport {
                state: crate::message::GitSyncState::Syncing,
                message: status,
                retryable: false,
            },
        )
        .await;
        let report =
            git::sync_repository_with_guard(guard, repo.clone(), None, true, resolution).await;
        send_git_status(&tx, report).await;
        let _ = events
            .send(SessionEvent::Internal(InternalEvent::RepoSynced { repo }))
            .await;
    })
}

/// Clones `url` into `repo_path`, or activates an existing clone there. Returns the repo to
/// activate, or `None` (after notifying the client) if there is none.
async fn clone_or_reuse_repo(
    tx: &mpsc::Sender<MsgServer>,
    url: &str,
    repo_name: &str,
    repo_path: PathBuf,
) -> Option<PathBuf> {
    if repo_path.join(".git").exists() {
        // Already a git repo — activate it (may have a different remote)
        let existing = git::get_remote(&repo_path).await;
        if existing.as_deref() != Some(url) {
            if existing.is_none() {
                // No remote set — add the provided URL
                let _ = git::set_remote(&repo_path, Some(url)).await;
            } else {
                notify(
                    tx,
                    &format!("{repo_name}/ already cloned with a different remote — activating it"),
                    "info",
                )
                .await;
            }
        }
    } else if repo_path.exists() {
        notify(
            tx,
            &format!("Directory {repo_name}/ exists but is not a git repo"),
            "error",
        )
        .await;
        return None;
    } else {
        if let Err(e) = git::clone_repo(url, &repo_path).await {
            notify(tx, &format!("Clone failed: {e}"), "error").await;
            return None;
        }
        if !git::check_push_access(&repo_path).await {
            notify(
                tx,
                "No push access to this repo — changes will be stored locally only",
                "warn",
            )
            .await;
        }
    }
    Some(repo_path)
}

#[cfg(test)]
mod websocket_security_tests {
    use super::*;

    fn settings(host: &str, port: u16) -> Settings {
        Settings {
            home: None,
            host: host.to_string(),
            port,
            allowed_origins: Vec::new(),
            ws_token: None,
            verbosity: 0,
            dev: false,
        }
    }

    #[test]
    fn accepts_same_port_loopback_origins() {
        let settings = settings("localhost", 8005);
        assert!(websocket_origin_allowed("http://localhost:8005", &settings));
        assert!(websocket_origin_allowed("http://127.0.0.1:8005", &settings));
        assert!(websocket_origin_allowed("http://[::1]:8005", &settings));
    }

    #[test]
    fn rejects_cross_site_and_wrong_port_origins() {
        let settings = settings("localhost", 8005);
        assert!(!websocket_origin_allowed(
            "https://attacker.example",
            &settings
        ));
        assert!(!websocket_origin_allowed(
            "http://localhost:9999",
            &settings
        ));
        assert!(!websocket_origin_allowed("null", &settings));
    }

    #[test]
    fn accepts_any_loopback_dev_port_only_in_dev_mode() {
        let mut settings = settings("localhost", 8055);
        assert!(!websocket_origin_allowed(
            "http://localhost:5174",
            &settings
        ));
        settings.dev = true;
        assert!(websocket_origin_allowed("http://localhost:5174", &settings));
        assert!(websocket_origin_allowed("http://127.0.0.1:6123", &settings));
        assert!(!websocket_origin_allowed(
            "https://attacker.example",
            &settings
        ));
    }

    #[test]
    fn wildcard_bind_requires_an_explicit_remote_origin() {
        let mut settings = settings("0.0.0.0", 8005);
        assert!(!websocket_origin_allowed(
            "http://stereo.local:8005",
            &settings
        ));
        settings
            .allowed_origins
            .push("https://music.example".to_string());
        assert!(websocket_origin_allowed("https://music.example", &settings));
    }

    #[test]
    fn websocket_token_uses_a_browser_safe_subprotocol() {
        let protocol = websocket_auth_protocol("correct horse");
        assert_eq!(protocol, "stereo.auth.636f727265637420686f727365");

        let mut headers = HeaderMap::new();
        headers.insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            format!("other, {protocol}").parse().unwrap(),
        );
        assert!(websocket_auth_supplied(&headers, &protocol));
        assert!(!websocket_auth_supplied(&headers, "stereo.auth.bad"));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use serde_json::{json, Value};
    use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};

    use super::*;

    type Socket = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    const TIMEOUT: Duration = Duration::from_secs(10);

    /// A home dir with repo `music` holding the given collections (each with track "one"), and
    /// a backend serving it; returns the repo and the port.
    async fn start_backend(home: &Path, collections: &[&str]) -> (PathBuf, u16) {
        let repo = home.join("gitrepos").join("music");
        git::init_if_needed(&repo).await.unwrap();
        for name in collections {
            let pool = db::open_pool(&repo.join(format!("{name}.db")))
                .await
                .unwrap();
            db::init_db(&pool).await.unwrap();
            let track = Track {
                yt_id: "one".into(),
                title: "One".into(),
                artists: vec!["Artist".into()],
                ..Default::default()
            };
            db::insert_track(&pool, &track, false).await.unwrap();
            pool.close().await;
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let settings = Settings {
            home: Some(home.to_path_buf()),
            host: "127.0.0.1".into(),
            port,
            allowed_origins: vec![],
            ws_token: None,
            verbosity: 0,
            dev: false,
        };
        let router = axum::Router::new()
            .route("/ws", axum::routing::get(ws_handler))
            .with_state(Arc::new(settings));
        tokio::spawn(async move { axum::serve(listener, router).await });
        (repo, port)
    }

    struct Client {
        socket: Socket,
        /// Received messages not yet consumed by `expect`.
        pending: VecDeque<Value>,
    }

    impl Client {
        async fn connect(port: u16) -> Self {
            let mut request = format!("ws://127.0.0.1:{port}/ws")
                .into_client_request()
                .unwrap();
            request.headers_mut().insert(
                header::ORIGIN,
                format!("http://127.0.0.1:{port}").parse().unwrap(),
            );
            let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
            Client {
                socket,
                pending: VecDeque::new(),
            }
        }

        async fn send(&mut self, msg: Value) {
            self.socket
                .send(tungstenite::Message::Text(msg.to_string()))
                .await
                .unwrap();
        }

        /// Consumes the first message (received so far or later) that `matches`, leaving the
        /// others for later calls.
        async fn expect(&mut self, what: &str, matches: impl Fn(&Value) -> bool) -> Value {
            let deadline = tokio::time::Instant::now() + TIMEOUT;
            loop {
                if let Some(i) = self.pending.iter().position(&matches) {
                    return self.pending.remove(i).unwrap();
                }
                let frame = tokio::time::timeout_at(deadline, self.socket.next())
                    .await
                    .unwrap_or_else(|_| panic!("timed out waiting for {what}: {:?}", self.pending))
                    .expect("socket closed")
                    .unwrap();
                if let tungstenite::Message::Text(text) = frame {
                    let batch: Vec<Value> = serde_json::from_str(&text).unwrap();
                    self.pending.extend(batch);
                }
            }
        }

        async fn open(&mut self, repo: &Path, collection: &Path) {
            self.send(json!({ "type": "set-repo", "path": repo })).await;
            self.send(json!({ "type": "set-collection", "id": 1, "path": collection }))
                .await;
            self.expect("collection opened", |m| {
                m["type"] == "collection-info"
                    && m["id"] == 1
                    && m["collection"]["path"].is_string()
            })
            .await;
        }

        async fn rate(&mut self, rating: i64) {
            self.send(json!({ "type": "update-rating", "yt_id": "one", "rating": rating }))
                .await;
            self.expect("track update", |m| {
                m["type"] == "track-update" && m["track"]["rating"] == rating
            })
            .await;
        }
    }

    fn is_collection_info(m: &Value, path: Option<&Path>) -> bool {
        m["type"] == "collection-info"
            && m["id"].is_null()
            && match path {
                Some(path) => m["collection"]["path"] == json!(path),
                None => m["collection"].is_null(),
            }
    }

    fn is_collection_list(m: &Value, paths: &[&Path]) -> bool {
        let expected: Vec<Value> = paths
            .iter()
            .map(|p| json!({ "path": p, "size": 0 }))
            .collect();
        m["type"] == "collections" && m["collections"] == json!(expected)
    }

    async fn rating(db_path: &Path) -> Option<i64> {
        let pool = db::open_pool(db_path).await.unwrap();
        let rating = db::get_track(&pool, "one").await.unwrap().unwrap().rating;
        pool.close().await;
        rating
    }

    #[tokio::test]
    async fn other_session_follows_renamed_collection() {
        let home = tempfile::tempdir().unwrap();
        let (repo, port) = start_backend(home.path(), &["old"]).await;
        let (old, new) = (repo.join("old.db"), repo.join("new.db"));
        let mut a = Client::connect(port).await;
        let mut b = Client::connect(port).await;
        a.open(&repo, &old).await;
        b.open(&repo, &old).await;

        a.send(json!({ "type": "rename-collection", "path": old, "name": "new" }))
            .await;
        a.expect("renamed collection", |m| is_collection_info(m, Some(&new)))
            .await;

        b.expect("followed rename", |m| is_collection_info(m, Some(&new)))
            .await;
        b.expect("updated collection list", |m| {
            is_collection_list(m, &[&new])
        })
        .await;
        b.rate(5).await;
        assert_eq!(rating(&new).await, Some(5));
        assert!(!old.exists());
    }

    #[tokio::test]
    async fn other_session_closes_deleted_collection() {
        let home = tempfile::tempdir().unwrap();
        let (repo, port) = start_backend(home.path(), &["gone", "keep"]).await;
        let (gone, keep) = (repo.join("gone.db"), repo.join("keep.db"));
        let mut a = Client::connect(port).await;
        let mut b = Client::connect(port).await;
        a.open(&repo, &gone).await;
        b.open(&repo, &gone).await;

        a.send(json!({ "type": "delete-collection", "path": gone }))
            .await;
        b.expect("closed collection", |m| is_collection_info(m, None))
            .await;
        b.expect("updated collection list", |m| {
            is_collection_list(m, &[&keep])
        })
        .await;
        assert!(!gone.exists());
    }

    #[tokio::test]
    async fn failed_rename_keeps_collection_open() {
        let home = tempfile::tempdir().unwrap();
        let (repo, port) = start_backend(home.path(), &["old"]).await;
        let (old, new) = (repo.join("old.db"), repo.join("new.db"));
        let size = std::fs::metadata(&old).unwrap().len();
        let mut a = Client::connect(port).await;
        a.open(&repo, &old).await;
        // Moving the database succeeds, then moving this "sidecar" fails: directories can't be
        // hard-linked.
        std::fs::create_dir(repo.join("old.db-shm")).unwrap();

        a.send(json!({ "type": "rename-collection", "path": old, "name": "new" }))
            .await;
        a.expect("rename error", |m| {
            m["type"] == "notification"
                && m["kind"] == "error"
                && m["message"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("Could not rename old"))
        })
        .await;
        a.expect("collection still open", |m| {
            is_collection_info(m, Some(&old))
        })
        .await;
        assert_eq!(std::fs::metadata(&old).unwrap().len(), size);
        assert!(!new.exists());
        a.rate(5).await;
        assert_eq!(rating(&old).await, Some(5));
        // Everything the rename sent came before the rating's reply; nothing else failed.
        let errors: Vec<_> = a
            .pending
            .iter()
            .filter(|m| m["type"] == "notification" && m["kind"] == "error")
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    }
}
