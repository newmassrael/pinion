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
//! callback, and hosts the **shipping** [`AudioControllerExternal`] — verbatim, no
//! step-verb — on the `pinion_shell` stdin/stdout JSON-RPC surface.
//! `tools/demos/hello_audio_device.py` then plays, re-gains, and stops a live
//! voice over the real wire while the callback is running underneath.
//!
//! ## Making no sound: the silent card
//!
//! A device proof that blasts a tone out of the developer's speakers (and cannot
//! run in CI at all) is not much of a proof. So this opens a **silent virtual
//! card** — Linux ALSA's `snd-dummy`, a timer-paced device with no output:
//!
//! ```text
//! sudo modprobe snd-dummy                       # card "Dummy" appears
//! PINION_AUDIO_DEVICE=Dummy cargo run -p hello-audio-device
//! ```
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
//! - **Not the game-loop subsystem tick.** Audio here is clocked by the *device*,
//!   not by a fixed-timestep frame. A general "per-frame subsystem" seam that the
//!   game loop fans out to (audio joining physics / AI) remains unbuilt and
//!   remains Phase-C, exactly as `hello-audio-rt` said; this round does not
//!   change that.

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

/// Pick the device to open from `requested` against the host's `names`.
///
/// Exact match wins; otherwise the **first case-insensitive substring match in
/// host order**, so `PINION_AUDIO_DEVICE=Dummy` resolves the ALSA card's full
/// `hw:CARD=Dummy,DEV=0` name without the caller spelling it out. This fuzziness
/// is deliberately *the binding's* policy, not the substrate's:
/// [`CpalOutput::start_on`] matches exactly and refuses a miss, so a typo can
/// never silently fall back to the speakers.
fn resolve_device(requested: &str, names: &[String]) -> Option<String> {
    if let Some(exact) = names.iter().find(|name| *name == requested) {
        return Some(exact.clone());
    }
    let needle = requested.to_lowercase();
    names
        .iter()
        .find(|name| name.to_lowercase().contains(&needle))
        .cloned()
}

/// Open the output device, failing **loudly** if the requested one is absent.
///
/// A silent fallback here would be the worst outcome: the demo would appear to
/// pass while the callback never ran (or ran on the speakers). So an absent
/// device aborts with the list of what the host *does* offer.
fn open_output() -> (pinion_audio::AudioController, CpalOutput) {
    let requested = std::env::var(DEVICE_ENV).ok();

    let opened = match &requested {
        None => CpalOutput::start_default(RING_CAP, MAX_VOICES),
        Some(want) => {
            let names = CpalOutput::output_device_names().unwrap_or_else(|e| {
                eprintln!("hello-audio-device: cannot enumerate output devices: {e}");
                std::process::exit(1);
            });
            let Some(name) = resolve_device(want, &names) else {
                eprintln!(
                    "hello-audio-device: no output device matches {DEVICE_ENV}={want:?}.\n\
                     available: {names:?}\n\
                     hint: the silent test card comes from `sudo modprobe snd-dummy`."
                );
                std::process::exit(1);
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
/// Every wire verb delegates to the inner RT External **verbatim** — the contract
/// this binary proves is exactly [`AudioControllerExternal`]'s, with no harness
/// verb bolted on. Unlike `hello-audio-rt` there is no `render` step-verb,
/// because there is nothing to step: the [`pinion_audio::AudioRenderer`] lives in cpal's
/// callback thread and is pumped by the device clock. The only additions are the
/// read-only device facts ([`DEVICE_FIELDS`]).
struct DeviceAudioExternal {
    /// The §2 #7 RT surface — the sole home of the wire contract.
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
            .and_then(|v| match v {
                IntrospectValue::Int(n) => Some(n),
                _ => None,
            })
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
    use super::{DEVICE_FIELDS, RT_EXTERNAL_FIELDS, SCHEMA_FIELDS, resolve_device};

    fn names() -> Vec<String> {
        [
            "sysdefault:CARD=PCH",
            "hw:CARD=Dummy,DEV=0",
            "plughw:CARD=Dummy,DEV=0",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[test]
    fn exact_name_wins_over_a_substring_match() {
        // "plughw:CARD=Dummy,DEV=0" is also a substring match for itself, but an
        // exact hit must never be passed over for an earlier fuzzy one.
        let got = resolve_device("plughw:CARD=Dummy,DEV=0", &names());
        assert_eq!(got.as_deref(), Some("plughw:CARD=Dummy,DEV=0"));
    }

    #[test]
    fn substring_resolves_the_first_match_in_host_order() {
        // The demo/CI contract: `PINION_AUDIO_DEVICE=Dummy` finds the card
        // without spelling out the full ALSA name, deterministically.
        let got = resolve_device("Dummy", &names());
        assert_eq!(got.as_deref(), Some("hw:CARD=Dummy,DEV=0"));
        // …and case-insensitively.
        assert_eq!(resolve_device("dummy", &names()), got);
    }

    #[test]
    fn an_absent_device_resolves_to_nothing_not_to_the_first_device() {
        // The failure that matters: a typo'd name must NOT quietly select the
        // speakers. `None` is what makes the caller abort loudly.
        assert_eq!(resolve_device("Loopback", &names()), None);
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
}
