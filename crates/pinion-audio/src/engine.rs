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

/// What happens when a play arrives at a full bounded voice pool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, pinion_derive::VariantCensus)]
pub enum VoicePolicy {
    /// Refuse the newcomer, keeping every live voice — the real-time voice
    /// budget's v1 default. The refused play is counted in
    /// [`crate::AudioSnapshot::rejected`].
    #[default]
    RejectNewest,
    /// Evict the oldest live voice (lowest [`VoiceId`]) to make room, and admit
    /// the newcomer — game-audio "voice stealing". Age is the eviction key: the
    /// pool mints ids monotonically, so the lowest id is the oldest voice.
    /// Priority-weighted stealing (keep the most important / loudest) is a
    /// documented refinement that would add per-voice priority metadata `Voice`
    /// does not yet carry — a follow-up, not this policy.
    StealOldest,
}

impl VoicePolicy {
    /// Every policy, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; Self::ARMS] = [Self::RejectNewest, Self::StealOldest];

    /// R1638 — every spelling [`from_wire`](Self::from_wire) admits, in
    /// declaration order, as a `const` the schema can point an argument's
    /// domain at.
    ///
    /// **Projected from [`ALL`](Self::ALL) through [`as_wire`](Self::as_wire)**
    /// rather than written out, so the published vocabulary and the encoder
    /// cannot disagree — and `ALL`'s length is the derived `ARMS`, so a new
    /// policy is a build failure here instead of a vocabulary that is quietly
    /// one short on the wire. That projection is what makes a literal domain
    /// defensible at all; a hand-written list would be the disconnected census
    /// R1630 exists to end.
    pub const WIRE_NAMES: [&'static str; Self::ARMS] = {
        let mut out = [""; Self::ARMS];
        let mut i = 0;
        while i < Self::ARMS {
            out[i] = Self::ALL[i].as_wire();
            i += 1;
        }
        out
    };

    /// The stable wire string for RPC introspection (`query`/`invoke`).
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            VoicePolicy::RejectNewest => "reject_newest",
            VoicePolicy::StealOldest => "steal_oldest",
        }
    }

    /// Parse the wire string back, or `None` for an unknown policy.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "reject_newest" => Some(VoicePolicy::RejectNewest),
            "steal_oldest" => Some(VoicePolicy::StealOldest),
            _ => None,
        }
    }
}

/// The outcome of admitting a play at the (possibly bounded) pool — what, if
/// anything, the caller must dispose off the audio thread.
#[derive(Debug)]
#[must_use = "a returned voice must be disposed off the audio thread, not dropped on it"]
pub enum Admission {
    /// The newcomer was admitted and nothing was displaced.
    Admitted,
    /// The pool was full and the newcomer was refused
    /// ([`VoicePolicy::RejectNewest`]); its un-admitted [`Voice`] is handed back
    /// for off-thread disposal, keyed by its id (so both disposal arms carry
    /// their own key — the caller never re-derives it from the command).
    Rejected {
        /// The refused newcomer's id.
        id: VoiceId,
        /// The un-admitted voice, for off-thread disposal.
        voice: Voice,
    },
    /// The pool was full and the newcomer was admitted by evicting `victim`
    /// ([`VoicePolicy::StealOldest`]) — the evicted voice is handed back for
    /// off-thread disposal, keyed by its id.
    Stole {
        /// The evicted voice's id, so the control thread can prune its
        /// bookkeeping (label map).
        victim_id: VoiceId,
        /// The evicted voice, for off-thread disposal.
        victim: Voice,
    },
}

impl Admission {
    /// Whether the newcomer was admitted with nothing handed back to dispose
    /// (either an admit into a free slot, or a steal that displaced a victim —
    /// both put the newcomer in the pool; only `Rejected` did not).
    #[must_use]
    pub fn is_admitted(&self) -> bool {
        matches!(self, Admission::Admitted | Admission::Stole { .. })
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
    /// What a full-pool play does (reject the newcomer or steal the oldest).
    /// Only meaningful for a bounded engine.
    voice_policy: VoicePolicy,
}

impl AudioEngine {
    /// A fresh engine rendering at `sample_rate` (its stereo output rate),
    /// with the listener at the origin and the default distance model. The
    /// voice buffer grows on demand (unbounded); a real-time consumer bounds
    /// it with [`AudioEngine::set_voice_capacity`] so the audio thread never
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
            voice_policy: VoicePolicy::default(),
        }
    }

    /// The full-pool policy (reject the newcomer, or steal the oldest voice).
    #[must_use]
    pub fn voice_policy(&self) -> VoicePolicy {
        self.voice_policy
    }

    /// Set the full-pool policy. `RejectNewest` (default) refuses a play at the
    /// bound; `StealOldest` evicts the oldest live voice to admit it.
    pub fn set_voice_policy(&mut self, policy: VoicePolicy) {
        self.voice_policy = policy;
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
    ///
    /// This is the unbounded single-thread convenience: a bounded engine is
    /// driven through an [`crate::AudioController`], which reports rejection
    /// via its snapshot. On a bounded engine at capacity `play` would return an
    /// id for a voice that was never admitted (and not advance the id counter),
    /// so that misuse is guarded in debug builds.
    pub fn play(
        &mut self,
        clip: Arc<AudioClip>,
        label: impl Into<String>,
        opts: PlayOptions,
    ) -> VoiceId {
        debug_assert!(
            self.voice_capacity.is_none(),
            "AudioEngine::play is the unbounded path; drive a bounded engine via AudioController"
        );
        let id = self.next_id;
        // Unbounded, so this always admits (never rejects or steals).
        let _ = self.play_with_id(id, clip, label, opts);
        id
    }

    /// Start playing `clip` under a caller-chosen `id` — the entry point for a
    /// command queue that minted the id on the control thread. The engine's
    /// own counter is advanced past `id` so a later [`AudioEngine::play`]
    /// cannot collide with it.
    ///
    /// Returns an [`Admission`] describing what the caller must dispose off the
    /// audio thread. At the bounded voice-pool capacity
    /// ([`AudioEngine::set_voice_capacity`]) the pool is full and growing it
    /// would reallocate on the audio thread, so per the [`VoicePolicy`] either
    /// the newcomer is refused (`Rejected`) or the oldest live voice is evicted
    /// to admit it (`Stole`); the handed-back voice must be disposed off-thread.
    /// An admitted play (and every play on an unbounded engine) returns
    /// `Admitted`.
    pub fn play_with_id(
        &mut self,
        id: VoiceId,
        clip: Arc<AudioClip>,
        label: impl Into<String>,
        opts: PlayOptions,
    ) -> Admission {
        let mut voice = Voice::new(clip, label, opts.gain, opts.pan, opts.looping);
        if let Some(pos) = opts.position {
            voice = voice.with_position(pos);
        }
        if let Some(cap) = self.voice_capacity {
            if self.voices.len() >= cap {
                // At the pre-allocated bound: grow would reallocate on the audio
                // thread, so apply the full-pool policy instead.
                match self.voice_policy {
                    VoicePolicy::RejectNewest => return Admission::Rejected { id, voice },
                    VoicePolicy::StealOldest => {
                        // Evict the oldest live voice (lowest id — ids are minted
                        // monotonically) to free a slot for the newcomer.
                        // `remove` is O(pool) and keeps the reserved capacity, so
                        // it neither reallocates nor frees the buffer (still
                        // real-time-safe); an empty pool (cap 0) has nothing to
                        // steal, so the newcomer is refused.
                        let Some(idx) = self
                            .voices
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, (vid, _))| *vid)
                            .map(|(i, _)| i)
                        else {
                            return Admission::Rejected { id, voice };
                        };
                        let (victim_id, victim) = self.voices.remove(idx);
                        self.next_id = self.next_id.max(id.saturating_add(1));
                        self.voices.push((id, voice));
                        return Admission::Stole { victim_id, victim };
                    }
                }
            }
        }
        self.next_id = self.next_id.max(id.saturating_add(1));
        self.voices.push((id, voice));
        Admission::Admitted
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
        self.render_reaping(out, |_id, voice| drop(voice));
    }

    /// Render one block and hand each finished voice — with its [`VoiceId`] — to
    /// `retire` instead of dropping it here, the real-time seam.
    /// [`AudioEngine::render`] passes a drop; [`crate::AudioRenderer`] passes a
    /// closure that ships each retired `(id, voice)` to the control thread's
    /// resource-return queue, so the audio thread never frees a clip and the
    /// control thread can prune the retired id from its per-voice bookkeeping.
    /// The headless `render` and the real-time path reap through this one
    /// method, so a given engine + command sequence renders identically on both;
    /// the reap is order-preserving so a captured golden buffer stays
    /// byte-reproducible across refactors (float-sum order is stable). Note this
    /// is not a §2 #3 `dry_run` claim about the *device* path — voice
    /// *admission* at the pool bound is timing-dependent (which plays win the
    /// budget depends on how commands batch into callbacks); §2 #3 determinism
    /// is a property of the single-thread engine, which the real-time device
    /// path is not.
    pub fn render_reaping(&mut self, out: &mut [f32], retire: impl FnMut(VoiceId, Voice)) {
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

    /// Remove finished voices, moving each retired `(VoiceId, Voice)` out to
    /// `retire` rather than dropping it here. Order-preserving, O(n),
    /// non-allocating, and safe — `Vec::extract_if` yields each removed element
    /// by value in place, keeping the buffer's reserved capacity, so it neither
    /// reallocates nor frees the buffer. The work is bounded by the live voice
    /// count, so it is real-time-safe (bounded, lock-free, alloc-free). `render`
    /// drops each; the real-time [`crate::AudioRenderer`] passes the
    /// resource-return-queue push (keyed by id), and also calls this right after
    /// a stop so a freed pool slot is reusable by a play later in the same
    /// command batch.
    pub(crate) fn reap_finished(&mut self, mut retire: impl FnMut(VoiceId, Voice)) {
        for (id, voice) in self.voices.extract_if(.., |(_, voice)| voice.is_finished()) {
            retire(id, voice);
        }
    }
}

#[cfg(test)]
mod tests {
    /// R1638 — the vocabulary published to a client is exactly the one
    /// [`VoicePolicy::from_wire`] admits, in BOTH directions.
    ///
    /// One direction alone is not enough and the asymmetry is the reason: a
    /// list that is too SHORT passes a "every published name parses" check
    /// while leaving a real spelling undiscoverable, and a list that is too
    /// LONG passes a "every spelling is published" check while promising a
    /// value the parser refuses. Only the pair pins the set.
    #[test]
    fn r1638_the_policy_vocabulary_is_exactly_what_from_wire_admits() {
        for (i, name) in VoicePolicy::WIRE_NAMES.iter().enumerate() {
            assert_eq!(
                VoicePolicy::from_wire(name),
                Some(VoicePolicy::ALL[i]),
                "{name:?} is published, so it must be accepted",
            );
        }
        for policy in VoicePolicy::ALL {
            assert!(
                VoicePolicy::WIRE_NAMES.contains(&policy.as_wire()),
                "{policy:?} is accepted, so it must be published",
            );
        }
        assert_eq!(VoicePolicy::from_wire("steal_newest"), None);
    }

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
                    .is_admitted(),
                "unbounded engine never rejects"
            );
        }
        assert_eq!(engine.voice_count(), 100);
    }

    #[test]
    fn bounded_pool_rejects_beyond_capacity_and_returns_the_voice() {
        let mut engine = AudioEngine::new(48_000);
        engine.set_voice_capacity(2);
        assert_eq!(engine.voice_capacity(), Some(2));
        assert!(
            engine
                .play_with_id(1, tone(64), "a", PlayOptions::one_shot())
                .is_admitted()
        );
        assert!(
            engine
                .play_with_id(2, tone(64), "b", PlayOptions::one_shot())
                .is_admitted()
        );
        // The pool is full and the default policy is RejectNewest: the third
        // play is refused and its voice handed back.
        match engine.play_with_id(3, tone(64), "c", PlayOptions::one_shot()) {
            Admission::Rejected { id, voice } => {
                assert_eq!(id, 3, "the refused newcomer's own id");
                assert_eq!(voice.label(), "c", "the rejected voice is returned");
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        assert_eq!(engine.voice_count(), 2, "only the pool-sized set is live");
    }

    #[test]
    fn bounded_pool_never_reallocates() {
        let mut engine = AudioEngine::new(48_000);
        engine.set_voice_capacity(4);
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
                        .is_admitted()
                );
            }
            assert!(
                matches!(
                    engine.play_with_id(engine.next_id, tone(1), "over", PlayOptions::one_shot()),
                    Admission::Rejected { .. }
                ),
                "the fifth play is rejected at the bound"
            );
            let mut out = [0.0f32; 8]; // 4 output frames > the 1-frame clips
            engine.render(&mut out);
            assert_eq!(engine.voice_count(), 0, "all one-shots reaped");
            assert_eq!(engine.voices.capacity(), reserved, "no reallocation");
        }
    }

    #[test]
    fn steal_oldest_policy_evicts_the_oldest_and_admits_the_newcomer() {
        let mut engine = AudioEngine::new(48_000);
        engine.set_voice_capacity(2);
        engine.set_voice_policy(VoicePolicy::StealOldest);
        assert!(
            engine
                .play_with_id(1, tone(64), "a", PlayOptions::one_shot())
                .is_admitted()
        );
        assert!(
            engine
                .play_with_id(2, tone(64), "b", PlayOptions::one_shot())
                .is_admitted()
        );
        // Pool full: the newcomer STEALS the oldest (id 1) instead of being
        // rejected, and the evicted voice is handed back keyed by its own id.
        match engine.play_with_id(3, tone(64), "c", PlayOptions::one_shot()) {
            Admission::Stole { victim_id, victim } => {
                assert_eq!(victim_id, 1, "the oldest voice is evicted");
                assert_eq!(victim.label(), "a", "the evicted voice is returned");
            }
            other => panic!("expected Stole, got {other:?}"),
        }
        assert_eq!(engine.voice_count(), 2, "still pool-sized");
        let live: Vec<VoiceId> = engine.voices().map(|(id, _)| id).collect();
        assert!(
            live.contains(&2) && live.contains(&3) && !live.contains(&1),
            "the newcomer replaced the oldest: {live:?}"
        );
        // A further newcomer steals the now-oldest live voice (id 2).
        match engine.play_with_id(4, tone(64), "d", PlayOptions::one_shot()) {
            Admission::Stole { victim_id, .. } => assert_eq!(victim_id, 2, "next oldest"),
            other => panic!("expected Stole, got {other:?}"),
        }
    }
}
