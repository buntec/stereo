//! Apple Music's catalog, searched through Apple's public iTunes Search API
//! (`itunes.apple.com/search`): JSON, no key, cookie or login. It has no BPM, key or label, but
//! covers the mainstream catalog the DJ stores miss, and its track ids are what Shazam's
//! Apple-id mapping takes (`scraper::shazam_id_for_track`).
//!
//! Apple documents a limit of about 20 requests a minute per IP, so this backend is only used for
//! searches a person starts (fuzzy, artist, the Shazam lookup), never in loops over many names
//! such as new releases.

use std::sync::LazyLock;

use anyhow::Context;
use chrono::NaiveDate;
use regex::Regex;
use serde_json::Value;
use wreq::Client;

use crate::models::Track;

const SEARCH_URL: &str = "https://itunes.apple.com/search";
/// The storefront searched; Shazam's mapping (`/a2st/US/...`) is asked about the same one.
const COUNTRY: &str = "US";
/// The API's own maximum page size.
const MAX_LIMIT: usize = 200;

pub struct AMTrack {
    /// The Apple Music song id (`trackId`).
    pub id: i64,
    pub artists: Vec<String>,
    pub track_name: String,
    /// Taken from the end of Apple's title, where it keeps the version: "Song [Extended Mix]".
    pub mix_name: Option<String>,
    pub album: Option<String>,
    pub release_date: NaiveDate,
    pub genre: Option<String>,
}

impl AMTrack {
    pub fn into_track(self, yt_id: String) -> Track {
        Track {
            yt_id,
            title: self.track_name,
            mix_name: self.mix_name,
            artists: self.artists,
            album: self.album,
            release_date: Some(self.release_date),
            genre: self.genre,
            ..Default::default()
        }
    }
}

/// What a search term is matched against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchField {
    /// Titles, artists and albums alike.
    Any,
    /// Artist names only.
    Artist,
}

/// Searches Apple Music's songs for `term`.
pub async fn search_tracks(
    client: &Client,
    term: &str,
    field: SearchField,
    limit: usize,
) -> anyhow::Result<Vec<AMTrack>> {
    tracing::info!("Apple Music track search ({field:?}): {term}");
    let limit = limit.clamp(1, MAX_LIMIT).to_string();
    let mut query = vec![
        ("term", term),
        ("media", "music"),
        ("entity", "song"),
        ("country", COUNTRY),
        ("limit", limit.as_str()),
    ];
    if field == SearchField::Artist {
        query.push(("attribute", "artistTerm"));
    }
    let resp = client.get(SEARCH_URL).query(&query).send().await?;
    let status = resp.status();
    // Apple answers its rate limit with a bare 403.
    anyhow::ensure!(
        status.as_u16() != 403,
        "Apple Music refused the search (HTTP 403), most likely its rate limit of about 20 \
         searches a minute; try again in a minute"
    );
    anyhow::ensure!(
        status.is_success(),
        "Apple Music search failed with HTTP {status}"
    );
    let json: Value = serde_json::from_str(&resp.text().await?)
        .context("Apple Music search response is not valid JSON")?;
    let results = json["results"]
        .as_array()
        .context("no results in Apple Music search response")?;
    Ok(results.iter().filter_map(parse_track).collect())
}

fn parse_track(item: &Value) -> Option<AMTrack> {
    if item["kind"].as_str() != Some("song") {
        return None;
    }
    let id = item["trackId"].as_i64()?;
    let (title, mix_name) = split_mix(item["trackName"].as_str()?.trim());
    let (title, featured) = split_featured(&title);
    let mut artists = split_artists(item["artistName"].as_str()?);
    for name in featured {
        if !artists.iter().any(|a| a.eq_ignore_ascii_case(&name)) {
            artists.push(name);
        }
    }
    if title.is_empty() || artists.is_empty() {
        return None;
    }
    let release_date = item["releaseDate"]
        .as_str()
        .and_then(|s| s.get(..10))
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())?;
    let album = item["collectionName"]
        .as_str()
        .map(strip_release_kind)
        .filter(|s| !s.is_empty());
    let genre = item["primaryGenreName"]
        .as_str()
        .map(str::trim)
        // Apple's catch-all, not a genre.
        .filter(|s| !s.is_empty() && *s != "Music")
        .map(String::from);
    Some(AMTrack {
        id,
        artists,
        track_name: title,
        mix_name,
        album,
        release_date,
        genre,
    })
}

/// Splits Apple's joined artist credit, "A, B & C", into its names.
fn split_artists(credit: &str) -> Vec<String> {
    credit
        .split(", ")
        .flat_map(|part| part.split(" & "))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(String::from)
        .collect()
}

/// Splits a trailing bracketed version off a title, keeping its case: "Song [Extended Mix]" ->
/// ("Song", "Extended Mix"). A trailing group that isn't a version (e.g. "(Bam Bam)") stays put.
fn split_mix(title: &str) -> (String, Option<String>) {
    let Some(close) = title.chars().last().filter(|c| matches!(c, ')' | ']')) else {
        return (title.to_string(), None);
    };
    let open = if close == ')' { '(' } else { '[' };
    let Some(start) = title.rfind(open) else {
        return (title.to_string(), None);
    };
    let inner = title[start + 1..title.len() - 1].trim();
    let lower = inner.to_lowercase();
    let is_version = crate::scraper::MIX_KEYWORDS.iter().any(|k| {
        lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word == *k)
    });
    let rest = title[..start].trim();
    if !is_version || rest.is_empty() {
        return (title.to_string(), None);
    }
    (rest.to_string(), Some(inner.to_string()))
}

static FEATURED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*[(\[]\s*(?:feat\.?|ft\.?|featuring)\s+([^)\]]+)[)\]]").unwrap()
});

/// Takes Apple's "(feat. X)" out of a title, returning X's names: the stores this app mostly
/// takes metadata from credit featured artists as artists, not in the title.
fn split_featured(title: &str) -> (String, Vec<String>) {
    let featured = FEATURED_RE
        .captures_iter(title)
        .flat_map(|caps| split_artists(&caps[1]))
        .collect();
    let title = FEATURED_RE.replace_all(title, "").trim().to_string();
    (title, featured)
}

/// Drops Apple's " - Single"/" - EP" from a release name.
fn strip_release_kind(name: &str) -> String {
    let name = name.trim();
    [" - Single", " - EP"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_search_result() {
        let item = serde_json::json!({
            "wrapperType": "track",
            "kind": "song",
            "trackId": 1883679770_i64,
            "artistName": "HUGEL & SOLTO (FR)",
            "collectionName": "Jamaican (Bam Bam) [Extended Mix] - Single",
            "trackName": "Jamaican (Bam Bam) [Extended Mix]",
            "releaseDate": "2025-11-07T12:00:00Z",
            "primaryGenreName": "Afro House",
        });
        let track = parse_track(&item).unwrap();
        assert_eq!(track.id, 1883679770);
        assert_eq!(track.artists, ["HUGEL", "SOLTO (FR)"]);
        assert_eq!(track.track_name, "Jamaican (Bam Bam)");
        assert_eq!(track.mix_name.as_deref(), Some("Extended Mix"));
        assert_eq!(
            track.album.as_deref(),
            Some("Jamaican (Bam Bam) [Extended Mix]")
        );
        assert_eq!(
            track.release_date,
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap()
        );
        assert_eq!(track.genre.as_deref(), Some("Afro House"));
    }

    #[test]
    fn skips_non_songs_and_incomplete_items() {
        let video = serde_json::json!({
            "kind": "music-video", "trackId": 1, "artistName": "A", "trackName": "T",
            "releaseDate": "2020-01-01T00:00:00Z",
        });
        assert!(parse_track(&video).is_none());
        let undated = serde_json::json!({
            "kind": "song", "trackId": 1, "artistName": "A", "trackName": "T",
        });
        assert!(parse_track(&undated).is_none());
    }

    #[test]
    fn moves_featured_artists_from_the_title() {
        let item = serde_json::json!({
            "kind": "song",
            "trackId": 2,
            "artistName": "Anyma & Solomun",
            "trackName": "Till I Die (feat. Claudia Valentina) [Extended Mix]",
            "releaseDate": "2025-11-07T12:00:00Z",
            "primaryGenreName": "Music",
        });
        let track = parse_track(&item).unwrap();
        assert_eq!(track.track_name, "Till I Die");
        assert_eq!(track.mix_name.as_deref(), Some("Extended Mix"));
        assert_eq!(track.artists, ["Anyma", "Solomun", "Claudia Valentina"]);
        assert_eq!(track.genre, None);
    }

    #[test]
    fn splits_only_versions_off_titles() {
        assert_eq!(
            split_mix("Take Care Of Business (Solomun Mix)"),
            (
                "Take Care Of Business".to_string(),
                Some("Solomun Mix".to_string())
            )
        );
        assert_eq!(
            split_mix("Jamaican (Bam Bam)"),
            ("Jamaican (Bam Bam)".to_string(), None)
        );
        assert_eq!(split_mix("(Remix)"), ("(Remix)".to_string(), None));
        assert_eq!(split_mix("Mixed Up"), ("Mixed Up".to_string(), None));
    }

    #[test]
    fn splits_joined_artist_credits() {
        assert_eq!(
            split_artists("Dom Dolla, Solomun & Skepta"),
            ["Dom Dolla", "Solomun", "Skepta"]
        );
        assert_eq!(strip_release_kind("Rumpta - Single"), "Rumpta");
        assert_eq!(strip_release_kind("Night Moves - EP"), "Night Moves");
    }
}
