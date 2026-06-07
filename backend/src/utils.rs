use std::path::{Path, PathBuf};

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
            .filter(|e| e.file_name().to_string_lossy().starts_with(partial.as_str()))
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
