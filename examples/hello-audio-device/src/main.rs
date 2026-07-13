// Example bindings tolerate looser doc-markdown lints than substrate crates;
// the narrative carries many proper-noun identifiers (AudioController, cpal,
// JSON-RPC, ALSA, …).
#![allow(clippy::doc_markdown)]

//! `hello-audio-device` — a **real cpal callback thread** driven concurrently
//! with the **JSON-RPC control thread** (§5.54, §2 #2).
//!
//! ## What this closes
//!
//! R1293's `hello-audio-rt` brought the real-time audio *control surface* onto
//! the wire, but it deliberately ran the mixer on the dispatch thread behind a
//! synchronous `render` step-verb: no device, no background thread, fully
//! deterministic. That left exactly one audio path unproven — the one that
//! actually ships:
//!
//! > a **free-running audio callback thread**, clocked by the sound card, calling
//! > [`pinion_audio::AudioRenderer::render`] while the RPC thread concurrently
//! > reads the lock-free [`pinion_audio::AudioSnapshot`] and pushes commands over
//! > the rtrb ring.
//!
//! Neither the crate tests (orchestrated, single-threaded interleave) nor
//! `hello-audio-rt` (one thread, stepped) exercise that. This binary does: it
//! opens a real output device with [`CpalOutput`], hands the renderer to cpal's
//! callback, and hosts the **shipping** [`AudioControllerExternal`] — every audio
//! verb delegated untouched, and **no step-verb added** — on the `pinion_shell`
//! stdin/stdout JSON-RPC surface. `tools/demos/hello_audio_device.py` then plays,
//! re-gains, saturates and stops live voices over the real wire while the callback
//! is running underneath.
//!
//! (The GUI shell, not the TUI, for a harness reason rather than a deep one:
//! `pinion_tui` *does* host JSON-RPC — it replies on stderr, since stdout is its
//! canvas — but `tools/rpc_verify.py` reads replies from stdout only, so every
//! demo in the repo drives a GUI binding. Under `PINION_HIDDEN_WINDOW=1` the
//! window is never mapped, so this runs headless.)
//!
//! ## Making no sound: the silent card
//!
//! A device proof that blasts a tone out of the developer's speakers (and cannot
//! run in CI at all) is not much of a proof. So this opens a **silent virtual
//! card** — Linux ALSA's `snd-dummy`, a timer-paced device with no output:
//!
//! ```text
//! sudo modprobe snd-dummy                       # card "Dummy" appears
//! PINION_AUDIO_DEVICE=hw:CARD=Dummy,DEV=0 cargo run -p hello-audio-device
//! ```
//!
//! Name it exactly (or by a substring that matches exactly one device). A bare
//! `Dummy` matches several ALSA PCMs and is *refused* rather than guessed at —
//! see [`resolve_device`], and note that a bare `hw` would match the real sound
//! card first, which is how a "first match wins" policy sends a silent-card test
//! out of the speakers.
//!
//! The environment variable only picks the *boot* device. Switching afterwards is
//! on the wire — `invoke set_device "<name>"` — because `devices` telling an agent
//! where sound *could* go while offering no way to go there would make the human's
//! shell more capable than the AI's §2 #2 primary path.
//!
//! It is a *real* device with a *real* hardware-style clock — the callback fires
//! on a timer exactly as a sound card's does — it simply discards the samples.
//! That is the audio analogue of rendering through lavapipe instead of a GPU, and
//! it is what lets this run unattended in CI. Any real output device works too
//! (name it, or unset the variable for the host default); the only cost is noise.
//!
//! ## Zero-flake without a step-verb
//!
//! A free-running callback cannot be stepped, so assertions cannot count frames
//! exactly — and they do not need to. The demo polls with `wait_until` until the
//! observed snapshot *settles* ("a voice becomes live", "peak rises above the
//! floor", "stop silences it"), which is outcome-based and wall-clock-independent
//! — the [[zero-flake-policy]] definition. The assertions are coarser than the
//! step-verb's, not flakier.
//!
//! ## What this does NOT prove (stated plainly)
//!
//! - **Not "no data race, ever."** No test can show that. The lock-free protocol
//!   itself (Release/Acquire id-fence on the per-voice slots, SPSC ring) is
//!   verified *deterministically* by the orchestrated cross-thread tests in
//!   `crates/pinion-audio/tests/realtime_channel.rs`. What this adds is
//!   integration confidence that the shipping configuration — real callback,
//!   real RPC thread, concurrently — works end to end.
//! - **Not that the samples reaching the card are correct.** The snapshot's `peak`
//!   is measured on the stereo mix *before* [`pinion_audio`]'s device path converts
//!   it to the card's sample format and channel layout — so no *introspection read*
//!   can see a bug in that conversion, and the unit tests on `write_frames` (in
//!   `pinion-audio`'s `device.rs`) are what cover it today. That is a **deferral,
//!   not an impossibility**: an `snd-aloop` loopback (output → capture on the same
//!   host — the same trick as the silent card, one step further) would verify the
//!   bytes the card actually *received*, including the two things a pure unit test
//!   structurally cannot — the channel count really passed in, and the buffer
//!   slicing in the live callback. Not done yet.
//!
//! ## The per-frame, control-side audio tick — built, not deferred
//!
//! The mixer is clocked by the **device**, and that is correct and permanent — no
//! engine frame-clocks a mixer; the card pulls when it pulls.
//!
//! But a game *also* does control-side audio work every frame: the listener
//! follows the camera, emitters follow entities, distant voices get culled. An
//! earlier round filed that under "Phase C". That was **wrong**, and rather than
//! just correct the sentence this binary now does it: [`AudioFollowClock`] is a
//! retained [`Tickable`], registered with the owner's animation driver and ticked
//! once per paint by the shell — the same substrate `CaretBlink` and
//! `pinion_narrative`'s `VnClock` ride. `invoke set_camera` moves *the world* and
//! never touches the audio engine; the listener follows because a **frame**
//! carried the pose over the lock-free ring.
//!
//! Three things had to be true, and only the first was as cheap as claimed:
//!
//! 1. **The clock substrate** — genuinely already there, unchanged.
//! 2. **A shareable controller.** `AudioControllerExternal` owned its
//!    `AudioController` outright, and the controller cannot be cloned (its command
//!    ring is SPSC). Two control-thread drivers — RPC and the frame — need one
//!    queue, so `pinion-audio` grew
//!    [`SharedController`]. That is the part the
//!    "nothing new is needed" claim got wrong.
//! 3. **A reactive world pose.** The camera is a
//!    [`Signal`] that `view` *reads*. This is
//!    load-bearing and was the whole bug: a `Signal` notifies its observers, a
//!    view subscribes by reading it, and only a dirty owner arms a repaint. With
//!    the camera in a plain `Cell` (or a `Signal` no view read), moving it painted
//!    nothing, so the clock never ticked and the listener never followed —
//!    measured, not theorised. An idle retained app has no free-running frame
//!    loop; a world change must *schedule* the frame that carries it.
//!
//! What remains genuinely Phase-C is narrower than "per-frame audio": the
//! fixed-timestep game loop's own `Send` subsystem registry (audio alongside
//! physics / AI, off the UI thread). Per-frame audio itself is here.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_audio::{
    AudioClip, AudioControllerExternal, CpalError, CpalOutput, RT_EXTERNAL_FIELDS,
    SharedController, Vec3, parse_vec3, shared_controller,
};
use pinion_core::animation::Tickable;
use pinion_core::external::{
    External, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    forward_intents, read_only_or_unknown,
};
use pinion_core::intent::Intent;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::{Frame, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};

// pinion-forge codegen output (see build.rs / app.pinion.xml).
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloAudioDeviceRenderer, HelloAudioDeviceRendererError);

/// Command-ring depth (control thread → audio thread).
const RING_CAP: usize = 64;
/// Voice-pool bound; the audio thread never reallocates past it.
const MAX_VOICES: usize = 8;
/// The primary External / paint-focus tag.
const TAG: &str = "audio_device";
/// Names the output device to open. Unset → the host default (**audible**).
const DEVICE_ENV: &str = "PINION_AUDIO_DEVICE";
/// `Owner::cache` key for the shared rig (device + world), resolved from both
/// `create_external` and `view`.
const RIG_KEY: &str = "audio_device.rig";
/// `Owner::register_animation_once` key for the per-frame follow clock.
const CLOCK_KEY: &str = "audio_device.follow_clock";

/// Tone amplitude. Well clear of the demo's "audible" threshold.
const TONE_AMPLITUDE: f32 = 0.9;

/// The device half of the surface — what the device-agnostic
/// [`AudioControllerExternal`] cannot know, put on the wire because an agent (or a
/// player's settings panel) must be able to answer three questions without
/// guessing:
///
/// - *where is my sound going?* — `device` / `sample_rate` / `channels` /
///   `sample_format`. (This demo asserts `device` names the **silent** card, so a
///   misconfiguration cannot quietly play out of the speakers.)
/// - *where could it go?* — `devices`, the host's output list. Enumeration is
///   what a settings panel is built on, and leaving it callable only from Rust
///   while the whole point of this binary is §2 #2 would be an odd place to stop.
/// - *is it still going?* — `stream_errors`. A dead device just stops calling
///   back; without this, silence-because-idle and silence-because-the-output-is-
///   gone read identically.
///
/// The fields stay **read-only**, but switching device is not therefore absent:
/// it is `invoke set_device "<name>"`. A device change is an *action* with real
/// consequences (the old stream stops, live voices are lost), not a slot to poke —
/// so it is a verb, and the reads report the outcome.
const DEVICE_FIELDS: &[(&str, &str)] = &[
    ("device", "text"),
    ("devices", "json"),
    ("sample_rate", "int"),
    ("channels", "int"),
    ("sample_format", "text"),
    ("stream_errors", "int"),
    // The WORLD pose the game owns (see `set_camera` / [`AudioFollowClock`]).
    // Read-only like the rest: it is moved with `invoke set_camera`, because a
    // camera move is an event in the world, not a slot in the audio surface.
    ("camera", "json"),
    // How many times the per-frame audio sync has run — "is my audio sync
    // running?". Declared, because a field that `query` answers but `$schema`
    // hides and `intervene` calls UnknownPath tells an agent three different
    // things about one path.
    ("frame_ticks", "int"),
];

/// Length of the composed schema.
const SCHEMA_LEN: usize = RT_EXTERNAL_FIELDS.len() + DEVICE_FIELDS.len();

/// The RT surface's fields followed by this binding's device fields, composed at
/// **compile time** from [`RT_EXTERNAL_FIELDS`] rather than hand-copied — so the
/// RT External stays the single source of truth for its own schema and a field
/// added there cannot silently go missing here.
const fn compose_schema() -> [(&'static str, &'static str); SCHEMA_LEN] {
    let mut fields = [("", ""); SCHEMA_LEN];
    let mut i = 0;
    while i < RT_EXTERNAL_FIELDS.len() {
        fields[i] = RT_EXTERNAL_FIELDS[i];
        i += 1;
    }
    let mut j = 0;
    while j < DEVICE_FIELDS.len() {
        fields[i + j] = DEVICE_FIELDS[j];
        j += 1;
    }
    fields
}

/// The composed field list, in a `static` so it outlives the `IntrospectSchema`
/// borrow.
static SCHEMA_FIELDS: [(&str, &str); SCHEMA_LEN] = compose_schema();

/// What matching `requested` against the host's device list produced.
#[derive(Debug, PartialEq, Eq)]
enum DeviceChoice {
    /// Exactly one device answers to the request.
    One(String),
    /// Several do. Refuse rather than guess — see [`resolve_device`].
    Ambiguous(Vec<String>),
    /// None do.
    NotFound,
}

/// Pick the device to open from `requested` against the host's `names`.
///
/// An exact name wins outright. Otherwise the request is treated as a
/// case-insensitive substring — `PINION_AUDIO_DEVICE=Dummy` should find a card
/// without the caller spelling out ALSA's full `hw:CARD=Dummy,DEV=0` — but **only
/// if it matches exactly one device**. Several matches is
/// [`DeviceChoice::Ambiguous`], and the caller aborts.
///
/// Refusing an ambiguous match is the whole point, and it is not theoretical: on
/// a typical Linux box `hw`, `DEV=0`, `Card` and even `d` all match the real
/// sound card *first*, so a "pick the first match" policy quietly opens the
/// **speakers** — the exact outcome naming a silent card was meant to prevent. A
/// partial name persisted by a settings panel would reopen on a different, and
/// audible, output. So: match one device or none.
fn resolve_device(requested: &str, names: &[String]) -> DeviceChoice {
    // An empty request matches EVERYTHING (`contains("")` is always true), so on a
    // single-output host it would resolve to `One` — the audible card — instead of
    // refusing. Treat it as no request at all.
    if requested.is_empty() {
        return DeviceChoice::NotFound;
    }
    if let Some(exact) = names.iter().find(|name| *name == requested) {
        return DeviceChoice::One(exact.clone());
    }
    let needle = requested.to_lowercase();
    let mut hits = names
        .iter()
        .filter(|name| name.to_lowercase().contains(&needle))
        .cloned()
        .collect::<Vec<_>>();
    match hits.len() {
        0 => DeviceChoice::NotFound,
        1 => DeviceChoice::One(hits.remove(0)),
        _ => DeviceChoice::Ambiguous(hits),
    }
}

/// Open the output device, failing **loudly** on anything but a single match.
///
/// A silent fallback here would be the worst outcome: the binary would appear to
/// work while the callback never ran — or worse, while it ran on the speakers. So
/// an absent or ambiguous request aborts, printing what the host actually offers.
fn open_output() -> (pinion_audio::AudioController, CpalOutput) {
    let requested = std::env::var(DEVICE_ENV).ok();

    let opened = match &requested {
        None => CpalOutput::start_default(RING_CAP, MAX_VOICES),
        Some(want) => {
            let names = CpalOutput::output_device_names().unwrap_or_else(|e| {
                eprintln!("hello-audio-device: cannot enumerate output devices: {e}");
                std::process::exit(1);
            });
            let name = match resolve_device(want, &names) {
                DeviceChoice::One(name) => name,
                DeviceChoice::Ambiguous(hits) => {
                    eprintln!(
                        "hello-audio-device: {DEVICE_ENV}={want:?} is ambiguous — it matches \
                         {} devices: {hits:?}\nname one exactly; guessing could open an \
                         AUDIBLE device.",
                        hits.len()
                    );
                    std::process::exit(1);
                }
                DeviceChoice::NotFound => {
                    eprintln!(
                        "hello-audio-device: no output device matches {DEVICE_ENV}={want:?}.\n\
                         available: {names:?}\n\
                         hint: the silent test card comes from `sudo modprobe snd-dummy`."
                    );
                    std::process::exit(1);
                }
            };
            CpalOutput::start_on(&name, RING_CAP, MAX_VOICES)
        }
    };

    opened.unwrap_or_else(|e| {
        eprintln!("hello-audio-device: could not open the audio output: {e}");
        std::process::exit(1);
    })
}

/// The open device plus the little bit of **world** that drives it: everything
/// both the RPC surface and the per-frame clock need, built once per `Owner`
/// scope and resolved by key from `create_external` and `view` alike (the
/// `use_vn_state` pattern).
///
/// `camera` is the world pose the *game* owns. Nothing here writes it to the
/// audio engine — that is the clock's job, once a frame. See
/// [`AudioFollowClock`].
struct AudioRig {
    /// Shared with [`AudioFollowClock`]: two control-thread drivers, one queue.
    controller: SharedController,
    /// Owns the live stream — dropping the rig stops the device. In a `RefCell`
    /// because the output is **switchable at runtime** (see `set_device`).
    out: RefCell<CpalOutput>,
    /// The game-side state the clock carries to the audio thread.
    world: Rc<World>,
}

/// The control-side world the frame carries across — deliberately **separate from
/// the device**, because [`AudioFollowClock`] has no business knowing about a
/// sound card. That separation is also what makes the clock unit-testable with no
/// device at all (see the tests): a `realtime_channel` needs no hardware.
#[derive(Debug)]
struct World {
    /// Where the listener *is* in the world, as the game last moved it.
    ///
    /// A reactive [`Signal`], not a plain `Cell`, and that is load-bearing: a
    /// `Signal::set` marks the reactive `Owner` dirty, which is the bridge the
    /// shell's dispatch tail reads to arm a repaint. Written to a `Cell` the
    /// camera would move and *nothing would ever paint* — so the frame that
    /// carries the pose to the audio thread would never run, and an idle app
    /// would sit there with its sound in the wrong place. (Measured: with a
    /// `Cell`, `frame_ticks` stayed at 1 and the listener never followed.)
    /// `(write sequence, position)`.
    ///
    /// ★ The sequence number is **load-bearing, not bookkeeping**. [`Signal::set`]
    /// **equality-skips**: writing the value it already holds notifies nobody, so
    /// with a bare `Signal<Vec3>` a `set_camera` to the *current* pose armed no
    /// repaint — while `camera_dirty` latched `true` regardless. The frame that
    /// services it never ran, the audio thread was never told, and the flag stayed
    /// pending forever. (Reproduced over the wire: re-asserting an unchanged pose
    /// after an `invoke set_listener` left camera and listener permanently
    /// disagreeing, with the RPC call returning success.) Re-asserting the same
    /// pose is exactly what a save/load restore or a respawn-in-place does.
    ///
    /// Bumping the sequence on every write makes the new value never equal to the
    /// old, so a write *always* notifies, *always* arms a frame, and the dirty
    /// flag is always serviced. One signal, one writer ([`World::set_camera`]) —
    /// the pose and its schedule token cannot drift apart.
    camera: Signal<(u64, Vec3)>,
    /// How many times the per-frame clock has run. The honest measure of whether
    /// the frame loop is actually carrying the world across — and a real §2 #7
    /// read for a game ("is my audio sync running?").
    frame_ticks: Cell<u64>,
    /// Set when `camera` has moved and the audio thread has not been told yet.
    /// This is what makes the clock a *propagator* and not a busy loop: it is the
    /// clock's `is_at_rest`, so the shell's frame loop is armed only while there
    /// is something to push, then released.
    camera_dirty: Cell<bool>,
}

impl Default for World {
    fn default() -> Self {
        Self {
            camera: Signal::new((0, Vec3::default())),
            frame_ticks: Cell::new(0),
            camera_dirty: Cell::new(false),
        }
    }
}

impl World {
    /// Move the world camera. Deliberately does **not** touch the audio engine —
    /// the whole point is that the *frame* carries it there.
    fn set_camera(&self, position: Vec3) {
        // The Signal write is what wakes the shell: `view` reads this signal, so
        // setting it marks the owner dirty and a repaint is armed. With a plain
        // `Cell` nothing would ever paint and the pose would never land. The
        // sequence bump is what stops `Signal`'s equality-skip from swallowing a
        // re-assert of the current pose (see the field doc).
        let seq = self.camera.get().0;
        self.camera.set((seq.wrapping_add(1), position));
        self.camera_dirty.set(true);
    }

    /// The camera's world position (without the write sequence).
    fn camera_position(&self) -> Vec3 {
        self.camera.get().1
    }
}

impl std::fmt::Debug for AudioRig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioRig")
            .field("device", &self.out.borrow().device_name())
            .field("camera", &self.world.camera_position())
            .finish_non_exhaustive()
    }
}

impl AudioRig {
    /// Open the device and start its callback thread.
    fn open() -> Self {
        let (controller, out) = open_output();
        eprintln!(
            "hello-audio-device: {:?} @ {} Hz, {} channel(s) — callback thread live",
            out.device_name(),
            out.sample_rate(),
            out.channels()
        );
        Self {
            controller: shared_controller(controller),
            out: RefCell::new(out),
            world: Rc::new(World::default()),
        }
    }

    /// **Switch the output device at runtime.** Open `name`, hand the new
    /// renderer to its callback, and swap both halves behind the shared handles —
    /// the RPC surface and the frame clock keep the same `Rc`s and never notice.
    ///
    /// An earlier round refused to build this and justified it by claiming a
    /// switch "means rebuilding the renderer, controller **and clip registry** and
    /// re-homing live voices". Two thirds of that was invented:
    ///
    /// - **Clips need nothing.** They are `Arc<AudioClip>` held by the External,
    ///   and every voice *resamples* its clip to the engine's rate — so a device
    ///   at a different rate reuses them untouched. The library is not even
    ///   reachable from here.
    /// - **The controller swap is one assignment**, because it lives behind a
    ///   [`SharedController`] — which the very round that wrote the excuse shipped.
    ///
    /// What is genuinely true is the third: **live voices do not survive.** The old
    /// engine dies with the old stream. That is what every DAW and game engine does
    /// on a device switch, and it is honest to say so rather than to pretend the
    /// whole feature is expensive.
    ///
    /// On failure the CURRENT device keeps playing and the error is returned — a
    /// bad name must never leave the app silent.
    fn set_device(&self, name: &str) -> Result<(), CpalError> {
        let (controller, out) = CpalOutput::start_on(name, RING_CAP, MAX_VOICES)?;
        // Swap behind the Rc: every holder of the shared controller (the External,
        // the clock) now drives the new device's queue.
        *self.controller.borrow_mut() = controller;
        // Dropping the old `CpalOutput` here stops the old stream.
        *self.out.borrow_mut() = out;
        // The new engine has never heard of the world, so re-arm the push: the
        // next frame carries the listener pose onto the new device.
        self.world.set_camera(self.world.camera_position());
        Ok(())
    }
}

/// The shared rig for this `Owner` scope.
///
/// # Panics
///
/// Panics if called outside an active `Owner` scope (`create_external` and
/// `view` both run inside one).
fn audio_rig() -> Rc<AudioRig> {
    Owner::current()
        .expect("audio_rig requires an active Owner scope")
        .cache(RIG_KEY, AudioRig::open)
}

/// **The per-frame, control-side audio tick** — the thing a game actually does
/// every frame: push the world's listener pose to the audio engine.
///
/// This exists to make a claim concrete rather than assert it. The mixer is
/// clocked by the *device* and always will be. But the control-side work — the
/// listener following the camera, emitters following entities — is per-*frame*,
/// and it needs no Phase-C machinery: it is a retained
/// [`Tickable`], registered with the owner's animation driver
/// (`Owner::register_animation_once`) and ticked once per paint by the shell,
/// exactly as `CaretBlink` and `pinion_narrative`'s `VnClock` are.
///
/// Unlike those two it is a **propagator, not an integrator**: it does not
/// advance anything by `dt`, it pushes the current world pose over the same
/// lock-free ring the RPC surface uses. So it is idempotent, `dt`-independent,
/// and — the reason the VN clock had to stay out of its harness — it cannot
/// drift a headless demo. It settles, and a demo polls for the settled outcome.
#[derive(Debug)]
struct AudioFollowClock {
    /// The same queue the RPC surface drives — one controller, two drivers.
    controller: SharedController,
    /// What the game changed; what the frame must carry across.
    world: Rc<World>,
}

impl Tickable for AudioFollowClock {
    fn tick(&self, _dt: f32) {
        // `dt` is unused ON PURPOSE: a pose is a pose, not a rate. Pushing it
        // twice is harmless; pushing it late just means the sound follows the
        // camera one frame behind, which is exactly the game contract.
        self.world.frame_ticks.set(self.world.frame_ticks.get() + 1);
        if !self.world.camera_dirty.get() {
            return;
        }
        let mut controller = self.controller.borrow_mut();
        // Keep the listener's orientation; move only where it is standing.
        let mut listener = controller.listener();
        listener.position = self.world.camera_position();
        if controller.set_listener(listener) {
            // Queued. If the ring were full the write did NOT reach the audio
            // thread, so stay dirty and try again next frame rather than
            // silently dropping the camera move.
            self.world.camera_dirty.set(false);
        }
    }

    fn is_at_rest(&self, _epsilon: f32) -> bool {
        // Nothing pending → release the shell's continuous frame loop. A camera
        // move re-arms it, the next paint pushes, and it settles again.
        !self.world.camera_dirty.get()
    }
}

/// Register the follow-clock with the animation driver (once per `Owner`).
///
/// # Panics
///
/// Panics if called outside an active `Owner` scope.
fn use_audio_follow_clock(rig: &Rc<AudioRig>) -> Rc<AudioFollowClock> {
    // The RIG owns the queue and lends it to both drivers (the External via
    // `from_shared`, this clock via the same handle) — one queue, two
    // control-thread writers.
    let controller = rig.controller.clone();
    let world = rig.world.clone();
    Owner::current()
        .expect("use_audio_follow_clock requires an active Owner scope")
        .register_animation_once(CLOCK_KEY, || AudioFollowClock { controller, world })
}

/// The binding's [`External`]: the shipping [`AudioControllerExternal`] plus the
/// live device stream it is feeding.
///
/// Every *audio* verb — `play` / `stop` / `set_*` and every RT read — delegates to
/// the inner External untouched, so the audio contract this binary proves is
/// exactly [`AudioControllerExternal`]'s, with **no harness verb bolted on**:
/// unlike `hello-audio-rt` there is no `render` step-verb, because there is
/// nothing to step (the [`pinion_audio::AudioRenderer`] lives in cpal's callback
/// thread and is pumped by the device clock).
///
/// What this wrapper adds is the *device* half of the surface, which the
/// device-agnostic controller cannot know: the read-only [`DEVICE_FIELDS`]. So
/// the wire contract's home is the inner External **plus** those fields — see
/// [`compose_schema`].
///
/// It also declares [`pinion_core::external::ThreadOwnership::OwnThread`], because
/// that is the truth: a cpal callback thread is running underneath, and the
/// framework reaches it only over the lock-free ring. Every other External in the
/// repo is `UiThreadSync`; this is the first that genuinely is not.
#[derive(Debug)]
struct DeviceAudioExternal {
    /// The §2 #7 RT surface — the home of every *audio* verb and read. It drives
    /// the SAME controller the per-frame [`AudioFollowClock`] does.
    inner: AudioControllerExternal,
    /// The open device + the world pose, shared with the clock. The
    /// [`pinion_audio::AudioRenderer`] is not reachable from this side at all —
    /// it lives on the audio thread, which is the whole point of the split.
    rig: Rc<AudioRig>,
    /// §5.20 intents forwarded from the inner controller (e.g. `audio.play`).
    pending_intents: Vec<Intent>,
}

impl DeviceAudioExternal {
    /// Take the shared rig and register the demo clips.
    fn open() -> Self {
        let rig = audio_rig();
        // The engine renders at the device's rate, so author the clips there too
        // (per-voice resampling would handle a mismatch; matching is just tidier).
        let rate = rig.out.borrow().sample_rate();
        let inner = AudioControllerExternal::from_shared(rig.controller.clone())
            // Looping, so it stays live for as long as the demo polls it.
            .with_clip(
                "tone",
                AudioClip::sine(rate, 440.0, 1.0, TONE_AMPLITUDE).shared(),
            )
            .with_clip(
                "bell",
                AudioClip::sine(rate, 880.0, 1.0, TONE_AMPLITUDE).shared(),
            );
        Self {
            inner,
            rig,
            pending_intents: Vec::new(),
        }
    }
}

impl ExternalIntrospect for DeviceAudioExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&SCHEMA_FIELDS)
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "device" => Some(IntrospectValue::Text(
                self.rig.out.borrow().device_name().to_owned(),
            )),
            // What the host offers — the list a settings panel is built on. Read
            // live, so a device appearing or vanishing shows up without a restart.
            // An enumeration FAILURE is `Null`, not `[]`: on the one surface whose
            // job is telling "nothing there" apart from "the thing is broken",
            // collapsing an error into an empty list is the same lie it exists to
            // prevent.
            "devices" => Some(match CpalOutput::output_device_names() {
                Ok(names) => IntrospectValue::json(&names),
                Err(_) => IntrospectValue::Null,
            }),
            "sample_rate" => Some(IntrospectValue::Int(i64::from(
                self.rig.out.borrow().sample_rate(),
            ))),
            "channels" => Some(IntrospectValue::Int(i64::from(
                self.rig.out.borrow().channels(),
            ))),
            // A stable wire token, not `Debug` of a foreign `#[non_exhaustive]`
            // enum — cpal's rendering is not ours to promise.
            "sample_format" => Some(IntrospectValue::Text(
                self.rig.out.borrow().sample_format_wire().to_owned(),
            )),
            // Non-zero means the output has faulted — the only reading that tells
            // "nothing is playing" apart from "the device is gone".
            "stream_errors" => Some(IntrospectValue::Int(
                i64::try_from(self.rig.out.borrow().stream_errors()).unwrap_or(i64::MAX),
            )),
            // Where the game's camera is. Compare it with `listener` to watch the
            // per-frame clock catch up: `set_camera` moves THIS, and only the
            // frame tick moves the listener to match.
            "camera" => Some(IntrospectValue::json(&self.rig.world.camera_position())),
            "frame_ticks" => Some(IntrospectValue::Int(
                i64::try_from(self.rig.world.frame_ticks.get()).unwrap_or(i64::MAX),
            )),
            // Everything else is the RT surface's, read lock-free off the
            // snapshot the audio thread publishes each callback.
            _ => self.inner.query(path),
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        // Every field on this surface is an observation, not a slot — the RT half
        // is driven by `invoke`, and a device fact is changed by opening a
        // different device. So the answer is the composed SCHEMA's: declared =>
        // ReadOnly, else UnknownPath. Keying off `DEVICE_FIELDS` instead would be
        // a second source of truth for "which of my fields exist" — which is
        // exactly how `frame_ticks` came to be queryable yet undeclared.
        Err(read_only_or_unknown(&self.schema(), path))
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        // ★ Switch the output device — the half of the seam that was missing.
        // `devices` told an agent where sound COULD go while giving it no way to
        // go there: selection was possible only through an environment variable,
        // i.e. a human with a shell could do what the AI's §2 #2 *primary* path
        // could not. This closes that.
        if path == "set_device" {
            let IntrospectValue::Text(name) = args else {
                return Err(InvokeError::TypeMismatch);
            };
            return match self.rig.set_device(&name) {
                Ok(()) => {
                    self.pending_intents.push(Intent::new_static(
                        "audio.device",
                        IntrospectValue::Text(name),
                    ));
                    Ok(IntrospectValue::Null)
                }
                // The CURRENT device is still playing — a bad name must never
                // leave the app silent, so this is a loud refusal, not a fault.
                Err(_) => Err(InvokeError::Rejected),
            };
        }
        // On THIS binding the WORLD owns the listener: the per-frame clock is its
        // sole writer. A raw `set_listener` would silently win — and could not be
        // taken back, because the clock only re-asserts when the camera *changes*
        // — so the two writers are arbitrated here rather than left to race.
        // (Refused, not ignored: the write is loud, and it names its replacement.)
        if path == "set_listener" {
            return Err(InvokeError::Rejected);
        }
        // The one verb this binding owns: move the world camera. Note what it
        // does NOT do — it never touches the audio engine. The listener follows
        // only because the per-frame [`AudioFollowClock`] carries the pose across
        // on the next paint, which is what makes `listener` catching up to
        // `camera` a proof that the frame tick ran.
        if path == "set_camera" {
            let IntrospectValue::Json(v) = args else {
                return Err(InvokeError::TypeMismatch);
            };
            let pos = parse_vec3(Some(&v)).ok_or(InvokeError::TypeMismatch)?;
            self.rig.world.set_camera(pos);
            // A §5.20 symbolic event — "the camera moved" — visible on
            // `scene/intents`. It does NOT schedule the frame: nothing in the
            // shell consults `External::is_dirty` (it only skips the
            // `drain_intents` call), and this binding's `State` does not change on
            // a camera move. The `camera` Signal the view reads is the only thing
            // that arms a repaint. Do not delete that read believing this intent
            // covers it — it does not.
            self.pending_intents.push(Intent::new_static(
                "audio.camera",
                IntrospectValue::json(&pos),
            ));
            return Ok(IntrospectValue::Null);
        }
        // Verbatim: `play` / `stop` / `stop_all` / `set_master_gain` /
        // `set_voice_*` / `set_listener` / `set_attenuation` / `set_voice_policy`
        // all queue onto the lock-free ring the *live callback thread* is
        // draining. No step-verb — the device clock is the pump.
        let result = self.inner.invoke(path, args);
        forward_intents(&mut self.inner, &mut self.pending_intents);
        result
    }
}

// §5.15 item 3: this External owns a real OS thread (cpal's audio callback)
// and is spoken to over the lock-free ring — so it declares `OwnThread`, not the
// `UiThreadSync` default every other binding correctly uses.
pinion_core::intent_query_external_impl!(DeviceAudioExternal, OwnThread);

/// The binding unit type.
struct HelloAudioDevice;

impl WidgetCore for HelloAudioDevice {
    /// Live voice count, so the shell repaints when the audio thread's snapshot
    /// changes underneath it.
    type State = u16;
    /// No keyboard affordances: this is an RPC harness.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(DeviceAudioExternal::open())
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> u16 {
        scene
            .primary_external()
            .and_then(|node| node.handle.introspect())
            .and_then(|intro| intro.query("voice_count"))
            // `IntrospectValue::as_i64` is core's own typed extractor — no reason
            // to hand-roll the match.
            .and_then(|v| v.as_i64())
            .map_or(0, |n| {
                u16::try_from(n.clamp(0, i64::from(u16::MAX))).unwrap_or(0)
            })
    }

    fn view(state: u16, _frame: &Frame) -> Scene {
        // ★ The per-frame control-side audio tick, registered here because `view`
        // is the Owner scope the shell re-enters every paint. Unlike the VN
        // clock — which had to stay OUT of its headless harness, because a
        // wall-clock integrator drifts the play-head run to run — this one is
        // safe to leave on: it *propagates* the camera pose rather than
        // integrating `dt`, so it settles to the same state no matter how many
        // frames the shell happened to paint.
        let rig = audio_rig();
        let _clock = use_audio_follow_clock(&rig);

        // ★ Reading the camera Signal HERE is what closes the loop, and it is not
        // decoration. `Signal::set` notifies its *observers*, and a view acquires
        // that subscription by calling `get()` inside the Owner scope. Without
        // this read, moving the camera marks nothing dirty, the shell never
        // paints, the animation driver never ticks, and the listener never
        // follows — measured, before this line existed. The view depending on the
        // world pose is exactly what makes a world change schedule a frame.
        // Subscribing to the signal (see `World::camera`) is what arms the frame.
        let camera = rig.world.camera_position();

        let mut children: Vec<Scene> = Vec::new();
        let mut y = 16u32;

        for line in [
            format!("hello-audio-device — real cpal callback + RPC ({state} live voice(s))"),
            format!("camera {camera:?} — the per-frame clock carries this to the listener"),
            "The audio thread is clocked by the DEVICE, not by an RPC step-verb.".to_string(),
            "Run it on a silent card: sudo modprobe snd-dummy + PINION_AUDIO_DEVICE=Dummy"
                .to_string(),
            "query: device / sample_rate / channels + the RT surface (voice_count,".to_string(),
            "     peak, frames_rendered, voices, rejected, stolen, listener, …)".to_string(),
            "invoke: play / stop / stop_all / set_master_gain / set_voice_{gain,pan,position}"
                .to_string(),
        ] {
            children.push(Scene::Text(TextNode::new(line, Rect::new(16, y, 540, 16))));
            y += 26;
        }

        // R55.G.17 — the paint scene carries a node tagged `tag()` so AI-side
        // path routing / `rect_for_tag` resolve.
        let mut root = ContainerNode::new(children).with_tag(TAG);
        root.rect = Rect::new(0, 0, 560, y + 16);
        Scene::Container(root)
    }

    fn event_name((): ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-audio-device — real device callback + RPC (§5.54 / §2 #2)"
    }
}

impl WidgetA11y for HelloAudioDevice {
    fn access_node(state: &u16, focused: Option<&str>) -> Vec<AccessNode> {
        vec![
            AccessNode::new(TAG, AriaRole::List)
                .with_name(format!("device audio — {state} live voice(s)"))
                .with_focused(focused == Some(TAG)),
        ]
    }
}

impl WidgetView for HelloAudioDevice {
    type Renderer = HelloAudioDeviceRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: 560,
            height: 220,
        }
    }
}

fn main() {
    pinion_shell::run::<HelloAudioDevice>();
}

#[cfg(test)]
mod tests {
    //! Device-**free** tests: `cargo test --workspace` runs on machines (and CI
    //! jobs) with no sound card, so nothing here may open an output. The live
    //! callback + RPC concurrency is proven by `tools/demos/hello_audio_device.py`
    //! against a real device. What is left to lock down here is the pure logic
    //! that a demo failure would not localise: the device-matching policy and the
    //! compile-time schema composition.
    use super::{
        AudioFollowClock, DEVICE_FIELDS, DeviceChoice, RT_EXTERNAL_FIELDS, SCHEMA_FIELDS, Vec3,
        World, resolve_device,
    };
    use pinion_audio::{AudioEngine, realtime_channel, shared_controller};
    use pinion_core::animation::Tickable;
    use std::rc::Rc;

    /// A realistic ALSA list: a real (AUDIBLE) card plus the silent dummy under
    /// several PCM prefixes — the shape that makes substring matching dangerous.
    fn names() -> Vec<String> {
        [
            "sysdefault:CARD=PCH",
            "hw:CARD=PCH,DEV=0",
            "hw:CARD=Dummy,DEV=0",
            "plughw:CARD=Dummy,DEV=0",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[test]
    fn exact_name_wins_over_a_substring_match() {
        // "hw:CARD=Dummy,DEV=0" is a substring of "plughw:CARD=Dummy,DEV=0", so a
        // substring pass would find TWO. An exact hit must short-circuit that.
        assert_eq!(
            resolve_device("hw:CARD=Dummy,DEV=0", &names()),
            DeviceChoice::One("hw:CARD=Dummy,DEV=0".to_owned())
        );
    }

    #[test]
    fn a_unique_substring_resolves_case_insensitively() {
        // "PCH,DEV" hits exactly one device, so the convenience is safe here.
        assert_eq!(
            resolve_device("pch,dev", &names()),
            DeviceChoice::One("hw:CARD=PCH,DEV=0".to_owned())
        );
    }

    /// ★ The finding that matters: a partial name must NEVER be resolved by
    /// guessing, because on a real box the guess lands on the SPEAKERS.
    /// "Dummy" matches two PCMs here, and "hw" matches the real card first.
    #[test]
    fn an_ambiguous_substring_refuses_rather_than_opening_the_speakers() {
        assert_eq!(
            resolve_device("Dummy", &names()),
            DeviceChoice::Ambiguous(vec![
                "hw:CARD=Dummy,DEV=0".to_owned(),
                "plughw:CARD=Dummy,DEV=0".to_owned(),
            ]),
            "two dummy PCMs match — refuse, do not pick one"
        );
        // The dangerous case, demonstrated on a real host: "hw" matches the
        // AUDIBLE card before the silent one. Picking the first would send a
        // silent-card test out of the speakers.
        assert!(
            matches!(resolve_device("hw", &names()), DeviceChoice::Ambiguous(_)),
            "a request that matches the real card AND the dummy must be refused"
        );
    }

    #[test]
    fn an_absent_device_resolves_to_nothing_not_to_the_first_device() {
        // A typo'd name must NOT quietly select the speakers; NotFound is what
        // makes the caller abort loudly.
        assert_eq!(resolve_device("Loopback", &names()), DeviceChoice::NotFound);
    }

    /// Positions are exactly representable here, but an exact `==` on floats is
    /// not a habit worth forming.
    #[track_caller]
    fn assert_pos(got: Vec3, want: Vec3) {
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert!((g - w).abs() < 1e-6, "axis {i}: got {got:?}, want {want:?}");
        }
    }

    /// ★ Regression: a `set_camera` to the pose it ALREADY holds must still be
    /// serviced. `Signal::set` equality-skips, so before the write-sequence stamp
    /// this armed no repaint while latching `camera_dirty` — the push was stranded
    /// forever and the audio thread never heard about it.
    #[test]
    fn re_asserting_the_same_camera_pose_is_not_silently_dropped() {
        let world = World::default();
        world.set_camera([3.0, 0.0, -4.0]);
        let first = world.camera.get();
        // The very same position again …
        world.set_camera([3.0, 0.0, -4.0]);
        let second = world.camera.get();
        assert_ne!(
            first, second,
            "an unchanged pose must still produce a NEW signal value, or the \
             equality-skip swallows the write and no frame is ever scheduled"
        );
        assert_pos(world.camera_position(), [3.0, 0.0, -4.0]);
        assert!(world.camera_dirty.get(), "and it is still pending a push");
    }

    /// ★ The per-frame tick, with **no sound card**: a `realtime_channel` needs no
    /// hardware, and splitting [`World`] from the device is what lets the clock be
    /// tested at all. Locks the contract the demo proves over the wire.
    #[test]
    fn the_frame_tick_carries_the_camera_to_the_audio_thread() {
        let (controller, _renderer) = realtime_channel(AudioEngine::new(48_000), 8, 4);
        let world = Rc::new(World::default());
        let clock = AudioFollowClock {
            controller: shared_controller(controller),
            world: world.clone(),
        };

        // A clean world is at rest — the shell's frame loop is released.
        assert!(clock.is_at_rest(0.01));
        assert_pos(
            clock.controller.borrow().listener().position,
            [0.0, 0.0, 0.0],
        );

        // Moving the camera does NOT touch the audio engine…
        world.set_camera([3.0, 0.0, -4.0]);
        assert!(!clock.is_at_rest(0.01), "a camera move re-arms the loop");
        assert_pos(
            clock.controller.borrow().listener().position,
            [0.0, 0.0, 0.0],
        );

        // …a frame does.
        clock.tick(0.016);
        assert_pos(
            clock.controller.borrow().listener().position,
            [3.0, 0.0, -4.0],
        );
        assert!(clock.is_at_rest(0.01), "pushed, so it settles again");
        assert_eq!(world.frame_ticks.get(), 1);
    }

    /// `dt`-independent: it is a propagator, not an integrator. This is exactly
    /// why it may stay registered in a headless harness where a wall-clock
    /// integrator (`VnClock`) could not — it cannot drift.
    #[test]
    fn the_clock_is_dt_independent_and_keeps_the_listener_facing() {
        let (controller, _renderer) = realtime_channel(AudioEngine::new(48_000), 8, 4);
        let world = Rc::new(World::default());
        let clock = AudioFollowClock {
            controller: shared_controller(controller),
            world: world.clone(),
        };
        let facing = clock.controller.borrow().listener().forward;

        world.set_camera([1.0, 2.0, 3.0]);
        clock.tick(0.001); // a tiny frame …
        let after_small = clock.controller.borrow().listener().position;
        world.set_camera([1.0, 2.0, 3.0]);
        clock.tick(10.0); // … and a huge one land the SAME pose.
        assert_pos(clock.controller.borrow().listener().position, after_small);
        assert_pos(after_small, [1.0, 2.0, 3.0]);
        // It moves where the ears ARE, never where they look.
        assert_pos(clock.controller.borrow().listener().forward, facing);
    }

    #[test]
    fn schema_is_the_rt_surface_plus_the_device_fields() {
        // Guards the const-composition: the RT fields come first, in order, and
        // the device fields are appended — no drift, nothing dropped.
        assert_eq!(
            SCHEMA_FIELDS.len(),
            RT_EXTERNAL_FIELDS.len() + DEVICE_FIELDS.len()
        );
        assert_eq!(
            &SCHEMA_FIELDS[..RT_EXTERNAL_FIELDS.len()],
            RT_EXTERNAL_FIELDS
        );
        assert_eq!(&SCHEMA_FIELDS[RT_EXTERNAL_FIELDS.len()..], DEVICE_FIELDS);
        // And the fields the demo reads are actually declared.
        for field in [
            "device",
            "sample_rate",
            "channels",
            "peak",
            "frames_rendered",
        ] {
            assert!(
                SCHEMA_FIELDS.iter().any(|(name, _)| *name == field),
                "{field} must be declared in the schema"
            );
        }
    }

    /// ★ The composition's *other* hazard — the one the omission guard above does
    /// not catch. `query` matches this binding's device names BEFORE delegating to
    /// the RT surface, so a name declared on both sides would be announced twice
    /// over the wire and the RT value would be silently shadowed. No error, no
    /// failing test — a silent misroute.
    ///
    /// This is live, not hypothetical: `AudioEngineExternal` already declares
    /// `sample_rate`, and it is the most obvious field for someone to add to the
    /// RT surface next. Then this fires instead of shipping the shadow.
    /// ★ The direction the shadow test below does NOT cover, and the one that
    /// actually broke: **every path `query` answers must be declared**. A field
    /// that `query` returns but `$schema` omits tells an agent it does not exist,
    /// while `intervene` calls it `UnknownPath` — three answers for one path.
    ///
    /// Enforced from the declared side (the answerable direction): every field in
    /// the composed schema must be a path this binding, or the RT surface beneath
    /// it, actually answers. The demo asserts the same set over the real wire.
    #[test]
    fn every_declared_field_is_a_real_path() {
        // The RT half is covered by the inner External's own tests; here assert
        // the binding's own fields are all declared AND all answered.
        for field in [
            "device",
            "devices",
            "sample_rate",
            "channels",
            "sample_format",
            "stream_errors",
            "camera",
            "frame_ticks",
        ] {
            assert!(
                DEVICE_FIELDS.iter().any(|(name, _)| *name == field),
                "{field} is answered by `query` but not declared in DEVICE_FIELDS"
            );
            assert!(
                SCHEMA_FIELDS.iter().any(|(name, _)| *name == field),
                "{field} must reach the composed schema"
            );
        }
    }

    #[test]
    fn the_device_fields_do_not_shadow_any_rt_field() {
        for (device_field, _) in DEVICE_FIELDS {
            assert!(
                !RT_EXTERNAL_FIELDS.iter().any(|(rt, _)| rt == device_field),
                "{device_field:?} is declared by BOTH the RT surface and this \
                 binding: the wire would list it twice and `query` would shadow \
                 the RT value. Rename this binding's field, or drop it and \
                 delegate."
            );
        }
    }
}
