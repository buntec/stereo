use std::path::Path;
use tokio::process::Command;

pub async fn get_remote(home: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(home)
        .output()
        .await
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

pub async fn set_remote(home: &Path, url: Option<&str>) -> anyhow::Result<()> {
    // Remove existing origin; ignore error if it doesn't exist
    let _ = Command::new("git")
        .args(["remote", "remove", "origin"])
        .current_dir(home)
        .output()
        .await;

    if let Some(url) = url {
        if !url.is_empty() {
            let out = Command::new("git")
                .args(["remote", "add", "origin", url])
                .current_dir(home)
                .output()
                .await?;
            if !out.status.success() {
                anyhow::bail!(
                    "git remote add failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
        }
    }
    Ok(())
}

async fn git(args: &[&str], dir: &Path) -> anyhow::Result<bool> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stderr.trim().is_empty() {
            tracing::warn!("git {}: {}", args.join(" "), stderr.trim());
        }
    }
    Ok(out.status.success())
}

pub fn repo_name_from_url(url: &str) -> String {
    url.trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit('/')
        .next()
        .unwrap_or("repo")
        .to_string()
}

pub async fn clone_repo(url: &str, dest: &Path) -> anyhow::Result<()> {
    let dest_str = dest.to_string_lossy();
    let out = Command::new("git")
        .args(["clone", url, &*dest_str])
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

pub async fn check_push_access(dir: &Path) -> bool {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        Command::new("git")
            .args(["push", "--dry-run", "origin", "HEAD"])
            .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
            .current_dir(dir)
            .output(),
    )
    .await;
    matches!(result, Ok(Ok(out)) if out.status.success())
}

pub async fn init_if_needed(home: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(home)?;
    if home.join(".git").exists() {
        return Ok(());
    }
    if !git(&["init"], home).await? {
        anyhow::bail!("git init failed");
    }
    tokio::fs::write(home.join(".gitignore"), "*\n!.gitignore\n!*.db\n").await?;
    tracing::info!("Initialized git repo in {}", home.display());
    Ok(())
}

pub async fn commit_and_push(home: std::path::PathBuf, db_path: std::path::PathBuf) {
    let Some(db_name) = db_path.file_name().and_then(|n| n.to_str()) else {
        return;
    };

    // Only manage DBs inside the home dir
    if !db_path.starts_with(&home) {
        return;
    }

    if let Err(e) = do_commit_and_push(&home, db_name).await {
        tracing::warn!("git commit failed: {e}");
    }
}

async fn do_commit_and_push(home: &Path, db_name: &str) -> anyhow::Result<()> {
    git(&["add", db_name], home).await?;

    // Nothing staged → nothing to commit
    let clean = git(&["diff", "--cached", "--quiet"], home).await?;
    if clean {
        return Ok(());
    }

    let msg = format!("update {db_name}");
    if !git(&["commit", "-m", &msg], home).await? {
        anyhow::bail!("git commit failed");
    }
    tracing::info!("git committed {db_name}");

    // Push only if a remote named "origin" is configured
    let has_remote = git(&["remote", "get-url", "origin"], home).await?;
    if has_remote {
        if !git(&["push", "--set-upstream", "origin", "HEAD"], home).await? {
            tracing::warn!("git push failed");
        } else {
            tracing::info!("git pushed {db_name}");
        }
    }

    Ok(())
}
