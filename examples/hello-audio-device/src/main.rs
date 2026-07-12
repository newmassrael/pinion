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
//!   it to the card's sample format and channel layout — so no introspection read,
//!   and therefore no assertion in the demo, can see a bug in that conversion. It
//!   is covered by unit tests instead (`write_frames` in `pinion-audio`'s
//!   `device.rs`). Capturing what the card actually *received* (an `snd-aloop`
//!   loopback) is a further step this binary does not take.
//!
//! ## Where the per-frame audio seam really stands (do not mislabel this)
//!
//! The mixer is clocked by the **device**, and that is correct and permanent — no
//! engine frame-clocks a mixer; the card pulls when it pulls.
//!
//! What a game *also* wants is per-frame **control-side** audio work: the listener
//! follows the camera, emitters follow entities, distant voices get culled. It
//! would be wrong to file that under "Phase C": pinion already has a retained
//! per-frame clock — [`pinion_core::animation::Tickable`] +
//! `Owner::register_animation_once` + `Owner::tick_animations`, fanned out from the
//! shell each paint — and it already carries a *non-animation* subsystem
//! (`pinion_narrative`'s `VnClock`). A control-side audio ticker could ride that
//! **today**, holding an `AudioController` and pushing over the same lock-free
//! ring; nothing new is needed. This binary simply does not do it, because it is
//! an RPC proof of the device path, not a game.
//!
//! What is *genuinely* still absent is narrower: the fixed-timestep game loop's own
//! `Send` subsystem registry (audio alongside physics / AI, off the UI thread).
//! That is the real Phase-C boundary — not "per-frame audio", which is buildable
//! now.

use std::sync::Arc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_audio::{AudioClip, AudioControllerExternal, CpalOutput, RT_EXTERNAL_FIELDS};
use pinion_core::external::{
    External, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
};
use pinion_core::intent::Intent;
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

/// A looping tone, long enough that it stays live while the demo polls.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sine_clip(rate: u32, freq: f32, secs: f32) -> Arc<AudioClip> {
    let frames = (secs * rate as f32) as usize;
    let samples: Vec<f32> = (0..frames)
        .map(|i| {
            let t = i as f32 / rate as f32;
            (t * freq * std::f32::consts::TAU).sin() * 0.9
        })
        .collect();
    AudioClip::new(rate, 1, samples).shared()
}

/// The device facts this binding adds on top of the RT surface — *which* output
/// the audio thread is actually feeding. Worth having on the wire: an agent (or a
/// player's settings panel) asking "where is my sound going?" should not have to
/// guess, and this demo asserts it opened the **silent** card rather than the
/// speakers.
const DEVICE_FIELDS: &[(&str, &str)] = &[
    ("device", "text"),
    ("sample_rate", "int"),
    ("channels", "int"),
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
/// device-agnostic controller cannot know: the read-only [`DEVICE_FIELDS`]
/// (`device` / `sample_rate` / `channels`). So the wire contract's home is the
/// inner External **plus** those three fields — see [`compose_schema`].
struct DeviceAudioExternal {
    /// The §2 #7 RT surface — the home of every *audio* verb and read.
    inner: AudioControllerExternal,
    /// The live stream. Owning it here is the lifecycle: dropping this External
    /// stops the device. The [`pinion_audio::AudioRenderer`] it was built with now lives on the
    /// audio thread, so it is not reachable from this side at all — which is the
    /// whole point of the lock-free split.
    out: CpalOutput,
    /// §5.20 intents forwarded from the inner controller (e.g. `audio.play`).
    pending_intents: Vec<Intent>,
}

impl std::fmt::Debug for DeviceAudioExternal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceAudioExternal")
            .field("inner", &self.inner)
            .field("device", &self.out.device_name())
            .finish_non_exhaustive()
    }
}

impl DeviceAudioExternal {
    /// Open the device, start its callback thread, and register the demo clips.
    fn open() -> Self {
        let (controller, out) = open_output();
        // The engine renders at the device's rate, so author the clips there too
        // (per-voice resampling would handle a mismatch; matching is just tidier).
        let rate = out.sample_rate();
        eprintln!(
            "hello-audio-device: {:?} @ {rate} Hz, {} channel(s) — callback thread live",
            out.device_name(),
            out.channels()
        );
        let inner = AudioControllerExternal::new(controller)
            // Looping, so it stays live for as long as the demo polls it.
            .with_clip("tone", sine_clip(rate, 440.0, 1.0))
            .with_clip("bell", sine_clip(rate, 880.0, 1.0));
        Self {
            inner,
            out,
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
            "device" => Some(IntrospectValue::Text(self.out.device_name().to_owned())),
            "sample_rate" => Some(IntrospectValue::Int(i64::from(self.out.sample_rate()))),
            "channels" => Some(IntrospectValue::Int(i64::from(self.out.channels()))),
            // Everything else is the RT surface's, read lock-free off the
            // snapshot the audio thread publishes each callback.
            _ => self.inner.query(path),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        if DEVICE_FIELDS.iter().any(|(name, _)| *name == path) {
            // Declared, but a device fact is an observation, not a slot: you
            // change the output by opening a different one, not by writing here.
            return Err(InterveneError::ReadOnly);
        }
        self.inner.intervene(path, value)
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        // Verbatim: `play` / `stop` / `stop_all` / `set_master_gain` /
        // `set_voice_*` / `set_listener` / `set_attenuation` / `set_voice_policy`
        // all queue onto the lock-free ring the *live callback thread* is
        // draining. No step-verb — the device clock is the pump.
        let result = self.inner.invoke(path, args);
        let Self {
            inner,
            pending_intents,
            ..
        } = &mut *self;
        inner.drain_intents(&mut |intent| pending_intents.push(intent));
        result
    }
}

pinion_core::intent_query_external_impl!(DeviceAudioExternal);

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
        let mut children: Vec<Scene> = Vec::new();
        let mut y = 16u32;

        for line in [
            format!("hello-audio-device — real cpal callback + RPC ({state} live voice(s))"),
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

    #[test]
    fn schema_is_the_rt_surface_plus_the_device_fields() {
        // Guards the const-composition: the RT fields come first, in order, and
        // the device fields are appended — no drift, nothing dropped.
        assert_eq!(SCHEMA_FIELDS.len(), RT_EXTERNAL_FIELDS.len() + 3);
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
