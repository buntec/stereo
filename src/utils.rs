use std::{
    io,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use regex::Regex;
use tokio::io::AsyncWriteExt;

/// Read a file containing credentials after restricting it to the current user.
///
/// Windows user-profile directories already inherit per-user ACLs. On Unix, explicitly repair
/// files created by older Stereo versions, which otherwise inherited a potentially permissive
/// process umask.
pub async fn read_secret_file(path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(unix)]
    tokio::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).await?;

    tokio::fs::read(path).await
}

/// Write credentials to an owner-only file.
pub async fn write_secret_file(path: &Path, data: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    if tokio::fs::try_exists(path).await? {
        tokio::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .await?;
    }

    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }

    let mut file = options.open(path).await?;
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .await?;
    file.write_all(data).await?;
    file.sync_all().await
}

// Strips a trailing "feat./ft./featuring X" clause from a title. That artist is normally
// already present in the artists list, so leaving it in the title duplicates it as free text in
// a search query, which empirically drives search engines (Spotify's included) to bury the
// correct track outside the first few results.
static FEAT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*[(\[]?\b(feat\.?|ft\.?|featuring)\b.*$").unwrap());

pub fn strip_feat(title: &str) -> String {
    FEAT_RE.replace(title, "").trim().to_string()
}

// Beatport disambiguates identically-named artists by appending a short parenthetical tag to
// the artist name, e.g. "THMS (US)", "Kadosh (ofc)", "KOKA (DE)" — scraped and stored verbatim
// as part of the artist name. Other catalogs (Spotify included) don't use this convention, and
// empirically it drives search engines to bury the correct track outside the first few results
// just as badly as an un-stripped "feat." clause does.
static ARTIST_DISAMBIGUATOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\([A-Za-z0-9]{2,6}\)").unwrap());

pub fn strip_artist_disambiguators(artists: &str) -> String {
    ARTIST_DISAMBIGUATOR_RE
        .replace_all(artists, "")
        .trim()
        .to_string()
}

/// True if a mix name denotes the plain original (no remix identity to match/search on).
pub fn is_original_mix(mix_name: &str) -> bool {
    matches!(
        mix_name.trim().to_lowercase().as_str(),
        "" | "original mix" | "original"
    )
}

// A trailing "(Original Mix)"/"- Original Mix" carries no distinguishing information — by
// definition there's only one "original" — so a third party spelling it out in their track name
// while we have no mix info at all (meaning we didn't record one, not that we know it's a remix)
// shouldn't count against an otherwise-exact match.
static GENERIC_MIX_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*[-(]\s*original(?:\s+mix)?\)?\s*$").unwrap());

pub fn strip_generic_mix_suffix(track_name: &str) -> String {
    GENERIC_MIX_SUFFIX_RE
        .replace(track_name, "")
        .trim()
        .to_string()
}

/// Lowercases, strips punctuation, and sorts words alphabetically, so similarity comparisons
/// are insensitive to artist-ordering differences between our metadata and a third party's
/// (e.g. "Marcus Worgull, Daniel Bortz" vs. Spotify's "Daniel Bortz, Marcus Worgull" for the
/// same track) instead of penalizing them as if they were edits.
///
/// Words are normalized before sorting, so neither case nor attached punctuation can change
/// their order.
pub fn normalize_for_match(s: &str) -> String {
    let cleaned: String = s
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let mut words: Vec<&str> = cleaned.split_whitespace().collect();
    words.sort_unstable();
    words.join(" ")
}

pub fn collection_label(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
}

pub async fn ensure_default_repo(home: &Path) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(home).await?;

    let has_repos = std::fs::read_dir(home)
        .map(|entries| {
            entries.flatten().any(|e| {
                e.file_type().map(|t| t.is_dir()).unwrap_or(false) && e.path().join(".git").exists()
            })
        })
        .unwrap_or(false);

    if has_repos {
        return Ok(());
    }

    let root_dbs: Vec<PathBuf> = std::fs::read_dir(home)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    e.path().extension().map(|x| x == "db").unwrap_or(false)
                        && e.file_type().map(|t| t.is_file()).unwrap_or(false)
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();

    let default_repo = home.join("stereo");
    tokio::fs::create_dir_all(&default_repo).await?;

    for src in &root_dbs {
        if let Some(filename) = src.file_name() {
            let dst = default_repo.join(filename);
            if let Err(e) = tokio::fs::rename(src, &dst).await {
                tracing::warn!("Failed to move {} to stereo/: {e}", src.display());
            }
        }
    }

    let root_gitignore = home.join(".gitignore");
    if root_gitignore.exists() {
        let _ = tokio::fs::rename(&root_gitignore, default_repo.join(".gitignore")).await;
    }

    crate::git::init_if_needed(&default_repo).await?;
    tracing::info!("Created default repo at {}", default_repo.display());
    Ok(())
}

pub async fn list_repos(home: &Path) -> Vec<crate::models::Repo> {
    let Ok(entries) = std::fs::read_dir(home) else {
        return vec![];
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            e.file_type().map(|t| t.is_dir()).unwrap_or(false) && e.path().join(".git").exists()
        })
        .map(|e| e.path())
        .collect();
    dirs.sort();

    let mut repos = Vec::new();
    for path in dirs {
        let remote = crate::git::get_remote(&path).await;
        repos.push(crate::models::Repo { path, remote });
    }
    repos
}

pub fn list_repo_collections(repo: &Path) -> Vec<crate::models::Collection> {
    let Ok(entries) = std::fs::read_dir(repo) else {
        return vec![];
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            e.path().extension().map(|x| x == "db").unwrap_or(false)
                && e.file_type().map(|t| t.is_file()).unwrap_or(false)
        })
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| crate::models::Collection { path, size: 0 })
        .collect()
}

/// Removes a collection's database file along with any SQLite journal files left beside it.
pub fn remove_collection_file(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)?;
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        match std::fs::remove_file(PathBuf::from(sidecar)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// Moves a collection's database file (and any SQLite journal files beside it) to `to`, failing
/// rather than overwriting if `to` already exists. Both paths must be on the same filesystem.
///
/// Either everything moves or nothing does: on error, every file is back at its original path.
pub fn rename_collection_file(from: &Path, to: &Path) -> std::io::Result<()> {
    let with_suffix = |path: &Path, suffix: &str| {
        let mut path = path.as_os_str().to_owned();
        path.push(suffix);
        PathBuf::from(path)
    };
    let mut pairs = vec![(from.to_path_buf(), to.to_path_buf())];
    for suffix in ["-journal", "-wal", "-shm"] {
        let (src, dst) = (with_suffix(from, suffix), with_suffix(to, suffix));
        let src_exists = std::fs::symlink_metadata(&src).is_ok();
        if std::fs::symlink_metadata(&dst).is_ok() {
            // Even without a counterpart to move, a stray journal at the destination would be
            // taken for the moved database's own and could be "rolled back" into it.
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", dst.display()),
            ));
        }
        if src_exists {
            pairs.push((src, dst));
        }
    }

    // Link every file under its new name before removing any old name, so that undoing a
    // failure only means removing the links made so far. A hard link can't clobber an existing
    // file, unlike `rename`. On a case-insensitive filesystem this also refuses case-only
    // renames, which git wouldn't pick up anyway.
    for (linked, (src, dst)) in pairs.iter().enumerate() {
        if let Err(e) = std::fs::hard_link(src, dst) {
            for (_, dst) in &pairs[..linked] {
                let _ = std::fs::remove_file(dst);
            }
            return Err(e);
        }
    }
    // The main file goes last, so a failure leaves it at its original path.
    for (removed, (src, _)) in pairs.iter().enumerate().rev() {
        if let Err(e) = std::fs::remove_file(src) {
            for (src, dst) in &pairs[removed + 1..] {
                let _ = std::fs::hard_link(dst, src);
            }
            for (_, dst) in &pairs {
                let _ = std::fs::remove_file(dst);
            }
            return Err(e);
        }
    }
    Ok(())
}

pub fn get_path_completions(prefix: &str) -> Vec<String> {
    let expanded = expand_tilde(prefix);
    let path = PathBuf::from(&expanded);

    let (parent, partial) = if prefix.ends_with('/') || prefix == "~" {
        (path.clone(), String::new())
    } else {
        let parent = path.parent().unwrap_or(&path).to_path_buf();
        let partial = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        (parent, partial)
    };

    if !parent.is_dir() {
        return vec![];
    }

    match std::fs::read_dir(&parent) {
        Ok(entries) => entries
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(partial.as_str())
            })
            .map(|e| e.path().to_string_lossy().to_string())
            .collect(),
        Err(_) => vec![],
    }
}

fn expand_tilde(s: &str) -> String {
    if s.starts_with('~') {
        if let Some(home) = std::env::var("HOME")
            .ok()
            .or_else(|| std::env::var("USERPROFILE").ok())
        {
            return s.replacen('~', &home, 1);
        }
    }
    s.to_string()
}

#[derive(Debug, PartialEq)]
pub enum PathKind {
    File,
    Url,
}

pub fn is_file_or_url(s: &str) -> Option<PathKind> {
    if s.starts_with("http://") || s.starts_with("https://") {
        return Some(PathKind::Url);
    }
    if Path::new(s).is_absolute() {
        return Some(PathKind::File);
    }
    None
}

pub async fn download_file(url: &str, destination: &Path) -> anyhow::Result<()> {
    use anyhow::Context;
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let client = wreq::Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .context("download request failed")?;
    resp.error_for_status_ref()
        .context("download returned error status")?;

    let mut file = tokio::fs::File::create(destination).await?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        file.write_all(&chunk?).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renaming_collection_moves_journal_along() {
        let temp = tempfile::tempdir().unwrap();
        let (old, new) = (temp.path().join("old.db"), temp.path().join("new.db"));
        std::fs::write(&old, "db").unwrap();
        std::fs::write(temp.path().join("old.db-journal"), "journal").unwrap();
        rename_collection_file(&old, &new).unwrap();
        assert!(!old.exists());
        assert!(!temp.path().join("old.db-journal").exists());
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "db");
        assert_eq!(
            std::fs::read_to_string(temp.path().join("new.db-journal")).unwrap(),
            "journal"
        );
    }

    #[test]
    fn failed_sidecar_move_leaves_collection_in_place() {
        let temp = tempfile::tempdir().unwrap();
        let (old, new) = (temp.path().join("old.db"), temp.path().join("new.db"));
        std::fs::write(&old, "db").unwrap();
        std::fs::write(temp.path().join("old.db-journal"), "journal").unwrap();
        // Directories can't be hard-linked, so the database and journal are linked before the
        // "shm file" fails, as a full disk or a permission error could; nothing may stay moved.
        std::fs::create_dir(temp.path().join("old.db-shm")).unwrap();

        assert!(rename_collection_file(&old, &new).is_err());
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "db");
        assert_eq!(
            std::fs::read_to_string(temp.path().join("old.db-journal")).unwrap(),
            "journal"
        );
        assert!(temp.path().join("old.db-shm").is_dir());
        assert!(!new.exists());
        assert!(!temp.path().join("new.db-journal").exists());
        assert!(!temp.path().join("new.db-shm").exists());
    }

    #[test]
    fn renaming_collection_refuses_stray_destination_journal() {
        let temp = tempfile::tempdir().unwrap();
        let (old, new) = (temp.path().join("old.db"), temp.path().join("new.db"));
        std::fs::write(&old, "db").unwrap();
        std::fs::write(temp.path().join("new.db-journal"), "stale").unwrap();
        assert!(rename_collection_file(&old, &new).is_err());
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "db");
        assert!(!new.exists());
    }

    #[test]
    fn normalize_for_match_ignores_case_order_and_punctuation() {
        assert_eq!(
            normalize_for_match("DEADMAU5 Kaskade"),
            normalize_for_match("Kaskade deadmau5")
        );
        assert_eq!(
            normalize_for_match("Marcus Worgull, Daniel Bortz"),
            normalize_for_match("Daniel Bortz, Marcus Worgull")
        );
        assert_eq!(normalize_for_match("A & B"), "a b");
    }
}

#[cfg(all(test, unix))]
mod secret_file_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn secret_files_are_owner_only_and_existing_permissions_are_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");

        tokio::fs::write(&path, b"old secret").await.unwrap();
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .await
            .unwrap();

        assert_eq!(read_secret_file(&path).await.unwrap(), b"old secret");
        assert_eq!(
            tokio::fs::metadata(&path)
                .await
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        write_secret_file(&path, b"new secret").await.unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"new secret");
        assert_eq!(
            tokio::fs::metadata(&path)
                .await
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let new_path = dir.path().join("new-session.json");
        write_secret_file(&new_path, b"fresh secret").await.unwrap();
        assert_eq!(
            tokio::fs::metadata(&new_path)
                .await
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
