//! The mixer — the algorithmic heart. Sums active voices into a stereo
//! output buffer, resampling each clip to the output rate.
//!
//! A [`Voice`] is one playing instance of an [`AudioClip`]: a fractional
//! playhead, a gain, a stereo pan, and a loop flag. Mixing is a pure,
//! deterministic sum — no RNG, no wall-clock — so a headless render and a
//! live device agree sample-for-sample (§2 #3).
//!
//! Each voice **resamples** its clip to the engine's output rate by a
//! fractional playhead + linear interpolation, so a 44.1 kHz asset plays at
//! the right pitch on a 48 kHz engine (rather than the earlier silent
//! octave shift). A mono clip is panned with a constant-power pot; a stereo
//! clip uses a linear L/R balance. Higher-order resampling and richer 3D
//! spatialisation are refinements on this same voice model.

use std::sync::Arc;

use crate::clip::AudioClip;

/// One playing instance of a clip.
#[derive(Clone, Debug)]
pub struct Voice {
    clip: Arc<AudioClip>,
    label: String,
    /// Fractional frame position in the clip (advanced by the resample
    /// ratio each output frame).
    playhead: f64,
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
            playhead: 0.0,
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
        #[allow(clippy::cast_possible_truncation)] // position is small, f64->f32 exact here.
        {
            (self.playhead / f64::from(rate)) as f32
        }
    }

    /// Mark the voice finished so the engine drops it on the next sweep.
    pub fn stop(&mut self) {
        self.finished = true;
    }

    /// Mix this voice into an interleaved stereo `out` buffer (length must be
    /// even) at `out_rate`, resampling the clip and advancing the playhead.
    /// Adds to `out` (does not clear it), so the engine clears once and sums
    /// every voice.
    #[allow(clippy::cast_precision_loss)] // frame_count is small, exact in f64.
    pub fn mix_into(&mut self, out: &mut [f32], out_rate: u32) {
        if self.finished {
            return;
        }
        let frame_count = self.clip.frame_count();
        if frame_count == 0 {
            self.finished = true;
            return;
        }
        let (pan_l, pan_r) = pan_gains(self.pan);
        let channels = self.clip.channels() as usize;
        // Clip frames advanced per output frame (the resample ratio).
        let step = f64::from(self.clip.sample_rate().max(1)) / f64::from(out_rate.max(1));
        let len = frame_count as f64;

        for frame_out in out.chunks_exact_mut(2) {
            if self.playhead >= len {
                if self.looping {
                    self.playhead = self.playhead.rem_euclid(len);
                } else {
                    self.finished = true;
                    return;
                }
            }
            let (l, r) = self.sample_at(channels, pan_l, pan_r);
            frame_out[0] += l;
            frame_out[1] += r;
            self.playhead += step;
        }
    }

    /// Linearly interpolate the clip at the current fractional playhead and
    /// map it to a stereo `(left, right)` contribution.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn sample_at(&self, channels: usize, pan_l: f32, pan_r: f32) -> (f32, f32) {
        let frame_count = self.clip.frame_count();
        // playhead is guaranteed in `[0, frame_count)` by the caller.
        let i0 = self.playhead.floor() as usize;
        let frac = (self.playhead - self.playhead.floor()) as f32;
        let i1 = if i0 + 1 < frame_count {
            i0 + 1
        } else if self.looping {
            0
        } else {
            i0
        };
        let f0 = self.clip.frame(i0);
        let f1 = self.clip.frame(i1);
        if f0.is_empty() {
            return (0.0, 0.0);
        }
        if channels == 1 {
            let s0 = f0[0];
            let s1 = f1.first().copied().unwrap_or(s0);
            let s = lerp(s0, s1, frac) * self.gain;
            (s * pan_l, s * pan_r)
        } else {
            let l = lerp(f0[0], f1.first().copied().unwrap_or(f0[0]), frac);
            let r = lerp(
                f0.get(1).copied().unwrap_or(f0[0]),
                f1.get(1).copied().unwrap_or(f0[0]),
                frac,
            );
            let balance_l = if self.pan <= 0.0 { 1.0 } else { 1.0 - self.pan };
            let balance_r = if self.pan >= 0.0 { 1.0 } else { 1.0 + self.pan };
            (l * self.gain * balance_l, r * self.gain * balance_r)
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

/// Constant-power pan gains for a mono source: at centre both legs are
/// `~0.707` (equal power, -3 dB), hard-left is `(1, 0)`, hard-right `(0, 1)`.
fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (angle.cos(), angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono_at(rate: u32, samples: &[f32]) -> Arc<AudioClip> {
        AudioClip::new(rate, 1, samples.to_vec()).shared()
    }

    fn mono(samples: &[f32]) -> Arc<AudioClip> {
        mono_at(48_000, samples)
    }

    #[test]
    fn center_pan_is_equal_power() {
        let mut v = Voice::new(mono(&[1.0]), "s", 1.0, 0.0, false);
        let mut out = [0.0f32; 2];
        v.mix_into(&mut out, 48_000);
        assert!((out[0] - out[1]).abs() < 1e-6, "L==R at centre");
        assert!((out[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-4);
    }

    #[test]
    fn hard_left_silences_right() {
        let mut v = Voice::new(mono(&[1.0]), "s", 1.0, -1.0, false);
        let mut out = [0.0f32; 2];
        v.mix_into(&mut out, 48_000);
        assert!((out[0] - 1.0).abs() < 1e-4, "L full");
        assert!(out[1].abs() < 1e-4, "R silent");
    }

    #[test]
    fn two_voices_sum_and_gain_applies() {
        let mut a = Voice::new(mono(&[1.0, 1.0]), "a", 0.5, 0.0, false);
        let mut b = Voice::new(mono(&[1.0, 1.0]), "b", 0.25, 0.0, false);
        let mut out = [0.0f32; 4];
        a.mix_into(&mut out, 48_000);
        b.mix_into(&mut out, 48_000);
        let expected = 0.75 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((out[0] - expected).abs() < 1e-4);
        assert!((out[2] - expected).abs() < 1e-4);
    }

    #[test]
    fn one_shot_finishes_and_stops_contributing() {
        let mut v = Voice::new(mono(&[1.0]), "s", 1.0, 0.0, false);
        let mut out = [0.0f32; 4]; // 2 frames, clip has 1
        v.mix_into(&mut out, 48_000);
        assert!(v.is_finished());
        assert!(out[0] > 0.0);
        assert!(out[2].abs() < 1e-9);
        assert!(out[3].abs() < 1e-9);
    }

    #[test]
    fn loop_wraps_the_playhead() {
        let mut v = Voice::new(mono(&[1.0]), "s", 1.0, 0.0, true);
        let mut out = [0.0f32; 6]; // 3 frames, clip has 1 → wraps
        v.mix_into(&mut out, 48_000);
        assert!(!v.is_finished(), "looping never finishes");
        assert!(
            out[0] > 0.0 && out[2] > 0.0 && out[4] > 0.0,
            "every frame filled"
        );
    }

    #[test]
    fn resamples_a_half_rate_clip_to_double_duration() {
        // A 24 kHz clip on a 48 kHz engine advances 0.5 clip-frames per
        // output frame, so N clip frames fill ~2N output frames.
        let clip = mono_at(24_000, &[1.0; 10]);
        let mut v = Voice::new(clip, "s", 1.0, 0.0, false);
        let mut out = [0.0f32; 64]; // 32 output frames, ample
        v.mix_into(&mut out, 48_000);
        let audible = out.chunks_exact(2).filter(|f| f[0].abs() > 1e-6).count();
        // 10 clip frames at step 0.5 → playhead reaches 10.0 after 20 frames.
        assert_eq!(audible, 20, "half-rate clip doubles in output frames");
    }

    #[test]
    fn interpolates_between_frames() {
        // A 24 kHz ramp on 48 kHz: the odd output frames land on the .5
        // interpolation midpoints.
        let clip = mono_at(24_000, &[0.0, 1.0]);
        let mut v = Voice::new(clip, "s", 1.0, 0.0, false);
        let mut out = [0.0f32; 8];
        v.mix_into(&mut out, 48_000);
        let center = std::f32::consts::FRAC_1_SQRT_2;
        // frame0 = clip[0]=0; frame1 = lerp(0,1,0.5)=0.5.
        assert!(out[0].abs() < 1e-4);
        assert!((out[2] - 0.5 * center).abs() < 1e-3);
    }
}
