//! The audio engine — the in-memory graph the mixer renders and RPC reads.
//!
//! [`AudioEngine`] owns the active [`Voice`]s and a master gain, mints a
//! [`VoiceId`] per [`AudioEngine::play`], and renders the mix on demand via
//! [`AudioEngine::render`]. The whole graph (which sounds, their
//! gain/pan/position/loop) is plain data readable through
//! [`AudioEngine::voices`] — that is the §5.54 requirement: audio is
//! scene-as-data, not an opaque handle.

use std::sync::Arc;

use crate::clip::AudioClip;
use crate::mixer::Voice;

/// A handle to one playing voice, unique for the engine's lifetime.
pub type VoiceId = u64;

/// How a clip should start playing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayOptions {
    /// Linear gain.
    pub gain: f32,
    /// Stereo pan, `-1.0` left … `1.0` right.
    pub pan: f32,
    /// Loop at the clip end instead of finishing (ambience vs one-shot).
    pub looping: bool,
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            looping: false,
        }
    }
}

impl PlayOptions {
    /// A one-shot SFX at unit gain, centred.
    #[must_use]
    pub fn one_shot() -> Self {
        Self::default()
    }

    /// A looping ambience at unit gain, centred.
    #[must_use]
    pub fn looping() -> Self {
        Self {
            looping: true,
            ..Self::default()
        }
    }

    /// Set the gain.
    #[must_use]
    pub fn with_gain(mut self, gain: f32) -> Self {
        self.gain = gain;
        self
    }

    /// Set the pan.
    #[must_use]
    pub fn with_pan(mut self, pan: f32) -> Self {
        self.pan = pan;
        self
    }
}

/// The engine: active voices, a master bus gain, and the mix render.
#[derive(Debug)]
pub struct AudioEngine {
    sample_rate: u32,
    voices: Vec<(VoiceId, Voice)>,
    next_id: VoiceId,
    master_gain: f32,
}

impl AudioEngine {
    /// A fresh engine rendering at `sample_rate` (its stereo output rate).
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            voices: Vec::new(),
            next_id: 1,
            master_gain: 1.0,
        }
    }

    /// The engine's output sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Start playing `clip` (tagged `label`) with `opts`; returns its id.
    pub fn play(
        &mut self,
        clip: Arc<AudioClip>,
        label: impl Into<String>,
        opts: PlayOptions,
    ) -> VoiceId {
        let id = self.next_id;
        self.next_id += 1;
        self.voices.push((
            id,
            Voice::new(clip, label, opts.gain, opts.pan, opts.looping),
        ));
        id
    }

    /// Stop the voice with `id` (it stops contributing and is dropped on the
    /// next render). Returns whether a voice matched.
    pub fn stop(&mut self, id: VoiceId) -> bool {
        if let Some((_, voice)) = self.voices.iter_mut().find(|(vid, _)| *vid == id) {
            voice.stop();
            true
        } else {
            false
        }
    }

    /// Stop every voice.
    pub fn stop_all(&mut self) {
        for (_, voice) in &mut self.voices {
            voice.stop();
        }
    }

    /// The master (output-bus) gain.
    #[must_use]
    pub fn master_gain(&self) -> f32 {
        self.master_gain
    }

    /// Set the master gain.
    pub fn set_master_gain(&mut self, gain: f32) {
        self.master_gain = gain;
    }

    /// Set the gain of the voice with `id`. Returns whether it matched.
    pub fn set_voice_gain(&mut self, id: VoiceId, gain: f32) -> bool {
        if let Some((_, voice)) = self.voices.iter_mut().find(|(vid, _)| *vid == id) {
            voice.set_gain(gain);
            true
        } else {
            false
        }
    }

    /// Number of live voices.
    #[must_use]
    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// Iterate the live voices with their ids — the introspection surface.
    pub fn voices(&self) -> impl Iterator<Item = (VoiceId, &Voice)> {
        self.voices.iter().map(|(id, voice)| (*id, voice))
    }

    /// Render the current mix into an interleaved stereo `out` buffer,
    /// applying the master gain and dropping voices that finished.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        for (_, voice) in &mut self.voices {
            voice.mix_into(out);
        }
        if (self.master_gain - 1.0).abs() > f32::EPSILON {
            for sample in out.iter_mut() {
                *sample *= self.master_gain;
            }
        }
        self.voices.retain(|(_, voice)| !voice.is_finished());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize) -> Arc<AudioClip> {
        AudioClip::new(48_000, 1, vec![1.0; frames]).shared()
    }

    #[test]
    fn play_then_stop_drops_the_voice_on_render() {
        let mut engine = AudioEngine::new(48_000);
        let id = engine.play(tone(4), "bell", PlayOptions::one_shot());
        assert_eq!(engine.voice_count(), 1);
        assert!(engine.stop(id));
        let mut out = [0.0f32; 8];
        engine.render(&mut out);
        assert_eq!(engine.voice_count(), 0, "stopped voice dropped");
    }

    #[test]
    fn one_shot_is_reaped_after_it_ends() {
        let mut engine = AudioEngine::new(48_000);
        engine.play(tone(2), "click", PlayOptions::one_shot());
        let mut out = [0.0f32; 8]; // 4 frames > clip's 2
        engine.render(&mut out);
        assert_eq!(engine.voice_count(), 0, "finished one-shot reaped");
    }

    #[test]
    fn looping_ambience_survives_render() {
        let mut engine = AudioEngine::new(48_000);
        engine.play(tone(2), "waves", PlayOptions::looping());
        let mut out = [0.0f32; 16];
        engine.render(&mut out);
        assert_eq!(engine.voice_count(), 1, "loop stays alive");
    }

    #[test]
    fn master_gain_scales_output() {
        let mut engine = AudioEngine::new(48_000);
        engine.set_master_gain(0.5);
        engine.play(tone(4), "bell", PlayOptions::one_shot().with_pan(-1.0));
        let mut full = [0.0f32; 8];
        engine.render(&mut full);
        // Hard-left mono at gain 1, master 0.5 → left leg ~0.5.
        assert!((full[0] - 0.5).abs() < 1e-4);
    }
}
