use std::{
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::Context;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::{
    db, git,
    message::{MsgClient, MsgServer, SearchKind},
    models::{Collection, Track},
    scraper,
    settings::Settings,
    spotify,
    utils::{self, PathKind},
    youtube,
};

const GIT_PUSH_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
struct SessionState {
    pool: Option<sqlx::SqlitePool>,
    collection: Option<Collection>,
    repo: Option<std::path::PathBuf>,
    search_task: Option<tokio::task::JoinHandle<()>>,
    youtube_playlist_task: Option<tokio::task::JoinHandle<()>>,
    git_push_handle: Option<tokio::task::JoinHandle<()>>,
    last_git_push: Option<std::time::Instant>,
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
    State(settings): State<Arc<Settings>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, settings))
}

async fn handle_socket(socket: WebSocket, settings: Arc<Settings>) {
    let (ws_sink, ws_stream) = socket.split();
    let uid = uuid::Uuid::new_v4();
    tracing::info!("WS connected: {uid}");

    let (server_tx, server_rx) = mpsc::channel::<MsgServer>(10_000);
    let (client_tx, client_rx) = mpsc::channel::<MsgClient>(10_000);

    let home = settings.home_dir();
    let repos_dir = home.join("gitrepos");
    if let Err(e) = tokio::fs::create_dir_all(&repos_dir).await {
        tracing::warn!("Failed to create gitrepos dir: {e}");
    }

    // Spawn send / recv tasks
    let recv_task = tokio::spawn(recv_loop(ws_stream, client_tx));
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
    {
        let session = youtube::YouTubeSession::load(&home).await;
        let connected = session
            .as_ref()
            .is_some_and(youtube::YouTubeSession::connected);
        let client_id = session.map(|s| s.client_id);
        let _ = server_tx
            .send(MsgServer::YouTubeSession {
                id: 0,
                connected,
                client_id,
            })
            .await;
    }

    let mut state = SessionState::default();
    let http_client = Arc::new(match scraper::create_client() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to create HTTP client: {e}");
            return;
        }
    });

    let mut client_rx = client_rx;
    while let Some(msg) = client_rx.recv().await {
        if let Err(e) =
            handle_msg(&mut state, &server_tx, &http_client, &home, &repos_dir, msg).await
        {
            tracing::error!("Message handling error: {e}");
        }
    }

    // WebSocket disconnected
    if let Some(task) = state.search_task.take() {
        task.abort();
    }
    if let Some(pool) = state.pool.take() {
        pool.close().await;
    }
    recv_task.abort();
    send_task.abort();
    tracing::info!("WS disconnected: {uid}");
}

async fn recv_loop(
    mut stream: futures::stream::SplitStream<WebSocket>,
    tx: mpsc::Sender<MsgClient>,
) {
    while let Some(result) = stream.next().await {
        match result {
            Ok(Message::Text(text)) => {
                tracing::debug!("recv: {}", &text[..text.len().min(500)]);
                match serde_json::from_str::<MsgClient>(&text) {
                    Ok(msg) => {
                        if tx.send(msg).await.is_err() {
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
                    tracing::debug!("send: {}", &json[..json.len().min(500)]);
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
fn union_names(
    a: Vec<crate::models::ScoredName>,
    b: Vec<crate::models::ScoredName>,
) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    a.into_iter()
        .chain(b)
        .filter_map(|s| seen.insert(s.name.clone()).then_some(s.name))
        .collect()
}

fn git_spawn(state: &mut SessionState, _home: &std::path::Path) {
    if let (Some(col), Some(repo)) = (&state.collection, &state.repo) {
        // If a push is already pending it will capture any changes made since it was scheduled.
        if let Some(h) = &state.git_push_handle {
            if !h.is_finished() {
                return;
            }
        }

        let repo = repo.clone();
        let db_path = col.path.clone();
        let delay = state
            .last_git_push
            .map(|last| {
                let elapsed = last.elapsed();
                if elapsed >= GIT_PUSH_INTERVAL {
                    Duration::ZERO
                } else {
                    GIT_PUSH_INTERVAL - elapsed
                }
            })
            .unwrap_or(Duration::ZERO);

        state.last_git_push = Some(std::time::Instant::now());
        state.git_push_handle = Some(tokio::spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            git::commit_and_push(repo, db_path).await;
        }));
    }
}

async fn handle_msg(
    state: &mut SessionState,
    tx: &mpsc::Sender<MsgServer>,
    client: &Arc<wreq::Client>,
    data_home: &std::path::Path,
    home: &std::path::Path,
    msg: MsgClient,
) -> anyhow::Result<()> {
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
            if let Some(old_pool) = state.pool.take() {
                old_pool.close().await;
            }
            state.collection = None;
            state.repo = Some(repo_path.clone());
            let _ = tx
                .send(MsgServer::Collections {
                    collections: utils::list_repo_collections(&repo_path),
                })
                .await;
            let _ = tx
                .send(MsgServer::GitRemote {
                    url: git::get_remote(&repo_path).await,
                })
                .await;
        }

        MsgClient::CloneRepo { url } => {
            let repo_name = git::repo_name_from_url(&url);
            let repo_path = home.join(&repo_name);

            if repo_path.join(".git").exists() {
                // Already a git repo — activate it (may have a different remote)
                let existing = git::get_remote(&repo_path).await;
                if existing.as_deref() != Some(&url) {
                    if existing.is_none() {
                        // No remote set — add the provided URL
                        let _ = git::set_remote(&repo_path, Some(&url)).await;
                    } else {
                        notify(
                            tx,
                            &format!(
                                "{repo_name}/ already cloned with a different remote — activating it"
                            ),
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
                return Ok(());
            } else {
                if let Err(e) = git::clone_repo(&url, &repo_path).await {
                    notify(tx, &format!("Clone failed: {e}"), "error").await;
                    return Ok(());
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

            if let Some(old_pool) = state.pool.take() {
                old_pool.close().await;
            }
            state.collection = None;
            state.repo = Some(repo_path.clone());
            let _ = tx
                .send(MsgServer::Collections {
                    collections: utils::list_repo_collections(&repo_path),
                })
                .await;
            let _ = tx
                .send(MsgServer::GitRemote {
                    url: git::get_remote(&repo_path).await,
                })
                .await;
            let _ = tx
                .send(MsgServer::Repos {
                    repos: utils::list_repos(home).await,
                })
                .await;
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
                }
                Err(e) => notify(tx, &format!("Failed to set git remote: {e}"), "error").await,
            }
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

        MsgClient::DeleteTracks { ids } => {
            if let Some(pool) = state.pool() {
                db::delete_tracks(pool, &ids).await?;
                let _ = tx.send(MsgServer::ReloadTracks {}).await;
                update_collection_info(state, tx).await;
                git_spawn(state, home);
            }
        }

        MsgClient::ExportTracksToCollection {
            tracks,
            collection,
            source_collection,
        } => {
            if !tracks.is_empty() {
                let path = std::path::PathBuf::from(&collection);
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

        MsgClient::CreateSpotifyPlaylist { id, name, tracks } => {
            let Some(session) = spotify::SpotifySession::load(data_home).await else {
                notify(tx, "Connect a Spotify account first", "error").await;
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
                    let playlist_uri = spotify::create_playlist(&client, &session, &name).await?;
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
                            })
                            .await;
                    }
                    Err(e) => {
                        notify(
                            &tx,
                            &format!("Failed to create Spotify playlist: {e}"),
                            "error",
                        )
                        .await;
                    }
                }
            });
        }

        MsgClient::BackfillSpotifyIds { id } => {
            let (Some(pool), Some(col)) = (state.pool.clone(), state.collection.clone()) else {
                notify(tx, "No collection open", "error").await;
                return Ok(());
            };
            let repo = state.repo.clone();
            let client = Arc::clone(client);
            let tx = tx.clone();
            tokio::spawn(async move {
                let tracks = match db::get_tracks_missing_sp_id(&pool).await {
                    Ok(t) => t,
                    Err(e) => {
                        notify(&tx, &format!("Spotify backfill failed: {e}"), "error").await;
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
                        git::commit_and_push(repo, col.path).await;
                    }
                }
                let _ = tx
                    .send(MsgServer::SpotifyBackfillComplete {
                        id,
                        matched: matched_n,
                        total,
                    })
                    .await;
            });
        }

        MsgClient::GetYouTubeSession { id } => {
            let session = youtube::YouTubeSession::load(data_home).await;
            let connected = session
                .as_ref()
                .is_some_and(youtube::YouTubeSession::connected);
            let client_id = session.map(|s| s.client_id);
            let _ = tx
                .send(MsgServer::YouTubeSession {
                    id,
                    connected,
                    client_id,
                })
                .await;
        }

        MsgClient::SetYouTubeCredentials {
            id,
            client_id,
            client_secret,
        } => match youtube::save_credentials(data_home, &client_id, &client_secret).await {
            Ok(session) => {
                let _ = tx
                    .send(MsgServer::YouTubeSession {
                        id,
                        connected: session.connected(),
                        client_id: Some(session.client_id),
                    })
                    .await;
            }
            Err(e) => {
                notify(
                    tx,
                    &format!("Failed to save YouTube credentials: {e}"),
                    "error",
                )
                .await
            }
        },

        MsgClient::DisconnectYouTubeSession { id } => {
            match youtube::YouTubeSession::clear(data_home).await {
                Ok(()) => {
                    let _ = tx
                        .send(MsgServer::YouTubeSession {
                            id,
                            connected: false,
                            client_id: None,
                        })
                        .await;
                }
                Err(e) => {
                    notify(
                        tx,
                        &format!("Failed to disconnect YouTube session: {e}"),
                        "error",
                    )
                    .await
                }
            }
        }

        MsgClient::ExchangeYouTubeCode {
            id,
            code,
            verifier,
            redirect_uri,
        } => {
            let client = Arc::clone(client);
            let tx = tx.clone();
            let data_home = data_home.to_path_buf();
            tokio::spawn(async move {
                match youtube::exchange_code(&client, &data_home, &code, &verifier, &redirect_uri)
                    .await
                {
                    Ok(session) => {
                        let _ = tx
                            .send(MsgServer::YouTubeSession {
                                id,
                                connected: session.connected(),
                                client_id: Some(session.client_id),
                            })
                            .await;
                    }
                    Err(e) => notify(&tx, &format!("YouTube sign-in failed: {e}"), "error").await,
                }
            });
        }

        MsgClient::CreateYouTubePlaylist { id, name, yt_ids } => {
            if let Some(task) = state.youtube_playlist_task.take() {
                task.abort();
            }
            let client = Arc::clone(client);
            let tx = tx.clone();
            let data_home = data_home.to_path_buf();
            let handle = tokio::spawn(async move {
                let access_token = match youtube::ensure_access_token(&client, &data_home).await {
                    Ok(t) => t,
                    Err(e) => {
                        notify(&tx, &format!("{e}"), "error").await;
                        return;
                    }
                };

                let total = yt_ids.len() as i64;
                let result: anyhow::Result<String> = async {
                    let (playlist_id, url) =
                        youtube::create_playlist(&client, &access_token, &name).await?;
                    for (i, yt_id) in yt_ids.iter().enumerate() {
                        youtube::add_video(&client, &access_token, &playlist_id, yt_id).await?;
                        let _ = tx
                            .send(MsgServer::YouTubePlaylistProgress {
                                id,
                                current: i as i64 + 1,
                                total,
                            })
                            .await;
                    }
                    Ok(url)
                }
                .await;

                match result {
                    Ok(url) => {
                        let _ = tx
                            .send(MsgServer::YouTubePlaylist { id, url: Some(url) })
                            .await;
                    }
                    Err(e) => {
                        notify(
                            &tx,
                            &format!("Failed to create YouTube playlist: {e}"),
                            "error",
                        )
                        .await;
                        let _ = tx.send(MsgServer::YouTubePlaylist { id, url: None }).await;
                    }
                }
            });
            state.youtube_playlist_task = Some(handle);
        }

        MsgClient::CancelYouTubePlaylist { id } => {
            if let Some(task) = state.youtube_playlist_task.take() {
                task.abort();
            }
            let _ = tx.send(MsgServer::YouTubePlaylist { id, url: None }).await;
        }

        MsgClient::CreateYtAnonPlaylist { id, yt_ids } => {
            match scraper::yt_create_anon_playlist(client, &yt_ids).await {
                Ok(url) => {
                    let _ = tx.send(MsgServer::YtAnonPlaylist { id, url }).await;
                }
                Err(e) => notify(tx, &format!("Failed to create playlist: {e}"), "error").await,
            }
        }

        MsgClient::UpdateTrack { old, new } => {
            if let Some(pool) = state.pool() {
                db::insert_track(pool, &new, false).await?;
                if old.yt_id != new.yt_id {
                    db::delete_track(pool, &old.yt_id).await?;
                }
                update_collection_info(state, tx).await;
                git_spawn(state, home);
            }
        }

        MsgClient::CollectionContainsId { id, yt_id } => {
            if let Some(pool) = state.pool() {
                let contains_id = db::track_exists(pool, &yt_id).await?;
                let _ = tx
                    .send(MsgServer::CollectionContainsIdResponse { id, contains_id })
                    .await;
            }
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
                let handle = match kind {
                    SearchKind::Fuzzy => tokio::spawn(scraper::search_fuzzy(
                        c, tx2, query, query_id, limit, sources,
                    )),
                    SearchKind::ByLabel => tokio::spawn(scraper::search_by_label(
                        c, tx2, query, query_id, limit, sources,
                    )),
                    SearchKind::ByArtist => tokio::spawn(scraper::search_by_artist(
                        c, tx2, query, query_id, limit, sources,
                    )),
                };
                state.search_task = Some(handle);
            }
        }

        MsgClient::SearchNewReleases {
            query_id,
            days,
            limit,
            sources,
        } => {
            if let Some(task) = state.search_task.take() {
                task.abort();
            }
            if let Some(pool) = state.pool() {
                let stats = db::get_collection_stats(pool).await?;
                let artists = union_names(stats.top_artists_five_star, stats.top_artists_four_plus);
                let labels: Vec<String> = stats
                    .top_labels_four_plus
                    .into_iter()
                    .map(|s| s.name)
                    .collect();
                let existing_yt_ids = db::get_all_yt_ids(pool).await?;
                if artists.is_empty() && labels.is_empty() {
                    notify(
                        tx,
                        "No rated tracks yet to base a new-releases search on",
                        "info",
                    )
                    .await;
                    let _ = tx.send(MsgServer::SearchComplete { query_id }).await;
                } else {
                    let tx2 = tx.clone();
                    let c = client.clone();
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
                        },
                    ));
                    state.search_task = Some(handle);
                }
            }
        }

        MsgClient::FixupTrack { id, track } => {
            match scraper::fixup_track(client.clone(), track).await {
                Ok(found @ Some(_)) => {
                    let _ = tx
                        .send(MsgServer::FixupTrackReply { id, track: found })
                        .await;
                }
                Ok(None) => {
                    notify(tx, "Fixup found no matching track", "info").await;
                    let _ = tx
                        .send(MsgServer::FixupTrackReply { id, track: None })
                        .await;
                }
                Err(e) => {
                    notify(tx, &format!("Fixup failed: {e}"), "error").await;
                    let _ = tx
                        .send(MsgServer::FixupTrackReply { id, track: None })
                        .await;
                }
            }
        }

        MsgClient::UpdateRating { yt_id, rating } => {
            if let Some(pool) = state.pool() {
                db::update_rating(pool, &yt_id, rating).await?;
                if let Some(track) = db::get_track(pool, &yt_id).await? {
                    let _ = tx.send(MsgServer::TrackUpdate { track }).await;
                }
                git_spawn(state, home);
            }
        }

        MsgClient::IncPlayCount { yt_id } => {
            if let Some(pool) = state.pool() {
                if let Some(mut track) = db::get_track(pool, &yt_id).await? {
                    track.play_count += 1;
                    track.last_played = Some(chrono::Local::now().date_naive());
                    db::insert_track(pool, &track, false).await?;
                    let _ = tx.send(MsgServer::TrackUpdate { track }).await;
                }
                git_spawn(state, home);
            }
        }

        MsgClient::GetRows(content) => {
            if let Some(pool) = state.pool() {
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
        }

        MsgClient::GetRowIndex(content) => {
            if let Some(pool) = state.pool() {
                if !content.sort_model.is_empty() {
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
            }
        }

        MsgClient::AddTrack {
            track,
            overwrite_existing,
        } => {
            if let Some(pool) = state.pool() {
                db::insert_track(pool, &track, !overwrite_existing).await?;
                let _ = tx.send(MsgServer::TrackUpdate { track }).await;
                update_collection_info(state, tx).await;
                git_spawn(state, home);
            }
        }

        MsgClient::AddTracks {
            tracks,
            overwrite_existing,
        } => {
            if let Some(pool) = state.pool() {
                db::insert_tracks(pool, &tracks, !overwrite_existing).await?;
                let _ = tx.send(MsgServer::ReloadTracks {}).await;
                update_collection_info(state, tx).await;
                git_spawn(state, home);
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
            git_spawn(state, home);
        }

        MsgClient::SetCollection { id, path } => {
            let p = std::path::PathBuf::from(&path);
            let valid = state.set_collection(p.clone(), true).await?;
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
        } => {
            let src = match utils::is_file_or_url(&path) {
                Some(k) => k,
                None => {
                    notify(
                        tx,
                        &format!("Import source {path} is neither file path nor URL"),
                        "error",
                    )
                    .await;
                    return Ok(());
                }
            };

            if let Some(pool) = state.pool() {
                let db_path = if src == PathKind::Url {
                    let tmp = tempfile::tempdir()?;
                    let dest = tmp.path().join("temp-collection.db");
                    if let Err(e) = utils::download_file(&path, &dest).await {
                        notify(tx, &format!("Download failed: {e}"), "error").await;
                        return Ok(());
                    }
                    // keep tmp alive until we're done
                    let result = if db::validate_db_schema(&dest).await {
                        db::import_from_db(pool, &dest, keep_user_data).await
                    } else {
                        notify(tx, &format!("Invalid collection at {path}"), "error").await;
                        return Ok(());
                    };
                    drop(tmp);
                    result?;
                    update_collection_info(state, tx).await;
                    git_spawn(state, home);
                    return Ok(());
                } else {
                    std::path::PathBuf::from(&path)
                };

                if !db::validate_db_schema(&db_path).await {
                    notify(tx, &format!("Invalid collection: {path}"), "error").await;
                    return Ok(());
                }
                db::import_from_db(pool, &db_path, keep_user_data).await?;
                update_collection_info(state, tx).await;
                git_spawn(state, home);
            }
        }

        MsgClient::CheckImportFrom { path } => {
            let is_valid = match utils::is_file_or_url(&path) {
                None => false,
                Some(PathKind::File) => {
                    db::validate_db_schema(&std::path::PathBuf::from(&path)).await
                }
                Some(PathKind::Url) => {
                    let tmp = match tempfile::tempdir() {
                        Ok(t) => t,
                        Err(_) => {
                            let _ = tx
                                .send(MsgServer::ImportFromValid {
                                    path,
                                    is_valid: false,
                                })
                                .await;
                            return Ok(());
                        }
                    };
                    let dest = tmp.path().join("temp-collection.db");
                    match utils::download_file(&path, &dest).await {
                        Ok(()) => db::validate_db_schema(&dest).await,
                        Err(_) => false,
                    }
                }
            };
            let _ = tx.send(MsgServer::ImportFromValid { path, is_valid }).await;
        }
    }

    Ok(())
}
