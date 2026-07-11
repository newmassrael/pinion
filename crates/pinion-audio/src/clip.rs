//! Decoded PCM audio — the currency the mixer sums.
//!
//! An [`AudioClip`] is already-decoded PCM: interleaved `f32` samples in
//! `[-1.0, 1.0]`. Decoding a compressed/container format into this shape is
//! the [`crate::wav`] layer (and, later, OGG/FLAC); the mixer never sees a
//! codec. This is the "audio playback is not codec embedding" boundary
//! (§3 / §5.54) made concrete: PCM is the in-engine currency.

use std::sync::Arc;

/// A block of decoded PCM audio.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioClip {
    /// Frames per second (e.g. `44_100`).
    sample_rate: u32,
    /// Interleaved channel count (1 = mono, 2 = stereo).
    channels: u16,
    /// Interleaved samples in `[-1.0, 1.0]`; length is `frame_count *
    /// channels`.
    samples: Vec<f32>,
}

impl AudioClip {
    /// Build a clip from interleaved PCM. `channels` must be non-zero and
    /// divide `samples.len()`; a ragged tail is truncated to whole frames.
    #[must_use]
    pub fn new(sample_rate: u32, channels: u16, mut samples: Vec<f32>) -> Self {
        let ch = channels.max(1) as usize;
        let whole = (samples.len() / ch) * ch;
        samples.truncate(whole);
        Self {
            sample_rate,
            channels: channels.max(1),
            samples,
        }
    }

    /// Wrap in an [`Arc`] so many voices share one clip without copying.
    #[must_use]
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Frames per second.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Interleaved channel count.
    #[must_use]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Interleaved samples.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Number of whole frames (a frame = one sample per channel).
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    /// `true` when the clip carries no frames.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Duration in seconds.
    #[must_use]
    pub fn duration_secs(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        // frame_count and sample_rate are small enough to be exact in f32.
        #[allow(clippy::cast_precision_loss)]
        {
            self.frame_count() as f32 / self.sample_rate as f32
        }
    }

    /// The interleaved samples of frame `frame`, or an empty slice if out of
    /// range. Slice length is [`Self::channels`].
    #[must_use]
    pub fn frame(&self, frame: usize) -> &[f32] {
        let ch = self.channels as usize;
        let start = frame * ch;
        self.samples.get(start..start + ch).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_ragged_tail_to_whole_frames() {
        // 5 samples, stereo → 2 whole frames (4 samples), tail dropped.
        let clip = AudioClip::new(48_000, 2, vec![0.1, 0.2, 0.3, 0.4, 0.5]);
        assert_eq!(clip.frame_count(), 2);
        assert_eq!(clip.samples().len(), 4);
        assert_eq!(clip.frame(1), &[0.3, 0.4]);
        assert_eq!(clip.frame(9), &[] as &[f32]);
    }

    #[test]
    fn duration_from_frames_and_rate() {
        let clip = AudioClip::new(1000, 1, vec![0.0; 500]);
        assert!((clip.duration_secs() - 0.5).abs() < 1e-6);
    }
}
