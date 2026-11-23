//! Tracks exported to a JSON file, for sharing a selection with someone who imports it into a
//! collection of their own. The user data (rating, play count, last played) is optional both
//! ways: left out of the file on export, and ignored on import.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::models::Track;

/// Names the file's format, so that importing some other JSON fails with a clear message.
const FORMAT: &str = "stereo-tracks";
/// Bumped on a change older versions can't read.
const VERSION: u64 = 1;

const USER_DATA: [&str; 3] = ["rating", "play_count", "last_played"];

/// The file's contents: the tracks with their metadata, empty fields left out.
pub fn export(tracks: &[Track], include_user_data: bool) -> anyhow::Result<Vec<u8>> {
    let tracks = tracks
        .iter()
        .map(|track| {
            let Value::Object(fields) = serde_json::to_value(track)? else {
                anyhow::bail!("a track didn't serialize to an object");
            };
            let fields: Map<String, Value> = fields
                .into_iter()
                .filter(|(k, v)| {
                    !v.is_null() && (include_user_data || !USER_DATA.contains(&k.as_str()))
                })
                .collect();
            Ok(Value::Object(fields))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let file = json!({ "format": FORMAT, "version": VERSION, "tracks": tracks });
    Ok(serde_json::to_vec_pretty(&file)?)
}

#[derive(Deserialize)]
struct Header {
    format: Option<String>,
    version: Option<u64>,
}

/// The tracks in an exported file, parsed as JSON, without their user data unless
/// `keep_user_data`, each id once (the first). Fails without any tracks if one couldn't be played.
pub fn import(value: Value, keep_user_data: bool) -> anyhow::Result<Vec<Track>> {
    let header = Header::deserialize(&value).unwrap_or(Header {
        format: None,
        version: None,
    });
    anyhow::ensure!(
        header.format.as_deref() == Some(FORMAT),
        "not a file of tracks exported from Stereo"
    );
    let version = header.version.unwrap_or(0);
    anyhow::ensure!(
        version <= VERSION,
        "the file is from a newer version of Stereo (format version {version}), update Stereo to import it"
    );
    #[derive(Deserialize)]
    struct File {
        tracks: Vec<Track>,
    }
    let File { mut tracks } =
        File::deserialize(value).map_err(|e| anyhow::anyhow!("invalid track list: {e}"))?;
    if let Some((i, track)) = tracks
        .iter()
        .enumerate()
        .find(|(_, t)| !crate::audio::is_valid_id(&t.yt_id))
    {
        anyhow::bail!(
            "track {} ({}) has an invalid YouTube id: {:?}",
            i + 1,
            track.display_name(),
            track.yt_id
        );
    }
    let mut seen = std::collections::HashSet::new();
    tracks.retain(|t| seen.insert(t.yt_id.clone()));
    if !keep_user_data {
        for track in &mut tracks {
            track.rating = None;
            track.play_count = 0;
            track.last_played = None;
        }
    }
    Ok(tracks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn track() -> Track {
        Track {
            yt_id: "dQw4w9WgXcQ".into(),
            title: "Song".into(),
            artists: vec!["Artist".into()],
            bpm: Some(124),
            release_date: NaiveDate::from_ymd_opt(2024, 5, 1),
            rating: Some(5),
            play_count: 3,
            last_played: NaiveDate::from_ymd_opt(2025, 1, 2),
            ..Default::default()
        }
    }

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn round_trips_with_user_data() {
        let data = export(&[track()], true).unwrap();
        let text = String::from_utf8(data).unwrap();
        assert!(!text.contains("null"), "empty fields are left out");
        assert_eq!(import(parse(&text), true).unwrap(), vec![track()]);
    }

    #[test]
    fn user_data_is_optional_both_ways() {
        let without = Track {
            rating: None,
            play_count: 0,
            last_played: None,
            ..track()
        };
        let text = String::from_utf8(export(&[track()], false).unwrap()).unwrap();
        for key in USER_DATA {
            assert!(!text.contains(key), "{key} left out");
        }
        assert_eq!(import(parse(&text), true).unwrap(), vec![without.clone()]);
        let text = String::from_utf8(export(&[track()], true).unwrap()).unwrap();
        assert_eq!(import(parse(&text), false).unwrap(), vec![without]);
    }

    #[test]
    fn keeps_the_first_of_tracks_with_the_same_id() {
        let other = Track {
            title: "Other".into(),
            ..track()
        };
        let text = String::from_utf8(export(&[track(), other], true).unwrap()).unwrap();
        assert_eq!(import(parse(&text), true).unwrap(), vec![track()]);
    }

    #[test]
    fn rejects_other_files() {
        assert!(import(parse(r#"[{"yt_id": "dQw4w9WgXcQ"}]"#), true).is_err());
        assert!(import(parse(r#"{"version": 1, "tracks": []}"#), true).is_err());
        let newer = import(
            parse(r#"{"format": "stereo-tracks", "version": 2, "tracks": []}"#),
            true,
        );
        assert!(newer.unwrap_err().to_string().contains("newer version"));
    }

    #[test]
    fn rejects_tracks_that_cannot_play() {
        for id in ["", "abc", "../../etc/p", "dQw4w9 gXcQ"] {
            let file = json!({
                "format": "stereo-tracks",
                "version": 1,
                "tracks": [track(), { "yt_id": id, "title": "x", "artists": [] }],
            });
            let error = import(file, true).unwrap_err().to_string();
            assert!(error.contains("track 2"), "{id:?}: {error}");
        }
    }
}
