use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
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
    playlists::PLAYLISTS_FILE,
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
            // Collection names may contain `*`, `?` or `[`, which git would otherwise expand as
            // patterns, so every pathspec is a literal path.
            .env("GIT_LITERAL_PATHSPECS", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "Never")
            .env(
                "GIT_SSH_COMMAND",
                "ssh -o BatchMode=yes -o ConnectTimeout=15",
            )
            .current_dir(repo)
            // `output()` leaves stdin inherited: on Windows, a child holding the desktop app's
            // stdin pipe can't exit while our `--exit-with-stdin` read waits on it.
            .stdin(std::process::Stdio::null())
            // A timed-out git is killed, not left running.
            .kill_on_drop(true)
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

/// Where a clone keeps the id in its author, in the clone's own config, which isn't synced.
const AUTHOR_ID_KEY: &str = "stereo.authorId";

/// Who Stereo commits as in `repo`: "Stereo on <hostname>", with an id of this clone in the
/// email. The hostname tells in the history which machine made a commit; the id keeps two
/// clones' commits apart even where hostnames repeat (or the dev and release data dirs share a
/// machine), since commits of identical files on the same parent in the same second would
/// otherwise be the very same commit, and the plays each side recorded would merge into one.
async fn author(repo: &Path) -> Result<(String, String), GitFailure> {
    let id = match run_git(repo, &["config", "--get", AUTHOR_ID_KEY], GIT_TIMEOUT).await {
        Ok(out) => output_text(out),
        Err(_) => {
            let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
            run_git(
                repo,
                &["config", "--local", AUTHOR_ID_KEY, &id],
                GIT_TIMEOUT,
            )
            .await?;
            id
        }
    };
    let host: String = whoami::fallible::hostname()
        .unwrap_or_default()
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | '\n' | '\r'))
        .collect();
    let name = match host.trim() {
        "" => "Stereo".to_string(),
        host => format!("Stereo on {host}"),
    };
    Ok((name, format!("stereo-{id}@localhost")))
}

/// Runs a git command that commits (`commit`, `merge`), as Stereo (`author`) and without the
/// user's signing or hooks, which could prompt or fail a background sync.
async fn run_git_committing(repo: &Path, args: &[&str]) -> Result<Output, GitFailure> {
    let (name, email) = author(repo).await?;
    let name = format!("user.name={name}");
    let email = format!("user.email={email}");
    let mut all = vec![
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        &name,
        "-c",
        &email,
    ];
    all.extend_from_slice(args);
    run_git(repo, &all, GIT_TIMEOUT).await
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
            // `set-url` leaves a separate push URL in place, and pushes would still go there:
            // the remote shown and set is where collections go.
            if git_ok(home, &["config", "--get-all", "remote.origin.pushurl"]).await {
                run_git(
                    home,
                    &["config", "--unset-all", "remote.origin.pushurl"],
                    GIT_TIMEOUT,
                )
                .await?;
            }
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
    tokio::fs::write(
        home.join(".gitignore"),
        format!("*\n!.gitignore\n!*.db\n!{PLAYLISTS_FILE}\n"),
    )
    .await?;
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

/// The collections and the playlists file with changes to commit. The playlists file is always
/// included when it exists: repos created before it existed ignore it, so `git status` doesn't
/// report it until it is tracked, and staging it unchanged commits nothing.
async fn discover_dirty_databases(repo: &Path) -> Result<BTreeSet<String>, GitFailure> {
    let out = run_git(
        repo,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        GIT_TIMEOUT,
    )
    .await?;
    let mut dirty: BTreeSet<String> = out
        .stdout
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.get(3..))
        .filter(|path| path.ends_with(b".db") || *path == PLAYLISTS_FILE.as_bytes())
        .map(|path| String::from_utf8_lossy(path).to_string())
        .collect();
    if repo.join(PLAYLISTS_FILE).exists() {
        dirty.insert(PLAYLISTS_FILE.to_string());
    }
    Ok(dirty)
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
        if name == PLAYLISTS_FILE && repo.join(name).exists() {
            // Forced: repos created before playlists existed ignore everything but databases.
            run_git(repo, &["add", "-f", "--", name], GIT_TIMEOUT).await?;
        } else if repo.join(name).exists() {
            stage_consistent_database(repo, name).await?;
        } else {
            // A deleted collection. It may never have been committed (created and deleted
            // before a sync ran), which `git add -u` would reject as an unmatched pathspec.
            run_git(
                repo,
                &["rm", "--cached", "--quiet", "--ignore-unmatch", "--", name],
                GIT_TIMEOUT,
            )
            .await?;
        }
    }
    let staged = staged_paths(repo).await?;
    if staged.is_empty() {
        dirty.clear();
        write_dirty(repo, dirty).await?;
        return Ok(());
    }
    // The commit message describes the collections; the playlists file only counts on its own.
    let names: Vec<String> = if staged.len() == 1 && staged.contains(PLAYLISTS_FILE) {
        vec![PLAYLISTS_FILE.to_string()]
    } else {
        names.into_iter().filter(|n| n != PLAYLISTS_FILE).collect()
    };
    let missing: Vec<&String> = names.iter().filter(|n| !repo.join(n).exists()).collect();
    let message = if names.len() == 2 && missing.len() == 1 {
        let added = names.iter().find(|n| *n != missing[0]).expect("two names");
        format!("rename {} to {added}", missing[0])
    } else if names.len() == 1 && !missing.is_empty() {
        format!("delete {}", names[0])
    } else if names.len() == 1 && names[0] == PLAYLISTS_FILE {
        "update playlists".to_string()
    } else if names.len() == 1 {
        format!("update {}", names[0])
    } else {
        format!("update {} collections", names.len())
    };
    run_git_committing(repo, &["commit", "-m", &message]).await?;
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

/// Paths that `range` changed. A rename counts as a deletion of its old path plus an addition of
/// its new one: Git's rename detection is a content-similarity guess, and reporting only the new
/// path would hide that the old one changed (see `renamed_paths` for following renames).
async fn changed_paths(repo: &Path, range: &str) -> Result<BTreeSet<String>, GitFailure> {
    let out = run_git(
        repo,
        &["diff", "--no-renames", "--name-only", "-z", range],
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

/// Files that `range` renamed (as far as Git's rename detection can tell), old path → new path.
async fn renamed_paths(repo: &Path, range: &str) -> Result<BTreeMap<String, String>, GitFailure> {
    let out = run_git(
        repo,
        &[
            "diff",
            "-M",
            "--diff-filter=R",
            "--name-status",
            "-z",
            range,
        ],
        GIT_TIMEOUT,
    )
    .await?;
    // `-z` output is `R<score>\0<old>\0<new>\0` per rename.
    let mut fields = out
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).to_string());
    let mut renames = BTreeMap::new();
    while let (Some(_status), Some(from), Some(to)) = (fields.next(), fields.next(), fields.next())
    {
        renames.insert(from, to);
    }
    Ok(renames)
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
    // Plays recorded on both sides since the merge base are separate plays even when they left
    // the rows identical (one play each on the same day), so their counts still add up.
    if local == remote && base.is_none_or(|base| base.play_count == local.play_count) {
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

/// A path that both sides of a merge changed, differently.
struct Conflict {
    /// The changed path; for a rename that's being followed, its old path.
    path: String,
    /// Where the local version is: `path`, or where the local side renamed it to.
    local: String,
    /// Where the upstream version is: `path`, or where upstream renamed it to.
    remote: String,
}

impl Conflict {
    /// Where the reconciled version goes. At most one side renamed the file, the other didn't
    /// touch the new path, so following the rename can't clobber anything.
    fn target(&self) -> &str {
        if self.local != self.path {
            &self.local
        } else {
            &self.remote
        }
    }

    fn paths(&self) -> [&str; 3] {
        [&self.path, &self.local, &self.remote]
    }
}

/// The paths both sides changed (with renames counted as deletion plus addition, see
/// `changed_paths`), skipping those that ended up the same on both sides, except collections,
/// whose identical play counts may still have to be added up (`merge_track`). A path one side
/// deleted because it renamed it, which the other side edited, follows the rename, so that the
/// edit is merged into the renamed collection rather than conflicting with its deletion.
async fn find_conflicts(
    repo: &Path,
    base: &str,
    remote_ref: &str,
    local_paths: &BTreeSet<String>,
    remote_paths: &BTreeSet<String>,
) -> anyhow::Result<Vec<Conflict>> {
    let local_renames = renamed_paths(repo, &format!("{base}..HEAD")).await?;
    let remote_renames = renamed_paths(repo, &format!("{base}..{remote_ref}")).await?;
    let mut conflicts = Vec::new();
    for path in local_paths.intersection(remote_paths) {
        let local_blob = show_blob(repo, "HEAD", path).await?;
        let remote_blob = show_blob(repo, remote_ref, path).await?;
        // Identical collections still need merging: both sides may have recorded the same plays.
        if local_blob == remote_blob && !(local_blob.is_some() && path.ends_with(".db")) {
            continue;
        }
        let mut conflict = Conflict {
            path: path.clone(),
            local: path.clone(),
            remote: path.clone(),
        };
        if local_blob.is_none() {
            if let Some(to) = local_renames
                .get(path)
                .filter(|to| !remote_paths.contains(*to))
            {
                conflict.local = to.clone();
            }
        } else if remote_blob.is_none() {
            if let Some(to) = remote_renames
                .get(path)
                .filter(|to| !local_paths.contains(*to))
            {
                conflict.remote = to.clone();
            }
        }
        conflicts.push(conflict);
    }
    Ok(conflicts)
}

/// Both sides' playlists merged per playlist (`playlists::merge`); a side that deleted the file
/// counts as having no playlists.
async fn merge_playlists_file(
    repo: &Path,
    base_rev: &str,
    remote_rev: &str,
) -> anyhow::Result<Vec<u8>> {
    let empty = b"{}".to_vec();
    let local = show_blob(repo, "HEAD", PLAYLISTS_FILE).await?;
    let remote = show_blob(repo, remote_rev, PLAYLISTS_FILE).await?;
    let base = show_blob(repo, base_rev, PLAYLISTS_FILE).await?;
    crate::playlists::merge_files(
        base.as_deref(),
        local.as_deref().unwrap_or(&empty),
        remote.as_deref().unwrap_or(&empty),
    )
}

async fn build_merged_database(
    repo: &Path,
    base_rev: &str,
    remote_rev: &str,
    conflict: &Conflict,
    temp_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let local_bytes = show_blob(repo, "HEAD", &conflict.local)
        .await?
        .context("local collection was deleted")?;
    let remote_bytes = show_blob(repo, remote_rev, &conflict.remote)
        .await?
        .context("upstream collection was deleted")?;
    let base_bytes = show_blob(repo, base_rev, &conflict.path).await?;
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
                    run_git_committing(
                        repo,
                        &[
                            "merge",
                            "--allow-unrelated-histories",
                            "-s",
                            "ours",
                            "--no-edit",
                            remote_ref,
                        ],
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
    let conflicts = find_conflicts(repo, &base, remote_ref, &local_paths, &remote_paths).await?;
    let recovery = if conflicts.is_empty() {
        None
    } else {
        let paths: BTreeSet<String> = conflicts
            .iter()
            .flat_map(Conflict::paths)
            .map(str::to_string)
            .collect();
        Some(create_recovery(repo, remote_ref, &paths).await?)
    };

    let temp = tempfile::tempdir()?;
    let mut merged_files = HashMap::new();
    if resolution.is_none() {
        for conflict in &conflicts {
            let path = &conflict.path;
            if conflict.paths().iter().all(|p| *p == PLAYLISTS_FILE) {
                match merge_playlists_file(repo, &base, remote_ref).await {
                    Ok(bytes) => {
                        let unique = temp.path().join(PLAYLISTS_FILE);
                        tokio::fs::write(&unique, bytes).await?;
                        merged_files.insert(path.clone(), unique);
                        continue;
                    }
                    Err(e) => {
                        return Ok(SyncReport::new(
                            GitSyncState::ConflictRequiresAttention,
                            format!(
                                "Could not merge playlists: {e:#}; recovery copies are in {}",
                                recovery.unwrap().display()
                            ),
                            false,
                        ));
                    }
                }
            }
            if !conflict.paths().iter().all(|p| p.ends_with(".db")) {
                return Ok(SyncReport::new(
                    GitSyncState::ConflictRequiresAttention,
                    format!(
                        "Non-collection conflict in {path}; recovery copies are in {}",
                        recovery.unwrap().display()
                    ),
                    false,
                ));
            }
            match build_merged_database(repo, &base, remote_ref, conflict, temp.path()).await {
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
    // Without rename detection, like `changed_paths`: otherwise Git may merge a rename on one
    // side with an edit of the old path on the other into a binary conflict at the new path,
    // which none of the paths handled below cover.
    let merge = run_git(
        repo,
        &[
            "merge",
            "--no-commit",
            "--no-edit",
            "-X",
            "no-renames",
            remote_ref,
        ],
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
    for conflict in &conflicts {
        // A strategy picks whose content wins; a rename is followed either way.
        let content = match resolution {
            Some(GitConflictStrategy::KeepLocal) => {
                show_blob(repo, "HEAD", &conflict.local).await?
            }
            Some(GitConflictStrategy::KeepUpstream) => {
                show_blob(repo, remote_ref, &conflict.remote).await?
            }
            None => Some(tokio::fs::read(&merged_files[&conflict.path]).await?),
        };
        let target = conflict.target();
        if target != conflict.path {
            run_git(
                repo,
                &[
                    "rm",
                    "-f",
                    "--quiet",
                    "--ignore-unmatch",
                    "--",
                    &conflict.path,
                ],
                GIT_TIMEOUT,
            )
            .await?;
        }
        match content {
            Some(bytes) => {
                tokio::fs::write(repo.join(target), bytes).await?;
                run_git(repo, &["add", "--", target], GIT_TIMEOUT).await?;
            }
            None => {
                run_git(
                    repo,
                    &["rm", "-f", "--quiet", "--ignore-unmatch", "--", target],
                    GIT_TIMEOUT,
                )
                .await?;
            }
        }
    }
    run_git_committing(repo, &["commit", "--no-edit"]).await?;
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
    async fn changing_the_remote_moves_pushes_to_it() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("old.git");
        let new = temp.path().join("new.git");
        for remote in [&old, &new] {
            std::fs::create_dir(remote).unwrap();
            command(remote, &["init", "--bare"]);
        }

        let repo = temp.path().join("repo");
        init_if_needed(&repo).await.unwrap();
        set_remote(&repo, Some(old.to_str().unwrap()))
            .await
            .unwrap();
        command(
            &repo,
            &["config", "remote.origin.pushurl", old.to_str().unwrap()],
        );
        set_remote(&repo, Some(new.to_str().unwrap()))
            .await
            .unwrap();
        assert!(!git_ok(&repo, &["config", "--get-all", "remote.origin.pushurl"]).await);

        let db_path = repo.join("music.db");
        create_collection(&db_path, &track("one")).await;
        let report = sync_repository(repo, Some(db_path), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
        assert!(git_ok(&new, &["rev-parse", "--verify", "HEAD"]).await);
        assert!(!git_ok(&old, &["rev-parse", "--verify", "HEAD"]).await);
    }

    #[tokio::test]
    async fn sync_commits_and_pushes_deleted_collection() {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        command(&remote, &["init", "--bare"]);
        let repo = temp.path().join("repo");
        init_if_needed(&repo).await.unwrap();
        set_remote(&repo, Some(remote.to_str().unwrap()))
            .await
            .unwrap();
        let keep = repo.join("keep.db");
        let gone = repo.join("gone.db");
        create_collection(&keep, &track("one")).await;
        create_collection(&gone, &track("two")).await;
        let report = sync_repository(repo.clone(), Some(gone.clone()), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        crate::utils::remove_collection_file(&gone).unwrap();
        let report = sync_repository(repo.clone(), Some(gone), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        let pushed = tree_paths(&remote, "HEAD").await.unwrap();
        assert!(pushed.contains("keep.db"));
        assert!(!pushed.contains("gone.db"));
    }

    #[tokio::test]
    async fn sync_commits_and_pushes_renamed_collection() {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        command(&remote, &["init", "--bare"]);
        let repo = temp.path().join("repo");
        init_if_needed(&repo).await.unwrap();
        set_remote(&repo, Some(remote.to_str().unwrap()))
            .await
            .unwrap();
        let old = repo.join("old.db");
        let new = repo.join("new.db");
        create_collection(&old, &track("one")).await;
        let report = sync_repository(repo.clone(), Some(old.clone()), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        crate::utils::rename_collection_file(&old, &new).unwrap();
        // Only the new path is passed; the old one is found as a deleted file.
        let report = sync_repository(repo.clone(), Some(new.clone()), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        let pushed = tree_paths(&remote, "HEAD").await.unwrap();
        assert!(pushed.contains("new.db"));
        assert!(!pushed.contains("old.db"));
        let ids = db::get_all_yt_ids(&db::open_pool(&new).await.unwrap())
            .await
            .unwrap();
        assert!(ids.contains("one"));
    }

    /// Two clones of a remote that has `music.db` with track "one".
    async fn two_clones(temp: &Path) -> (PathBuf, PathBuf, PathBuf) {
        two_clones_of(temp, &track("one")).await
    }

    /// A remote holding a collection with `seed`, and two clones of it.
    async fn two_clones_of(temp: &Path, seed: &Track) -> (PathBuf, PathBuf, PathBuf) {
        let remote = temp.join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        command(&remote, &["init", "--bare"]);
        let first = temp.join("first");
        init_if_needed(&first).await.unwrap();
        set_remote(&first, Some(remote.to_str().unwrap()))
            .await
            .unwrap();
        let db_path = first.join("music.db");
        create_collection(&db_path, seed).await;
        let report = sync_repository(first.clone(), Some(db_path), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
        let second = temp.join("second");
        clone_repo(remote.to_str().unwrap(), &second).await.unwrap();
        (remote, first, second)
    }

    async fn set_rating(db_path: &Path, rating: i64) {
        let pool = db::open_pool(db_path).await.unwrap();
        db::update_rating(&pool, "one", Some(rating)).await.unwrap();
        pool.close().await;
    }

    async fn play(db_path: &Path) {
        play_on(
            db_path,
            chrono::NaiveDate::from_ymd_opt(2026, 8, 18).unwrap(),
        )
        .await;
    }

    async fn play_on(db_path: &Path, date: chrono::NaiveDate) {
        let pool = db::open_pool(db_path).await.unwrap();
        db::inc_play_count(&pool, "one", date).await.unwrap();
        pool.close().await;
    }

    async fn track_one(db_path: &Path) -> Track {
        let pool = db::open_pool(db_path).await.unwrap();
        let track = db::get_track(&pool, "one").await.unwrap().unwrap();
        pool.close().await;
        track
    }

    fn playlist(id: &str) -> crate::playlists::Playlist {
        crate::playlists::Playlist {
            id: id.into(),
            name: format!("Playlist {id}"),
            collection: "music.db".into(),
            filter_model: Default::default(),
            sort_model: vec![],
            limit: None,
            spotify: None,
            youtube: None,
            updated_at: 1,
        }
    }

    async fn playlist_ids(repo: &Path) -> Vec<String> {
        crate::playlists::load(repo)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect()
    }

    #[tokio::test]
    async fn sync_merges_playlists_added_on_both_sides() {
        let temp = tempfile::tempdir().unwrap();
        let (_remote, first, second) = two_clones(temp.path()).await;
        let path = crate::playlists::playlists_path(&first);
        crate::playlists::save(&first, &[playlist("a")])
            .await
            .unwrap();
        let report = sync_repository(first.clone(), Some(path.clone()), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
        let report = sync_repository(second.clone(), None, true, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
        assert_eq!(playlist_ids(&second).await, ["a"]);

        crate::playlists::save(&first, &[playlist("a"), playlist("b")])
            .await
            .unwrap();
        let report = sync_repository(first.clone(), Some(path), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
        crate::playlists::save(&second, &[playlist("a"), playlist("c")])
            .await
            .unwrap();
        let report = sync_repository(second.clone(), None, true, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
        assert_eq!(playlist_ids(&second).await, ["a", "c", "b"]);

        let report = sync_repository(first.clone(), None, true, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
        assert_eq!(playlist_ids(&first).await, ["a", "c", "b"]);
    }

    #[tokio::test]
    async fn sync_commits_playlists_in_repos_that_ignore_them() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_if_needed(&repo).await.unwrap();
        // The .gitignore of repos created before playlists existed.
        std::fs::write(repo.join(".gitignore"), "*\n!.gitignore\n!*.db\n").unwrap();
        create_collection(&repo.join("music.db"), &track("one")).await;
        crate::playlists::save(&repo, &[playlist("a")])
            .await
            .unwrap();
        // Not told about the playlists file, as after a restart with uncommitted changes.
        let report = sync_repository(repo.clone(), None, false, None).await;
        assert_eq!(report.state, GitSyncState::LocalOnly, "{}", report.message);
        let tracked = std::process::Command::new("git")
            .args(["ls-files"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&tracked.stdout);
        assert!(tracked.contains("playlists.json"), "{tracked}");
        assert!(tracked.contains("music.db"), "{tracked}");
    }

    #[tokio::test]
    async fn sync_adds_up_plays_of_the_same_track_on_both_sides() {
        let temp = tempfile::tempdir().unwrap();
        // Plays before the clones diverged count once, not once per side.
        let seeded = Track {
            play_count: 4,
            ..track("one")
        };
        let (remote, first, second) = two_clones_of(temp.path(), &seeded).await;
        let earlier = chrono::NaiveDate::from_ymd_opt(2026, 8, 18).unwrap();
        let later = chrono::NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        play_on(&first.join("music.db"), later).await;
        play_on(&first.join("music.db"), later).await;
        let report = sync_repository(first.clone(), None, false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        play_on(&second.join("music.db"), earlier).await;
        let report = sync_repository(second.clone(), None, false, None).await;
        assert_eq!(report.state, GitSyncState::ConflictRequiresAttention);
        let report = sync_repository(second.clone(), None, true, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        let merged = track_one(&second.join("music.db")).await;
        assert_eq!(merged.play_count, 7);
        assert_eq!(merged.last_played, Some(later));

        let report = sync_repository(first.clone(), None, true, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
        assert_eq!(track_one(&first.join("music.db")).await, merged);
        let pushed = run_git(&remote, &["rev-parse", "HEAD"], GIT_TIMEOUT)
            .await
            .unwrap();
        let local = run_git(&first, &["rev-parse", "HEAD"], GIT_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(output_text(pushed), output_text(local));
    }

    /// One play on each side on the same day leaves the rows, and possibly the whole files,
    /// identical; they're still two plays.
    async fn merge_same_day_plays(other_change_upstream: bool) {
        let temp = tempfile::tempdir().unwrap();
        let (_remote, first, second) = two_clones(temp.path()).await;
        play(&first.join("music.db")).await;
        if other_change_upstream {
            let pool = db::open_pool(&first.join("music.db")).await.unwrap();
            db::insert_track(&pool, &track("two"), false).await.unwrap();
            pool.close().await;
        }
        let report = sync_repository(first, None, false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        // Committed in the same second as the first clone's: only the author tells the commits
        // apart, which would otherwise be the very same commit, with nothing to merge.
        play(&second.join("music.db")).await;
        let report = sync_repository(second.clone(), None, false, None).await;
        assert_eq!(report.state, GitSyncState::ConflictRequiresAttention);
        let report = sync_repository(second.clone(), None, true, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        let merged = track_one(&second.join("music.db")).await;
        assert_eq!(merged.play_count, 2);
        let pool = db::open_pool(&second.join("music.db")).await.unwrap();
        let has_two = db::get_track(&pool, "two").await.unwrap().is_some();
        pool.close().await;
        assert_eq!(has_two, other_change_upstream);
    }

    #[tokio::test]
    async fn clones_commit_as_different_authors() {
        let temp = tempfile::tempdir().unwrap();
        let (_remote, first, second) = two_clones(temp.path()).await;
        let (first_name, first_email) = author(&first).await.unwrap();
        let (second_name, second_email) = author(&second).await.unwrap();
        assert!(first_name.starts_with("Stereo"), "{first_name}");
        assert_eq!(first_name, second_name);
        assert_ne!(first_email, second_email);
        // Kept, not drawn anew for each commit.
        assert_eq!(author(&first).await.unwrap().1, first_email);
        let log = run_git(&first, &["log", "-1", "--format=%an <%ae>"], GIT_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(output_text(log), format!("{first_name} <{first_email}>"));
    }

    #[tokio::test]
    async fn sync_adds_up_same_day_plays_in_identical_collections() {
        merge_same_day_plays(false).await;
    }

    #[tokio::test]
    async fn sync_adds_up_same_day_plays_in_otherwise_different_collections() {
        merge_same_day_plays(true).await;
    }

    #[tokio::test]
    async fn sync_merges_upstream_edit_into_locally_renamed_collection() {
        let temp = tempfile::tempdir().unwrap();
        let (remote, first, second) = two_clones(temp.path()).await;
        set_rating(&first.join("music.db"), 5).await;
        let report = sync_repository(first, None, false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        let renamed = second.join("renamed.db");
        play(&second.join("music.db")).await;
        crate::utils::rename_collection_file(&second.join("music.db"), &renamed).unwrap();
        let report = sync_repository(second.clone(), Some(renamed.clone()), false, None).await;
        assert_eq!(report.state, GitSyncState::ConflictRequiresAttention);
        let report = sync_repository(second.clone(), None, true, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        let merged = track_one(&renamed).await;
        assert_eq!(merged.rating, Some(5));
        assert_eq!(merged.play_count, 1);
        assert!(!second.join("music.db").exists());
        let pushed = tree_paths(&remote, "HEAD").await.unwrap();
        assert!(pushed.contains("renamed.db"));
        assert!(!pushed.contains("music.db"));
    }

    #[tokio::test]
    async fn sync_merges_local_edit_into_upstream_renamed_collection() {
        let temp = tempfile::tempdir().unwrap();
        let (remote, first, second) = two_clones(temp.path()).await;
        set_rating(&first.join("music.db"), 5).await;
        crate::utils::rename_collection_file(&first.join("music.db"), &first.join("renamed.db"))
            .unwrap();
        let report = sync_repository(first, None, false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        play(&second.join("music.db")).await;
        let report = sync_repository(second.clone(), None, true, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        let merged = track_one(&second.join("renamed.db")).await;
        assert_eq!(merged.rating, Some(5));
        assert_eq!(merged.play_count, 1);
        assert!(!second.join("music.db").exists());
        let pushed = tree_paths(&remote, "HEAD").await.unwrap();
        assert!(pushed.contains("renamed.db"));
        assert!(!pushed.contains("music.db"));
    }

    #[tokio::test]
    async fn explicit_resolution_follows_rename() {
        for (strategy, rating) in [
            (GitConflictStrategy::KeepLocal, 4),
            (GitConflictStrategy::KeepUpstream, 5),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (remote, first, second) = two_clones(temp.path()).await;
            set_rating(&first.join("music.db"), 5).await;
            let report = sync_repository(first, None, false, None).await;
            assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

            let renamed = second.join("renamed.db");
            set_rating(&second.join("music.db"), 4).await;
            crate::utils::rename_collection_file(&second.join("music.db"), &renamed).unwrap();
            let report = sync_repository(second.clone(), Some(renamed.clone()), true, None).await;
            assert_eq!(report.state, GitSyncState::ConflictRequiresAttention);
            let recovery = std::fs::read_dir(temp.path().join(".stereo-recovery/second"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            assert!(recovery.join("local-renamed.db").exists());
            assert!(recovery.join("upstream-music.db").exists());

            let report = sync_repository(second.clone(), None, true, Some(strategy)).await;
            assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);
            assert_eq!(
                track_one(&renamed).await.rating,
                Some(rating),
                "{strategy:?}"
            );
            assert!(!second.join("music.db").exists());
            let pushed = tree_paths(&remote, "HEAD").await.unwrap();
            assert!(pushed.contains("renamed.db"));
            assert!(!pushed.contains("music.db"));
        }
    }

    #[test]
    fn renaming_collection_does_not_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a.db");
        let b = temp.path().join("b.db");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        assert!(crate::utils::rename_collection_file(&a, &b).is_err());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b");
    }

    #[tokio::test]
    async fn collection_names_are_not_git_patterns() {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        command(&remote, &["init", "--bare"]);
        let repo = temp.path().join("repo");
        init_if_needed(&repo).await.unwrap();
        set_remote(&repo, Some(remote.to_str().unwrap()))
            .await
            .unwrap();
        let pattern = repo.join("mix*.db");
        create_collection(&pattern, &track("one")).await;
        create_collection(&repo.join("mix1.db"), &track("two")).await;
        create_collection(&repo.join("keep.db"), &track("three")).await;
        let report = sync_repository(repo.clone(), Some(pattern.clone()), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        crate::utils::remove_collection_file(&pattern).unwrap();
        let report = sync_repository(repo.clone(), Some(pattern), false, None).await;
        assert_eq!(report.state, GitSyncState::Synced, "{}", report.message);

        let pushed = tree_paths(&remote, "HEAD").await.unwrap();
        assert!(!pushed.contains("mix*.db"));
        assert!(pushed.contains("mix1.db"));
        assert!(pushed.contains("keep.db"));
    }

    #[tokio::test]
    async fn deleting_never_committed_collection_does_not_block_sync() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_if_needed(&repo).await.unwrap();
        let keep = repo.join("keep.db");
        create_collection(&keep, &track("one")).await;
        let report = sync_repository(repo.clone(), Some(keep.clone()), false, None).await;
        assert_eq!(report.state, GitSyncState::LocalOnly, "{}", report.message);

        // Created and deleted again before any sync committed it.
        let scratch = repo.join("scratch.db");
        create_collection(&scratch, &track("two")).await;
        crate::utils::remove_collection_file(&scratch).unwrap();
        let report = sync_repository(repo.clone(), Some(scratch), false, None).await;
        assert_eq!(report.state, GitSyncState::LocalOnly, "{}", report.message);
        assert!(read_dirty(&repo).is_empty());

        // Later changes still sync.
        let pool = db::open_pool(&keep).await.unwrap();
        db::insert_track(&pool, &track("three"), false)
            .await
            .unwrap();
        pool.close().await;
        let report = sync_repository(repo.clone(), Some(keep), false, None).await;
        assert_eq!(report.state, GitSyncState::LocalOnly, "{}", report.message);
        assert!(discover_dirty_databases(&repo).await.unwrap().is_empty());
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
