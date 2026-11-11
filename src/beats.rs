//! Beat tracking of downloaded audio, for the logo that pulses with the playing track.
//!
//! The onset envelope is the rise in energy of the bass (below [`LOWPASS_HZ`], where the kick drum
//! is) and of the highs (above [`HIGHPASS_HZ`]: hi-hats, claps and the kick's click, which keep a
//! syncopated bassline from passing for the beat), sampled about every 10ms. The tempo is the envelope's strongest periodicity, weighted towards
//! [`PRIOR_BPM`], and the beats are the onsets that best fit that tempo while letting it drift a
//! little (dynamic programming, as in Ellis, "Beat Tracking by Dynamic Programming", 2007).

/// The beats' times in milliseconds, with their strength (0–255), relative to the track's strongest
/// beats: weak in a breakdown without kicks, and in quiet passages and fades.
pub type Beat = (u32, u8);

/// Cutoff of the low-pass filter that isolates the kick drum and bass.
const LOWPASS_HZ: f64 = 150.0;
/// Cutoff of the high-pass filter that isolates the percussion.
const HIGHPASS_HZ: f64 = 1500.0;
/// Envelope samples per second, about.
const ENVELOPE_HZ: f64 = 100.0;
/// Hops over which energy is measured: longer than a period of the lowest bass notes, which would
/// otherwise ripple the envelope.
const ENERGY_WINDOW: usize = 4;
/// Quieter energy than this many dB below the track's loudest counts as silence.
const DYNAMIC_RANGE_DB: f32 = 80.0;
/// Tempo range searched.
const MIN_BPM: f64 = 60.0;
const MAX_BPM: f64 = 200.0;
/// The tempo prior's center and width (in octaves): favors the 4/4 beat over half or double time.
const PRIOR_BPM: f64 = 120.0;
const PRIOR_OCTAVES: f64 = 0.5;
/// How strongly the beat tracker holds on to the tempo.
const TIGHTNESS: f64 = 400.0;

/// Collects the onset envelope from decoded audio frames.
pub struct BeatTracker {
    sample_rate: f64,
    hop: usize,
    low_coeff: f32,
    high_coeff: f32,
    /// Per channel: two one-pole low-pass stages for the bass, one for the high-pass.
    filters: Vec<[f32; 3]>,
    sum_squares: [f64; 2],
    frames: usize,
    /// Mean square energy of the bass and of the highs in each hop.
    energies: [Vec<f32>; 2],
}

/// The coefficient of a one-pole low-pass filter.
fn one_pole(cutoff: f64, sample_rate: f64) -> f32 {
    (1.0 - (-2.0 * std::f64::consts::PI * cutoff / sample_rate).exp()) as f32
}

impl BeatTracker {
    pub fn new(sample_rate: u32) -> Self {
        let sample_rate = f64::from(sample_rate.max(1));
        Self {
            sample_rate,
            hop: (sample_rate / ENVELOPE_HZ).round().max(1.0) as usize,
            low_coeff: one_pole(LOWPASS_HZ, sample_rate),
            high_coeff: one_pole(HIGHPASS_HZ, sample_rate),
            filters: Vec::new(),
            sum_squares: [0.0; 2],
            frames: 0,
            energies: [Vec::new(), Vec::new()],
        }
    }

    /// Adds one frame of samples, one per channel.
    pub fn push(&mut self, frame: &[f32]) {
        if self.filters.len() != frame.len() {
            self.filters.resize(frame.len(), [0.0; 3]);
        }
        let channels = frame.len().max(1) as f64;
        for (&sample, state) in frame.iter().zip(&mut self.filters) {
            state[0] += self.low_coeff * (sample - state[0]);
            state[1] += self.low_coeff * (state[0] - state[1]);
            state[2] += self.high_coeff * (sample - state[2]);
            let high = sample - state[2];
            self.sum_squares[0] += f64::from(state[1]) * f64::from(state[1]) / channels;
            self.sum_squares[1] += f64::from(high) * f64::from(high) / channels;
        }
        self.frames += 1;
        if self.frames == self.hop {
            for (energies, sum) in self.energies.iter_mut().zip(&mut self.sum_squares) {
                energies.push((*sum / self.hop as f64) as f32);
                *sum = 0.0;
            }
            self.frames = 0;
        }
    }

    /// The beats, shifted `offset` seconds earlier: the audio a player skips at the start.
    pub fn finish(self, offset: f64) -> Vec<Beat> {
        let rate = self.sample_rate / self.hop as f64;
        let windowed = self.energies.map(|energies| windowed(&energies));
        // Each band's onsets relative to their spread, so both count.
        let bands = windowed
            .each_ref()
            .map(|energies| standardized(onset_envelope(energies)));
        let onsets: Vec<f32> = bands[0].iter().zip(&bands[1]).map(|(a, b)| a + b).collect();
        let Some(period) = tempo_period(&onsets, rate) else {
            return Vec::new();
        };
        let (beats, contrasts) = track(&onsets, period);
        let energy: Vec<f64> = windowed[0]
            .iter()
            .zip(&windowed[1])
            .map(|(a, b)| a + b)
            .collect();
        let strengths = strengths(&beats, &contrasts, &energy);
        beats
            .into_iter()
            .zip(strengths)
            .filter_map(|(index, strength)| {
                // The envelope sample at `index` is the rise into the hop's middle.
                let seconds = (index as f64 + 0.5) / rate - offset;
                (seconds >= 0.0).then(|| ((seconds * 1000.0).round() as u32, strength))
            })
            .collect()
    }
}

/// `values` divided by their standard deviation (unchanged if it's 0).
fn standardized(mut values: Vec<f32>) -> Vec<f32> {
    let n = values.len().max(1) as f64;
    let mean = values.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
    let std = (values
        .iter()
        .map(|&v| (f64::from(v) - mean).powi(2))
        .sum::<f64>()
        / n)
        .sqrt();
    if std > 0.0 {
        values.iter_mut().for_each(|v| *v /= std as f32);
    }
    values
}

/// Each hop's energy summed over the [`ENERGY_WINDOW`] hops up to it.
fn windowed(energies: &[f32]) -> Vec<f64> {
    let mut sum = 0.0f64;
    energies
        .iter()
        .enumerate()
        .map(|(i, &e)| {
            sum += f64::from(e);
            if i >= ENERGY_WINDOW {
                sum -= f64::from(energies[i - ENERGY_WINDOW]);
            }
            sum.max(0.0)
        })
        .collect()
}

/// The rise in windowed energy (dB) into each hop. Being logarithmic, it's as large in a quiet
/// passage as in a loud one: it finds the beats, not their loudness.
fn onset_envelope(windowed: &[f64]) -> Vec<f32> {
    let windowed: Vec<f32> = windowed
        .iter()
        .map(|&sum| 10.0 * (sum.max(1e-12) as f32).log10())
        .collect();
    let loudest = windowed.iter().copied().fold(f32::MIN, f32::max);
    let floor = loudest - DYNAMIC_RANGE_DB;
    let mut onsets = vec![0.0; windowed.len()];
    for i in 1..windowed.len() {
        onsets[i] = (windowed[i].max(floor) - windowed[i - 1].max(floor)).max(0.0);
    }
    onsets
}

/// The beat period in envelope samples (fractional), or `None` without a periodic onset pattern.
fn tempo_period(onsets: &[f32], rate: f64) -> Option<f64> {
    let min_lag = (rate * 60.0 / MAX_BPM).floor().max(1.0) as usize;
    let max_lag = (rate * 60.0 / MIN_BPM).ceil() as usize;
    // At least a few beats at the slowest tempo.
    if onsets.len() < 8 * (max_lag + 1) {
        return None;
    }
    let mean = onsets.iter().map(|&o| f64::from(o)).sum::<f64>() / onsets.len() as f64;
    let centered: Vec<f64> = onsets.iter().map(|&o| f64::from(o) - mean).collect();
    let autocorrelation = |lag: usize| {
        let n = centered.len() - lag;
        centered[..n]
            .iter()
            .zip(&centered[lag..])
            .map(|(a, b)| a * b)
            .sum::<f64>()
            / n as f64
    };
    let scores: Vec<f64> = (min_lag - 1..=max_lag + 1)
        .map(|lag| {
            let bpm = rate * 60.0 / lag as f64;
            let octaves = (bpm / PRIOR_BPM).log2() / PRIOR_OCTAVES;
            // Bars and half bars repeat too, unlike an off-beat pattern at a near tempo.
            let periodicity = autocorrelation(lag)
                + 0.5 * autocorrelation(2 * lag)
                + 0.25 * autocorrelation(4 * lag);
            periodicity * (-0.5 * octaves * octaves).exp()
        })
        .collect();
    // Best lag within the range (the scores have one extra lag on each side).
    let best = (1..scores.len() - 1).max_by(|&a, &b| scores[a].total_cmp(&scores[b]))?;
    if scores[best] <= 0.0 {
        return None;
    }
    // Refines the period between lags with a parabola through the neighbors.
    let (left, center, right) = (scores[best - 1], scores[best], scores[best + 1]);
    let curvature = left - 2.0 * center + right;
    let shift = if curvature < 0.0 {
        (0.5 * (left - right) / curvature).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    Some((min_lag - 1 + best) as f64 + shift)
}

/// The beats (envelope sample indices) that best fit `period`, and their onsets' contrast (the
/// smoothed onset envelope at each beat, relative to its spread).
fn track(onsets: &[f32], period: f64) -> (Vec<usize>, Vec<f64>) {
    let n = onsets.len();
    let std = {
        let mean = onsets.iter().map(|&o| f64::from(o)).sum::<f64>() / n as f64;
        (onsets
            .iter()
            .map(|&o| (f64::from(o) - mean).powi(2))
            .sum::<f64>()
            / n as f64)
            .sqrt()
    };
    if std <= 0.0 {
        return (Vec::new(), Vec::new());
    }
    // Onsets smoothed over a few percent of a beat, so beats can land next to their peak.
    let reach = period.round() as isize;
    let kernel: Vec<f64> = (-reach..=reach)
        .map(|k| (-0.5 * (k as f64 * 32.0 / period).powi(2)).exp())
        .collect();
    let local: Vec<f64> = (0..n as isize)
        .map(|i| {
            kernel
                .iter()
                .enumerate()
                .filter_map(|(j, w)| {
                    let at = i + j as isize - reach;
                    (0..n as isize)
                        .contains(&at)
                        .then(|| w * f64::from(onsets[at as usize]) / std)
                })
                .sum()
        })
        .collect();

    // The best score of a beat sequence ending at each sample, and its previous beat.
    let earliest = (2.0 * period).round() as usize;
    let latest = (period / 2.0).round().max(1.0) as usize;
    let mut cumulative = vec![0.0f64; n];
    let mut previous = vec![usize::MAX; n];
    for t in 0..n {
        let mut best = 0.0;
        if t >= latest {
            let mut best_score = f64::NEG_INFINITY;
            let first = t.saturating_sub(earliest);
            for (prev, &previous_score) in cumulative[first..=t - latest].iter().enumerate() {
                let prev = first + prev;
                let interval = ((t - prev) as f64 / period).ln();
                let score = previous_score - TIGHTNESS * interval * interval;
                if score > best_score {
                    best_score = score;
                    previous[t] = prev;
                }
            }
            // A sequence may also start here.
            if best_score > 0.0 {
                best = best_score;
            } else {
                previous[t] = usize::MAX;
            }
        }
        cumulative[t] = local[t] + best;
    }

    // The last beat: the last local maximum scoring at least half the median local maximum.
    let maxima: Vec<usize> = (1..n.saturating_sub(1))
        .filter(|&t| cumulative[t] > cumulative[t - 1] && cumulative[t] >= cumulative[t + 1])
        .collect();
    if maxima.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut values: Vec<f64> = maxima.iter().map(|&t| cumulative[t]).collect();
    values.sort_by(f64::total_cmp);
    let threshold = 0.5 * values[values.len() / 2];
    let Some(&last) = maxima.iter().rev().find(|&&t| cumulative[t] >= threshold) else {
        return (Vec::new(), Vec::new());
    };
    let mut beats = vec![last];
    while let Some(&prev) = previous.get(*beats.last().unwrap()) {
        if prev == usize::MAX {
            break;
        }
        beats.push(prev);
    }
    beats.reverse();

    // Drops the weak beats the tracker extrapolates into the silence before and after the music.
    let rms = (beats.iter().map(|&b| local[b] * local[b]).sum::<f64>() / beats.len() as f64).sqrt();
    let strong = |&b: &usize| local[b] >= 0.5 * rms;
    let first = beats.iter().position(strong).unwrap_or(0);
    let end = beats.len() - beats.iter().rev().position(strong).unwrap_or(0);
    let beats = beats[first..end.max(first)].to_vec();
    let contrasts = beats.iter().map(|&b| local[b]).collect();
    (beats, contrasts)
}

/// The beats' strengths: their onsets' contrast, scaled by their loudness (the amplitude of the
/// peak windowed `energy` around each beat), relative to the track's strong beats so a few
/// outliers don't flatten the rest. The contrast alone would make a beat 40dB down pulse as hard
/// as one at full volume.
fn strengths(beats: &[usize], contrasts: &[f64], energy: &[f64]) -> Vec<u8> {
    let levels: Vec<f64> = beats
        .iter()
        .zip(contrasts)
        .map(|(&b, &contrast)| {
            // From a little before the beat, which may land next to its onset, to past its attack.
            let around = &energy[b.saturating_sub(ENERGY_WINDOW / 2).min(energy.len())
                ..(b + ENERGY_WINDOW + 1).min(energy.len())];
            let peak = around.iter().copied().fold(0.0, f64::max);
            contrast * peak.sqrt()
        })
        .collect();
    let mut sorted = levels.clone();
    sorted.sort_by(f64::total_cmp);
    let reference = sorted
        .get(sorted.len() * 9 / 10)
        .copied()
        .unwrap_or(0.0)
        .max(f64::MIN_POSITIVE);
    levels
        .iter()
        .map(|level| (level / reference * 255.0).round().clamp(0.0, 255.0) as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 8000;

    /// Kicks (decaying 60Hz bursts) at `bpm`, starting at `first` seconds, for `seconds`.
    fn kicks(bpm: f64, first: f64, seconds: f64) -> Vec<f32> {
        kicks_at(bpm, first, seconds, 30.0, |_| 0.8)
    }

    /// Kicks decaying at `decay` per second, with the amplitude `amplitude(t)` at `t` seconds.
    fn kicks_at(
        bpm: f64,
        first: f64,
        seconds: f64,
        decay: f64,
        amplitude: impl Fn(f64) -> f64,
    ) -> Vec<f32> {
        let n = (seconds * f64::from(RATE)) as usize;
        let period = 60.0 / bpm;
        (0..n)
            .map(|i| {
                let t = i as f64 / f64::from(RATE);
                if t < first {
                    return 0.0;
                }
                let since = (t - first) % period;
                ((2.0 * std::f64::consts::PI * 60.0 * since).sin()
                    * (-since * decay).exp()
                    * amplitude(t)) as f32
            })
            .collect()
    }

    /// The median strength of the beats from `from` to `to` seconds.
    fn median_strength(beats: &[Beat], from: f64, to: f64) -> u8 {
        let mut strengths: Vec<u8> = beats
            .iter()
            .filter(|&&(ms, _)| (from * 1000.0..to * 1000.0).contains(&f64::from(ms)))
            .map(|&(_, strength)| strength)
            .collect();
        assert!(!strengths.is_empty(), "no beats from {from}s to {to}s");
        strengths.sort();
        strengths[strengths.len() / 2]
    }

    fn beats_of(samples: &[f32], offset: f64) -> Vec<Beat> {
        let mut tracker = BeatTracker::new(RATE);
        for &s in samples {
            tracker.push(&[s, s]);
        }
        tracker.finish(offset)
    }

    #[test]
    fn finds_the_kicks_of_a_four_on_the_floor_beat() {
        let beats = beats_of(&kicks(128.0, 0.25, 30.0), 0.0);
        let period_ms = 60_000.0 / 128.0;
        assert!(beats.len() >= 60, "{} beats", beats.len());
        for (i, &(ms, strength)) in beats.iter().enumerate() {
            let expected = 250.0 + i as f64 * period_ms;
            assert!(
                (f64::from(ms) - expected).abs() <= 20.0,
                "beat {i} at {ms}ms, expected {expected:.0}ms"
            );
            assert!(strength > 200, "beat {i} has strength {strength}");
        }
    }

    #[test]
    fn prefers_the_beat_over_half_and_double_time() {
        for bpm in [95.0, 124.0, 140.0, 174.0] {
            let beats = beats_of(&kicks(bpm, 0.1, 30.0), 0.0);
            let intervals: Vec<u32> = beats.windows(2).map(|w| w[1].0 - w[0].0).collect();
            let median = {
                let mut sorted = intervals.clone();
                sorted.sort();
                f64::from(sorted[sorted.len() / 2])
            };
            let found = 60_000.0 / median;
            assert!(
                (found / bpm - 1.0).abs() < 0.03 || (found / (bpm / 2.0) - 1.0).abs() < 0.03,
                "{bpm} BPM found as {found:.1}"
            );
            if bpm < 150.0 {
                assert!(
                    (found / bpm - 1.0).abs() < 0.03,
                    "{bpm} BPM found as {found:.1}"
                );
            }
        }
    }

    #[test]
    fn shifts_beats_by_the_skipped_start() {
        let samples = kicks(120.0, 1.0, 20.0);
        let first = beats_of(&samples, 0.0)[0].0;
        let shifted = beats_of(&samples, 0.25)[0].0;
        assert_eq!(first - shifted, 250);
    }

    #[test]
    fn quiet_passages_pulse_less() {
        // Loud, 40dB down, loud; then loud, 6dB down, loud.
        for (quiet, at_most) in [(0.008, 10), (0.4, 160)] {
            let samples = kicks_at(120.0, 0.25, 60.0, 10.0, |t| {
                if (20.0..40.0).contains(&t) {
                    quiet
                } else {
                    0.8
                }
            });
            let beats = beats_of(&samples, 0.0);
            let loud = [
                median_strength(&beats, 5.0, 15.0),
                median_strength(&beats, 45.0, 55.0),
            ];
            let soft = median_strength(&beats, 25.0, 35.0);
            assert!(loud.iter().all(|&s| s > 200), "loud beats {loud:?}");
            assert!(soft <= at_most, "beats at {quiet} have strength {soft}");
            assert!(
                soft > 0 || quiet < 0.01,
                "beats at {quiet} have no strength"
            );
        }
    }

    #[test]
    fn a_fade_out_fades_the_pulse() {
        let samples = kicks_at(120.0, 0.25, 40.0, 30.0, |t| {
            if t < 20.0 {
                0.8
            } else {
                0.8 * (40.0 - t) / 20.0
            }
        });
        let beats = beats_of(&samples, 0.0);
        let early = median_strength(&beats, 22.0, 26.0);
        let late = median_strength(&beats, 32.0, 36.0);
        assert!(median_strength(&beats, 5.0, 15.0) > 200);
        assert!(early > late, "{early} then {late} while fading out");
    }

    #[test]
    fn silence_and_short_audio_have_no_beats() {
        assert!(beats_of(&vec![0.0; RATE as usize * 20], 0.0).is_empty());
        assert!(beats_of(&kicks(128.0, 0.0, 2.0), 0.0).is_empty());
    }
}
