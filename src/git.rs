use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    process::Output,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use anyhow::Context;
use serde_json::Value;
use sqlx::{sqlite::SqliteConnectOptions, Connection};
use tokio::{
    process::Command,
    sync::{Mutex as AsyncMutex, OwnedMutexGuard},
};

use crate::{
    db,
    message::{GitConflictStrategy, GitSyncState},
    models::Track,
};

const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(60);
const DIRTY_FILE: &str = "stereo-dirty";
const MERGE_MARKER: &str = "stereo-merge-in-progress";
const NEEDS_SYNC_FILE: &str = "stereo-needs-sync";

/// Guards a repo's working tree and index: held by anything that writes collection databases or
/// runs Git commands that touch tracked files.
static REPO_LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Serializes a repo's network phase (fetch, merge with upstream, push). Background syncs drop
/// the repo lock before taking this one, so database writes never wait on the network.
static NETWORK_LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static RETRY_WORKERS: LazyLock<Mutex<HashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Debug, Clone)]
pub struct SyncReport {
    pub state: GitSyncState,
    pub message: String,
    pub retryable: bool,
}

impl SyncReport {
    fn new(state: GitSyncState, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            state,
            message: message.into(),
            retryable,
        }
    }
}

#[derive(Debug)]
struct GitFailure {
    operation: String,
    detail: String,
    timeout: bool,
}

impl std::fmt::Display for GitFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.timeout {
            write!(f, "{} timed out", self.operation)
        } else if self.detail.is_empty() {
            write!(f, "{} failed", self.operation)
        } else {
            write!(f, "{} failed: {}", self.operation, self.detail)
        }
    }
}

impl std::error::Error for GitFailure {}

fn lock_for(
    registry: &Mutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>>,
    repo: &Path,
) -> Arc<AsyncMutex<()>> {
    let key = std::fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
    let mut locks = registry.lock().expect("lock registry poisoned");
    locks
        .entry(key)
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

fn repo_lock(repo: &Path) -> Arc<AsyncMutex<()>> {
    lock_for(&REPO_LOCKS, repo)
}

fn network_lock(repo: &Path) -> Arc<AsyncMutex<()>> {
    lock_for(&NETWORK_LOCKS, repo)
}

async fn run_git(repo: &Path, args: &[&str], timeout: Duration) -> Result<Output, GitFailure> {
    let operation = format!("git {}", args.join(" "));
    let result = tokio::time::timeout(
        timeout,
        Command::new("git")
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "Never")
            .env(
                "GIT_SSH_COMMAND",
                "ssh -o BatchMode=yes -o ConnectTimeout=15",
            )
            .current_dir(repo)
            .output(),
    )
    .await;
    match result {
        Err(_) => Err(GitFailure {
            operation,
            detail: String::new(),
            timeout: true,
        }),
        Ok(Err(e)) => Err(GitFailure {
            operation,
            detail: e.to_string(),
            timeout: false,
        }),
        Ok(Ok(out)) if out.status.success() => Ok(out),
        Ok(Ok(out)) => Err(GitFailure {
            operation,
            detail: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            timeout: false,
        }),
    }
}

async fn git_ok(repo: &Path, args: &[&str]) -> bool {
    run_git(repo, args, GIT_TIMEOUT).await.is_ok()
}

fn output_text(out: Output) -> String {
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn failure_report(error: &GitFailure) -> SyncReport {
    let detail = error.detail.to_lowercase();
    if error.timeout
        || detail.contains("could not resolve host")
        || detail.contains("failed to connect")
        || detail.contains("network is unreachable")
        || detail.contains("connection timed out")
    {
        SyncReport::new(
            GitSyncState::Offline,
            format!("Repository sync is offline: {error}"),
            true,
        )
    } else if detail.contains("authentication failed")
        || detail.contains("permission denied")
        || detail.contains("could not read username")
        || detail.contains("repository not found")
    {
        SyncReport::new(
            GitSyncState::AuthenticationRequired,
            format!("Repository authentication is required: {error}"),
            true,
        )
    } else {
        SyncReport::new(GitSyncState::Error, error.to_string(), true)
    }
}

pub async fn get_remote(home: &Path) -> Option<String> {
    run_git(home, &["remote", "get-url", "origin"], GIT_TIMEOUT)
        .await
        .ok()
        .map(output_text)
}

pub async fn set_remote(home: &Path, url: Option<&str>) -> anyhow::Result<()> {
    let lock = repo_lock(home);
    let _guard = lock.lock().await;
    let existing = get_remote(home).await;
    match (existing.as_deref(), url.filter(|u| !u.is_empty())) {
        (_, None) => {
            if existing.is_some() {
                run_git(home, &["remote", "remove", "origin"], GIT_TIMEOUT).await?;
            }
        }
        (Some(_), Some(url)) => {
            run_git(home, &["remote", "set-url", "origin", url], GIT_TIMEOUT).await?;
        }
        (None, Some(url)) => {
            run_git(home, &["remote", "add", "origin", url], GIT_TIMEOUT).await?;
        }
    }
    Ok(())
}

pub fn repo_name_from_url(url: &str) -> String {
    url.trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("repo")
        .to_string()
}

pub async fn clone_repo(url: &str, dest: &Path) -> anyhow::Result<()> {
    let parent = dest.parent().context("clone destination has no parent")?;
    tokio::fs::create_dir_all(parent).await?;
    let temp = tempfile::Builder::new()
        .prefix(".stereo-clone-")
        .tempdir_in(parent)?;
    let temp_path = temp.path().to_string_lossy().to_string();
    run_git(parent, &["clone", "--", url, &temp_path], NETWORK_TIMEOUT).await?;
    let _ = temp.keep();
    tokio::fs::rename(&temp_path, dest).await?;
    Ok(())
}

pub async fn check_push_access(dir: &Path) -> bool {
    run_git(
        dir,
        &["push", "--dry-run", "origin", "HEAD"],
        Duration::from_secs(15),
    )
    .await
    .is_ok()
}

pub async fn init_if_needed(home: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(home)?;
    if home.join(".git").exists() {
        return Ok(());
    }
    run_git(home, &["init"], GIT_TIMEOUT).await?;
    tokio::fs::write(home.join(".gitignore"), "*\n!.gitignore\n!*.db\n").await?;
    tracing::info!("Initialized git repo in {}", home.display());
    Ok(())
}

fn dirty_path(repo: &Path) -> PathBuf {
    repo.join(".git").join(DIRTY_FILE)
}

fn read_dirty(repo: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(dirty_path(repo))
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

async fn write_dirty(repo: &Path, dirty: &BTreeSet<String>) -> anyhow::Result<()> {
    let path = dirty_path(repo);
    if dirty.is_empty() {
        if path.exists() {
            tokio::fs::remove_file(path).await?;
        }
    } else {
        let temp = path.with_extension("tmp");
        let contents = dirty.iter().cloned().collect::<Vec<_>>().join("\n") + "\n";
        tokio::fs::write(&temp, contents).await?;
        tokio::fs::rename(temp, path).await?;
    }
    Ok(())
}

async fn mark_needs_sync(repo: &Path) -> anyhow::Result<()> {
    tokio::fs::write(repo.join(".git").join(NEEDS_SYNC_FILE), b"pending\n").await?;
    Ok(())
}

async fn clear_needs_sync(repo: &Path) {
    let path = repo.join(".git").join(NEEDS_SYNC_FILE);
    if path.exists() {
        let _ = tokio::fs::remove_file(path).await;
    }
}

async fn quick_check(path: &Path) -> anyhow::Result<()> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true);
    let mut conn = sqlx::SqliteConnection::connect_with(&opts).await?;
    let result: String = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(&mut conn)
        .await?;
    anyhow::ensure!(result == "ok", "SQLite integrity check failed: {result}");
    Ok(())
}

/// Checkpoint WAL if necessary, then hold a SQLite read transaction while Git reads the database.
/// In rollback-journal mode this prevents another SQLite connection from changing database pages
/// during `git add`.
async fn stage_consistent_database(repo: &Path, db_name: &str) -> anyhow::Result<()> {
    let path = repo.join(db_name);
    quick_check(&path).await?;
    let opts = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(false)
        .busy_timeout(Duration::from_secs(5));
    let mut conn = sqlx::SqliteConnection::connect_with(&opts).await?;
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode=DELETE")
        .fetch_one(&mut conn)
        .await?;
    anyhow::ensure!(
        journal_mode.eq_ignore_ascii_case("delete"),
        "could not checkpoint collection before snapshot"
    );
    sqlx::query("BEGIN").execute(&mut conn).await?;
    let _: i64 = sqlx::query_scalar("SELECT count(*) FROM tracks")
        .fetch_one(&mut conn)
        .await?;
    let staged = run_git(repo, &["add", "--", db_name], GIT_TIMEOUT).await;
    let _ = sqlx::query("ROLLBACK").execute(&mut conn).await;
    staged?;
    Ok(())
}

async fn staged_paths(repo: &Path) -> Result<BTreeSet<String>, GitFailure> {
    let out = run_git(
        repo,
        &["diff", "--cached", "--name-only", "-z"],
        GIT_TIMEOUT,
    )
    .await?;
    Ok(out
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).to_string())
        .collect())
}

async fn discover_dirty_databases(repo: &Path) -> Result<BTreeSet<String>, GitFailure> {
    let out = run_git(
        repo,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            "*.db",
        ],
        GIT_TIMEOUT,
    )
    .await?;
    Ok(out
        .stdout
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.get(3..))
        .filter(|path| path.ends_with(b".db"))
        .map(|path| String::from_utf8_lossy(path).to_string())
        .collect())
}

async fn commit_dirty(repo: &Path, dirty: &mut BTreeSet<String>) -> anyhow::Result<()> {
    if dirty.is_empty() {
        return Ok(());
    }
    let already_staged = staged_paths(repo).await?;
    if !already_staged.is_empty() && !already_staged.is_subset(dirty) {
        anyhow::bail!(
            "repository has staged changes not created by Stereo: {}",
            already_staged.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    let names: Vec<String> = dirty.iter().cloned().collect();
    for name in &names {
        if repo.join(name).exists() {
            stage_consistent_database(repo, name).await?;
        } else {
            run_git(repo, &["add", "-u", "--", name], GIT_TIMEOUT).await?;
        }
    }
    if staged_paths(repo).await?.is_empty() {
        dirty.clear();
        write_dirty(repo, dirty).await?;
        return Ok(());
    }
    let message = if names.len() == 1 {
        format!("update {}", names[0])
    } else {
        format!("update {} collections", names.len())
    };
    run_git(
        repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=Stereo",
            "-c",
            "user.email=stereo@localhost",
            "commit",
            "-m",
            &message,
        ],
        GIT_TIMEOUT,
    )
    .await?;
    mark_needs_sync(repo).await?;
    dirty.clear();
    write_dirty(repo, dirty).await?;
    Ok(())
}

async fn current_branch(repo: &Path) -> Result<String, GitFailure> {
    run_git(repo, &["symbolic-ref", "--short", "HEAD"], GIT_TIMEOUT)
        .await
        .map(output_text)
}

async fn is_ancestor(repo: &Path, older: &str, newer: &str) -> bool {
    git_ok(repo, &["merge-base", "--is-ancestor", older, newer]).await
}

async fn changed_paths(repo: &Path, range: &str) -> Result<BTreeSet<String>, GitFailure> {
    let out = run_git(repo, &["diff", "--name-only", "-z", range], GIT_TIMEOUT).await?;
    Ok(out
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).to_string())
        .collect())
}

async fn tree_paths(repo: &Path, revision: &str) -> Result<BTreeSet<String>, GitFailure> {
    let out = run_git(
        repo,
        &["ls-tree", "-r", "--name-only", "-z", revision],
        GIT_TIMEOUT,
    )
    .await?;
    Ok(out
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).to_string())
        .collect())
}

async fn validate_changed_databases(repo: &Path, paths: &BTreeSet<String>) -> anyhow::Result<()> {
    for path in paths.iter().filter(|p| p.ends_with(".db")) {
        let full = repo.join(path);
        if full.exists() {
            quick_check(&full).await?;
            anyhow::ensure!(
                db::validate_db_schema(&full).await,
                "invalid collection: {path}"
            );
        }
    }
    Ok(())
}

async fn validate_revision_databases(
    repo: &Path,
    revision: &str,
    paths: &BTreeSet<String>,
) -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    for path in paths.iter().filter(|path| path.ends_with(".db")) {
        if let Some(bytes) = show_blob(repo, revision, path).await? {
            let copy = temp.path().join(path.replace('/', "__"));
            write_blob(&copy, &bytes).await?;
            anyhow::ensure!(
                db::validate_db_schema(&copy).await,
                "invalid collection: {path}"
            );
        }
    }
    Ok(())
}

async fn show_blob(repo: &Path, revision: &str, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
    let spec = format!("{revision}:{path}");
    match run_git(repo, &["show", &spec], GIT_TIMEOUT).await {
        Ok(out) => Ok(Some(out.stdout)),
        Err(e) if e.detail.contains("does not exist") || e.detail.contains("exists on disk") => {
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

async fn tracks_from(path: &Path) -> anyhow::Result<HashMap<String, Track>> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true);
    let mut conn = sqlx::SqliteConnection::connect_with(&opts).await?;
    let rows = sqlx::query("SELECT * FROM tracks")
        .fetch_all(&mut conn)
        .await?;
    rows.iter()
        .map(|row| Track::from_db_row(row).map(|track| (track.yt_id.clone(), track)))
        .collect()
}

fn merge_track(base: Option<&Track>, local: &Track, remote: &Track) -> anyhow::Result<Track> {
    if local == remote {
        return Ok(local.clone());
    }
    if base == Some(local) {
        return Ok(remote.clone());
    }
    if base == Some(remote) {
        return Ok(local.clone());
    }
    let base_value = base
        .map(serde_json::to_value)
        .transpose()?
        .unwrap_or(Value::Null);
    let local_value = serde_json::to_value(local)?;
    let remote_value = serde_json::to_value(remote)?;
    let base_obj = base_value.as_object();
    let local_obj = local_value.as_object().context("track is not an object")?;
    let remote_obj = remote_value.as_object().context("track is not an object")?;
    let mut merged = local_obj.clone();
    for (field, remote_field) in remote_obj {
        let local_field = local_obj.get(field).unwrap_or(&Value::Null);
        let base_field = base_obj
            .and_then(|obj| obj.get(field))
            .unwrap_or(&Value::Null);
        let value = if field == "play_count" {
            let b = base_field.as_i64().unwrap_or(0);
            Value::from(
                b + (local_field.as_i64().unwrap_or(0) - b).max(0)
                    + (remote_field.as_i64().unwrap_or(0) - b).max(0),
            )
        } else if field == "last_played" {
            match (local_field.as_str(), remote_field.as_str()) {
                (Some(a), Some(b)) => Value::from(a.max(b)),
                (Some(a), None) => Value::from(a),
                (None, Some(b)) => Value::from(b),
                _ => Value::Null,
            }
        } else if local_field == remote_field || remote_field == base_field {
            local_field.clone()
        } else if local_field == base_field {
            remote_field.clone()
        } else {
            anyhow::bail!("conflicting {field} values for {}", local.yt_id);
        };
        merged.insert(field.clone(), value);
    }
    Ok(serde_json::from_value(Value::Object(merged))?)
}

fn merge_tracks(
    base: &HashMap<String, Track>,
    local: &HashMap<String, Track>,
    remote: &HashMap<String, Track>,
) -> anyhow::Result<Vec<Track>> {
    let ids: BTreeSet<&String> = base
        .keys()
        .chain(local.keys())
        .chain(remote.keys())
        .collect();
    let mut merged = Vec::new();
    for id in ids {
        match (base.get(id), local.get(id), remote.get(id)) {
            (_, Some(a), Some(b)) => merged.push(merge_track(base.get(id), a, b)?),
            (Some(base), None, Some(remote)) if base == remote => {}
            (Some(base), Some(local), None) if base == local => {}
            (None, Some(track), None) | (None, None, Some(track)) => merged.push(track.clone()),
            (Some(_), None, Some(_)) | (Some(_), Some(_), None) => {
                anyhow::bail!("delete/edit conflict for track {id}")
            }
            (Some(_), None, None) | (None, None, None) => {}
        }
    }
    Ok(merged)
}

async fn write_blob(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    tokio::fs::write(path, bytes).await?;
    quick_check(path).await
}

async fn build_merged_database(
    repo: &Path,
    base_rev: &str,
    remote_rev: &str,
    path: &str,
    temp_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let local_bytes = show_blob(repo, "HEAD", path)
        .await?
        .context("local collection was deleted")?;
    let remote_bytes = show_blob(repo, remote_rev, path)
        .await?
        .context("upstream collection was deleted")?;
    let base_bytes = show_blob(repo, base_rev, path).await?;
    let local_path = temp_dir.join("local.db");
    let remote_path = temp_dir.join("remote.db");
    let base_path = temp_dir.join("base.db");
    let merged_path = temp_dir.join("merged.db");
    write_blob(&local_path, &local_bytes).await?;
    write_blob(&remote_path, &remote_bytes).await?;
    anyhow::ensure!(
        db::validate_db_schema(&local_path).await,
        "invalid local collection"
    );
    anyhow::ensure!(
        db::validate_db_schema(&remote_path).await,
        "invalid upstream collection"
    );
    if let Some(bytes) = &base_bytes {
        write_blob(&base_path, bytes).await?;
        anyhow::ensure!(
            db::validate_db_schema(&base_path).await,
            "invalid base collection"
        );
    }
    let base = if base_bytes.is_some() {
        tracks_from(&base_path).await?
    } else {
        HashMap::new()
    };
    let local = tracks_from(&local_path).await?;
    let remote = tracks_from(&remote_path).await?;
    let tracks = merge_tracks(&base, &local, &remote)?;
    tokio::fs::copy(&local_path, &merged_path).await?;
    let pool = db::open_pool(&merged_path).await?;
    sqlx::query("DELETE FROM tracks").execute(&pool).await?;
    db::insert_tracks(&pool, &tracks, false).await?;
    pool.close().await;
    quick_check(&merged_path).await?;
    Ok(merged_path)
}

async fn create_recovery(
    repo: &Path,
    remote_ref: &str,
    paths: &BTreeSet<String>,
) -> anyhow::Result<PathBuf> {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let repo_name = repo.file_name().and_then(|n| n.to_str()).unwrap_or("repo");
    let recovery = repo
        .parent()
        .unwrap_or(repo)
        .join(".stereo-recovery")
        .join(repo_name)
        .join(&stamp);
    tokio::fs::create_dir_all(&recovery).await?;
    run_git(
        repo,
        &[
            "update-ref",
            &format!("refs/stereo/recovery/{stamp}-local"),
            "HEAD",
        ],
        GIT_TIMEOUT,
    )
    .await?;
    run_git(
        repo,
        &[
            "update-ref",
            &format!("refs/stereo/recovery/{stamp}-upstream"),
            remote_ref,
        ],
        GIT_TIMEOUT,
    )
    .await?;
    for path in paths {
        let safe = path.replace('/', "__");
        if let Some(bytes) = show_blob(repo, "HEAD", path).await? {
            tokio::fs::write(recovery.join(format!("local-{safe}")), bytes).await?;
        }
        if let Some(bytes) = show_blob(repo, remote_ref, path).await? {
            tokio::fs::write(recovery.join(format!("upstream-{safe}")), bytes).await?;
        }
    }
    Ok(recovery)
}

async fn merge_diverged(
    repo: &Path,
    remote_ref: &str,
    allow_update: bool,
    resolution: Option<GitConflictStrategy>,
) -> anyhow::Result<SyncReport> {
    if !allow_update {
        return Ok(SyncReport::new(
            GitSyncState::ConflictRequiresAttention,
            "Local and upstream histories diverged; retry to reconcile them safely",
            true,
        ));
    }
    let base = match run_git(repo, &["merge-base", "HEAD", remote_ref], GIT_TIMEOUT).await {
        Ok(output) => output_text(output),
        Err(_) => {
            let local_paths = tree_paths(repo, "HEAD").await?;
            let remote_paths = tree_paths(repo, remote_ref).await?;
            let all_paths: BTreeSet<String> = local_paths.union(&remote_paths).cloned().collect();
            let recovery = create_recovery(repo, remote_ref, &all_paths).await?;
            match resolution {
                None => {
                    return Ok(SyncReport::new(
                        GitSyncState::ConflictRequiresAttention,
                        format!(
                            "Local and upstream histories are unrelated; recovery copies are in {}",
                            recovery.display()
                        ),
                        false,
                    ));
                }
                Some(GitConflictStrategy::KeepLocal) => {
                    run_git(
                        repo,
                        &[
                            "-c",
                            "commit.gpgsign=false",
                            "-c",
                            "core.hooksPath=/dev/null",
                            "-c",
                            "user.name=Stereo",
                            "-c",
                            "user.email=stereo@localhost",
                            "merge",
                            "--allow-unrelated-histories",
                            "-s",
                            "ours",
                            "--no-edit",
                            remote_ref,
                        ],
                        GIT_TIMEOUT,
                    )
                    .await?;
                    return Ok(SyncReport::new(
                        GitSyncState::LocalChanges,
                        "Kept local collections and joined the upstream history",
                        true,
                    ));
                }
                Some(GitConflictStrategy::KeepUpstream) => {
                    validate_revision_databases(repo, remote_ref, &remote_paths).await?;
                    run_git(repo, &["reset", "--hard", remote_ref], GIT_TIMEOUT).await?;
                    return Ok(SyncReport::new(
                        GitSyncState::LocalChanges,
                        "Adopted upstream collections; local recovery copies were preserved",
                        true,
                    ));
                }
            }
        }
    };
    let local_paths = changed_paths(repo, &format!("{base}..HEAD")).await?;
    let remote_paths = changed_paths(repo, &format!("{base}..{remote_ref}")).await?;
    let conflicts: BTreeSet<String> = local_paths.intersection(&remote_paths).cloned().collect();
    let recovery = if conflicts.is_empty() {
        None
    } else {
        Some(create_recovery(repo, remote_ref, &conflicts).await?)
    };

    let temp = tempfile::tempdir()?;
    let mut merged_files = HashMap::new();
    if resolution.is_none() {
        for path in &conflicts {
            if !path.ends_with(".db") {
                return Ok(SyncReport::new(
                    GitSyncState::ConflictRequiresAttention,
                    format!(
                        "Non-collection conflict in {path}; recovery copies are in {}",
                        recovery.unwrap().display()
                    ),
                    false,
                ));
            }
            match build_merged_database(repo, &base, remote_ref, path, temp.path()).await {
                Ok(merged) => {
                    let unique = temp.path().join(path.replace('/', "__"));
                    tokio::fs::copy(merged, &unique).await?;
                    merged_files.insert(path.clone(), unique);
                }
                Err(e) => {
                    return Ok(SyncReport::new(
                        GitSyncState::ConflictRequiresAttention,
                        format!(
                            "{e}; recovery copies are in {}",
                            recovery.unwrap().display()
                        ),
                        false,
                    ));
                }
            }
        }
    }

    validate_revision_databases(repo, remote_ref, &remote_paths).await?;
    let merge_marker = repo.join(".git").join(MERGE_MARKER);
    tokio::fs::write(&merge_marker, remote_ref).await?;
    let merge = run_git(
        repo,
        &["merge", "--no-commit", "--no-edit", remote_ref],
        GIT_TIMEOUT,
    )
    .await;
    if conflicts.is_empty() {
        if let Err(error) = merge {
            let _ = run_git(repo, &["merge", "--abort"], GIT_TIMEOUT).await;
            let _ = tokio::fs::remove_file(&merge_marker).await;
            return Err(error.into());
        }
    }
    for path in &conflicts {
        match resolution {
            Some(GitConflictStrategy::KeepLocal) => {
                if show_blob(repo, "HEAD", path).await?.is_some() {
                    run_git(repo, &["checkout", "--ours", "--", path], GIT_TIMEOUT).await?;
                } else {
                    run_git(repo, &["rm", "-f", "--", path], GIT_TIMEOUT).await?;
                }
            }
            Some(GitConflictStrategy::KeepUpstream) => {
                if show_blob(repo, remote_ref, path).await?.is_some() {
                    run_git(repo, &["checkout", "--theirs", "--", path], GIT_TIMEOUT).await?;
                } else {
                    run_git(repo, &["rm", "-f", "--", path], GIT_TIMEOUT).await?;
                }
            }
            None => {
                tokio::fs::copy(&merged_files[path], repo.join(path)).await?;
            }
        }
        if repo.join(path).exists() {
            run_git(repo, &["add", "--", path], GIT_TIMEOUT).await?;
        }
    }
    run_git(
        repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=Stereo",
            "-c",
            "user.email=stereo@localhost",
            "commit",
            "--no-edit",
        ],
        GIT_TIMEOUT,
    )
    .await?;
    let _ = tokio::fs::remove_file(&merge_marker).await;
    let all_paths: BTreeSet<String> = local_paths.union(&remote_paths).cloned().collect();
    validate_changed_databases(repo, &all_paths).await?;
    let mut migrated: BTreeSet<String> = all_paths
        .iter()
        .filter(|path| path.ends_with(".db") && repo.join(path).exists())
        .cloned()
        .collect();
    if !migrated.is_empty() {
        write_dirty(repo, &migrated).await?;
        commit_dirty(repo, &mut migrated).await?;
    }
    Ok(SyncReport::new(
        GitSyncState::LocalChanges,
        "Local and upstream changes were reconciled",
        true,
    ))
}

/// Runs a sync while holding the repo lock in `repo_guard`. Without `allow_update` nothing past
/// the local commit touches the working tree or index, so the repo lock is released at that
/// point and only the network lock is held for fetch/push.
async fn sync_locked(
    repo: &Path,
    repo_guard: &mut Option<OwnedMutexGuard<()>>,
    db_path: Option<&Path>,
    allow_update: bool,
    resolution: Option<GitConflictStrategy>,
) -> SyncReport {
    if !repo.join(".git").is_dir() {
        return SyncReport::new(
            GitSyncState::Error,
            "Selected directory is not a Git repository",
            false,
        );
    }
    let mut dirty = read_dirty(repo);
    match discover_dirty_databases(repo).await {
        Ok(discovered) => dirty.extend(discovered),
        Err(error) => return failure_report(&error),
    }
    if let Some(path) = db_path {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return SyncReport::new(
                GitSyncState::Error,
                "Collection path is not valid UTF-8",
                false,
            );
        };
        if !path.starts_with(repo) {
            return SyncReport::new(
                GitSyncState::Error,
                "Collection is outside its repository",
                false,
            );
        }
        dirty.insert(name.to_string());
        if let Err(e) = write_dirty(repo, &dirty).await {
            return SyncReport::new(
                GitSyncState::Error,
                format!("Could not persist pending sync: {e}"),
                true,
            );
        }
    }
    let merge_marker = repo.join(".git").join(MERGE_MARKER);
    if repo.join(".git/MERGE_HEAD").exists() && merge_marker.exists() {
        if let Err(e) = run_git(repo, &["merge", "--abort"], GIT_TIMEOUT).await {
            return failure_report(&e);
        }
        let _ = tokio::fs::remove_file(&merge_marker).await;
    }
    if repo.join(".git/MERGE_HEAD").exists()
        || repo.join(".git/rebase-merge").exists()
        || repo.join(".git/rebase-apply").exists()
    {
        return SyncReport::new(
            GitSyncState::Error,
            "An interrupted Git operation requires attention before Stereo can sync",
            false,
        );
    }
    if let Err(e) = commit_dirty(repo, &mut dirty).await {
        return SyncReport::new(
            GitSyncState::Error,
            format!("Could not save local changes: {e}"),
            true,
        );
    }
    if !allow_update {
        repo_guard.take();
    }
    if get_remote(repo).await.is_none() {
        return SyncReport::new(
            GitSyncState::LocalOnly,
            "Changes are committed locally; no remote is configured",
            false,
        );
    }
    let network = network_lock(repo);
    let _network_guard = network.lock().await;
    let mut branch = match current_branch(repo).await {
        Ok(branch) => branch,
        Err(e) => return failure_report(&e),
    };
    if let Err(e) = run_git(repo, &["fetch", "--prune", "origin"], NETWORK_TIMEOUT).await {
        return failure_report(&e);
    }
    let mut remote_ref = format!("refs/remotes/origin/{branch}");
    let has_local_head = git_ok(repo, &["rev-parse", "--verify", "HEAD"]).await;
    if !has_local_head && !git_ok(repo, &["rev-parse", "--verify", &remote_ref]).await {
        if let Ok(remote_head) = run_git(
            repo,
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
            GIT_TIMEOUT,
        )
        .await
        {
            let short = output_text(remote_head);
            if let Some(default_branch) = short.strip_prefix("origin/") {
                branch = default_branch.to_string();
                remote_ref = format!("refs/remotes/origin/{branch}");
            }
        }
    }
    if !git_ok(repo, &["rev-parse", "--verify", &remote_ref]).await {
        return push(repo, &branch).await;
    }
    if !has_local_head {
        if !allow_update {
            return SyncReport::new(
                GitSyncState::RemoteAhead,
                "The remote repository has collections ready to import",
                true,
            );
        }
        let paths = match tree_paths(repo, &remote_ref).await {
            Ok(paths) => paths,
            Err(error) => return failure_report(&error),
        };
        if let Err(error) = validate_revision_databases(repo, &remote_ref, &paths).await {
            return SyncReport::new(
                GitSyncState::CorruptCollection,
                format!("Upstream collection failed validation: {error}"),
                false,
            );
        }
        if let Err(error) =
            run_git(repo, &["checkout", "-B", &branch, &remote_ref], GIT_TIMEOUT).await
        {
            return failure_report(&error);
        }
        clear_needs_sync(repo).await;
        return SyncReport::new(GitSyncState::Synced, "Imported upstream collections", false);
    }
    if is_ancestor(repo, &remote_ref, "HEAD").await {
        return push(repo, &branch).await;
    }
    if is_ancestor(repo, "HEAD", &remote_ref).await {
        if !allow_update {
            return SyncReport::new(
                GitSyncState::RemoteAhead,
                "Upstream has new changes; retry to apply them",
                true,
            );
        }
        if !git_ok(repo, &["diff", "--quiet"]).await {
            return SyncReport::new(
                GitSyncState::LocalChanges,
                "New local database writes are waiting to be saved",
                true,
            );
        }
        let paths = match changed_paths(repo, &format!("HEAD..{remote_ref}")).await {
            Ok(paths) => paths,
            Err(e) => return failure_report(&e),
        };
        if let Err(e) = validate_revision_databases(repo, &remote_ref, &paths).await {
            return SyncReport::new(
                GitSyncState::CorruptCollection,
                format!("Upstream collection failed validation: {e}"),
                false,
            );
        }
        if let Err(e) = run_git(repo, &["merge", "--ff-only", &remote_ref], GIT_TIMEOUT).await {
            return failure_report(&e);
        }
        if let Err(e) = validate_changed_databases(repo, &paths).await {
            return SyncReport::new(
                GitSyncState::CorruptCollection,
                format!("Upstream collection failed validation: {e}"),
                false,
            );
        }
        let mut migrated: BTreeSet<String> = paths
            .iter()
            .filter(|path| path.ends_with(".db") && repo.join(path).exists())
            .cloned()
            .collect();
        if !migrated.is_empty() {
            if let Err(e) = write_dirty(repo, &migrated).await {
                return SyncReport::new(GitSyncState::Error, e.to_string(), true);
            }
            if let Err(e) = commit_dirty(repo, &mut migrated).await {
                return SyncReport::new(GitSyncState::Error, e.to_string(), true);
            }
        }
        return push(repo, &branch).await;
    }
    match merge_diverged(repo, &remote_ref, allow_update, resolution).await {
        Ok(report) if matches!(report.state, GitSyncState::LocalChanges) => {
            push(repo, &branch).await
        }
        Ok(report) => report,
        Err(e) => SyncReport::new(
            GitSyncState::Error,
            format!("Could not reconcile repository: {e}"),
            true,
        ),
    }
}

async fn push(repo: &Path, branch: &str) -> SyncReport {
    match run_git(
        repo,
        &[
            "push",
            "--set-upstream",
            "origin",
            &format!("HEAD:{branch}"),
        ],
        NETWORK_TIMEOUT,
    )
    .await
    {
        Ok(_) => {
            clear_needs_sync(repo).await;
            SyncReport::new(GitSyncState::Synced, "Repository is synchronized", false)
        }
        Err(e) if e.detail.contains("non-fast-forward") || e.detail.contains("fetch first") => {
            SyncReport::new(
                GitSyncState::RemoteAhead,
                "Upstream changed during synchronization; retrying is safe",
                true,
            )
        }
        Err(e) => failure_report(&e),
    }
}

pub async fn sync_repository(
    repo: PathBuf,
    db_path: Option<PathBuf>,
    allow_update: bool,
    resolution: Option<GitConflictStrategy>,
) -> SyncReport {
    let guard = lock_repository(&repo).await;
    sync_repository_with_guard(guard, repo, db_path, allow_update, resolution).await
}

/// Like [`sync_repository`], for a caller that already holds the repo lock — e.g. so that it can
/// close its database pool and be sure nothing reopens one before the sync has taken over.
pub async fn sync_repository_with_guard(
    guard: OwnedMutexGuard<()>,
    repo: PathBuf,
    db_path: Option<PathBuf>,
    allow_update: bool,
    resolution: Option<GitConflictStrategy>,
) -> SyncReport {
    let mut guard = Some(guard);
    let report = sync_locked(
        &repo,
        &mut guard,
        db_path.as_deref(),
        allow_update,
        resolution,
    )
    .await;
    drop(guard);
    if report.state == GitSyncState::Offline {
        schedule_offline_retry(repo);
    }
    report
}

fn schedule_offline_retry(repo: PathBuf) {
    let key = std::fs::canonicalize(&repo).unwrap_or_else(|_| repo.clone());
    {
        let mut workers = RETRY_WORKERS
            .lock()
            .expect("retry worker registry poisoned");
        if !workers.insert(key.clone()) {
            return;
        }
    }
    tokio::spawn(async move {
        for delay in [5, 30, 120, 300] {
            tokio::time::sleep(Duration::from_secs(delay)).await;
            let report = sync_repository(key.clone(), None, false, None).await;
            tracing::info!(
                "automatic repository retry for {}: {}",
                key.display(),
                report.message
            );
            if report.state != GitSyncState::Offline {
                break;
            }
        }
        RETRY_WORKERS
            .lock()
            .expect("retry worker registry poisoned")
            .remove(&key);
    });
}

pub async fn lock_repository(repo: &Path) -> tokio::sync::OwnedMutexGuard<()> {
    repo_lock(repo).lock_owned().await
}

pub async fn recover_pending_repositories(home: &Path) {
    for repo in crate::utils::list_repos(home).await {
        let path = repo.path.clone();
        tokio::spawn(async move {
            let report = sync_repository(path.clone(), None, false, None).await;
            tracing::info!("startup sync for {}: {}", path.display(), report.message);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(dir: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    async fn create_collection(path: &Path, track: &Track) {
        let pool = db::open_pool(path).await.unwrap();
        db::init_db(&pool).await.unwrap();
        db::insert_track(&pool, track, false).await.unwrap();
        pool.close().await;
    }

    fn track(id: &str) -> Track {
        Track {
            yt_id: id.into(),
            title: format!("Track {id}"),
            artists: vec!["Artist".into()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn sync_fast_forwards_remote_changes() {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        command(&remote, &["init", "--bare"]);

        let first = temp.path().join("first");
        init_if_needed(&first).await.unwrap();
        set_remote(&first, Some(remote.to_str().unwrap()))
            .await
            .unwrap();
        let db_path = first.join("music.db");
        create_collection(&db_path, &track("one")).await;
        let report = sync_repository(first.clone(), Some(db_path), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        let second = temp.path().join("second");
        clone_repo(remote.to_str().unwrap(), &second).await.unwrap();
        let second_db = second.join("music.db");
        let pool = db::open_pool(&second_db).await.unwrap();
        db::insert_track(&pool, &track("two"), false).await.unwrap();
        pool.close().await;
        let report = sync_repository(second, Some(second_db), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        let report = sync_repository(first.clone(), None, false, None).await;
        assert_eq!(report.state, GitSyncState::RemoteAhead);
        let report = sync_repository(first.clone(), None, true, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
        let ids = db::get_all_yt_ids(&db::open_pool(&first.join("music.db")).await.unwrap())
            .await
            .unwrap();
        assert!(ids.contains("two"));
    }

    #[tokio::test]
    async fn background_sync_releases_repo_lock_during_network_phase() {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        command(&remote, &["init", "--bare"]);
        let repo = temp.path().join("repo");
        init_if_needed(&repo).await.unwrap();
        set_remote(&repo, Some(remote.to_str().unwrap()))
            .await
            .unwrap();
        let db_path = repo.join("music.db");
        create_collection(&db_path, &track("one")).await;

        // Stall the sync at its network phase, as a slow fetch/push would.
        let network = network_lock(&repo);
        let network_guard = network.lock().await;
        let sync = tokio::spawn(sync_repository(
            repo.clone(),
            Some(db_path.clone()),
            false,
            None,
        ));

        // Wait for the local commit, which the sync makes just before its network phase.
        tokio::time::timeout(Duration::from_secs(10), async {
            while !git_ok(&repo, &["rev-parse", "--verify", "HEAD"]).await {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("sync should commit locally");

        // A database writer can still take the repo lock while the sync waits on the network.
        let repo_guard = tokio::time::timeout(Duration::from_secs(5), lock_repository(&repo))
            .await
            .expect("repo lock should be released before the network phase");
        assert!(!sync.is_finished());
        drop(repo_guard);

        drop(network_guard);
        let report = sync.await.unwrap();
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
    }

    #[tokio::test]
    async fn sync_semantically_merges_same_collection() {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        command(&remote, &["init", "--bare"]);

        let first = temp.path().join("first");
        init_if_needed(&first).await.unwrap();
        set_remote(&first, Some(remote.to_str().unwrap()))
            .await
            .unwrap();
        let first_db = first.join("music.db");
        create_collection(&first_db, &track("one")).await;
        assert_eq!(
            sync_repository(first.clone(), Some(first_db.clone()), false, None)
                .await
                .state,
            GitSyncState::Synced
        );

        let second = temp.path().join("second");
        clone_repo(remote.to_str().unwrap(), &second).await.unwrap();
        let second_db = second.join("music.db");

        let first_pool = db::open_pool(&first_db).await.unwrap();
        db::update_rating(&first_pool, "one", Some(5))
            .await
            .unwrap();
        first_pool.close().await;
        assert_eq!(
            sync_repository(first, Some(first_db), false, None)
                .await
                .state,
            GitSyncState::Synced
        );

        let second_pool = db::open_pool(&second_db).await.unwrap();
        db::inc_play_count(
            &second_pool,
            "one",
            chrono::NaiveDate::from_ymd_opt(2026, 8, 18).unwrap(),
        )
        .await
        .unwrap();
        second_pool.close().await;
        assert_eq!(
            sync_repository(second.clone(), Some(second_db.clone()), false, None)
                .await
                .state,
            GitSyncState::ConflictRequiresAttention
        );
        let report = sync_repository(second.clone(), None, true, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
        let pool = db::open_pool(&second_db).await.unwrap();
        let merged = db::get_track(&pool, "one").await.unwrap().unwrap();
        assert_eq!(merged.rating, Some(5));
        assert_eq!(merged.play_count, 1);
        pool.close().await;
    }

    #[tokio::test]
    async fn sync_preserves_recovery_copies_and_accepts_explicit_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        command(&remote, &["init", "--bare"]);

        let first = temp.path().join("first");
        init_if_needed(&first).await.unwrap();
        set_remote(&first, Some(remote.to_str().unwrap()))
            .await
            .unwrap();
        let first_db = first.join("music.db");
        create_collection(&first_db, &track("one")).await;
        assert_eq!(
            sync_repository(first.clone(), Some(first_db.clone()), false, None)
                .await
                .state,
            GitSyncState::Synced
        );

        let second = temp.path().join("second");
        clone_repo(remote.to_str().unwrap(), &second).await.unwrap();
        let second_db = second.join("music.db");
        let first_pool = db::open_pool(&first_db).await.unwrap();
        db::update_rating(&first_pool, "one", Some(5))
            .await
            .unwrap();
        first_pool.close().await;
        assert_eq!(
            sync_repository(first, Some(first_db), false, None)
                .await
                .state,
            GitSyncState::Synced
        );

        let second_pool = db::open_pool(&second_db).await.unwrap();
        db::update_rating(&second_pool, "one", Some(4))
            .await
            .unwrap();
        second_pool.close().await;
        assert_eq!(
            sync_repository(second.clone(), Some(second_db.clone()), false, None)
                .await
                .state,
            GitSyncState::ConflictRequiresAttention
        );
        let conflict = sync_repository(second.clone(), None, true, None).await;
        assert_eq!(conflict.state, GitSyncState::ConflictRequiresAttention);
        assert!(temp.path().join(".stereo-recovery/second").is_dir());

        let resolved = sync_repository(
            second.clone(),
            None,
            true,
            Some(GitConflictStrategy::KeepUpstream),
        )
        .await;
        assert_eq!(resolved.state, GitSyncState::Synced, "{}", resolved.message);
        let pool = db::open_pool(&second_db).await.unwrap();
        assert_eq!(
            db::get_track(&pool, "one").await.unwrap().unwrap().rating,
            Some(5)
        );
        pool.close().await;
    }
}
