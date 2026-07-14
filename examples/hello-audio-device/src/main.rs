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
//! ## The per-frame, control-side audio tick — built, and in the crate
//!
//! The mixer is clocked by the **device**, and that is correct and permanent — no
//! engine frame-clocks a mixer; the card pulls when it pulls.
//!
//! But a game *also* does control-side audio work every frame: the listener follows
//! the camera, and emitters follow entities. An earlier round filed that under
//! "Phase C". That was wrong, and correcting the sentence was not enough — so it is
//! built, and it lives in [`pinion_audio::world`] rather than here, for the reason
//! `VnClock` lives in `pinion-narrative`: the subtle part is the *protocol* (a world
//! write must schedule its own frame; `Signal::set` equality-skips; a full ring must
//! be retried), and every binding that re-rolled it would get it wrong identically.
//!
//! This binary is that substrate's first consumer. `invoke set_camera` and
//! `invoke set_emitter` write **only the world**; the listener and the emitters
//! follow because a **frame** carried them over the lock-free ring. Driving the
//! audio engine directly (`set_listener` / `set_voice_position`) is *refused* here:
//! two unarbitrated writers of the same spatial state is a bug that already shipped
//! once.
//!
//! Building it also refuted the tidy claim that "nothing new is needed": the
//! `AudioController` cannot be cloned (its command ring is SPSC), so two
//! control-thread drivers — RPC and the frame — needed
//! [`SharedController`] to share one queue.
//!
//! What remains genuinely Phase-C is narrower than "per-frame audio": the
//! fixed-timestep game loop's own `Send` subsystem registry (audio alongside physics
//! / AI, off the UI thread).

use std::cell::RefCell;
use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_audio::{
    AudioClip, AudioControllerExternal, AudioWorld, CpalError, CpalOutput, RT_EXTERNAL_FIELDS,
    SharedController, parse_emitter, parse_vec3, shared_controller, use_audio_world_clock,
};
use pinion_core::external::{
    External, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    forward_intents, read_only_or_unknown,
};
use pinion_core::intent::Intent;
use pinion_core::reactive::Owner;
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{Display, FlexDirection};
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
/// The world is the pose the *game* owns. Nothing here writes it to the audio
/// engine — that is the clock's job, once a frame. See
/// [`pinion_audio::AudioWorldClock`].
struct AudioRig {
    /// Shared with the frame clock: two control-thread drivers, one queue.
    controller: SharedController,
    /// Owns the live stream — dropping the rig stops the device. In a `RefCell`
    /// because the output is **switchable at runtime** (see `set_device`).
    out: RefCell<CpalOutput>,
    /// The game-side state the clock carries to the audio thread — `pinion-audio`'s
    /// own, not a bespoke copy: the pending/retry/sequence protocol is the subtle
    /// part, and every binding that re-rolled it would get it wrong identically.
    world: Rc<AudioWorld>,
}

impl std::fmt::Debug for AudioRig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioRig")
            .field("device", &self.out.borrow().device_name())
            .field("listener", &self.world.listener().position)
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
            world: Rc::new(AudioWorld::new()),
        }
    }

    /// **Switch the output device at runtime.** Open `name`, hand the new renderer
    /// to its callback, and swap both halves behind the shared handles — the RPC
    /// surface and the frame clock keep the same `Rc`s and never notice.
    ///
    /// An earlier round refused to build this, justifying it by claiming a switch
    /// "means rebuilding the renderer, controller **and clip registry** and
    /// re-homing live voices". Two thirds of that was invented:
    ///
    /// - **Clips need nothing.** They are `Arc<AudioClip>` held by the External, and
    ///   every voice *resamples* its clip to the engine's rate — so a device at a
    ///   different rate reuses them untouched.
    /// - **The controller swap is one assignment**, because it lives behind a
    ///   [`SharedController`] — which the very round that wrote the excuse shipped.
    ///
    /// The third is true: **live voices do not survive.** The old engine dies with
    /// the old stream. That is what every DAW and game engine does on a device
    /// switch, and saying so is better than pretending the feature is expensive.
    ///
    /// On failure the CURRENT device keeps playing — a bad name must never leave the
    /// app silent.
    fn set_device(&self, name: &str) -> Result<(), CpalError> {
        let (controller, out) = CpalOutput::start_on(name, RING_CAP, MAX_VOICES)?;
        // Swap behind the Rc: every holder of the shared controller (the External,
        // the clock) now drives the new device's queue.
        *self.controller.borrow_mut() = controller;
        // Dropping the old `CpalOutput` here stops the old stream.
        *self.out.borrow_mut() = out;
        // The new engine has never heard of the world, so re-arm the push: the next
        // frame carries the listener onto the new device.
        self.world.move_listener(self.world.listener().position);
        Ok(())
    }
}

/// The shared rig for this `Owner` scope.
///
/// # Panics
///
/// Panics if called outside an active `Owner` scope (`create_external` and `view`
/// both run inside one).
fn audio_rig() -> Rc<AudioRig> {
    Owner::current()
        .expect("audio_rig requires an active Owner scope")
        .cache(RIG_KEY, AudioRig::open)
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
    /// the SAME controller the per-frame `AudioWorldClock` does.
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
            "camera" => Some(IntrospectValue::json(&self.rig.world.listener().position)),
            "frame_ticks" => Some(IntrospectValue::Int(
                i64::try_from(self.rig.world.ticks()).unwrap_or(i64::MAX),
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
        // …and the same for emitters: `set_voice_position` would race the frame
        // clock exactly as `set_listener` did. On this binding the world owns ALL
        // spatial state; `set_emitter` is its verb.
        if path == "set_listener" || path == "set_voice_position" {
            return Err(InvokeError::Rejected);
        }
        // ★ Move an EMITTER — the operation a game performs hundreds of times a
        // frame (one listener, many sounds). World-only, like `set_camera`: the
        // frame carries it.
        if path == "set_emitter" {
            let IntrospectValue::Json(v) = args else {
                return Err(InvokeError::TypeMismatch);
            };
            let (id, position) = parse_emitter(&v).ok_or(InvokeError::TypeMismatch)?;
            self.rig.world.set_emitter(id, position);
            return Ok(IntrospectValue::Null);
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
            self.rig.world.move_listener(pos);
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
        let _clock = use_audio_world_clock(CLOCK_KEY, rig.controller.clone(), rig.world.clone());

        // ★ Reading the camera Signal HERE is what closes the loop, and it is not
        // decoration. `Signal::set` notifies its *observers*, and a view acquires
        // that subscription by calling `get()` inside the Owner scope. Without
        // this read, moving the camera marks nothing dirty, the shell never
        // paints, the animation driver never ticks, and the listener never
        // follows — measured, before this line existed. The view depending on the
        // world pose is exactly what makes a world change schedule a frame.
        // ★ READING the world here is what arms the frame. `Signal::set` notifies
        // observers, and a view subscribes by reading inside the Owner scope —
        // without this read nothing paints, the clock never ticks, and the pose
        // never lands (measured). See `pinion_audio::world`, point 1.
        let camera = rig.world.listener().position;

        let mut children: Vec<Scene> = Vec::new();

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
            // R1345 §5.21 — no authored `rect`: the column below places each
            // row and the text measure sizes it, so a long line wraps rather
            // than truncating.
            children.push(Scene::Text(TextNode::new(line, Rect::default())));
        }

        // R55.G.17 — the paint scene carries a node tagged `tag()` so AI-side
        // path routing / `rect_for_tag` resolve.
        let mut root = ContainerNode::new(children).with_tag(TAG);
        // R1345 §5.21 — a padded column with a uniform row gap. The pre-R1345
        // view authored `rect` from a running `y` cursor, none of which reached
        // a pixel: `compute_layout` overwrites `rect` (it is an OUTPUT), so the
        // rows painted flush at x=0 with none of the intended spacing.
        //
        // The old `y += 26` pitch was 16px of authored row + a 10px gap, so
        // `gap: 10` reproduces the GAP — but not the pitch: a measured row is
        // 24px, so the real pitch is 34. That is why the window grew.
        root.layout.display = Display::Flex;
        root.layout.flex_direction = FlexDirection::Column;
        root.layout.gap = 10;
        root.layout.padding = Rect::new(16, 16, 16, 16);
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
            // R1345 §5.21 — 7 rows at a 24px measured line + a 10px gap + 16px
            // padding top and bottom. The pre-R1345 window (220) assumed the
            // authored 16px rows that never reached a pixel.
            height: 300,
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
    //! (The clock + world protocol is tested in `pinion_audio::world`, where it
    //! now lives and where it needs no sound card.)
    //!
    //! R1345 §5.21 — this is why the R1345 column migration below has **no**
    //! geometry test here, unlike its `hello-audio-rt` sibling: `view()` reads
    //! `audio_rig()`, which opens a real output (and `process::exit(1)`s if it
    //! cannot), so any test that lays out this view would open a device — a
    //! zero-flake violation on a host without a sound card, and an abort of the
    //! whole test binary rather than a test failure. `tools/demos/
    //! hello_audio_device.py` covers the rendered window against a real device.
    use super::{DEVICE_FIELDS, DeviceChoice, RT_EXTERNAL_FIELDS, SCHEMA_FIELDS, resolve_device};

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
