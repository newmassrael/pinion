//! The real-time control model — a lock-free command queue from the control
//! (game / UI) thread to the audio thread, plus a lock-free snapshot back
//! (§5.54, R1274).
//!
//! ## Why this exists
//!
//! The single-thread [`AudioEngine`] + [`crate::AudioEngineExternal`] path
//! shares the engine through an `Rc<RefCell>` — fine for a headless test or a
//! demo pulling the mix on the UI thread, but *not* the model a real device
//! wants. A sound card calls back on its own high-priority audio thread and
//! must never block on a lock the UI thread holds. The standard game-audio
//! answer, and the one this module implements, is:
//!
//! - the **audio thread owns the engine** ([`AudioRenderer`]);
//! - the **control thread** ([`AudioController`]) sends mutations (play / stop
//!   / set-param) over a **lock-free SPSC ring** ([`AudioCommand`]);
//! - the audio thread drains the ring and renders in one alloc-free callback,
//!   publishing a lock-free [`AudioSnapshot`] the control thread can poll.
//!
//! [`AudioRenderer::render`] has the exact shape of a cpal output callback
//! (`&mut [f32]`), and the real device backend that consumes it is
//! `crate::device` (the `cpal-backend` feature) — it moves an
//! [`AudioRenderer`] straight into cpal's callback and hands back the
//! [`AudioController`], negotiating the device's sample format / rate /
//! channels. It is feature-gated (its Linux backend needs `libasound2-dev`),
//! not because it is unverifiable: `cargo run -p pinion-audio --example
//! device_out --features cpal-backend` drives real hardware.
//!
//! ## Real-time safety
//!
//! The callback holds **no lock and no shared mutable engine**, the command
//! ring is pre-allocated, and the render buffer is the caller's — and the two
//! remaining `alloc`/`free` hazards are now closed:
//!
//! - **No reallocation.** The engine is bounded to a `max_voices` **voice
//!   pool** whose backing `Vec` is pre-reserved
//!   ([`AudioEngine::set_voice_capacity`]), so starting a voice never grows it.
//!   At the bound the [`AudioEngine::voice_policy`] decides: the default
//!   `RejectNewest` refuses the newcomer (counted in
//!   [`AudioSnapshot::rejected`]), and `StealOldest` evicts the oldest live
//!   voice to admit it (game-audio **voice stealing**, counted in
//!   [`AudioSnapshot::stolen`]) — either way the pool never grows. Age (lowest
//!   [`VoiceId`]) is the steal key; priority-weighted stealing (keep the
//!   loudest / most important) is a documented refinement that would add
//!   per-voice priority metadata `Voice` does not yet carry.
//! - **No free on the audio thread.** A finished voice — or a play refused at
//!   the bound — is not dropped on the callback; it is shipped back over a
//!   second lock-free ring, the **resource-return queue**, and the control
//!   thread frees it in [`AudioController::reclaim`] (called on every command
//!   send and available for idle frames). Only if that ring is full does a
//!   retired voice fall back to a drop on the audio thread; it is sized to
//!   absorb a full render's retirements so a drained control thread never
//!   forces that.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::clip::AudioClip;
use crate::engine::{Admission, AudioEngine, PlayOptions, VoiceId, VoicePolicy};
use crate::mixer::Voice;
use crate::spatial::{Attenuation, Listener, Vec3};

/// A mutation of the engine, sent from the control thread to the audio thread.
/// Every variant maps to an existing [`AudioEngine`] method — the queue is a
/// wire form of the engine's API, not a second source of behaviour.
#[derive(Debug)]
pub enum AudioCommand {
    /// Start a clip under a control-thread-minted `id`.
    Play {
        /// The id the controller already returned to its caller.
        id: VoiceId,
        /// The decoded clip (shared; the `Arc` travels to the audio thread).
        clip: Arc<AudioClip>,
        /// The voice's introspection label.
        label: String,
        /// Gain / pan / loop / position.
        opts: PlayOptions,
    },
    /// Stop one voice by id.
    Stop(VoiceId),
    /// Stop every voice.
    StopAll,
    /// Set the master bus gain.
    SetMasterGain(f32),
    /// Set one voice's gain.
    SetVoiceGain {
        /// The target voice.
        id: VoiceId,
        /// The new linear gain.
        gain: f32,
    },
    /// Set one voice's authored stereo pan.
    SetVoicePan {
        /// The target voice.
        id: VoiceId,
        /// The new pan, `-1.0` left … `1.0` right.
        pan: f32,
    },
    /// Move (or, with `None`, un-spatialise) one voice — the RT peer of moving a
    /// 3D emitter after it starts (a car, projectile, NPC).
    SetVoicePosition {
        /// The target voice.
        id: VoiceId,
        /// The new world position, or `None` to un-spatialise back to pan.
        position: Option<Vec3>,
    },
    /// Move / re-orient the 3D listener.
    SetListener(Listener),
    /// Change the distance-attenuation model.
    SetAttenuation(Attenuation),
    /// Change the full-pool policy (reject the newcomer / steal the oldest).
    SetVoicePolicy(VoicePolicy),
}

impl AudioEngine {
    /// Apply one queued command — the audio thread's per-command step. Each
    /// arm delegates to the engine's own method so there is no divergent
    /// second implementation.
    ///
    /// Returns an [`Admission`]: a `Play` at the voice-pool bound hands back a
    /// [`Voice`] the caller must dispose **off** the audio thread (a rejected
    /// newcomer, or a stolen victim), whose `Arc`/`String` must not be freed on
    /// the callback. Route it to the resource-return queue — the real-time
    /// [`AudioRenderer::render`] does. `Admitted` for every other command and
    /// for an admitted play.
    pub fn apply(&mut self, command: AudioCommand) -> Admission {
        match command {
            AudioCommand::Play {
                id,
                clip,
                label,
                opts,
            } => return self.play_with_id(id, clip, label, opts),
            AudioCommand::Stop(id) => {
                self.stop(id);
            }
            AudioCommand::StopAll => self.stop_all(),
            AudioCommand::SetMasterGain(gain) => self.set_master_gain(gain),
            AudioCommand::SetVoiceGain { id, gain } => {
                self.set_voice_gain(id, gain);
            }
            AudioCommand::SetVoicePan { id, pan } => {
                self.set_voice_pan(id, pan);
            }
            AudioCommand::SetVoicePosition { id, position } => {
                self.set_voice_position(id, position);
            }
            AudioCommand::SetListener(listener) => self.set_listener(listener),
            AudioCommand::SetAttenuation(attenuation) => self.set_attenuation(attenuation),
            AudioCommand::SetVoicePolicy(policy) => self.set_voice_policy(policy),
        }
        Admission::Admitted
    }
}

/// One voice as read back off the lock-free [`AudioSnapshot`] — the real-time
/// peer of the single-thread `voices` introspection. It carries both the
/// *authored* gain/pan and the *effective* (post-spatialisation) gain/pan the
/// mixer rendered with, matching the single-thread shape. The only field it
/// omits is the `String` label — that genuinely cannot live in an atomic slot,
/// so the [`AudioController`] carries it and [`crate::AudioControllerExternal`]
/// joins it back in by id. `position`/`distance` are present only for a 3D voice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RtVoice {
    /// The control-thread-minted voice id (the join key for its label).
    pub id: VoiceId,
    /// Authored linear gain (what the caller set), before spatialisation.
    pub authored_gain: f32,
    /// Authored stereo pan, before spatialisation.
    pub authored_pan: f32,
    /// Effective linear gain the mixer used this block (post-attenuation).
    pub effective_gain: f32,
    /// Effective stereo pan the mixer used this block.
    pub effective_pan: f32,
    /// Playback position in seconds.
    pub position_secs: f32,
    /// Whether the voice loops at the clip end.
    pub looping: bool,
    /// World position of a 3D voice, or `None` for a flat one.
    pub position: Option<Vec3>,
    /// Distance from the listener, `Some` only for a 3D voice.
    pub distance: Option<f32>,
}

/// One pre-allocated per-voice slot the audio thread publishes into each render.
/// `id == 0` marks an empty slot. The id carries a `Release`/`Acquire` fence:
/// the writer stores the numeric fields (relaxed) and then the id (release), so
/// a reader that `Acquire`-loads a non-zero id sees that render's fields for it.
#[derive(Debug, Default)]
struct VoiceSlot {
    id: AtomicU64,
    /// Authored gain/pan (what the caller set) — the RT peer of the
    /// single-thread `voices` authored fields, so a debugger can tell "quiet
    /// because low gain" from "quiet because far".
    authored_gain: AtomicF32,
    authored_pan: AtomicF32,
    /// Effective (post-spatialisation) gain/pan the mixer rendered with.
    effective_gain: AtomicF32,
    effective_pan: AtomicF32,
    secs: AtomicF32,
    /// bit 0 = looping, bit 1 = has a 3D position.
    flags: AtomicU32,
    pos_x: AtomicF32,
    pos_y: AtomicF32,
    pos_z: AtomicF32,
    dist: AtomicF32,
}

const FLAG_LOOPING: u32 = 0b01;
const FLAG_POSITIONED: u32 = 0b10;

/// A lock-free `f32` cell (bit-packed into an `AtomicU32`, `Relaxed`) — the one
/// idiom the snapshot's meter/per-voice value cells all share, so the
/// `x.to_bits()` / `f32::from_bits(x)` pack is written once. The per-voice `id`
/// keeps its bespoke `Release`/`Acquire` fence (it publishes a whole slot); only
/// the value cells use this.
#[derive(Debug, Default)]
struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn store(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// A lock-free snapshot the audio thread publishes each render and the control
/// thread polls — the "read what is playing without touching the audio
/// thread's engine" seam. It carries the aggregate (counts + peak) **and** a
/// pre-allocated per-voice slot array ([`AudioSnapshot::voices`]) sized to the
/// voice pool, so a full per-voice read needs no audio-thread allocation. The
/// fields are published independently (each its own atomic), so this is a live
/// monitor, not a transactionally-consistent view — a concurrent poll may pair
/// a fresh `voice_count` with a one-render-old `peak`, or read a voice's fields
/// one render newer than the id it was matched on under heavy churn. That is a
/// bounded meter imprecision, never used for correctness.
#[derive(Debug, Default)]
pub struct AudioSnapshot {
    voice_count: AtomicU32,
    /// Last block's peak amplitude.
    peak: AtomicF32,
    /// Total stereo frames rendered — a liveness / progress counter.
    frames_rendered: AtomicU64,
    /// Total plays refused at the voice-pool bound since the channel was
    /// created — the control thread's view of voice-budget starvation (only the
    /// [`VoicePolicy::RejectNewest`] policy refuses).
    rejected: AtomicU64,
    /// Total voices evicted to admit a newcomer at the bound since the channel
    /// was created — nonzero only under [`VoicePolicy::StealOldest`]. The steal
    /// peer of `rejected`: a full pool that admits by stealing counts here, not
    /// there.
    stolen: AtomicU64,
    /// Pre-allocated per-voice slots, one per pool voice. Allocated once at
    /// channel creation (control thread); the audio thread only writes into
    /// them, never grows them.
    voices: Box<[VoiceSlot]>,
}

impl AudioSnapshot {
    /// A snapshot with `max_voices` pre-allocated per-voice slots — the audio
    /// thread never allocates, it only writes into these.
    fn with_capacity(max_voices: usize) -> Self {
        let voices = (0..max_voices).map(|_| VoiceSlot::default()).collect();
        Self {
            voices,
            ..Self::default()
        }
    }

    fn publish(&self, voice_count: u32, peak: f32, frames: u64, rejected: u64, stolen: u64) {
        self.voice_count.store(voice_count, Ordering::Relaxed);
        self.peak.store(peak);
        self.frames_rendered.fetch_add(frames, Ordering::Relaxed);
        if rejected > 0 {
            self.rejected.fetch_add(rejected, Ordering::Relaxed);
        }
        if stolen > 0 {
            self.stolen.fetch_add(stolen, Ordering::Relaxed);
        }
    }

    /// Publish the live voices into the per-voice slots — the audio thread's
    /// per-render per-voice write. Bounded by the pool size, allocation-free,
    /// lock-free: it re-resolves each live voice's effective gain/pan/distance
    /// against the current listener (the same values the mix used) and stores
    /// them, then clears any slot the shrinking voice set left behind. The
    /// numeric fields are stored before the id (with a `Release` on the id) so a
    /// reader sees a consistent slot for any non-zero id it observes.
    fn publish_voices(&self, engine: &AudioEngine) {
        let mut next = 0;
        for (id, voice) in engine.voices() {
            let Some(slot) = self.voices.get(next) else {
                // The live set never exceeds the pool the slots were sized to,
                // but guard rather than panic on the audio thread.
                break;
            };
            let resolved = engine.resolve_voice(voice);
            slot.authored_gain.store(voice.gain());
            slot.authored_pan.store(voice.pan());
            slot.effective_gain.store(resolved.gain);
            slot.effective_pan.store(resolved.pan);
            slot.secs.store(voice.position_secs());
            let mut flags = 0;
            if voice.looping() {
                flags |= FLAG_LOOPING;
            }
            if let Some(pos) = voice.position() {
                flags |= FLAG_POSITIONED;
                slot.pos_x.store(pos[0]);
                slot.pos_y.store(pos[1]);
                slot.pos_z.store(pos[2]);
                slot.dist.store(resolved.distance.unwrap_or(0.0));
            }
            slot.flags.store(flags, Ordering::Relaxed);
            // Release-publish the id last: any reader that Acquire-sees this id
            // sees the field stores above.
            slot.id.store(id, Ordering::Release);
            next += 1;
        }
        // Clear the slots the (possibly shrunk) live set no longer fills.
        for slot in &self.voices[next..] {
            slot.id.store(0, Ordering::Release);
        }
    }

    /// Voices active as of the last published render.
    #[must_use]
    pub fn voice_count(&self) -> u32 {
        self.voice_count.load(Ordering::Relaxed)
    }

    /// Peak amplitude of the last rendered block.
    #[must_use]
    pub fn peak(&self) -> f32 {
        self.peak.load()
    }

    /// Total stereo frames rendered since the channel was created.
    #[must_use]
    pub fn frames_rendered(&self) -> u64 {
        self.frames_rendered.load(Ordering::Relaxed)
    }

    /// Total plays refused at the voice-pool bound — nonzero means the game is
    /// asking for more simultaneous voices than the pool allows.
    #[must_use]
    pub fn rejected(&self) -> u64 {
        self.rejected.load(Ordering::Relaxed)
    }

    /// Total voices evicted to admit a newcomer at the bound — the
    /// [`VoicePolicy::StealOldest`] peer of [`AudioSnapshot::rejected`]. Nonzero
    /// means the pool is saturated but the game chose to steal rather than drop.
    #[must_use]
    pub fn stolen(&self) -> u64 {
        self.stolen.load(Ordering::Relaxed)
    }

    /// The live voices as of the last published render, read lock-free off the
    /// per-voice slots. Each slot's numeric fields are matched to the id via an
    /// `Acquire` load (see [`AudioSnapshot`] for the bounded live-monitor
    /// semantics). The `String` label is not here — it is control-thread state
    /// [`crate::AudioControllerExternal`] joins back in by id.
    #[must_use]
    pub fn voices(&self) -> Vec<RtVoice> {
        let mut out = Vec::new();
        for slot in &self.voices {
            let id = slot.id.load(Ordering::Acquire);
            if id == 0 {
                continue;
            }
            let flags = slot.flags.load(Ordering::Relaxed);
            let positioned = flags & FLAG_POSITIONED != 0;
            let position =
                positioned.then(|| [slot.pos_x.load(), slot.pos_y.load(), slot.pos_z.load()]);
            let distance = positioned.then(|| slot.dist.load());
            out.push(RtVoice {
                id,
                authored_gain: slot.authored_gain.load(),
                authored_pan: slot.authored_pan.load(),
                effective_gain: slot.effective_gain.load(),
                effective_pan: slot.effective_pan.load(),
                position_secs: slot.secs.load(),
                looping: flags & FLAG_LOOPING != 0,
                position,
                distance,
            });
        }
        out
    }
}

/// The control-thread handle: sends commands, polls the snapshot, and reclaims
/// retired resources. It never touches the engine directly, so it cannot block
/// the audio thread.
#[derive(Debug)]
pub struct AudioController {
    producer: Producer<AudioCommand>,
    /// Retired voices — each with its id — the audio thread shipped back for
    /// disposal here, the receiving end of the resource-return queue. Draining
    /// it is where a finished (or pool-refused) voice's clip is actually freed,
    /// on the control thread; the id lets [`AudioController::reclaim`] prune the
    /// voice's label at the same time.
    retired: Consumer<(VoiceId, Voice)>,
    next_id: u64,
    snapshot: Arc<AudioSnapshot>,
    /// The label each live voice was played under, keyed by id. Inserted when a
    /// play is queued and removed when the voice is reclaimed, so it mirrors the
    /// set of ids the audio thread can still be rendering — the control-thread
    /// half of the per-voice read (the lock-free [`AudioSnapshot`] carries the
    /// numbers, this carries the `String` an atomic slot cannot).
    labels: BTreeMap<VoiceId, String>,
    /// The control thread's mirror of the 3D listener / attenuation it last
    /// successfully queued. The audio thread owns the engine and cannot be read
    /// lock-free, but the control thread is the *sole writer* of these two, so
    /// the last value it queued is the authoritative current value — the
    /// read-back the snapshot does not carry, and the base a partial update
    /// patches against. Advanced only when the command is actually queued (see
    /// [`AudioController::set_listener`]), so it never claims a value the audio
    /// thread will not apply.
    listener: Listener,
    attenuation: Attenuation,
    /// The control thread's mirror of the full-pool [`VoicePolicy`], for the
    /// same sole-writer reason as `listener` — the read-back the snapshot does
    /// not carry, so `voice_policy` is introspectable over RPC.
    voice_policy: VoicePolicy,
}

impl AudioController {
    /// Play `clip` (tagged `label`) with `opts`, returning the voice id, or
    /// `None` if the command ring is full (the caller can retry or drop).
    pub fn play(
        &mut self,
        clip: Arc<AudioClip>,
        label: impl Into<String>,
        opts: PlayOptions,
    ) -> Option<VoiceId> {
        let id = self.next_id;
        let label = label.into();
        self.send(AudioCommand::Play {
            id,
            clip,
            label: label.clone(),
            opts,
        })?;
        // Only advance / record once the command is safely queued, so a full
        // ring neither burns an id nor leaks a never-played label.
        self.labels.insert(id, label);
        self.next_id += 1;
        Some(id)
    }

    /// The label the live voice with `id` was played under, or `None` if there
    /// is no such live voice — the control-thread half of the per-voice read.
    #[must_use]
    pub fn label_of(&self, id: VoiceId) -> Option<&str> {
        self.labels.get(&id).map(String::as_str)
    }

    /// Stop one voice. Returns whether the command was queued.
    pub fn stop(&mut self, id: VoiceId) -> bool {
        self.send(AudioCommand::Stop(id)).is_some()
    }

    /// Stop every voice. Returns whether the command was queued.
    pub fn stop_all(&mut self) -> bool {
        self.send(AudioCommand::StopAll).is_some()
    }

    /// Set the master bus gain. Returns whether the command was queued.
    pub fn set_master_gain(&mut self, gain: f32) -> bool {
        self.send(AudioCommand::SetMasterGain(gain)).is_some()
    }

    /// Set one voice's gain. Returns whether the command was queued.
    pub fn set_voice_gain(&mut self, id: VoiceId, gain: f32) -> bool {
        self.send(AudioCommand::SetVoiceGain { id, gain }).is_some()
    }

    /// Set one voice's authored pan. Returns whether the command was queued.
    pub fn set_voice_pan(&mut self, id: VoiceId, pan: f32) -> bool {
        self.send(AudioCommand::SetVoicePan { id, pan }).is_some()
    }

    /// Move (or, with `None`, un-spatialise) one 3D voice — the RT way to move
    /// an emitter after it starts. Returns whether the command was queued.
    pub fn set_voice_position(&mut self, id: VoiceId, position: Option<Vec3>) -> bool {
        self.send(AudioCommand::SetVoicePosition { id, position })
            .is_some()
    }

    /// Move / re-orient the listener, mirroring it on the control thread.
    /// Returns whether the command was queued; the mirror
    /// ([`AudioController::listener`]) advances only then, so a full ring leaves
    /// both the audio thread and the mirror on the previous listener rather than
    /// letting the read-back drift ahead of what will actually render.
    pub fn set_listener(&mut self, listener: Listener) -> bool {
        if self.send(AudioCommand::SetListener(listener)).is_some() {
            self.listener = listener;
            true
        } else {
            false
        }
    }

    /// Change the distance-attenuation model, mirroring it on the control
    /// thread. Returns whether it was queued; the mirror
    /// ([`AudioController::attenuation`]) advances only on success, for the same
    /// no-drift reason as [`AudioController::set_listener`].
    pub fn set_attenuation(&mut self, attenuation: Attenuation) -> bool {
        if self
            .send(AudioCommand::SetAttenuation(attenuation))
            .is_some()
        {
            self.attenuation = attenuation;
            true
        } else {
            false
        }
    }

    /// The listener the control thread last successfully queued — its
    /// authoritative mirror (the control thread is the listener's sole writer,
    /// so this *is* the current value even though the audio thread owns the
    /// engine). The base a partial listener update patches against, and the RT
    /// `listener` introspection read.
    #[must_use]
    pub fn listener(&self) -> Listener {
        self.listener
    }

    /// The distance-attenuation model the control thread last successfully
    /// queued — the control-thread mirror, for the same reason as
    /// [`AudioController::listener`].
    #[must_use]
    pub fn attenuation(&self) -> Attenuation {
        self.attenuation
    }

    /// Change the full-pool [`VoicePolicy`], mirroring it on the control thread.
    /// Returns whether it was queued; the mirror ([`AudioController::voice_policy`])
    /// advances only on success, for the same no-drift reason as
    /// [`AudioController::set_listener`].
    pub fn set_voice_policy(&mut self, policy: VoicePolicy) -> bool {
        if self.send(AudioCommand::SetVoicePolicy(policy)).is_some() {
            self.voice_policy = policy;
            true
        } else {
            false
        }
    }

    /// The full-pool policy the control thread last successfully queued — the
    /// control-thread mirror, so it is introspectable without touching the
    /// audio thread's engine.
    #[must_use]
    pub fn voice_policy(&self) -> VoicePolicy {
        self.voice_policy
    }

    /// The latest snapshot the audio thread published.
    #[must_use]
    pub fn snapshot(&self) -> &AudioSnapshot {
        &self.snapshot
    }

    /// Drop every resource the audio thread has returned since the last call —
    /// finished voices and pool-refused plays — freeing each one here on the
    /// control thread, never on the audio thread. Returns how many were
    /// reclaimed. Every command send reclaims opportunistically; call this
    /// directly on an idle frame to keep the return queue drained when no
    /// commands are flowing.
    pub fn reclaim(&mut self) -> usize {
        let mut n = 0;
        while let Ok((id, _voice)) = self.retired.pop() {
            // The popped `Voice` drops here, freeing its `Arc<AudioClip>` /
            // `String` on the control thread — the whole point of the queue —
            // and its id is pruned from the label map, since that voice can no
            // longer be rendering.
            self.labels.remove(&id);
            n += 1;
        }
        n
    }

    fn send(&mut self, command: AudioCommand) -> Option<()> {
        // Free anything the audio thread returned, off the audio thread, before
        // queuing more work — so retired resources never pile up.
        self.reclaim();
        self.producer.push(command).ok()
    }
}

/// The audio-thread handle: owns the engine, drains the command ring, and
/// renders. [`AudioRenderer::render`] is the audio callback body.
#[derive(Debug)]
pub struct AudioRenderer {
    engine: AudioEngine,
    consumer: Consumer<AudioCommand>,
    /// Retired voices — each with its id — shipped back to the control thread
    /// for disposal, the sending end of the resource-return queue, so the audio
    /// thread frees nothing.
    retired: Producer<(VoiceId, Voice)>,
    snapshot: Arc<AudioSnapshot>,
}

impl AudioRenderer {
    /// Drain every queued command, render one interleaved stereo block into
    /// `out`, and publish the snapshot. This is the audio callback, and it is
    /// real-time safe: it takes no lock, touches no shared mutable engine, and
    /// allocates nothing (the render buffer is the caller's, the command ring
    /// is pre-allocated, and the voice pool is pre-reserved so no play grows
    /// it). It also frees nothing on the audio thread **unless the return ring
    /// is full**: a finished voice, a play refused at the pool bound, or a voice
    /// stolen to admit a newcomer is pushed to the resource-return queue for the
    /// control thread to free; only on an overflow (which the ring is sized to
    /// preclude under the reclaim-in-send contract) does a voice fall back to a
    /// drop here.
    pub fn render(&mut self, out: &mut [f32]) {
        // Destructure for disjoint borrows: the reap closure needs `retired`
        // while `render_reaping` holds `engine` mutably.
        let Self {
            engine,
            consumer,
            retired,
            snapshot,
        } = self;

        let mut rejected = 0u64;
        let mut stolen = 0u64;
        while let Ok(command) = consumer.pop() {
            let frees_a_slot = matches!(command, AudioCommand::Stop(_) | AudioCommand::StopAll);
            // The id to pair a refused newcomer with (only a `Play` is refused).
            let play_id = if let AudioCommand::Play { id, .. } = &command {
                Some(*id)
            } else {
                None
            };
            match engine.apply(command) {
                Admission::Admitted => {}
                Admission::Rejected(refused) => {
                    // A play refused at the voice-pool bound: return the newcomer
                    // (with its id) for off-thread disposal, not freed here. Only
                    // a `Play` is ever refused, so `play_id` is always `Some`; the
                    // `if let` keeps the audio callback panic-free (an impossible
                    // `None` drops the voice here — the same audio-thread free as
                    // the ring-full fallback — never a panic).
                    rejected += 1;
                    debug_assert!(play_id.is_some(), "only a Play command is ever refused");
                    if let Some(id) = play_id {
                        return_voice(retired, id, refused);
                    }
                }
                Admission::Stole { victim_id, victim } => {
                    // The newcomer was admitted by evicting `victim`: dispose the
                    // victim off-thread (keyed by its own id, not the newcomer's).
                    // Not a rejection — the play succeeded — so it counts as a
                    // steal, not toward `rejected`.
                    stolen += 1;
                    return_voice(retired, victim_id, victim);
                }
            }
            if frees_a_slot {
                // Reap the just-stopped voice(s) now, so a play later in this
                // same command batch can reuse the freed slot instead of being
                // rejected at the still-full bound.
                engine.reap_finished(|id, voice| return_voice(retired, id, voice));
            }
        }
        // A finished voice: return it (with its id), do not free it here.
        engine.render_reaping(out, |id, voice| return_voice(retired, id, voice));

        // Publish the per-voice slots from the post-reap live set, then the
        // aggregate.
        snapshot.publish_voices(engine);
        let block_peak = crate::backend::peak(out);
        let frames = (out.len() / 2) as u64;
        let voice_count = u32::try_from(engine.voice_count()).unwrap_or(u32::MAX);
        snapshot.publish(voice_count, block_peak, frames, rejected, stolen);
    }

    /// The owned engine — for a headless test or single-thread introspection
    /// after the render loop has stopped.
    #[must_use]
    pub fn engine(&self) -> &AudioEngine {
        &self.engine
    }
}

/// Ship one retired voice (with its id) back to the control thread for
/// disposal. The return ring is sized so this always succeeds under the
/// reclaim-in-send contract; a debug build asserts that, and a release build
/// falls back to dropping the voice here — a free on the audio thread — rather
/// than blocking the callback.
fn return_voice(retired: &mut Producer<(VoiceId, Voice)>, id: VoiceId, voice: Voice) {
    if retired.push((id, voice)).is_err() {
        // Sized (in `realtime_channel`) to hold one render's worth of
        // retirements, so under the reclaim-in-send contract this is
        // unreachable. The failed push already dropped the voice here — a free
        // on the audio thread, the documented fallback — so a release build
        // degrades gracefully; a debug/test build flags the broken invariant.
        #[cfg(debug_assertions)]
        unreachable!("audio resource-return ring overflow — the sizing invariant was violated");
    }
}

/// Split an engine into a control-thread [`AudioController`] and an
/// audio-thread [`AudioRenderer`], joined by two lock-free rings: a
/// `command_capacity`-deep command ring (control → audio) and a
/// resource-return ring (audio → control). The engine is bounded to a
/// `max_voices` pre-reserved voice pool so the audio thread never reallocates.
/// The renderer is `Send`, so it moves onto the audio thread (or into a cpal
/// callback); the controller stays on the UI thread.
#[must_use]
pub fn realtime_channel(
    mut engine: AudioEngine,
    command_capacity: usize,
    max_voices: usize,
) -> (AudioController, AudioRenderer) {
    // Bound + pre-reserve the pool: past this, plays are rejected, not grown.
    engine.set_voice_capacity(max_voices);
    let voice_capacity = engine.voice_capacity().unwrap_or(max_voices);

    let (producer, consumer) = RingBuffer::new(command_capacity.max(1));
    // The return ring must absorb one render's worth of retirements without
    // forcing an on-thread free: up to `voice_capacity` finished voices plus up
    // to `command_capacity` pool-refused plays. Sized to that sum, a control
    // thread that reclaims between renders never overflows it.
    let (retired_tx, retired_rx) = RingBuffer::new(command_capacity.max(1) + voice_capacity.max(1));

    let next_id = engine.next_voice_id();
    // Seed the control-thread mirror from the engine's current values, so a
    // `listener` / `attenuation` read before any drive returns what the audio
    // thread actually starts with (the origin listener + default model, unless
    // the caller pre-set them).
    let listener = engine.listener();
    let attenuation = engine.attenuation();
    let voice_policy = engine.voice_policy();
    // Pre-allocate the per-voice snapshot slots to the pool size, so the audio
    // thread only ever writes into them.
    let snapshot = Arc::new(AudioSnapshot::with_capacity(voice_capacity));
    (
        AudioController {
            producer,
            retired: retired_rx,
            next_id,
            snapshot: Arc::clone(&snapshot),
            listener,
            attenuation,
            voice_policy,
            labels: BTreeMap::new(),
        },
        AudioRenderer {
            engine,
            consumer,
            retired: retired_tx,
            snapshot,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::peak;

    fn tone(frames: usize) -> Arc<AudioClip> {
        AudioClip::new(48_000, 1, vec![1.0; frames]).shared()
    }

    /// Component-wise near-equality for a 3D vector (the mirror stores exact
    /// copies, but `clippy::float_cmp` forbids `==` on float arrays).
    fn approx3(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    #[test]
    fn queued_play_renders_and_stop_silences() {
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 16);
        // A tone longer than the block so it is still playing after one render.
        let id = ctl
            .play(tone(512), "bell", PlayOptions::one_shot())
            .expect("queued");
        assert_eq!(id, 1, "control thread minted the id");

        let mut out = [0.0f32; 256]; // 128 stereo frames
        renderer.render(&mut out);
        assert!(peak(&out) > 0.5, "queued play is audible after render");
        assert_eq!(renderer.engine().voice_count(), 1);

        // Stop it; the next render applies the command and goes silent.
        assert!(ctl.stop(id));
        let mut out2 = [0.0f32; 256];
        renderer.render(&mut out2);
        assert!(peak(&out2) < 1e-6, "stopped voice is silent next block");
        assert_eq!(renderer.engine().voice_count(), 0);
    }

    #[test]
    fn snapshot_publishes_lock_free_state() {
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 16);
        ctl.play(tone(512), "bell", PlayOptions::one_shot())
            .expect("queued");
        assert_eq!(ctl.snapshot().frames_rendered(), 0, "nothing rendered yet");

        let mut out = [0.0f32; 256];
        renderer.render(&mut out);

        // The controller reads the snapshot the renderer just published.
        assert_eq!(ctl.snapshot().voice_count(), 1);
        assert!(ctl.snapshot().peak() > 0.5);
        assert_eq!(ctl.snapshot().frames_rendered(), 128);
    }

    #[test]
    fn set_param_commands_apply_across_renders() {
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 16);
        // A centred, full-scale, long tone → constant-power centre ~0.707/leg.
        ctl.play(tone(4096), "src", PlayOptions::one_shot())
            .expect("queued");
        assert!(ctl.set_master_gain(0.5), "queued");

        let center = std::f32::consts::FRAC_1_SQRT_2;
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        // If the queued SetMasterGain were dropped the peak would be ~0.707,
        // not ~0.354 — so this assertion actually exercises the command path.
        assert!(
            (peak(&out) - center * 0.5).abs() < 1e-3,
            "queued master gain applied: peak {}",
            peak(&out)
        );

        // It is engine state, so it persists into the next render block.
        let mut out2 = [0.0f32; 256];
        renderer.render(&mut out2);
        assert!(
            (peak(&out2) - center * 0.5).abs() < 1e-3,
            "master gain persists across renders: peak {}",
            peak(&out2)
        );
    }

    #[test]
    fn full_ring_rejects_without_panicking_and_burns_no_id() {
        // Command ring capacity 2 (voice pool ample). First play succeeds
        // (id 1); stop_all fills the ring.
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 2, 8);
        assert_eq!(ctl.play(tone(4), "a", PlayOptions::one_shot()), Some(1));
        assert!(ctl.stop_all());
        // Ring full: the next play is refused, no panic.
        assert!(ctl.play(tone(4), "b", PlayOptions::one_shot()).is_none());
        // Drain the ring, then the next play is id 2 — proving the rejected
        // play did NOT advance the id counter (else it would be 3).
        let mut out = [0.0f32; 8];
        renderer.render(&mut out);
        assert_eq!(
            ctl.play(tone(4), "c", PlayOptions::one_shot()),
            Some(2),
            "rejected play burned no id"
        );
    }

    #[test]
    fn apply_is_the_only_behaviour_source() {
        // apply(SetListener) matches calling the engine method directly.
        let mut engine = AudioEngine::new(48_000);
        // Non-Play commands never return a voice to dispose.
        assert!(
            engine
                .apply(AudioCommand::SetListener(Listener::new(
                    [1.0, 2.0, 3.0],
                    [0.0, 0.0, -1.0],
                    [0.0, 1.0, 0.0],
                )))
                .is_admitted()
        );
        let p = engine.listener().position;
        assert!(
            (p[0] - 1.0).abs() < 1e-6 && (p[1] - 2.0).abs() < 1e-6 && (p[2] - 3.0).abs() < 1e-6
        );
        assert!(
            engine
                .apply(AudioCommand::SetMasterGain(0.25))
                .is_admitted()
        );
        assert!((engine.master_gain() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn finished_voice_is_returned_not_freed_on_the_audio_thread() {
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 16);
        // A 2-frame one-shot finishes within one 128-frame render.
        let clip = tone(2);
        ctl.play(Arc::clone(&clip), "blip", PlayOptions::one_shot())
            .expect("queued");
        assert_eq!(
            Arc::strong_count(&clip),
            2,
            "the test holds one ref, the queued command another"
        );

        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        assert_eq!(renderer.engine().voice_count(), 0, "one-shot reaped");
        // The reap did NOT free the clip: the retired voice (still holding the
        // clip) sits in the return queue. If render had dropped it, the count
        // would be 1 here.
        assert_eq!(
            Arc::strong_count(&clip),
            2,
            "clip returned to the queue, not freed on the audio thread"
        );

        // The control thread reclaims: now the returned voice drops and frees.
        assert_eq!(ctl.reclaim(), 1, "one retired voice reclaimed");
        assert_eq!(
            Arc::strong_count(&clip),
            1,
            "reclaim freed the returned voice on the control thread"
        );
    }

    #[test]
    fn pool_refused_play_is_returned_and_counted() {
        // Voice pool of 2; three long tones are queued.
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 2);
        ctl.play(tone(4096), "a", PlayOptions::one_shot())
            .expect("queued");
        ctl.play(tone(4096), "b", PlayOptions::one_shot())
            .expect("queued");
        let refused_clip = tone(4096);
        ctl.play(Arc::clone(&refused_clip), "c", PlayOptions::one_shot())
            .expect("queued");

        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        assert_eq!(renderer.engine().voice_count(), 2, "pool capped at 2");
        assert_eq!(
            ctl.snapshot().rejected(),
            1,
            "one play rejected at the bound"
        );
        // The refused play's clip was returned, not freed on the audio thread.
        assert_eq!(Arc::strong_count(&refused_clip), 2);
        assert_eq!(ctl.reclaim(), 1, "the refused voice is reclaimed");
        assert_eq!(
            Arc::strong_count(&refused_clip),
            1,
            "then freed on control thread"
        );
    }

    #[test]
    fn stop_then_play_in_one_batch_reuses_the_freed_slot() {
        // Pool of 1. Fill it, then in a SINGLE command batch stop the voice and
        // play a new one. The stop must free the slot within the drain so the
        // new play is admitted — not rejected because the reap only runs at the
        // end of the render.
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 1);
        let first = ctl
            .play(tone(4096), "a", PlayOptions::one_shot())
            .expect("queued");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        assert_eq!(renderer.engine().voice_count(), 1);

        // One batch: stop the first, play a second, then one render.
        ctl.stop(first);
        ctl.play(tone(4096), "b", PlayOptions::one_shot())
            .expect("queued");
        renderer.render(&mut out);
        assert_eq!(
            renderer.engine().voice_count(),
            1,
            "the new voice took the slot the stop freed in the same batch"
        );
        assert_eq!(
            ctl.snapshot().rejected(),
            0,
            "no rejection — the slot was reclaimed mid-drain, not at render end"
        );
    }

    #[test]
    fn send_reclaims_opportunistically() {
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 16);
        let clip = tone(2);
        ctl.play(Arc::clone(&clip), "blip", PlayOptions::one_shot())
            .expect("queued");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out); // voice finishes and is returned to the queue

        // A later command send drains the return queue as a side effect, so the
        // returned voice is freed without an explicit reclaim() call.
        assert!(ctl.set_master_gain(0.9));
        assert_eq!(
            Arc::strong_count(&clip),
            1,
            "send() reclaimed the returned voice"
        );
        assert_eq!(ctl.reclaim(), 0, "nothing left to reclaim");
    }

    #[test]
    fn mirror_seeds_from_engine_and_advances_on_successful_set() {
        // The controller mirror starts from the engine's current values, so a
        // read before any drive reflects reality.
        let mut engine = AudioEngine::new(48_000);
        engine.set_listener(Listener::new(
            [1.0, 2.0, 3.0],
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
        ));
        let (mut ctl, _renderer) = realtime_channel(engine, 16, 8);
        assert!(
            approx3(ctl.listener().position, [1.0, 2.0, 3.0]),
            "seeded from engine"
        );
        assert!(
            (ctl.attenuation().reference_distance - 1.0).abs() < 1e-6,
            "engine default"
        );

        // A successful send advances the mirror.
        assert!(ctl.set_listener(Listener::new(
            [5.0, 0.0, 0.0],
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
        )));
        assert!(approx3(ctl.listener().position, [5.0, 0.0, 0.0]));
        assert!(ctl.set_attenuation(Attenuation {
            reference_distance: 2.0,
            max_distance: 100.0,
            rolloff: 0.5,
        }));
        assert!((ctl.attenuation().reference_distance - 2.0).abs() < 1e-6);
        assert!((ctl.attenuation().rolloff - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mirror_does_not_drift_when_the_command_ring_is_full() {
        // A full ring means the command never reached the audio thread, so the
        // mirror must stay put — it must never report a value the audio thread
        // will not apply.
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 1, 8);
        assert!(ctl.stop_all(), "fills the 1-deep ring");
        let before = ctl.listener().position;
        assert!(
            !ctl.set_listener(Listener::new(
                [9.0, 9.0, 9.0],
                [0.0, 0.0, -1.0],
                [0.0, 1.0, 0.0],
            )),
            "refused at a full ring"
        );
        assert!(
            approx3(ctl.listener().position, before),
            "mirror did not drift"
        );

        // Drain the ring; the same drive now takes and the mirror advances.
        let mut out = [0.0f32; 8];
        renderer.render(&mut out);
        assert!(ctl.set_listener(Listener::new(
            [9.0, 9.0, 9.0],
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
        )));
        assert!(approx3(ctl.listener().position, [9.0, 9.0, 9.0]));
    }

    #[test]
    fn per_voice_snapshot_publishes_effective_state() {
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 8);
        let flat = ctl
            .play(tone(4096), "flat", PlayOptions::one_shot())
            .expect("queued");
        // A voice 4 units to the world-right: distance 4 → effective gain 0.25
        // (inverse-distance), hard-right pan.
        let spatial = ctl
            .play(
                tone(4096),
                "spatial",
                PlayOptions {
                    position: Some([4.0, 0.0, 0.0]),
                    ..PlayOptions::one_shot()
                },
            )
            .expect("queued");

        let mut out = [0.0f32; 256];
        renderer.render(&mut out);

        let voices = ctl.snapshot().voices();
        assert_eq!(voices.len(), 2, "both voices published to the slots");
        let f = voices.iter().find(|v| v.id == flat).expect("flat present");
        assert!(
            (f.authored_gain - 1.0).abs() < 1e-4,
            "flat authored gain 1.0"
        );
        assert!(
            (f.effective_gain - 1.0).abs() < 1e-4,
            "flat effective gain 1.0"
        );
        assert!(
            f.position.is_none() && f.distance.is_none(),
            "flat is not 3D"
        );
        let s = voices
            .iter()
            .find(|v| v.id == spatial)
            .expect("spatial present");
        assert!(approx3(
            s.position.expect("spatial carries position"),
            [4.0, 0.0, 0.0]
        ));
        assert!((s.distance.expect("3D distance") - 4.0).abs() < 1e-3);
        // Authored gain stays 1.0; effective is halved to 0.25 at distance 4.
        assert!(
            (s.authored_gain - 1.0).abs() < 1e-3,
            "authored gain unchanged"
        );
        assert!(
            (s.effective_gain - 0.25).abs() < 1e-3,
            "effective gain at distance 4"
        );
        assert!((s.effective_pan - 1.0).abs() < 1e-3, "hard right");
    }

    #[test]
    fn per_voice_slots_clear_when_the_voice_set_shrinks() {
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 8);
        let a = ctl
            .play(tone(4096), "a", PlayOptions::one_shot())
            .expect("queued");
        ctl.play(tone(4096), "b", PlayOptions::one_shot())
            .expect("queued");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        assert_eq!(ctl.snapshot().voices().len(), 2);

        // Stop one; the next render leaves a single live voice and the freed
        // slot reads empty (not a stale ghost of the stopped voice).
        ctl.stop(a);
        renderer.render(&mut out);
        let voices = ctl.snapshot().voices();
        assert_eq!(voices.len(), 1, "stopped voice cleared from the slots");
        assert!(
            voices.iter().all(|v| v.id != a),
            "no ghost of the stopped voice"
        );
    }

    #[test]
    fn controller_labels_track_live_voices_and_prune_on_reclaim() {
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 8);
        // A 2-frame one-shot finishes within one render.
        let id = ctl
            .play(tone(2), "blip", PlayOptions::one_shot())
            .expect("queued");
        assert_eq!(ctl.label_of(id), Some("blip"), "label recorded on play");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out); // one-shot finishes, returned over the queue
        // The label survives until the voice is reclaimed.
        assert_eq!(ctl.label_of(id), Some("blip"), "label lives until reclaim");
        assert_eq!(ctl.reclaim(), 1);
        assert_eq!(ctl.label_of(id), None, "label pruned on reclaim");
    }

    #[test]
    fn full_ring_play_records_no_label() {
        // Command ring capacity 1: fill it, then a refused play must not leak a
        // never-played label (it burned no id either).
        let (mut ctl, _renderer) = realtime_channel(AudioEngine::new(48_000), 1, 8);
        assert!(ctl.stop_all(), "fills the 1-deep ring");
        assert!(
            ctl.play(tone(4), "ghost", PlayOptions::one_shot())
                .is_none()
        );
        // next_id was never advanced, so the id a later play would get has no
        // stale label.
        assert_eq!(ctl.label_of(1), None, "refused play recorded no label");
    }

    #[test]
    fn rt_steal_oldest_admits_newcomer_prunes_victim_and_counts() {
        use crate::engine::VoicePolicy;
        let mut engine = AudioEngine::new(48_000);
        engine.set_voice_policy(VoicePolicy::StealOldest);
        let (mut ctl, mut renderer) = realtime_channel(engine, 16, 2);
        let a = ctl
            .play(tone(4096), "a", PlayOptions::one_shot())
            .expect("queued");
        let b = ctl
            .play(tone(4096), "b", PlayOptions::one_shot())
            .expect("queued");
        let c = ctl
            .play(tone(4096), "c", PlayOptions::one_shot())
            .expect("queued");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);

        // The third play was admitted by stealing the oldest — not rejected.
        assert_eq!(ctl.snapshot().voice_count(), 2, "pool still full");
        assert_eq!(ctl.snapshot().stolen(), 1, "one steal");
        assert_eq!(
            ctl.snapshot().rejected(),
            0,
            "no rejection under StealOldest"
        );
        let ids: Vec<VoiceId> = ctl.snapshot().voices().iter().map(|v| v.id).collect();
        assert!(
            ids.contains(&b) && ids.contains(&c) && !ids.contains(&a),
            "newcomer c replaced oldest a: {ids:?}"
        );
        // The evicted voice's label is pruned on reclaim (its id crossed the
        // return queue), while the newcomer's label is kept.
        assert_eq!(
            ctl.label_of(a),
            Some("a"),
            "victim label lives until reclaim"
        );
        assert!(ctl.reclaim() >= 1, "the stolen victim is reclaimed");
        assert_eq!(ctl.label_of(a), None, "victim label pruned");
        assert_eq!(ctl.label_of(c), Some("c"), "newcomer label kept");
    }
}
