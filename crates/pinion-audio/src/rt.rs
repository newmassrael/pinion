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
//! ## Real-time hardening still owed (honest scope)
//!
//! The primary real-time win — **no lock and no shared mutable engine on the
//! callback** — is in place, as is the pre-allocated command ring and the
//! alloc-free render buffer. Two known refinements are *not* done and are
//! documented rather than hidden: (1) a finished voice's `Arc<AudioClip>` /
//! `String` is dropped on the audio thread (a `free`), and (2) the engine's
//! voice `Vec` may reallocate when many voices start. The textbook fix for
//! both is a pre-allocated voice pool plus a return channel that ships retired
//! resources back to the control thread for disposal; that is the next
//! increment, forced by a real device consumer, not guessed at here.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::clip::AudioClip;
use crate::engine::{AudioEngine, PlayOptions, VoiceId};
use crate::spatial::{Attenuation, Listener};

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
    /// Move / re-orient the 3D listener.
    SetListener(Listener),
    /// Change the distance-attenuation model.
    SetAttenuation(Attenuation),
}

impl AudioEngine {
    /// Apply one queued command — the audio thread's per-command step. Each
    /// arm delegates to the engine's own method so there is no divergent
    /// second implementation.
    pub fn apply(&mut self, command: AudioCommand) {
        match command {
            AudioCommand::Play {
                id,
                clip,
                label,
                opts,
            } => self.play_with_id(id, clip, label, opts),
            AudioCommand::Stop(id) => {
                self.stop(id);
            }
            AudioCommand::StopAll => self.stop_all(),
            AudioCommand::SetMasterGain(gain) => self.set_master_gain(gain),
            AudioCommand::SetVoiceGain { id, gain } => {
                self.set_voice_gain(id, gain);
            }
            AudioCommand::SetListener(listener) => self.set_listener(listener),
            AudioCommand::SetAttenuation(attenuation) => self.set_attenuation(attenuation),
        }
    }
}

/// A lock-free snapshot the audio thread publishes each render and the control
/// thread polls — the "read what is playing without touching the audio
/// thread's engine" seam. Lightweight (counts + peak) by design: the rich
/// per-voice introspection stays on the single-thread [`crate::AudioEngineExternal`]
/// path; a full lock-free voice list is a later refinement. The fields are
/// published independently (each its own relaxed atomic), so this is a live
/// monitor, not a transactionally-consistent view — a concurrent poll may pair
/// a fresh `voice_count` with a one-render-old `peak`, which is fine for a
/// meter and never used for correctness.
#[derive(Debug, Default)]
pub struct AudioSnapshot {
    voice_count: AtomicU32,
    /// Last block's peak amplitude, stored as `f32::to_bits` for a lock-free
    /// exact publish.
    peak_bits: AtomicU32,
    /// Total stereo frames rendered — a liveness / progress counter.
    frames_rendered: AtomicU64,
}

impl AudioSnapshot {
    fn publish(&self, voice_count: u32, peak: f32, frames: u64) {
        self.voice_count.store(voice_count, Ordering::Relaxed);
        self.peak_bits.store(peak.to_bits(), Ordering::Relaxed);
        self.frames_rendered.fetch_add(frames, Ordering::Relaxed);
    }

    /// Voices active as of the last published render.
    #[must_use]
    pub fn voice_count(&self) -> u32 {
        self.voice_count.load(Ordering::Relaxed)
    }

    /// Peak amplitude of the last rendered block.
    #[must_use]
    pub fn peak(&self) -> f32 {
        f32::from_bits(self.peak_bits.load(Ordering::Relaxed))
    }

    /// Total stereo frames rendered since the channel was created.
    #[must_use]
    pub fn frames_rendered(&self) -> u64 {
        self.frames_rendered.load(Ordering::Relaxed)
    }
}

/// The control-thread handle: sends commands and polls the snapshot. It never
/// touches the engine directly, so it cannot block the audio thread.
#[derive(Debug)]
pub struct AudioController {
    producer: Producer<AudioCommand>,
    next_id: u64,
    snapshot: Arc<AudioSnapshot>,
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
        self.send(AudioCommand::Play {
            id,
            clip,
            label: label.into(),
            opts,
        })?;
        // Only advance once the command is safely queued, so a full ring does
        // not burn ids.
        self.next_id += 1;
        Some(id)
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

    /// Move / re-orient the listener. Returns whether the command was queued.
    pub fn set_listener(&mut self, listener: Listener) -> bool {
        self.send(AudioCommand::SetListener(listener)).is_some()
    }

    /// Change the distance-attenuation model. Returns whether it was queued.
    pub fn set_attenuation(&mut self, attenuation: Attenuation) -> bool {
        self.send(AudioCommand::SetAttenuation(attenuation))
            .is_some()
    }

    /// The latest snapshot the audio thread published.
    #[must_use]
    pub fn snapshot(&self) -> &AudioSnapshot {
        &self.snapshot
    }

    fn send(&mut self, command: AudioCommand) -> Option<()> {
        self.producer.push(command).ok()
    }
}

/// The audio-thread handle: owns the engine, drains the command ring, and
/// renders. [`AudioRenderer::render`] is the audio callback body.
#[derive(Debug)]
pub struct AudioRenderer {
    engine: AudioEngine,
    consumer: Consumer<AudioCommand>,
    snapshot: Arc<AudioSnapshot>,
}

impl AudioRenderer {
    /// Drain every queued command, render one interleaved stereo block into
    /// `out`, and publish the snapshot. This is the audio callback: it takes a
    /// caller-owned buffer and allocates nothing in the steady state — the
    /// render buffer is the caller's and the command ring is pre-allocated.
    /// (The known exceptions — a `Play` may grow the voice `Vec`, and a
    /// retired voice frees its `Arc`/`String` — are the module note's owed
    /// refinements, not steady-state allocation.)
    pub fn render(&mut self, out: &mut [f32]) {
        while let Ok(command) = self.consumer.pop() {
            self.engine.apply(command);
        }
        self.engine.render(out);
        let block_peak = crate::backend::peak(out);
        let frames = (out.len() / 2) as u64;
        let voice_count = u32::try_from(self.engine.voice_count()).unwrap_or(u32::MAX);
        self.snapshot.publish(voice_count, block_peak, frames);
    }

    /// The owned engine — for a headless test or single-thread introspection
    /// after the render loop has stopped.
    #[must_use]
    pub fn engine(&self) -> &AudioEngine {
        &self.engine
    }
}

/// Split an engine into a control-thread [`AudioController`] and an
/// audio-thread [`AudioRenderer`] joined by a lock-free command ring of
/// `capacity` commands. The renderer is `Send`, so it moves onto the audio
/// thread (or into a cpal callback); the controller stays on the UI thread.
#[must_use]
pub fn realtime_channel(engine: AudioEngine, capacity: usize) -> (AudioController, AudioRenderer) {
    let (producer, consumer) = RingBuffer::new(capacity.max(1));
    let next_id = engine.next_voice_id();
    let snapshot = Arc::new(AudioSnapshot::default());
    (
        AudioController {
            producer,
            next_id,
            snapshot: Arc::clone(&snapshot),
        },
        AudioRenderer {
            engine,
            consumer,
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

    #[test]
    fn queued_play_renders_and_stop_silences() {
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16);
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
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16);
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
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16);
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
        // Capacity 2. First play succeeds (id 1); stop_all fills the ring.
        let (mut ctl, mut renderer) = realtime_channel(AudioEngine::new(48_000), 2);
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
        engine.apply(AudioCommand::SetListener(Listener::new(
            [1.0, 2.0, 3.0],
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
        )));
        let p = engine.listener().position;
        assert!(
            (p[0] - 1.0).abs() < 1e-6 && (p[1] - 2.0).abs() < 1e-6 && (p[2] - 3.0).abs() < 1e-6
        );
        engine.apply(AudioCommand::SetMasterGain(0.25));
        assert!((engine.master_gain() - 0.25).abs() < 1e-6);
    }
}
