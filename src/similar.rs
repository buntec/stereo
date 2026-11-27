//! Finds collection tracks that look like the same recording as a search result, usually under
//! a different YouTube video, so the results can warn before the track is added twice.
//!
//! The artists must share a name and the titles, without mix names and featured artists, must be
//! nearly equal. A shared Beatport or Spotify id allows for more difference in the titles, but
//! not for none in common: collections have tracks of one release that share an id. Different
//! mixes of a title only match when neither is a remix: an extended mix or radio edit is the
//! same track to a curator, someone's remix isn't. That holds for a shared id too.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::models::Track;
use crate::utils::normalize_for_match;

/// How much of a search result the comparison needs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackIdentity {
    pub yt_id: String,
    pub title: String,
    #[serde(default)]
    pub mix_name: Option<String>,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub bp_id: Option<i64>,
    #[serde(default)]
    pub sp_id: Option<String>,
}

/// At most this many similar tracks per result: the warning only needs a few to make its point.
const MAX_SIMILAR: usize = 5;
const TITLE_SIMILARITY: f64 = 0.9;
/// For tracks with the same Beatport or Spotify id.
const ID_TITLE_SIMILARITY: f64 = 0.6;

/// Versions that differ only in length or edit, not in who made them.
const PLAIN_MIXES: &[&str] = &[
    "",
    "original",
    "original mix",
    "original version",
    "extended",
    "extended mix",
    "extended version",
    "radio edit",
    "radio mix",
    "radio version",
    "club mix",
    "main mix",
    "single version",
    "album version",
];

// "(feat. …)" or "[ft. …]".
static BRACKETED_FEAT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*[(\[]\s*(?:feat|ft|featuring)\b[^)\]]*[)\]]").unwrap());
// The start of an unbracketed "feat. …", which runs up to a bracket or " - " (see
// `strip_inline_feat`).
static INLINE_FEAT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*\b(?:feat|ft|featuring)\b\.?\s").unwrap());
// A trailing "(…)"/"[…]" group, or " - … Mix" and the like, holding the mix name.
static MIX_SUFFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\s*(?:[(\[]([^)\]]*)[)\]]|\s-\s+(.*\b(?:mix|remix|edit|version|dub|rework|bootleg|vip)))\s*$",
    )
    .unwrap()
});
static ARTIST_SEP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*(?:,|&|\band\b|\bx\b|\bvs\.?|\bfeat\.?|\bft\.?|\bfeaturing\b)\s*").unwrap()
});

/// Lowercase words without punctuation, in their order.
fn words(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Removes "feat. …" credits, keeping what follows them in brackets or after " - ", which is
/// the mix in "Track feat. Singer (Other Remix)".
fn strip_inline_feat(title: &str) -> String {
    let mut out = String::new();
    let mut rest = title;
    while let Some(m) = INLINE_FEAT_RE.find(rest) {
        out.push_str(&rest[..m.start()]);
        let after = &rest[m.end()..];
        let end = [after.find('('), after.find('['), after.find(" - ")]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(after.len());
        rest = &after[end..];
        if !rest.is_empty() {
            out.push(' ');
        }
    }
    out.push_str(rest);
    out
}

/// The title without featured artists or mix, and the mix: `mix_name` if there is one, else
/// what the title carries.
fn split_title(title: &str, mix_name: Option<&str>) -> (String, String) {
    let bracketed = BRACKETED_FEAT_RE.replace_all(title, "");
    let mut base = strip_inline_feat(&bracketed).trim().to_string();
    let mut mix = mix_name.map(str::trim).unwrap_or_default().to_string();
    // Sources that also give the mix separately may still repeat it in the title.
    while let Some(caps) = MIX_SUFFIX_RE.captures(&base) {
        let found = caps.get(1).or(caps.get(2)).map_or("", |m| m.as_str());
        if mix.is_empty() {
            mix = found.trim().to_string();
        } else if words(found) != words(&mix) {
            break;
        }
        let start = caps.get(0).unwrap().start();
        base.truncate(start);
        base = base.trim().to_string();
    }
    (words(&base), words(&mix))
}

fn is_plain_mix(mix: &str) -> bool {
    PLAIN_MIXES.contains(&mix)
}

fn mixes_compatible(a: &str, b: &str) -> bool {
    (is_plain_mix(a) && is_plain_mix(b)) || strsim::normalized_levenshtein(a, b) >= TITLE_SIMILARITY
}

/// Each credited name, normalized, with "A & B" and "A feat. B" split into both.
fn artist_names(artists: &[String]) -> HashSet<String> {
    artists
        .iter()
        .flat_map(|a| {
            ARTIST_SEP_RE
                .split(a)
                .map(normalize_for_match)
                .collect::<Vec<_>>()
        })
        .filter(|n| !n.is_empty())
        .collect()
}

struct Candidate<'a> {
    track: &'a Track,
    title: String,
    mix: String,
}

/// Stops a running `find_similar` when dropped, as when the task awaiting it is aborted.
#[derive(Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.0)
    }
}

impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// For each of `queries`, the tracks in `collection` that seem to be the same recording, other
/// than the one with its own YouTube id. `None` if `cancelled` was set before it finished.
pub fn find_similar(
    collection: &[Track],
    queries: &[TrackIdentity],
    cancelled: &AtomicBool,
) -> Option<Vec<Vec<Track>>> {
    let candidates: Vec<Candidate> = collection
        .iter()
        .map(|track| {
            let (title, mix) = split_title(&track.title, track.mix_name.as_deref());
            Candidate { track, title, mix }
        })
        .collect();
    let mut by_artist: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_bp: HashMap<i64, Vec<usize>> = HashMap::new();
    let mut by_sp: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, c) in candidates.iter().enumerate() {
        for name in artist_names(&c.track.artists) {
            by_artist.entry(name).or_default().push(i);
        }
        if let Some(bp) = c.track.bp_id {
            by_bp.entry(bp).or_default().push(i);
        }
        if let Some(sp) = c.track.sp_id.as_deref().filter(|s| !s.is_empty()) {
            by_sp.entry(sp).or_default().push(i);
        }
    }

    queries
        .iter()
        .map(|q| {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            let (title, mix) = split_title(&q.title, q.mix_name.as_deref());
            let mut found: Vec<usize> = Vec::new();
            let mut add = |i: usize| {
                if candidates[i].track.yt_id != q.yt_id && !found.contains(&i) {
                    found.push(i);
                }
            };
            let sp = q.sp_id.as_deref().filter(|s| !s.is_empty());
            let by_id = q
                .bp_id
                .and_then(|bp| by_bp.get(&bp))
                .into_iter()
                .chain(sp.and_then(|sp| by_sp.get(sp)))
                .flatten();
            for &i in by_id {
                let c = &candidates[i];
                if strsim::normalized_levenshtein(&title, &c.title) >= ID_TITLE_SIMILARITY
                    && mixes_compatible(&mix, &c.mix)
                {
                    add(i);
                }
            }
            if !title.is_empty() {
                let mut seen = HashSet::new();
                for name in artist_names(&q.artists) {
                    for &i in by_artist.get(&name).into_iter().flatten() {
                        if !seen.insert(i) {
                            continue;
                        }
                        let c = &candidates[i];
                        if strsim::normalized_levenshtein(&title, &c.title) >= TITLE_SIMILARITY
                            && mixes_compatible(&mix, &c.mix)
                        {
                            add(i);
                        }
                    }
                }
            }
            Some(
                found
                    .into_iter()
                    .take(MAX_SIMILAR)
                    .map(|i| candidates[i].track.clone())
                    .collect(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(yt_id: &str, artists: &[&str], title: &str, mix: Option<&str>) -> Track {
        Track {
            yt_id: yt_id.into(),
            title: title.into(),
            mix_name: mix.map(Into::into),
            artists: artists.iter().map(|a| a.to_string()).collect(),
            ..Default::default()
        }
    }

    fn query(artists: &[&str], title: &str, mix: Option<&str>) -> TrackIdentity {
        TrackIdentity {
            yt_id: "result00001".into(),
            title: title.into(),
            mix_name: mix.map(Into::into),
            artists: artists.iter().map(|a| a.to_string()).collect(),
            ..Default::default()
        }
    }

    fn ids(collection: &[Track], q: TrackIdentity) -> Vec<String> {
        find_similar(collection, &[q], &AtomicBool::new(false))
            .unwrap()
            .remove(0)
            .into_iter()
            .map(|t| t.yt_id)
            .collect()
    }

    #[test]
    fn matches_across_artist_order_case_and_punctuation() {
        let c = [track(
            "a",
            &["Marcus Worgull", "Daniel Bortz"],
            "Gravity",
            None,
        )];
        assert_eq!(
            ids(
                &c,
                query(&["daniel bortz", "MARCUS WORGULL"], "Gravity!", None)
            ),
            ["a"]
        );
    }

    #[test]
    fn matches_a_shared_artist_in_a_combined_credit() {
        let c = [track("a", &["Bicep & Hammer"], "Glue", None)];
        assert_eq!(ids(&c, query(&["Bicep"], "Glue", None)), ["a"]);
    }

    #[test]
    fn original_mix_matches_no_mix_and_mix_in_title() {
        let c = [
            track("a", &["Artist"], "Track", Some("Original Mix")),
            track("b", &["Artist"], "Track (Extended Mix)", None),
        ];
        assert_eq!(ids(&c, query(&["Artist"], "Track", None)), ["a", "b"]);
        assert_eq!(
            ids(&c, query(&["Artist"], "Track - Radio Edit", None)),
            ["a", "b"]
        );
    }

    #[test]
    fn a_remix_matches_only_the_same_remix() {
        let c = [
            track("a", &["Artist"], "Track", Some("Original Mix")),
            track("b", &["Artist"], "Track", Some("Someone Remix")),
            track("c", &["Artist"], "Track (Other Remix)", None),
        ];
        assert_eq!(
            ids(&c, query(&["Artist"], "Track", Some("Someone Remix"))),
            ["b"]
        );
        assert_eq!(
            ids(&c, query(&["Artist", "Other"], "Track (Other Remix)", None)),
            ["c"]
        );
    }

    #[test]
    fn ignores_featured_artists_in_the_title() {
        let c = [track("a", &["Artist"], "Track (feat. Singer)", None)];
        assert_eq!(ids(&c, query(&["Artist"], "Track ft. Singer", None)), ["a"]);
    }

    #[test]
    fn keeps_the_mix_on_either_side_of_a_featured_credit() {
        let c = [
            track("a", &["Artist"], "Track", Some("Original Mix")),
            track("b", &["Artist"], "Track (Other Remix)", None),
        ];
        for title in [
            "Track feat. Singer (Other Remix)",
            "Track (Other Remix) feat. Singer",
            "Track feat. Singer - Other Remix",
            "Track (feat. Singer) [Other Remix]",
        ] {
            assert_eq!(ids(&c, query(&["Artist"], title, None)), ["b"], "{title}");
        }
    }

    #[test]
    fn a_shared_catalog_id_keeps_remixes_apart() {
        let mut original = track("a", &["Artist"], "Track", Some("Original Mix"));
        original.bp_id = Some(42);
        let mut remix = track("b", &["Artist"], "Track", Some("Other Remix"));
        remix.bp_id = Some(42);
        let mut q = query(&["Artist"], "Track (Other Remix)", None);
        q.bp_id = Some(42);
        assert_eq!(ids(&[original, remix], q), ["b"]);
    }

    #[test]
    fn stops_once_cancelled() {
        let c = [track("a", &["Artist"], "Track", None)];
        let cancel = Cancel::default();
        let flag = cancel.flag();
        drop(cancel);
        assert!(find_similar(&c, &[query(&["Artist"], "Track", None)], &flag).is_none());
    }

    #[test]
    fn needs_both_artist_and_title() {
        let c = [
            track("a", &["Artist"], "Home", None),
            track("b", &["Other"], "Hope", None),
        ];
        assert!(ids(&c, query(&["Artist"], "Hope", None)).is_empty());
        assert!(ids(&c, query(&["Someone"], "Home", None)).is_empty());
    }

    #[test]
    fn a_shared_catalog_id_allows_for_other_text_but_not_another_title() {
        let mut t = track("a", &["Artist"], "Le Track", None);
        t.bp_id = Some(42);
        let mut other = track("b", &["Artist"], "Another One", None);
        other.bp_id = Some(42);
        let mut q = query(&["Artiste"], "Track", None);
        q.bp_id = Some(42);
        assert_eq!(ids(&[t, other], q), ["a"]);
    }

    #[test]
    fn leaves_out_the_result_s_own_video() {
        let c = [
            track("result00001", &["Artist"], "Track", None),
            track("b", &["Artist"], "Track", None),
        ];
        assert_eq!(ids(&c, query(&["Artist"], "Track", None)), ["b"]);
    }
}
