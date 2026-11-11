//! Waveform overviews of downloaded audio, for the player's seek bar.
//!
//! A waveform is [`BUCKETS`] loudness values (0–255) evenly spread over the track: the RMS level of
//! the loudest short block in each bucket, relative to the loudest bucket of the track. RMS rather
//! than peak amplitude, since mastered dance music peaks near full scale almost everywhere, which
//! would flatten breakdowns and drops into one solid block.
//!
//! The same decoding pass also finds the track's beats (`beats.rs`).

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use crate::beats::{Beat, BeatTracker};
use anyhow::Context;
use symphonia::core::{
    codecs::audio::AudioDecoderOptions,
    errors::Error,
    formats::{probe::Hint, FormatOptions, TrackType},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

/// Number of values in a waveform. The client resamples them to the bars that fit its width.
pub const BUCKETS: usize = 1000;
/// Frames per RMS block, about 23ms at 44.1kHz.
const BLOCK_FRAMES: usize = 1024;

/// What a track's audio is analyzed into, cached with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// [`BUCKETS`] loudness values (0–255).
    pub peaks: Vec<u8>,
    pub beats: Vec<Beat>,
}

/// Decodes the audio file at `path` (blocking) and computes its waveform and beats.
pub fn compute(path: &Path) -> anyhow::Result<Analysis> {
    let file = File::open(path).with_context(|| format!("can't open {}", path.display()))?;
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .context("unsupported audio format")?;
    let track = format
        .default_track(TrackType::Audio)
        .context("no audio track")?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .context("no audio codec parameters")?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .context("unsupported audio codec")?;

    let mut blocks = Blocks::default();
    let mut beats: Option<BeatTracker> = None;
    let mut samples: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // The track list changed mid-stream (chained streams), not for our downloads.
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(e).context("can't read audio"),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(buf) => buf,
            // A corrupt packet: skip it, as players do.
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e).context("can't decode audio"),
        };
        let channels = buf.spec().channels().count().max(1);
        let tracker = beats.get_or_insert_with(|| {
            BeatTracker::new(params.sample_rate.unwrap_or(buf.spec().rate()))
        });
        buf.copy_to_vec_interleaved(&mut samples);
        for frame in samples.chunks_exact(channels) {
            blocks.push(frame_energy(frame));
            tracker.push(frame);
        }
    }
    let levels = blocks.finish();
    anyhow::ensure!(!levels.is_empty(), "no audio decoded");
    let offset = edit_list_offset(path).unwrap_or_else(|e| {
        tracing::debug!("No edit list in {}: {e:#}", path.display());
        0.0
    });
    Ok(Analysis {
        peaks: buckets(&levels, BUCKETS),
        beats: beats.map(|b| b.finish(offset)).unwrap_or_default(),
    })
}

/// Seconds of decoded audio a player skips at the start of an MP4 file's audio track: where its
/// edit list starts the presentation, usually the encoder's priming samples. Symphonia decodes them
/// anyway, which would make every beat late by as much.
fn edit_list_offset(path: &Path) -> anyhow::Result<f64> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let moov = find_box(&mut file, (0, len), b"moov")?.context("no moov box")?;
    for trak in child_boxes(&mut file, moov, b"trak")? {
        let Some(mdia) = find_box(&mut file, trak, b"mdia")? else {
            continue;
        };
        let Some(hdlr) = find_box(&mut file, mdia, b"hdlr")? else {
            continue;
        };
        // Version and flags, pre_defined, then the handler type.
        if &read_at::<12>(&mut file, hdlr.0)?[8..] != b"soun" {
            continue;
        }
        let mdhd = find_box(&mut file, mdia, b"mdhd")?.context("no mdhd box")?;
        let header = read_at::<32>(&mut file, mdhd.0)?;
        let timescale = if header[0] == 1 {
            u32::from_be_bytes(header[20..24].try_into()?)
        } else {
            u32::from_be_bytes(header[12..16].try_into()?)
        };
        let Some(edts) = find_box(&mut file, trak, b"edts")? else {
            return Ok(0.0);
        };
        let Some(elst) = find_box(&mut file, edts, b"elst")? else {
            return Ok(0.0);
        };
        let header = read_at::<8>(&mut file, elst.0)?;
        let version = header[0];
        let count = u32::from_be_bytes(header[4..8].try_into()?);
        let entry_len = if version == 1 { 20 } else { 12 };
        for i in 0..u64::from(count.min(16)) {
            let entry = read_at::<20>(&mut file, elst.0 + 8 + i * entry_len)?;
            let media_time = if version == 1 {
                i64::from_be_bytes(entry[8..16].try_into()?)
            } else {
                i64::from(i32::from_be_bytes(entry[4..8].try_into()?))
            };
            // -1 is an empty edit (a delay before the media starts), rare in audio files.
            if media_time >= 0 && timescale > 0 {
                return Ok(media_time as f64 / f64::from(timescale));
            }
        }
        return Ok(0.0);
    }
    anyhow::bail!("no audio track")
}

/// Payload ranges of the boxes of `kind` directly within `start..end`.
fn child_boxes(
    file: &mut File,
    (start, end): (u64, u64),
    kind: &[u8; 4],
) -> anyhow::Result<Vec<(u64, u64)>> {
    let mut found = Vec::new();
    let mut at = start;
    while at + 8 <= end {
        let header = read_at::<16>(file, at)?;
        let size = u64::from(u32::from_be_bytes(header[0..4].try_into()?));
        let (payload, size) = match size {
            0 => (at + 8, end - at),
            1 => (at + 16, u64::from_be_bytes(header[8..16].try_into()?)),
            size => (at + 8, size),
        };
        anyhow::ensure!(size >= payload - at && at + size <= end, "malformed box");
        if &header[4..8] == kind {
            found.push((payload, at + size));
        }
        at += size;
    }
    Ok(found)
}

/// The payload range of the first box of `kind` directly within `range`.
fn find_box(
    file: &mut File,
    range: (u64, u64),
    kind: &[u8; 4],
) -> anyhow::Result<Option<(u64, u64)>> {
    Ok(child_boxes(file, range, kind)?.into_iter().next())
}

/// `N` bytes at `offset`, zero-padded past the end of the file.
fn read_at<const N: usize>(file: &mut File, offset: u64) -> anyhow::Result<[u8; N]> {
    let mut buf = [0u8; N];
    file.seek(SeekFrom::Start(offset))?;
    let mut filled = 0;
    while filled < N {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(buf)
}

/// The mean square of a frame's channel samples. Combining channels by energy rather than averaging
/// the samples keeps out-of-phase content (e.g. stereo widening) from canceling out.
fn frame_energy(frame: &[f32]) -> f64 {
    frame
        .iter()
        .map(|&s| f64::from(s) * f64::from(s))
        .sum::<f64>()
        / frame.len().max(1) as f64
}

/// RMS levels of consecutive blocks of [`BLOCK_FRAMES`] frames, from each frame's energy.
#[derive(Default)]
struct Blocks {
    levels: Vec<f32>,
    sum_squares: f64,
    frames: usize,
}

impl Blocks {
    fn push(&mut self, energy: f64) {
        self.sum_squares += energy;
        self.frames += 1;
        if self.frames == BLOCK_FRAMES {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.frames > 0 {
            self.levels
                .push((self.sum_squares / self.frames as f64).sqrt() as f32);
        }
        self.sum_squares = 0.0;
        self.frames = 0;
    }

    fn finish(mut self) -> Vec<f32> {
        self.flush();
        self.levels
    }
}

/// The loudest block level in each of `n` equal spans of `levels`, scaled so the loudest is 255.
fn buckets(levels: &[f32], n: usize) -> Vec<u8> {
    let maxima: Vec<f32> = (0..n)
        .map(|i| {
            let start = i * levels.len() / n;
            // Fewer blocks than buckets (a very short track): repeat blocks.
            let end = ((i + 1) * levels.len() / n).max(start + 1);
            levels[start.min(levels.len() - 1)..end.min(levels.len())]
                .iter()
                .copied()
                .fold(0.0, f32::max)
        })
        .collect();
    let loudest = maxima.iter().copied().fold(0.0, f32::max);
    if loudest <= 0.0 {
        return vec![0; n];
    }
    maxima
        .iter()
        .map(|m| (m / loudest * 255.0).round().clamp(0.0, 255.0) as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_take_the_loudest_block_and_normalize() {
        let levels = [0.1, 0.2, 0.4, 0.1, 0.0, 0.05];
        assert_eq!(buckets(&levels, 3), [128, 255, 32]);
    }

    #[test]
    fn buckets_stretch_short_tracks() {
        assert_eq!(buckets(&[0.5, 1.0], 4), [128, 128, 255, 255]);
        assert_eq!(buckets(&[0.0, 0.0], 3), [0, 0, 0]);
    }

    #[test]
    fn blocks_compute_rms() {
        let mut blocks = Blocks::default();
        for _ in 0..BLOCK_FRAMES {
            blocks.push(frame_energy(&[0.5]));
        }
        blocks.push(frame_energy(&[-1.0]));
        blocks.push(frame_energy(&[1.0]));
        assert_eq!(blocks.finish(), [0.5, 1.0]);
    }

    #[test]
    fn opposite_phase_channels_keep_their_energy() {
        assert_eq!(frame_energy(&[0.5, 0.5]), frame_energy(&[0.5, -0.5]));
        assert_eq!(frame_energy(&[0.5, -0.5]), 0.25);
        assert_eq!(frame_energy(&[1.0, 0.0]), 0.5);
    }

    #[test]
    fn rejects_non_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.m4a");
        std::fs::write(&path, b"0123456789").unwrap();
        assert!(compute(&path).is_err());
    }

    /// An MP4 box of `kind` around `payload`.
    fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut b = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        b.extend_from_slice(kind);
        b.extend_from_slice(payload);
        b
    }

    /// An MP4 file whose track has `handler` and an edit list starting at `media_time`.
    fn mp4_with_edit(handler: &[u8; 4], media_time: i32) -> Vec<u8> {
        let mut mdhd = vec![0u8; 12];
        mdhd.extend_from_slice(&44_100u32.to_be_bytes());
        mdhd.extend_from_slice(&[0; 8]);
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(handler);
        hdlr.extend_from_slice(&[0; 13]);
        let mdia = [mp4_box(b"mdhd", &mdhd), mp4_box(b"hdlr", &hdlr)].concat();
        let mut elst = vec![0, 0, 0, 0, 0, 0, 0, 2];
        // An empty edit, which is skipped, then the edit that starts the presentation.
        for time in [-1, media_time] {
            elst.extend_from_slice(&1000u32.to_be_bytes());
            elst.extend_from_slice(&time.to_be_bytes());
            elst.extend_from_slice(&[0, 1, 0, 0]);
        }
        let trak = [
            mp4_box(b"tkhd", &[0; 84]),
            mp4_box(b"edts", &mp4_box(b"elst", &elst)),
            mp4_box(b"mdia", &mdia),
        ]
        .concat();
        [
            mp4_box(b"ftyp", b"M4A \0\0\0\0"),
            mp4_box(b"moov", &mp4_box(b"trak", &trak)),
            mp4_box(b"mdat", &[0; 16]),
        ]
        .concat()
    }

    #[test]
    fn reads_the_skipped_start_from_the_edit_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.m4a");
        std::fs::write(&path, mp4_with_edit(b"soun", 1600)).unwrap();
        assert_eq!(edit_list_offset(&path).unwrap(), 1600.0 / 44_100.0);
        std::fs::write(&path, mp4_with_edit(b"vide", 1600)).unwrap();
        assert!(edit_list_offset(&path).is_err(), "not an audio track");
        std::fs::write(&path, b"0123456789").unwrap();
        assert!(edit_list_offset(&path).is_err());
    }
}
