//! Waveform overviews of downloaded audio, for the player's seek bar.
//!
//! A waveform is [`BUCKETS`] loudness values (0–255) evenly spread over the track: the RMS level of
//! the loudest short block in each bucket, relative to the loudest bucket of the track. RMS rather
//! than peak amplitude, since mastered dance music peaks near full scale almost everywhere, which
//! would flatten breakdowns and drops into one solid block.

use std::path::Path;

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

/// Decodes the audio file at `path` (blocking) and computes its waveform.
pub fn compute(path: &Path) -> anyhow::Result<Vec<u8>> {
    let file =
        std::fs::File::open(path).with_context(|| format!("can't open {}", path.display()))?;
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
        buf.copy_to_vec_interleaved(&mut samples);
        for frame in samples.chunks_exact(channels) {
            blocks.push(frame_energy(frame));
        }
    }
    let levels = blocks.finish();
    anyhow::ensure!(!levels.is_empty(), "no audio decoded");
    Ok(buckets(&levels, BUCKETS))
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
}
