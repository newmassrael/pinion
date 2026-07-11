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
use crate::spatial::{Attenuation, Listener, Spatialization, Vec3, spatialize};

/// A handle to one playing voice, unique for the engine's lifetime.
pub type VoiceId = u64;

/// How a clip should start playing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayOptions {
    /// Linear gain.
    pub gain: f32,
    /// Stereo pan, `-1.0` left … `1.0` right. Ignored for a 3D voice (a
    /// `position` is set): the pan is then derived from listener geometry.
    pub pan: f32,
    /// Loop at the clip end instead of finishing (ambience vs one-shot).
    pub looping: bool,
    /// World position of a 3D voice, or `None` for a flat (statically panned)
    /// voice.
    pub position: Option<Vec3>,
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            looping: false,
            position: None,
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

    /// Place the voice in 3D at `position` (its pan/gain are then resolved
    /// from the listener each render).
    #[must_use]
    pub fn with_position(mut self, position: Vec3) -> Self {
        self.position = Some(position);
        self
    }
}

/// A voice's gain/pan after spatialisation — what the mixer is actually
/// driven with, and what introspection reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedOutput {
    /// Effective linear gain (authored gain × distance attenuation).
    pub gain: f32,
    /// Effective stereo pan.
    pub pan: f32,
    /// Distance from the listener, `Some` only for a 3D voice.
    pub distance: Option<f32>,
}

/// Resolve a voice's effective output — the single place authored gain/pan +
/// (optional) 3D position + the listener become the gain/pan the mixer uses.
/// A free function so [`AudioEngine::render`] can call it while iterating the
/// voices mutably (a `&self` method would alias the whole engine).
fn resolve(voice: &Voice, listener: &Listener, attenuation: &Attenuation) -> ResolvedOutput {
    match voice.position() {
        Some(pos) => {
            let Spatialization {
                distance,
                gain,
                pan,
            } = spatialize(listener, pos, attenuation);
            ResolvedOutput {
                gain: voice.gain() * gain,
                pan,
                distance: Some(distance),
            }
        }
        None => ResolvedOutput {
            gain: voice.gain(),
            pan: voice.pan(),
            distance: None,
        },
    }
}

/// The engine: active voices, a master bus gain, the 3D listener, and the
/// mix render.
#[derive(Debug)]
pub struct AudioEngine {
    sample_rate: u32,
    voices: Vec<(VoiceId, Voice)>,
    next_id: VoiceId,
    master_gain: f32,
    listener: Listener,
    attenuation: Attenuation,
}

impl AudioEngine {
    /// A fresh engine rendering at `sample_rate` (its stereo output rate),
    /// with the listener at the origin and the default distance model.
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            voices: Vec::new(),
            next_id: 1,
            master_gain: 1.0,
            listener: Listener::default(),
            attenuation: Attenuation::default(),
        }
    }

    /// The engine's output sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The id [`AudioEngine::play`] will mint next — the seam a command queue
    /// uses to pre-allocate ids on the control thread.
    #[must_use]
    pub fn next_voice_id(&self) -> VoiceId {
        self.next_id
    }

    /// Start playing `clip` (tagged `label`) with `opts`; returns its id.
    pub fn play(
        &mut self,
        clip: Arc<AudioClip>,
        label: impl Into<String>,
        opts: PlayOptions,
    ) -> VoiceId {
        let id = self.next_id;
        self.play_with_id(id, clip, label, opts);
        id
    }

    /// Start playing `clip` under a caller-chosen `id` — the entry point for a
    /// command queue that minted the id on the control thread. The engine's
    /// own counter is advanced past `id` so a later [`AudioEngine::play`]
    /// cannot collide with it.
    pub fn play_with_id(
        &mut self,
        id: VoiceId,
        clip: Arc<AudioClip>,
        label: impl Into<String>,
        opts: PlayOptions,
    ) {
        self.next_id = self.next_id.max(id + 1);
        let mut voice = Voice::new(clip, label, opts.gain, opts.pan, opts.looping);
        if let Some(pos) = opts.position {
            voice = voice.with_position(pos);
        }
        self.voices.push((id, voice));
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

    /// The 3D listener (the ears the mix is rendered for).
    #[must_use]
    pub fn listener(&self) -> Listener {
        self.listener
    }

    /// Move / re-orient the listener.
    pub fn set_listener(&mut self, listener: Listener) {
        self.listener = listener;
    }

    /// The distance-attenuation model applied to 3D voices.
    #[must_use]
    pub fn attenuation(&self) -> Attenuation {
        self.attenuation
    }

    /// Set the distance-attenuation model.
    pub fn set_attenuation(&mut self, attenuation: Attenuation) {
        self.attenuation = attenuation;
    }

    /// The effective gain/pan a voice renders with after spatialisation — the
    /// introspection view of "what does this sound's positioning resolve to".
    /// Flat voices resolve to their authored gain/pan with no distance.
    #[must_use]
    pub fn resolve_voice(&self, voice: &Voice) -> ResolvedOutput {
        resolve(voice, &self.listener, &self.attenuation)
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
    /// applying per-voice spatialisation and the master gain, and dropping
    /// voices that finished.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        // Copied out so the voices can be borrowed mutably in the loop while
        // the listener / attenuation are read (they are small and `Copy`).
        let (listener, attenuation) = (self.listener, self.attenuation);
        for (_, voice) in &mut self.voices {
            let out_gain_pan = resolve(voice, &listener, &attenuation);
            voice.mix_into_with(out, self.sample_rate, out_gain_pan.gain, out_gain_pan.pan);
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

    #[test]
    fn resolve_distinguishes_flat_and_spatial_voices() {
        let mut engine = AudioEngine::new(48_000);
        let flat = engine.play(
            tone(4),
            "flat",
            PlayOptions::one_shot().with_gain(0.7).with_pan(-0.5),
        );
        // Emitter to the world-right at twice the reference distance:
        // attenuation halves the gain, and the pan resolves hard right.
        let spatial = engine.play(
            tone(4),
            "spatial",
            PlayOptions::one_shot()
                .with_gain(1.0)
                .with_position([2.0, 0.0, 0.0]),
        );
        for (id, voice) in engine.voices() {
            let r = engine.resolve_voice(voice);
            if id == flat {
                assert!((r.gain - 0.7).abs() < 1e-6, "flat keeps authored gain");
                assert!((r.pan + 0.5).abs() < 1e-6, "flat keeps authored pan");
                assert!(r.distance.is_none(), "flat voice has no distance");
            } else if id == spatial {
                assert!((r.distance.unwrap() - 2.0).abs() < 1e-4);
                assert!((r.gain - 0.5).abs() < 1e-4, "gain halved at 2× reference");
                assert!((r.pan - 1.0).abs() < 1e-4, "hard right");
            }
        }
    }
}
