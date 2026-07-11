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
    /// The bounded voice-pool size, or `None` for an unbounded (growing)
    /// engine. A bounded engine pre-reserves `voices` to this size and rejects
    /// a play at the bound rather than reallocating — the real-time voice
    /// budget (see [`AudioEngine::set_voice_capacity`]).
    voice_capacity: Option<usize>,
}

impl AudioEngine {
    /// A fresh engine rendering at `sample_rate` (its stereo output rate),
    /// with the listener at the origin and the default distance model. The
    /// voice buffer grows on demand (unbounded); a real-time consumer bounds
    /// it with [`AudioEngine::with_voice_capacity`] so the audio thread never
    /// reallocates.
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            voices: Vec::new(),
            next_id: 1,
            master_gain: 1.0,
            listener: Listener::default(),
            attenuation: Attenuation::default(),
            voice_capacity: None,
        }
    }

    /// A fresh engine bounded to `max_voices` simultaneous voices, with the
    /// voice buffer pre-reserved so [`AudioEngine::play_with_id`] never
    /// reallocates on a real-time thread — the pool the [`crate::rt`] renderer
    /// wants.
    #[must_use]
    pub fn with_voice_capacity(sample_rate: u32, max_voices: usize) -> Self {
        let mut engine = Self::new(sample_rate);
        engine.set_voice_capacity(max_voices);
        engine
    }

    /// Bound the engine to `max_voices` simultaneous voices and pre-reserve the
    /// backing store, so [`AudioEngine::play_with_id`] can fill the pool to the
    /// bound without ever reallocating (the audio-thread real-time-safety
    /// property). At the bound a further play is rejected — its voice handed
    /// back for disposal off the audio thread — rather than growing the buffer.
    /// The bound is raised to the current live voice count if that is larger,
    /// so existing voices are never orphaned.
    pub fn set_voice_capacity(&mut self, max_voices: usize) {
        let cap = max_voices.max(self.voices.len());
        // `reserve(additional)` guarantees capacity >= len + additional == cap,
        // so `len` can grow to `cap` without a reallocation.
        self.voices.reserve(cap - self.voices.len());
        self.voice_capacity = Some(cap);
    }

    /// The bounded voice-pool size, or `None` if the engine grows unbounded.
    #[must_use]
    pub fn voice_capacity(&self) -> Option<usize> {
        self.voice_capacity
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
        // The single-thread engine is unbounded, so this never rejects. A
        // bounded engine driven single-threaded would drop the returned voice
        // here — a free on the *calling* (not audio) thread, which is fine off
        // the real-time path.
        let _ = self.play_with_id(id, clip, label, opts);
        id
    }

    /// Start playing `clip` under a caller-chosen `id` — the entry point for a
    /// command queue that minted the id on the control thread. The engine's
    /// own counter is advanced past `id` so a later [`AudioEngine::play`]
    /// cannot collide with it.
    ///
    /// Returns the un-admitted [`Voice`] when the engine is at its bounded
    /// voice-pool capacity ([`AudioEngine::set_voice_capacity`]): the pool is
    /// full and growing it would reallocate on the audio thread, so the voice
    /// is handed back instead for the caller to dispose off-thread. Returns
    /// `None` when the voice is admitted, and — for an unbounded engine —
    /// always admits.
    #[must_use = "a rejected voice must be disposed off the audio thread, not dropped on it"]
    pub fn play_with_id(
        &mut self,
        id: VoiceId,
        clip: Arc<AudioClip>,
        label: impl Into<String>,
        opts: PlayOptions,
    ) -> Option<Voice> {
        let mut voice = Voice::new(clip, label, opts.gain, opts.pan, opts.looping);
        if let Some(pos) = opts.position {
            voice = voice.with_position(pos);
        }
        if let Some(cap) = self.voice_capacity {
            if self.voices.len() >= cap {
                // At the pre-allocated bound: reject rather than reallocate.
                return Some(voice);
            }
        }
        self.next_id = self.next_id.max(id.saturating_add(1));
        self.voices.push((id, voice));
        None
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

    /// The live voice with `id`, if any — the one place id lookup happens.
    fn voice_mut(&mut self, id: VoiceId) -> Option<&mut Voice> {
        self.voices
            .iter_mut()
            .find(|(vid, _)| *vid == id)
            .map(|(_, voice)| voice)
    }

    /// Set the gain of the voice with `id`. Returns whether it matched.
    pub fn set_voice_gain(&mut self, id: VoiceId, gain: f32) -> bool {
        match self.voice_mut(id) {
            Some(voice) => {
                voice.set_gain(gain);
                true
            }
            None => false,
        }
    }

    /// Set the stereo pan of the voice with `id`. Returns whether it matched.
    /// (Ignored acoustically while the voice has a 3D position, whose pan is
    /// derived — but still stored as the authored value.)
    pub fn set_voice_pan(&mut self, id: VoiceId, pan: f32) -> bool {
        match self.voice_mut(id) {
            Some(voice) => {
                voice.set_pan(pan);
                true
            }
            None => false,
        }
    }

    /// Move (or, with `None`, un-spatialise) the voice with `id`. Returns
    /// whether it matched.
    pub fn set_voice_position(&mut self, id: VoiceId, position: Option<Vec3>) -> bool {
        match self.voice_mut(id) {
            Some(voice) => {
                voice.set_position(position);
                true
            }
            None => false,
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
    /// voices that finished (freeing them here). This is
    /// [`AudioEngine::render_reaping`] with `retire = drop`.
    pub fn render(&mut self, out: &mut [f32]) {
        self.render_reaping(out, drop);
    }

    /// Render one block and hand each finished voice to `retire` instead of
    /// dropping it here — the real-time seam. [`AudioEngine::render`] passes
    /// `drop`; [`crate::AudioRenderer`] passes a closure that ships each
    /// retired voice to the control thread's resource-return queue, so the
    /// audio thread never frees a clip. Reaping is order-preserving, so the
    /// headless `render` and the real-time path stay in sample-for-sample
    /// lockstep (§2 #3).
    pub fn render_reaping(&mut self, out: &mut [f32], retire: impl FnMut(Voice)) {
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
        self.reap_finished(retire);
    }

    /// Remove finished voices in place, moving each retired [`Voice`] to
    /// `retire` rather than dropping it here. Order-preserving and
    /// non-allocating: an in-place `Vec::remove` compaction that keeps the
    /// buffer's reserved capacity, so it never *frees* the buffer either. The
    /// work is bounded by the live voice count (real-time-safe: bounded,
    /// lock-free, alloc-free); the order-preserving `Vec::remove` is O(n²) in
    /// the worst case but n is the small voice-pool size, and the O(n) in-place
    /// extract (`Vec::extract_if`, 1.87+) is both beyond this crate's MSRV and
    /// would need the `unsafe` this crate forbids.
    fn reap_finished(&mut self, mut retire: impl FnMut(Voice)) {
        let mut i = 0;
        while i < self.voices.len() {
            if self.voices[i].1.is_finished() {
                let (_, voice) = self.voices.remove(i);
                retire(voice);
            } else {
                i += 1;
            }
        }
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

    #[test]
    fn unbounded_engine_admits_every_play() {
        let mut engine = AudioEngine::new(48_000);
        assert_eq!(engine.voice_capacity(), None);
        for _ in 0..100 {
            let id = engine.next_id;
            assert!(
                engine
                    .play_with_id(id, tone(4), "v", PlayOptions::one_shot())
                    .is_none(),
                "unbounded engine never rejects"
            );
        }
        assert_eq!(engine.voice_count(), 100);
    }

    #[test]
    fn bounded_pool_rejects_beyond_capacity_and_returns_the_voice() {
        let mut engine = AudioEngine::with_voice_capacity(48_000, 2);
        assert_eq!(engine.voice_capacity(), Some(2));
        assert!(
            engine
                .play_with_id(1, tone(64), "a", PlayOptions::one_shot())
                .is_none()
        );
        assert!(
            engine
                .play_with_id(2, tone(64), "b", PlayOptions::one_shot())
                .is_none()
        );
        // The pool is full: the third play is refused and its voice handed back.
        let refused = engine.play_with_id(3, tone(64), "c", PlayOptions::one_shot());
        assert!(refused.is_some(), "third play rejected at the bound");
        assert_eq!(
            refused.unwrap().label(),
            "c",
            "the rejected voice is returned"
        );
        assert_eq!(engine.voice_count(), 2, "only the pool-sized set is live");
    }

    #[test]
    fn bounded_pool_never_reallocates() {
        let mut engine = AudioEngine::with_voice_capacity(48_000, 4);
        let reserved = engine.voices.capacity();
        assert!(reserved >= 4);
        // Many rounds of fill-to-bound then reap-all. `play_with_id` never
        // pushes past the bound and `render`'s reap never shrinks, so the
        // backing buffer keeps its capacity — no reallocation on what would be
        // the audio thread.
        for _ in 0..50 {
            for _ in 0..4 {
                let id = engine.next_id;
                assert!(
                    engine
                        .play_with_id(id, tone(1), "v", PlayOptions::one_shot())
                        .is_none()
                );
            }
            assert!(
                engine
                    .play_with_id(engine.next_id, tone(1), "over", PlayOptions::one_shot())
                    .is_some(),
                "the fifth play is rejected at the bound"
            );
            let mut out = [0.0f32; 8]; // 4 output frames > the 1-frame clips
            engine.render(&mut out);
            assert_eq!(engine.voice_count(), 0, "all one-shots reaped");
            assert_eq!(engine.voices.capacity(), reserved, "no reallocation");
        }
    }
}
