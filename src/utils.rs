use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use regex::Regex;

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
pub fn normalize_for_match(s: &str) -> String {
    let mut words: Vec<&str> = s.split_whitespace().collect();
    words.sort_unstable();
    words
        .join(" ")
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
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
        if remote.is_some() {
            repos.push(crate::models::Repo { path, remote });
        }
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
