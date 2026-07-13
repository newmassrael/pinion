//! The control-side audio **world**, and the per-frame clock that carries it to
//! the audio thread (§5.54).
//!
//! # Why this is not "Phase C"
//!
//! The mixer is clocked by the **device** — the card pulls when it pulls, and no
//! engine frame-clocks a mixer. But a game does control-side audio work every
//! *frame*: the listener follows the camera, and emitters follow entities. That
//! needs no game loop and no new machinery. It is a retained
//! [`Tickable`], registered with the owner's
//! animation driver and ticked once per paint by the shell — the same substrate
//! `CaretBlink` and `pinion_narrative`'s `VnClock` ride.
//!
//! It lives in this crate, not in a binding, for the reason `VnClock` lives in
//! `pinion-narrative`: the subtle part is not the `Tickable` impl, it is the
//! protocol around it (below), and every consumer that re-rolls that protocol will
//! get it wrong in the same three ways.
//!
//! # A propagator, not an integrator
//!
//! [`AudioWorldClock`] ignores `dt`. It does not advance anything by time; it
//! pushes the *current* world across the lock-free ring. That makes it idempotent
//! and frame-rate-independent — and, unlike a wall-clock integrator, it cannot
//! drift a headless test, so it is safe to leave registered in an RPC harness.
//!
//! # The three things that are easy to get wrong
//!
//! 1. **A world write must SCHEDULE the frame that carries it.** The retained
//!    shell's frame loop is idle-driven: registering a `Tickable` is necessary but
//!    *not sufficient*: nothing paints, so nothing ticks. The world's state is
//!    therefore a [`Signal`] — a view that reads it subscribes, and writing it
//!    marks the owner dirty, which arms the repaint that runs the tick.
//! 2. **[`Signal::set`] equality-skips.** Re-asserting the pose the world already
//!    holds — a save/load restore, a respawn in place — would notify nobody, arm
//!    no frame, and strand the pending push forever. Every write here is therefore
//!    stamped with a monotonic sequence, so a write is never equal to its
//!    predecessor and can never be swallowed.
//! 3. **The command ring can be full.** A push that did not queue never reached
//!    the audio thread, so it stays pending and is retried on the next frame
//!    rather than being silently dropped.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use pinion_core::animation::Tickable;
use pinion_core::reactive::{Owner, Signal};

use crate::engine::VoiceId;
use crate::external::SharedController;
use crate::spatial::{Listener, Vec3};

/// The control-side audio world: where the ears are, and where the sounds are.
///
/// A game writes this (from its camera and its entities); [`AudioWorldClock`]
/// carries it to the audio thread once a frame. Nothing here touches the engine —
/// that separation is the point, and it is what makes both halves testable with no
/// sound card.
#[derive(Debug)]
pub struct AudioWorld {
    /// `(write sequence, listener)`. The sequence is load-bearing — see the module
    /// docs, point 2.
    listener: Signal<(u64, Listener)>,
    /// Emitter poses not yet pushed: voice id → position (`None` un-spatialises
    /// the voice back to its authored pan). A map, so moving the same emitter
    /// twice before a frame runs coalesces to one push instead of two.
    pending_emitters: RefCell<BTreeMap<VoiceId, Option<Vec3>>>,
    /// The listener has moved and the audio thread has not been told yet.
    listener_pending: Cell<bool>,
    /// How many times the clock has run — "is my audio sync running?".
    ticks: Cell<u64>,
}

impl Default for AudioWorld {
    fn default() -> Self {
        Self {
            listener: Signal::new((0, Listener::default())),
            pending_emitters: RefCell::new(BTreeMap::new()),
            listener_pending: Cell::new(false),
            ticks: Cell::new(0),
        }
    }
}

impl AudioWorld {
    /// A world with the default listener at the origin.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the ears are, as the game last set them.
    #[must_use]
    pub fn listener(&self) -> Listener {
        self.listener.get().1
    }

    /// Place the listener (position **and** orientation) — the camera, each frame.
    pub fn set_listener(&self, listener: Listener) {
        let seq = self.listener.get().0;
        // The sequence bump is what stops `Signal`'s equality-skip from swallowing
        // a re-assert of the pose the world already holds (module docs, point 2).
        self.listener.set((seq.wrapping_add(1), listener));
        self.listener_pending.set(true);
    }

    /// Move the listener, keeping its current facing.
    pub fn move_listener(&self, position: Vec3) {
        let mut listener = self.listener();
        listener.position = position;
        self.set_listener(listener);
    }

    /// Place a live voice in the world — an emitter following its entity.
    ///
    /// This is the operation a game performs most: one listener, **hundreds** of
    /// emitters. `None` un-spatialises the voice back to its authored pan.
    pub fn set_emitter(&self, id: VoiceId, position: Option<Vec3>) {
        self.pending_emitters.borrow_mut().insert(id, position);
        // Emitter pushes ride the listener's signal so that they, too, schedule a
        // frame; the sequence bump guarantees the write is never equality-skipped.
        let (seq, listener) = self.listener.get();
        self.listener.set((seq.wrapping_add(1), listener));
    }

    /// How many times [`AudioWorldClock`] has run.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.ticks.get()
    }

    /// Is anything waiting to be carried to the audio thread?
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.listener_pending.get() || !self.pending_emitters.borrow().is_empty()
    }
}

/// The per-frame clock that carries an [`AudioWorld`] to the audio thread.
///
/// Registered with the owner's animation driver (see [`use_audio_world_clock`]) and
/// ticked once per paint. It drives the **same** command queue as the RPC / UI
/// surface — see [`SharedController`], which is why the controller is shared rather
/// than owned.
#[derive(Debug)]
pub struct AudioWorldClock {
    controller: SharedController,
    world: Rc<AudioWorld>,
}

impl AudioWorldClock {
    /// Pair a controller with the world it should carry.
    #[must_use]
    pub fn new(controller: SharedController, world: Rc<AudioWorld>) -> Self {
        Self { controller, world }
    }
}

impl Tickable for AudioWorldClock {
    fn tick(&self, _dt: f32) {
        // `dt` is unused deliberately: a pose is a pose, not a rate. Pushing the
        // same pose twice is harmless; pushing it a frame late just means the sound
        // follows the camera a frame behind, which is the game contract.
        self.world.ticks.set(self.world.ticks.get() + 1);

        let mut controller = self.controller.borrow_mut();

        if self.world.listener_pending.get() && controller.set_listener(self.world.listener()) {
            // Cleared only when the command actually QUEUED. A full ring means the
            // audio thread never heard it, so it stays pending for the next frame
            // rather than being silently dropped.
            self.world.listener_pending.set(false);
        }

        // Same contract for emitters: a pose that did not queue stays pending.
        self.world
            .pending_emitters
            .borrow_mut()
            .retain(|&id, &mut position| !controller.set_voice_position(id, position));
    }

    fn is_at_rest(&self, _epsilon: f32) -> bool {
        // Nothing pending → the shell's continuous frame loop is released. The next
        // world write re-arms it, the next paint pushes, and it settles again.
        !self.world.is_pending()
    }
}

/// Retrieve (registering once) the [`AudioWorldClock`] for `key` in the current
/// [`Owner`] scope, wired into the animation driver — the `use_vn_clock` pattern.
/// Call it from a view fn.
///
/// The view must also **read** something from the world (e.g.
/// [`AudioWorld::listener`]), or nothing subscribes to its [`Signal`], no repaint is
/// armed, and this clock never ticks. See the module docs, point 1.
///
/// # Panics
///
/// Panics if called outside an active `Owner` scope.
#[must_use]
pub fn use_audio_world_clock(
    key: &'static str,
    controller: SharedController,
    world: Rc<AudioWorld>,
) -> Rc<AudioWorldClock> {
    Owner::current()
        .expect("use_audio_world_clock requires an active Owner scope")
        .register_animation_once(key, || AudioWorldClock::new(controller, world))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::AudioClip;
    use crate::engine::{AudioEngine, PlayOptions};
    use crate::external::shared_controller;
    use crate::rt::realtime_channel;

    /// Exactly-representable values, but an exact `==` on floats is not a habit
    /// worth forming.
    #[track_caller]
    fn assert_pos(got: Vec3, want: Vec3) {
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert!((g - w).abs() < 1e-6, "axis {i}: got {got:?}, want {want:?}");
        }
    }

    fn rig() -> (SharedController, Rc<AudioWorld>, AudioWorldClock) {
        let (controller, _renderer) = realtime_channel(AudioEngine::new(48_000), 16, 8);
        let controller = shared_controller(controller);
        let world = Rc::new(AudioWorld::new());
        let clock = AudioWorldClock::new(controller.clone(), world.clone());
        (controller, world, clock)
    }

    #[test]
    fn a_frame_carries_the_listener_and_the_engine_is_untouched_until_then() {
        let (controller, world, clock) = rig();
        assert!(
            clock.is_at_rest(0.01),
            "a clean world releases the frame loop"
        );

        world.move_listener([3.0, 0.0, -4.0]);
        assert!(!clock.is_at_rest(0.01), "a world write re-arms it");
        assert_pos(controller.borrow().listener().position, [0.0, 0.0, 0.0]);

        clock.tick(0.016);
        assert_pos(controller.borrow().listener().position, [3.0, 0.0, -4.0]);
        assert!(clock.is_at_rest(0.01), "pushed, so it settles");
        assert_eq!(world.ticks(), 1);
    }

    /// ★ The emitter half — the operation a game performs hundreds of times per
    /// frame, and the one an earlier cut of this shipped without.
    #[test]
    fn a_frame_carries_emitter_poses_too() {
        let (controller, world, clock) = rig();
        let clip = AudioClip::sine(48_000, 440.0, 1.0, 0.9).shared();
        let id = controller
            .borrow_mut()
            .play(clip, "emitter", PlayOptions::looping())
            .expect("queued");

        world.set_emitter(id, Some([1.0, 2.0, 3.0]));
        assert!(!clock.is_at_rest(0.01));
        clock.tick(0.016);
        assert!(clock.is_at_rest(0.01), "the emitter pose was carried");
        assert!(
            world.pending_emitters.borrow().is_empty(),
            "and it is no longer pending"
        );
    }

    /// Moving the same emitter twice before a frame runs coalesces to one push.
    #[test]
    fn repeated_moves_of_one_emitter_coalesce() {
        let (_controller, world, _clock) = rig();
        world.set_emitter(7, Some([1.0, 0.0, 0.0]));
        world.set_emitter(7, Some([2.0, 0.0, 0.0]));
        assert_eq!(world.pending_emitters.borrow().len(), 1);
        assert_eq!(
            world.pending_emitters.borrow().get(&7),
            Some(&Some([2.0, 0.0, 0.0])),
            "the latest pose wins"
        );
    }

    /// ★ Regression: re-asserting the pose the world ALREADY holds must still be
    /// carried. `Signal::set` equality-skips, so without the sequence stamp this
    /// armed no frame while latching the pending flag — stranded forever.
    #[test]
    fn re_asserting_the_same_listener_pose_is_not_equality_skipped() {
        let (_controller, world, _clock) = rig();
        world.move_listener([3.0, 0.0, -4.0]);
        let first = world.listener.get();
        world.move_listener([3.0, 0.0, -4.0]);
        let second = world.listener.get();
        assert_ne!(
            first, second,
            "an unchanged pose must still produce a NEW signal value, or no frame \
             is ever scheduled to carry it"
        );
        assert!(world.is_pending());
    }

    /// `dt`-independent: a propagator, not an integrator. This is precisely why it
    /// can stay registered in a headless harness where a wall-clock integrator
    /// (`VnClock`) could not — it cannot drift.
    #[test]
    fn the_clock_is_dt_independent() {
        let (controller, world, clock) = rig();
        world.move_listener([1.0, 2.0, 3.0]);
        clock.tick(0.001); // a tiny frame …
        let after_small = controller.borrow().listener().position;
        world.move_listener([1.0, 2.0, 3.0]);
        clock.tick(10.0); // … and a huge one land the SAME pose.
        assert_pos(controller.borrow().listener().position, after_small);
        assert_pos(after_small, [1.0, 2.0, 3.0]);
    }
}
