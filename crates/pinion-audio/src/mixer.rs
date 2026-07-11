//! The mixer — the algorithmic heart. Sums active voices into a stereo
//! output buffer.
//!
//! A [`Voice`] is one playing instance of an [`AudioClip`]:
//! a playhead, a gain, a stereo pan, and a loop flag. Mixing is a pure,
//! deterministic sum — no RNG, no wall-clock — so a headless render and a
//! live device agree sample-for-sample (the same determinism the rest of
//! pinion relies on, §2 #3).
//!
//! Increment 1 assumes clips are at the engine's output rate (no
//! resampling) and mixes to stereo. A mono clip is panned with a
//! constant-power pot; a stereo clip uses a linear L/R balance. Resampling
//! and richer 3D spatialisation are follow-ups on this same voice model.

use std::sync::Arc;

use crate::clip::AudioClip;

/// One playing instance of a clip.
#[derive(Clone, Debug)]
pub struct Voice {
    clip: Arc<AudioClip>,
    label: String,
    playhead: usize,
    gain: f32,
    pan: f32,
    looping: bool,
    finished: bool,
}

impl Voice {
    /// Start a voice for `clip`, tagged `label`, with `gain` (linear),
    /// `pan` (`-1.0` left … `1.0` right), and `looping`.
    #[must_use]
    pub fn new(
        clip: Arc<AudioClip>,
        label: impl Into<String>,
        gain: f32,
        pan: f32,
        looping: bool,
    ) -> Self {
        Self {
            clip,
            label: label.into(),
            playhead: 0,
            gain,
            pan: pan.clamp(-1.0, 1.0),
            looping,
            finished: false,
        }
    }

    /// This voice's label (the sound's name — the introspection handle).
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Linear gain.
    #[must_use]
    pub fn gain(&self) -> f32 {
        self.gain
    }

    /// Set the linear gain (live volume change).
    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
    }

    /// Stereo pan in `[-1.0, 1.0]`.
    #[must_use]
    pub fn pan(&self) -> f32 {
        self.pan
    }

    /// Set the stereo pan, clamped to `[-1.0, 1.0]`.
    pub fn set_pan(&mut self, pan: f32) {
        self.pan = pan.clamp(-1.0, 1.0);
    }

    /// Whether the voice loops at the clip end.
    #[must_use]
    pub fn looping(&self) -> bool {
        self.looping
    }

    /// Whether the voice has reached the end of a non-looping clip.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Playback position in seconds.
    #[must_use]
    pub fn position_secs(&self) -> f32 {
        let rate = self.clip.sample_rate();
        if rate == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)] // playhead / rate are small, exact in f32.
        {
            self.playhead as f32 / rate as f32
        }
    }

    /// Mark the voice finished so the engine drops it on the next sweep.
    pub fn stop(&mut self) {
        self.finished = true;
    }

    /// Mix this voice into an interleaved stereo `out` buffer (length must be
    /// even), advancing the playhead. Adds to `out` (does not clear it), so
    /// the engine clears once and sums every voice.
    pub fn mix_into(&mut self, out: &mut [f32]) {
        if self.finished {
            return;
        }
        let (pan_l, pan_r) = pan_gains(self.pan);
        let channels = self.clip.channels() as usize;
        let frame_count = self.clip.frame_count();

        for frame_out in out.chunks_exact_mut(2) {
            if self.playhead >= frame_count {
                if self.looping && frame_count > 0 {
                    self.playhead = 0;
                } else {
                    self.finished = true;
                    return;
                }
            }
            let (l, r) = mix_sample(
                self.clip.frame(self.playhead),
                channels,
                self.gain,
                self.pan,
                pan_l,
                pan_r,
            );
            frame_out[0] += l;
            frame_out[1] += r;
            self.playhead += 1;
        }
    }
}

/// Constant-power pan gains for a mono source: at centre both legs are
/// `~0.707` (equal power, -3 dB), hard-left is `(1, 0)`, hard-right `(0, 1)`.
fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (angle.cos(), angle.sin())
}

/// One frame's stereo contribution: mono is pan-potted, stereo is
/// L/R-balanced.
fn mix_sample(
    frame: &[f32],
    channels: usize,
    gain: f32,
    pan: f32,
    pan_l: f32,
    pan_r: f32,
) -> (f32, f32) {
    if frame.is_empty() {
        return (0.0, 0.0);
    }
    if channels == 1 {
        let s = frame[0] * gain;
        (s * pan_l, s * pan_r)
    } else {
        let left = frame[0];
        let right = frame.get(1).copied().unwrap_or(left);
        let balance_l = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
        let balance_r = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
        (left * gain * balance_l, right * gain * balance_r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono(samples: &[f32]) -> Arc<AudioClip> {
        AudioClip::new(48_000, 1, samples.to_vec()).shared()
    }

    #[test]
    fn center_pan_is_equal_power() {
        let mut v = Voice::new(mono(&[1.0]), "s", 1.0, 0.0, false);
        let mut out = [0.0f32; 2];
        v.mix_into(&mut out);
        assert!((out[0] - out[1]).abs() < 1e-6, "L==R at centre");
        assert!((out[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-4);
    }

    #[test]
    fn hard_left_silences_right() {
        let mut v = Voice::new(mono(&[1.0]), "s", 1.0, -1.0, false);
        let mut out = [0.0f32; 2];
        v.mix_into(&mut out);
        assert!((out[0] - 1.0).abs() < 1e-4, "L full");
        assert!(out[1].abs() < 1e-4, "R silent");
    }

    #[test]
    fn two_voices_sum_and_gain_applies() {
        let mut a = Voice::new(mono(&[1.0, 1.0]), "a", 0.5, 0.0, false);
        let mut b = Voice::new(mono(&[1.0, 1.0]), "b", 0.25, 0.0, false);
        let mut out = [0.0f32; 4];
        a.mix_into(&mut out);
        b.mix_into(&mut out);
        // Each leg = (0.5 + 0.25) * 0.707 per frame.
        let expected = 0.75 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((out[0] - expected).abs() < 1e-4);
        assert!((out[2] - expected).abs() < 1e-4);
    }

    #[test]
    fn one_shot_finishes_and_stops_contributing() {
        let mut v = Voice::new(mono(&[1.0]), "s", 1.0, 0.0, false);
        let mut out = [0.0f32; 4]; // 2 frames, clip has 1
        v.mix_into(&mut out);
        assert!(v.is_finished());
        // Frame 0 filled, frame 1 untouched (silence after end).
        assert!(out[0] > 0.0);
        assert!(out[2].abs() < 1e-9);
        assert!(out[3].abs() < 1e-9);
    }

    #[test]
    fn loop_wraps_the_playhead() {
        let mut v = Voice::new(mono(&[1.0]), "s", 1.0, 0.0, true);
        let mut out = [0.0f32; 6]; // 3 frames, clip has 1 → wraps twice
        v.mix_into(&mut out);
        assert!(!v.is_finished(), "looping never finishes");
        assert!(
            out[0] > 0.0 && out[2] > 0.0 && out[4] > 0.0,
            "every frame filled"
        );
    }
}
