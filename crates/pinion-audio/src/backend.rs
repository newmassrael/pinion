//! The output-device seam.
//!
//! [`AudioBackend`] is where a rendered stereo block goes: a real sound
//! device, or the headless [`InMemoryAudioBackend`] that captures it for
//! verification. The engine does the rendering; the backend is only the
//! sink. This is the same trait + in-memory-double pattern the platform
//! tray uses (`TrayBackend` / `InMemoryTrayBackend`): the substrate is
//! fully testable headlessly, and a real device backend (cpal, pushing into
//! a ring buffer its callback drains) drops in behind the same trait —
//! deferred here because a headless box has no audio device to verify it.

use crate::engine::AudioEngine;

/// A stereo output sink.
pub trait AudioBackend {
    /// The output sample rate this backend expects (the engine should
    /// render at the same rate).
    fn sample_rate(&self) -> u32;

    /// Accept one block of interleaved stereo samples for output.
    fn submit(&mut self, stereo: &[f32]);
}

/// Render `frames` stereo frames from `engine` and submit them to `backend`
/// — one pump step. A real backend calls this from (or feeds) its device
/// callback; a test calls it directly and inspects the captured output.
pub fn pump(engine: &mut AudioEngine, backend: &mut dyn AudioBackend, frames: usize) {
    let mut block = vec![0.0f32; frames * 2];
    engine.render(&mut block);
    backend.submit(&block);
}

/// Peak absolute amplitude of a sample buffer — the shared "is anything
/// audible" probe used by the capture backend, the RT snapshot, and tests.
#[must_use]
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()))
}

/// A headless backend that captures every submitted sample — the audio
/// analogue of a golden-buffer render target.
#[derive(Debug, Default)]
pub struct InMemoryAudioBackend {
    sample_rate: u32,
    captured: Vec<f32>,
}

impl InMemoryAudioBackend {
    /// A capture backend at `sample_rate`.
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            captured: Vec::new(),
        }
    }

    /// Every interleaved stereo sample submitted so far.
    #[must_use]
    pub fn captured(&self) -> &[f32] {
        &self.captured
    }

    /// Number of stereo frames captured.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.captured.len() / 2
    }

    /// Peak absolute sample — a headless "is anything audible" probe.
    #[must_use]
    pub fn peak(&self) -> f32 {
        peak(&self.captured)
    }

    /// Drop the captured buffer.
    pub fn clear(&mut self) {
        self.captured.clear();
    }
}

impl AudioBackend for InMemoryAudioBackend {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn submit(&mut self, stereo: &[f32]) {
        self.captured.extend_from_slice(stereo);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::AudioClip;
    use crate::engine::PlayOptions;

    #[test]
    fn capture_backend_records_audible_output() {
        let mut engine = AudioEngine::new(48_000);
        let clip = AudioClip::new(48_000, 1, vec![1.0; 64]).shared();
        engine.play(clip, "bell", PlayOptions::one_shot());

        let mut backend = InMemoryAudioBackend::new(48_000);
        pump(&mut engine, &mut backend, 64);

        assert_eq!(backend.frame_count(), 64);
        assert!(backend.peak() > 0.5, "output is audible");
    }

    #[test]
    fn silence_when_nothing_plays() {
        let mut engine = AudioEngine::new(48_000);
        let mut backend = InMemoryAudioBackend::new(48_000);
        pump(&mut engine, &mut backend, 32);
        assert_eq!(backend.frame_count(), 32);
        assert!(backend.peak().abs() < 1e-9, "no voices → silence");
    }
}
