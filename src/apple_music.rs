//! Apple Music's catalog, searched through Apple's public iTunes Search API
//! (`itunes.apple.com/search`): JSON, no key, cookie or login. It has no BPM, key or label, but
//! covers the mainstream catalog the DJ stores miss, and its track ids are what Shazam's
//! Apple-id mapping takes (`scraper::shazam_id_for_track`).
//!
//! Apple documents a limit of about 20 requests a minute per IP, so this backend is only used for
//! searches a person starts (fuzzy, artist, the Shazam lookup), never in loops over many names
//! such as new releases.

use std::{ops::Range, sync::LazyLock};

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
    /// Apple's joined artist credits as they came: the song's "A, B & C" and those of its
    /// "(feat. …)". Their separators also occur within names ("Earth, Wind & Fire"), so `artists`
    /// may have split a name apart that a search for it should still find (`regroup_artist`).
    pub credits: Vec<String>,
    pub track_name: String,
    /// Taken from the end of Apple's title, where it keeps the version: "Song [Extended Mix]".
    pub mix_name: Option<String>,
    pub album: Option<String>,
    pub release_date: NaiveDate,
    pub genre: Option<String>,
}

impl AMTrack {
    /// Whether one of the song's credits names an artist for which `is_name` holds, as a single
    /// name or a run of them ("Earth, Wind & Fire"). A run found split apart in `artists` is joined
    /// back into one artist there, since the search it was found for says it is one. A run never
    /// cuts through a name `artists` already has whole: "Above & Beyond" doesn't credit "Above".
    pub fn regroup_artist(&mut self, is_name: impl Fn(&str) -> bool) -> bool {
        for credit in &self.credits {
            let parts = credit_parts(credit);
            let span = |start: usize, end: usize| &credit[parts[start].start..parts[end - 1].end];
            // cut[b]: parts b - 1 and b belong to one name in `artists`.
            let mut cut = vec![false; parts.len() + 1];
            for start in 0..parts.len() {
                for end in start + 2..=parts.len() {
                    if self.artists.iter().any(|a| a == span(start, end)) {
                        cut[start + 1..end].fill(true);
                    }
                }
            }
            for start in (0..parts.len()).filter(|&b| !cut[b]) {
                for end in (start + 1..=parts.len()).filter(|&b| !cut[b]) {
                    let name = span(start, end);
                    if !is_name(name) {
                        continue;
                    }
                    let names: Vec<&str> = parts[start..end]
                        .iter()
                        .map(|r| &credit[r.clone()])
                        .collect();
                    if names.len() > 1 {
                        let split_at = (0..self.artists.len()).find(|&i| {
                            self.artists.len() - i >= names.len()
                                && self.artists[i..i + names.len()].iter().eq(names.iter())
                        });
                        if let Some(i) = split_at {
                            self.artists.splice(i..i + names.len(), [name.to_string()]);
                        }
                    }
                    return true;
                }
            }
        }
        false
    }

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

pub(crate) fn parse_track(item: &Value) -> Option<AMTrack> {
    if item["kind"].as_str() != Some("song") {
        return None;
    }
    let id = item["trackId"].as_i64()?;
    // Featured credits first: Apple puts them before or after the version, and the version is
    // only looked for at the end.
    let (title, featured) = split_featured(item["trackName"].as_str()?.trim());
    let (title, mix_name) = split_mix(&title);
    let credit = item["artistName"].as_str()?.trim();
    let primary = item["artistViewUrl"].as_str().and_then(artist_slug);
    let mut artists = split_artists(credit, primary.as_deref());
    let mut credits = vec![credit.to_string()];
    for (credit, names) in featured {
        credits.push(credit);
        for name in names {
            if !artists.iter().any(|a| a.eq_ignore_ascii_case(&name)) {
                artists.push(name);
            }
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
        credits,
        track_name: title,
        mix_name,
        album,
        release_date,
        genre,
    })
}

static CREDIT_SEPARATOR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r", | & ").unwrap());

/// The pieces of Apple's joined artist credit, "A, B & C", between its separators, as byte ranges
/// of it. A name may span several ("Earth, Wind & Fire"): the separators alone don't tell.
fn credit_parts(credit: &str) -> Vec<Range<usize>> {
    let mut parts = vec![];
    let mut start = 0;
    let ends = CREDIT_SEPARATOR_RE
        .find_iter(credit)
        .map(|m| (m.start(), m.end()))
        .chain([(credit.len(), credit.len())]);
    for (end, next) in ends {
        let piece = &credit[start..end];
        let trimmed = piece.trim_start();
        let from = start + piece.len() - trimmed.len();
        let to = from + trimmed.trim_end().len();
        if to > from {
            parts.push(from..to);
        }
        start = next;
    }
    parts
}

/// Splits Apple's joined artist credit, "A, B & C", into its names. `primary` is the URL slug of
/// the song's primary artist, the credit's first: it keeps that name whole when it contains the
/// separators itself ("Above & Beyond" is `above-beyond`, while "HUGEL & SOLTO (FR)" is `hugel`).
/// Later names can't be told apart this way; `AMTrack::regroup_artist` rejoins those an artist
/// search names.
fn split_artists(credit: &str, primary: Option<&str>) -> Vec<String> {
    let parts = credit_parts(credit);
    if parts.is_empty() {
        return vec![];
    }
    let first = primary
        .map(name_key)
        .filter(|key| !key.is_empty())
        .and_then(|key| {
            (1..=parts.len()).find(|&n| name_key(&credit[parts[0].start..parts[n - 1].end]) == key)
        })
        .unwrap_or(1);
    std::iter::once(parts[0].start..parts[first - 1].end)
        .chain(parts[first..].iter().cloned())
        .map(|r| credit[r].to_string())
        .collect()
}

/// A name reduced to what Apple's URL slugs keep of it: "Earth, Wind & Fire" and
/// `earth-wind-fire` are both "earthwindfire".
fn name_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The artist slug of an Apple artist URL, percent-decoded:
/// `https://music.apple.com/us/artist/sigur-r%C3%B3s/73720797` -> "sigur-rós".
fn artist_slug(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    let mut segments = url.path_segments()?;
    segments.find(|s| *s == "artist")?;
    let slug = segments.next()?;
    let bytes = slug.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| slug.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                i += 3;
            }
            None => {
                decoded.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

/// Splits a trailing bracketed version off a title, keeping its case: "Song [Extended Mix]" ->
/// ("Song", "Extended Mix"), and nested ones whole: "Song [A Remix [B Respray]]" -> ("Song",
/// "A Remix [B Respray]"). A trailing group that isn't a version (e.g. "(Bam Bam)") stays put.
fn split_mix(title: &str) -> (String, Option<String>) {
    let Some(start) = trailing_group_start(title) else {
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

/// Where the bracketed group that ends `title` opens: the bracket paired with its last one.
fn trailing_group_start(title: &str) -> Option<usize> {
    if !title.ends_with([')', ']']) {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in title.char_indices().rev() {
        match c {
            ')' | ']' => depth += 1,
            '(' | '[' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

static FEATURED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*[(\[]\s*(?:feat\.?|ft\.?|featuring)\s+([^)\]]+)[)\]]").unwrap()
});

/// Takes Apple's "(feat. X)" out of a title, returning each such credit with its names: the stores
/// this app mostly takes metadata from credit featured artists as artists, not in the title.
fn split_featured(title: &str) -> (String, Vec<(String, Vec<String>)>) {
    let featured = FEATURED_RE
        .captures_iter(title)
        .map(|caps| {
            let credit = caps[1].trim().to_string();
            let names = split_artists(&credit, None);
            (credit, names)
        })
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
    fn takes_featured_credits_out_before_the_version() {
        // Apple's song 499472452: the featured credit after the version.
        let item = serde_json::json!({
            "kind": "song",
            "trackId": 499472452_i64,
            "artistName": "Above & Beyond",
            "artistViewUrl": "https://music.apple.com/us/artist/above-beyond/20318188?uo=4",
            "trackName": "You Got To Go (Seven Lions Remix) [feat. Zoë Johnston]",
            "releaseDate": "2012-02-06T12:00:00Z",
        });
        let track = parse_track(&item).unwrap();
        assert_eq!(track.track_name, "You Got To Go");
        assert_eq!(track.mix_name.as_deref(), Some("Seven Lions Remix"));
        assert_eq!(track.artists, ["Above & Beyond", "Zoë Johnston"]);
    }

    #[test]
    fn keeps_nested_versions_whole() {
        // Apple's song 1194282836.
        let item = serde_json::json!({
            "kind": "song",
            "trackId": 1194282836_i64,
            "artistName": "Above & Beyond",
            "artistViewUrl": "https://music.apple.com/us/artist/above-beyond/20318188?uo=4",
            "trackName": "No One On Earth (feat. Zoë Johnston) [Gabriel & Dresden Remix [Above & Beyond Respray]]",
            "releaseDate": "2016-12-16T12:00:00Z",
        });
        let track = parse_track(&item).unwrap();
        assert_eq!(track.track_name, "No One On Earth");
        assert_eq!(
            track.mix_name.as_deref(),
            Some("Gabriel & Dresden Remix [Above & Beyond Respray]")
        );
        assert_eq!(
            split_mix("Song (A Remix (B Edit))"),
            ("Song".to_string(), Some("A Remix (B Edit)".to_string()))
        );
        assert_eq!(
            split_mix("Song (Bam [Bam)]"),
            ("Song (Bam [Bam)]".to_string(), None)
        );
        assert_eq!(split_mix("Remix)"), ("Remix)".to_string(), None));
    }

    #[test]
    fn keeps_the_primary_artist_whole_by_its_url() {
        let artists = |credit, url| split_artists(credit, artist_slug(url).as_deref());
        assert_eq!(
            artists(
                "Earth, Wind & Fire",
                "https://music.apple.com/us/artist/earth-wind-fire/290699?uo=4"
            ),
            ["Earth, Wind & Fire"]
        );
        assert_eq!(
            artists(
                "Above & Beyond & Seven Lions",
                "https://music.apple.com/us/artist/above-beyond/20318188?uo=4"
            ),
            ["Above & Beyond", "Seven Lions"]
        );
        assert_eq!(
            artists(
                "HUGEL & SOLTO (FR)",
                "https://music.apple.com/us/artist/hugel/978839124?uo=4"
            ),
            ["HUGEL", "SOLTO (FR)"]
        );
        assert_eq!(
            artists(
                "Röyksopp & Robyn",
                "https://music.apple.com/us/artist/r%C3%B6yksopp/3432068?uo=4"
            ),
            ["Röyksopp", "Robyn"]
        );
        // A slug that isn't the credit's start leaves the plain split.
        assert_eq!(
            artists("A & B", "https://music.apple.com/us/artist/c/1"),
            ["A", "B"]
        );
        assert_eq!(
            split_artists("Earth, Wind & Fire", None),
            ["Earth", "Wind", "Fire"]
        );
    }

    #[test]
    fn finds_artists_split_apart_in_a_credit() {
        let song = |credit: &str, url: &str, title: &str| {
            parse_track(&serde_json::json!({
                "kind": "song",
                "trackId": 1,
                "artistName": credit,
                "artistViewUrl": url,
                "trackName": title,
                "releaseDate": "2020-01-01T00:00:00Z",
            }))
            .unwrap()
        };
        let named = |wanted: &'static str| move |name: &str| name.eq_ignore_ascii_case(wanted);

        let mut track = song(
            "Dom Dolla & Earth, Wind & Fire",
            "https://music.apple.com/us/artist/dom-dolla/555348065?uo=4",
            "Song (feat. Above & Beyond)",
        );
        assert_eq!(
            track.artists,
            ["Dom Dolla", "Earth", "Wind", "Fire", "Above", "Beyond"]
        );
        assert!(track.regroup_artist(named("Earth, Wind & Fire")));
        assert_eq!(
            track.artists,
            ["Dom Dolla", "Earth, Wind & Fire", "Above", "Beyond"]
        );
        assert!(track.regroup_artist(named("Above & Beyond")));
        assert_eq!(
            track.artists,
            ["Dom Dolla", "Earth, Wind & Fire", "Above & Beyond"]
        );
        assert!(track.regroup_artist(named("Dom Dolla")));
        assert!(!track.regroup_artist(named("Earth")));
        assert!(!track.regroup_artist(named("Dom Dolla & Earth")));

        let mut track = song(
            "Above & Beyond",
            "https://music.apple.com/us/artist/above-beyond/20318188?uo=4",
            "Sun & Moon",
        );
        assert!(track.regroup_artist(named("Above & Beyond")));
        assert!(!track.regroup_artist(named("Above")));
        assert!(!track.regroup_artist(named("Beyond")));
        assert_eq!(track.artists, ["Above & Beyond"]);
    }

    #[test]
    fn splits_joined_artist_credits() {
        assert_eq!(
            split_artists("Dom Dolla, Solomun & Skepta", None),
            ["Dom Dolla", "Solomun", "Skepta"]
        );
        assert_eq!(split_artists("  ", None), Vec::<String>::new());
        assert_eq!(strip_release_kind("Rumpta - Single"), "Rumpta");
        assert_eq!(strip_release_kind("Night Moves - EP"), "Night Moves");
    }
}
