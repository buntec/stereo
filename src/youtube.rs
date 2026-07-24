//! YouTube Data API v3 access for playlist export.
//!
//! Unlike Spotify (an unofficial/scraped API), this uses real OAuth against Google. Stereo
//! doesn't ship its own OAuth client (that would require bundling a client secret and passing
//! Google's verification for a public app), so the user registers their own "Desktop app" OAuth
//! client in Google Cloud Console and pastes its id/secret in. The authorization-code + PKCE
//! exchange and all subsequent token refreshes happen here, server-side, so the long-lived
//! refresh token never has to touch the browser.

use anyhow::Context;
use serde_json::Value;
use wreq::Client;

const YOUTUBE_SESSION_FILE: &str = "youtube_session.json";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const API_BASE: &str = "https://www.googleapis.com/youtube/v3";

// Refresh a bit before real expiry to avoid skew-induced 401s.
const REFRESH_SKEW_SECS: i64 = 60;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct YouTubeSession {
    pub client_id: String,
    pub client_secret: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    /// Unix timestamp (seconds) the access token expires at.
    pub expires_at: i64,
}

impl YouTubeSession {
    pub async fn load(home: &std::path::Path) -> Option<YouTubeSession> {
        let path = home.join(YOUTUBE_SESSION_FILE);
        let data = tokio::fs::read(&path).await.ok()?;
        serde_json::from_slice(&data).ok()
    }

    async fn save(&self, home: &std::path::Path) -> anyhow::Result<()> {
        let path = home.join(YOUTUBE_SESSION_FILE);
        let data = serde_json::to_vec(self)?;
        tokio::fs::write(&path, data).await?;
        Ok(())
    }

    pub async fn clear(home: &std::path::Path) -> anyhow::Result<()> {
        let path = home.join(YOUTUBE_SESSION_FILE);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn connected(&self) -> bool {
        self.refresh_token.is_some()
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Persists the client id/secret for a not-yet-connected session, so the frontend can start the
/// OAuth redirect against them without re-prompting for credentials on every connect attempt.
pub async fn save_credentials(
    home: &std::path::Path,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<YouTubeSession> {
    let session = YouTubeSession {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        access_token: None,
        refresh_token: None,
        expires_at: 0,
    };
    session.save(home).await?;
    Ok(session)
}

/// Exchanges an authorization code (from the PKCE redirect) for access + refresh tokens, using
/// the client id/secret previously stored via [`save_credentials`].
pub async fn exchange_code(
    client: &Client,
    home: &std::path::Path,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> anyhow::Result<YouTubeSession> {
    let mut session = YouTubeSession::load(home)
        .await
        .context("No YouTube OAuth client id/secret saved")?;

    let resp: Value = client
        .post(TOKEN_URL)
        .form(&[
            ("code", code),
            ("client_id", &session.client_id),
            ("client_secret", &session.client_secret),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier),
        ])
        .send()
        .await?
        .json()
        .await?;

    let access_token = resp["access_token"]
        .as_str()
        .context("no access_token in token response")?
        .to_string();
    let refresh_token = resp["refresh_token"].as_str().map(String::from);
    let expires_in = resp["expires_in"].as_i64().unwrap_or(3600);

    session.access_token = Some(access_token);
    if refresh_token.is_some() {
        session.refresh_token = refresh_token;
    }
    session.expires_at = now() + expires_in;
    session.save(home).await?;
    Ok(session)
}

async fn refresh_access_token(
    client: &Client,
    home: &std::path::Path,
    mut session: YouTubeSession,
) -> anyhow::Result<YouTubeSession> {
    let refresh_token = session
        .refresh_token
        .clone()
        .context("No YouTube refresh token")?;

    let resp: Value = client
        .post(TOKEN_URL)
        .form(&[
            ("refresh_token", refresh_token.as_str()),
            ("client_id", session.client_id.as_str()),
            ("client_secret", session.client_secret.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await?
        .json()
        .await?;

    let access_token = resp["access_token"]
        .as_str()
        .context("no access_token in refresh response")?
        .to_string();
    let expires_in = resp["expires_in"].as_i64().unwrap_or(3600);

    session.access_token = Some(access_token);
    session.expires_at = now() + expires_in;
    session.save(home).await?;
    Ok(session)
}

/// Loads the saved session and returns a valid (non-expired) access token, refreshing it first
/// if needed. Errors if no account has been connected yet.
pub async fn ensure_access_token(
    client: &Client,
    home: &std::path::Path,
) -> anyhow::Result<String> {
    let session = YouTubeSession::load(home)
        .await
        .filter(YouTubeSession::connected)
        .context("Connect a YouTube account first")?;

    let session = if now() + REFRESH_SKEW_SECS >= session.expires_at {
        refresh_access_token(client, home, session).await?
    } else {
        session
    };

    session.access_token.context("no YouTube access token")
}

pub async fn create_playlist(
    client: &Client,
    access_token: &str,
    name: &str,
) -> anyhow::Result<(String, String)> {
    let resp: Value = client
        .post(format!("{API_BASE}/playlists?part=snippet,status"))
        .bearer_auth(access_token)
        .json(&serde_json::json!({
            "snippet": { "title": name, "description": "Created by stereo" },
            "status": { "privacyStatus": "private" },
        }))
        .send()
        .await?
        .json()
        .await?;

    let id = resp["id"]
        .as_str()
        .context("no id in create-playlist response")?
        .to_string();
    let url = format!("https://www.youtube.com/playlist?list={id}");
    Ok((id, url))
}

/// Adds a single video to a playlist. A 409 (already present) is treated as success.
pub async fn add_video(
    client: &Client,
    access_token: &str,
    playlist_id: &str,
    video_id: &str,
) -> anyhow::Result<()> {
    let resp = client
        .post(format!("{API_BASE}/playlistItems?part=snippet"))
        .bearer_auth(access_token)
        .json(&serde_json::json!({
            "snippet": {
                "playlistId": playlist_id,
                "resourceId": { "kind": "youtube#video", "videoId": video_id },
            }
        }))
        .send()
        .await?;

    if !resp.status().is_success() && resp.status().as_u16() != 409 {
        anyhow::bail!("Failed to add video {video_id}: {}", resp.status());
    }
    Ok(())
}
