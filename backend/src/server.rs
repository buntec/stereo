use std::{sync::Arc, time::Duration};

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::{
    db,
    message::{MsgClient, MsgServer, SearchKind},
    models::{Collection, Track},
    scraper,
    settings::Settings,
    utils::{self, PathKind},
};

struct SessionState {
    pool: Option<sqlx::SqlitePool>,
    collection: Option<Collection>,
    search_task: Option<tokio::task::JoinHandle<()>>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            pool: None,
            collection: None,
            search_task: None,
        }
    }
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

    // Open / init the default collection
    let default_path = home.join("stereo.db");
    let default_collection = match async {
        let pool = db::open_pool(&default_path).await?;
        db::init_db(&pool).await?;
        let n = db::count_rows(&pool, &Default::default()).await?;
        pool.close().await;
        anyhow::Ok(Collection {
            path: default_path,
            size: n,
        })
    }
    .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to initialize default collection: {e}");
            return;
        }
    };

    // Spawn send / recv tasks
    let recv_task = tokio::spawn(recv_loop(ws_stream, client_tx));
    let send_task = tokio::spawn(send_loop(server_rx, ws_sink));

    // Send initial messages
    let _ = server_tx
        .send(MsgServer::BackendInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
        })
        .await;
    let _ = server_tx
        .send(MsgServer::DefaultCollection {
            collection: default_collection,
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

    let mut client_rx = client_rx;
    while let Some(msg) = client_rx.recv().await {
        if let Err(e) =
            handle_msg(&mut state, &settings, &server_tx, &http_client, msg).await
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

async fn update_collection_info(
    state: &mut SessionState,
    tx: &mpsc::Sender<MsgServer>,
) {
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
}

async fn handle_msg(
    state: &mut SessionState,
    settings: &Arc<Settings>,
    tx: &mpsc::Sender<MsgServer>,
    client: &Arc<wreq::Client>,
    msg: MsgClient,
) -> anyhow::Result<()> {
    match msg {
        MsgClient::Heartbeat { timestamp } => {
            let _ = tx.send(MsgServer::Heartbeat { timestamp }).await;
        }

        MsgClient::ValidateTrack { id, track } => {
            let is_valid = serde_json::from_value::<Track>(track).is_ok();
            let _ = tx.send(MsgServer::ValidateTrackReply { id, is_valid }).await;
        }

        MsgClient::DeleteTracks { ids } => {
            if let Some(pool) = state.pool() {
                db::delete_tracks(pool, &ids).await?;
                let _ = tx.send(MsgServer::ReloadTracks {}).await;
                update_collection_info(state, tx).await;
            }
        }

        MsgClient::ExportTracksToCollection { tracks, collection } => {
            if !tracks.is_empty() {
                let path = std::path::PathBuf::from(&collection);
                if db::validate_db_schema(&path).await {
                    let export_pool = db::open_pool(&path).await?;
                    match db::insert_tracks(&export_pool, &tracks, false).await {
                        Ok(()) => {
                            notify(
                                tx,
                                &format!(
                                    "Successfully exported {} track(s) to {collection}",
                                    tracks.len()
                                ),
                                "info",
                            )
                            .await;
                        }
                        Err(e) => {
                            notify(tx, &format!("Export failed: {e}"), "error").await;
                        }
                    }
                    export_pool.close().await;
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
                        c, tx2, query, query_id, limit,
                    )),
                    SearchKind::ByLabel => tokio::spawn(scraper::search_by_label(
                        c, tx2, query, query_id, limit,
                    )),
                    SearchKind::ByArtist => tokio::spawn(scraper::search_by_artist(
                        c, tx2, query, query_id, limit,
                    )),
                };
                state.search_task = Some(handle);
            }
        }

        MsgClient::UpdateRating { yt_id, rating } => {
            if let Some(pool) = state.pool() {
                db::update_rating(pool, &yt_id, rating).await?;
                if let Some(track) = db::get_track(pool, &yt_id).await? {
                    let _ = tx.send(MsgServer::TrackUpdate { track }).await;
                }
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
            }
        }

        MsgClient::GetTrackInfo { yt_id } => {
            if let Some(pool) = state.pool() {
                if let Some(track) = db::get_track(pool, &yt_id).await? {
                    let _ = tx.send(MsgServer::TrackInfo { track }).await;
                }
            }
        }

        MsgClient::CreateCollection { path } => {
            let path = std::path::PathBuf::from(&path);
            if path.exists() {
                notify(tx, "Cannot create collection — file already exists", "error").await;
                return Ok(());
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let pool = db::open_pool(&path).await?;
            db::init_db(&pool).await?;
            let col = Collection {
                path: path.clone(),
                size: 0,
            };
            if let Some(old_pool) = state.pool.replace(pool) {
                old_pool.close().await;
            }
            state.collection = Some(col.clone());
            let _ = tx
                .send(MsgServer::CollectionInfo {
                    id: None,
                    collection: Some(col),
                    error_message: None,
                    path_completions: None,
                })
                .await;
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

        MsgClient::ImportFrom { path, keep_user_data } => {
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
                    return {
                        update_collection_info(state, tx).await;
                        Ok(())
                    };
                } else {
                    std::path::PathBuf::from(&path)
                };

                if !db::validate_db_schema(&db_path).await {
                    notify(tx, &format!("Invalid collection: {path}"), "error").await;
                    return Ok(());
                }
                db::import_from_db(pool, &db_path, keep_user_data).await?;
                update_collection_info(state, tx).await;
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
                            let _ = tx.send(MsgServer::ImportFromValid { path, is_valid: false }).await;
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
