// Example bindings tolerate looser doc-markdown lints than substrate crates;
// the narrative carries many proper-noun identifiers (AudioController, cpal,
// JSON-RPC, …).
#![allow(clippy::doc_markdown)]

//! `hello-audio-rt` — the **real-time device audio path** (§5.54) brought onto
//! the real JSON-RPC wire.
//!
//! ## What this closes
//!
//! The audio arc (R1274–R1292) built the whole first-class audio subsystem
//! around the §2 promises: the graph is scene-as-data (#7) and RPC is the AI
//! primary path (#2). But the path that actually *ships to a sound card* — the
//! lock-free real-time control model ([`pinion_audio::AudioController`] +
//! [`AudioRenderer`], driven from cpal on real hardware, R1282) — had only ever
//! been exercised by **in-process crate tests calling the trait methods**. It
//! was constructed only inside `#[cfg(test)]` and wired into no binary, so it
//! was unreachable over the real server even in principle. Crate tests that call
//! `ext.query` / `ext.invoke` in-process bypass the entire wire — the
//! `/external/` path split, JSON marshalling, and error-code mapping — which is
//! exactly what §2 #2 exists to guarantee. So the RT audio path was **0 %
//! wire-proven**.
//!
//! This binary is that proof. It hosts the RT [`AudioControllerExternal`] as its
//! primary widget on the `pinion_shell` GUI shell — which carries the built-in
//! stdin JSON-RPC reader (reply → stdout) — so `tools/demos/hello_audio_rt.py`
//! drives the *shipping* audio path over the same wire an AI agent uses:
//! `play` / `stop` / `set_voice_position` / `set_master_gain` / `set_listener` /
//! `set_voice_policy`, and reads `voices` / `rejected` / `stolen` back — all as
//! real `scene/invoke` + `scene/query` requests.
//!
//! ## Two surfaces on one wire
//!
//! It also hosts the *retained single-thread* [`AudioEngineExternal`] as an
//! extra External (`/engine/external/...`), driven by
//! `tools/demos/hello_audio_engine.py`. That path is `hello-audio`'s surface,
//! but `hello-audio` runs on `pinion_tui` and paints to stdout — the same
//! channel the RPC server replies on — so it was never wire-demoed; this is the
//! GUI variant that closes it. The single-thread contract differs in one
//! substantial way worth its own proof: its *writes* go through `intervene`
//! (`master_gain` / `listener` / `attenuation` /
//! `voice.<id>.{gain,pan,position}`), whereas the RT path is invoke-only — so
//! the two demos together cover both the `invoke`-write and the `intervene`-write
//! §2 #2 surfaces of the audio subsystem.
//!
//! ## Why the GUI shell, not the TUI
//!
//! The single-thread `hello-audio` runs on `pinion_tui`, which paints the
//! terminal to **stdout** — the same channel the JSON-RPC server replies on. A
//! TUI binary therefore cannot be cleanly driven over stdin/stdout RPC (the
//! escape-sequence paint stream corrupts the reply framing). `pinion_shell`
//! paints to a GPU window instead, leaving stdout a clean RPC channel; under
//! `PINION_HIDDEN_WINDOW=1` (the harness default) the window is created unmapped,
//! so the full real pipeline runs headless.
//!
//! ## The `render` step-verb — an explicit, honestly-scoped harness pump
//!
//! On real hardware the audio callback thread pulls [`AudioRenderer::render`]
//! continuously; a queued `play` takes effect on the next callback, and the
//! control thread reads the snapshot the callback publishes. Reproducing a
//! *background* audio thread in a demo would mean sleeps and polling — the exact
//! [[zero-flake-policy]] hazard. So [`RtAudioDemoExternal`] co-locates the
//! audio-thread [`AudioRenderer`] with the control-thread
//! [`AudioControllerExternal`] on the one dispatch thread and exposes a
//! deterministic **`render` step-verb**: `invoke render N` pumps exactly `N`
//! render blocks synchronously, then returns. This is the wire form of what the
//! crate tests already do (interleave `renderer.render(&mut out)` with
//! `ext.query` / `ext.invoke`) — a queued command is applied and its snapshot
//! published by the very next `render`, with zero wall-clock dependence.
//!
//! **What `render` is NOT, stated plainly** (this verb is the round's one bit of
//! throwaway scaffolding, so its scope must be exact): the north-star pump for
//! audio is the fixed-timestep **game-loop tick** — a real engine advances
//! audio, physics and AI from one clock. pinion's frame-step for that already
//! ships and is RPC-driven and deterministic today: `scene/set_fps {fps:0}`
//! pauses a window's loop and `scene/tick {dt}` advances it one fixed step
//! (R724 / R829 / R831). The reason this harness does NOT drive audio through
//! `scene/tick` is *not* that the frame-step is unbuilt (it is built) — it is
//! that the tick fan-out reaches only [`Scene::ImmediateModeNode`] paint drivers
//! (see `Scene::tick_immediate_mode`), and audio is not a paint node (no
//! viewport / layout / pointer). Hanging audio off an `ImmediateModeNode` to
//! borrow the verb would be glue-reuse of a paint abstraction, not contract
//! reuse; the *correct* structure is a general "per-frame subsystem" seam the
//! game-loop tick fans out to (audio joining physics / AI), and building that
//! well is Phase-C architecture that needs forcing consumers beyond audio — so
//! it is deliberately deferred, not faked here. `render` therefore proves only
//! the audio **control surface** over the wire; pumping audio from the game-loop
//! tick is the acknowledged Phase-C follow-up, not something this round did.
//!
//! The retained engine gets the *same* `render` step-verb ([`EngineDemoExternal`]).
//! It plays synchronously (a `play` shows in `voice_count` with no render), but it
//! *reaps* finished / stopped voices only in [`AudioEngine::render`] — reaping is
//! the pull loop's job, not part of the §2 #7 surface — so its `render` verb steps
//! the mixer to reap, and `stop` then `render` drops `voice_count` exactly as a
//! real capture / device loop would. Both surfaces therefore demonstrate the full
//! play → stop → reap lifecycle over the wire.
//!
//! This is a **headless RPC harness**, not a real-time interactive audio toy: it
//! opens no audio device and makes no sound — deliberately (it declares no `cpal`
//! dependency and pumps the mixer through the `render` verb instead of a device
//! callback), NOT because the machine lacks a device. The real cpal *push* path
//! is the `pinion-audio` `cpal-backend` feature, whose device output was
//! hardware-verified in R1282; note that is a *different* proof from this one —
//! neither R1282 nor this harness exercises a real cpal callback thread and the
//! stdin-RPC control thread concurrently, so that lock-free-snapshot-under-a-live-
//! callback path is not proven here. That is a genuine open item — but it is
//! *buildable* (poll the settled snapshot with `wait_until`, exactly like every
//! other async RPC path; the coarser cost is approximate not flaky assertions),
//! and its deterministic core is already covered (the lock-free protocol is
//! verified by the orchestrated cross-thread tests in
//! `crates/pinion-audio/tests/realtime_channel.rs`). The real reason it is
//! deferred is that a real callback needs an audio device, which CI lacks (so it
//! would be local-hardware-gated like R1282's `device_out`, or need a virtual
//! audio device wired into CI), and the marginal value over the verified protocol
//! is real-concurrency integration confidence. What this round proves is the RT
//! control surface end-to-end over the wire, which is the one thing the audio arc
//! never had.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_audio::{
    AudioClip, AudioControllerExternal, AudioEngine, AudioEngineExternal, AudioRenderer,
    decode_compressed, realtime_channel,
};
use pinion_core::external::{
    External, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    forward_intents,
};
use pinion_core::intent::Intent;
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::{Frame, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};

// pinion-forge codegen output. Defines `pub struct HelloAudioRtRenderer`
// + `pub enum HelloAudioRtRendererError` + async `new` + sync `render` /
// `resize`. The emit template uses fully-qualified `::vello::*` paths so the
// include is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// Bridge the inherent renderer methods into the `pinion_shell::VelloRenderer`
// trait so the generic `AppShell<V>` can construct + render + resize it.
vello_renderer_impl!(HelloAudioRtRenderer, HelloAudioRtRendererError);

/// Engine output rate (Hz).
const RATE: u32 = 48_000;
/// Command-ring capacity (control → audio thread). Comfortably deeper than any
/// single demo step's burst of queued commands.
const RING_CAP: usize = 64;
/// Voice-pool bound. Small on purpose so the demo can drive the pool to
/// starvation and observe `rejected` / `stolen` over the wire.
const MAX_VOICES: usize = 4;
/// Interleaved stereo samples pumped per `render` block (256 frames).
const BLOCK_SAMPLES: usize = 512;
/// The primary (RT device path) External / paint-focus tag.
const TAG: &str = "audio_rt";
/// The extra (retained single-thread engine) External tag, addressed over the
/// wire at `/engine/external/...`.
const ENGINE_TAG: &str = "engine";

/// A real FLAC asset, compiled in and decoded at startup — so the RT device
/// path is proven over the wire with a real game-format asset, not only
/// synthesized PCM (dogfooding the §5.54 compressed-decode path *and* the RT
/// path together, end to end).
const CHIME_FLAC: &[u8] = include_bytes!("../assets/chime.flac");

/// Decode the bundled FLAC chime to PCM. On the (fixture-guaranteed-absent)
/// decode failure the demo just omits the clip rather than panicking.
fn chime_clip() -> Option<Arc<AudioClip>> {
    match decode_compressed(CHIME_FLAC) {
        Ok(clip) => Some(clip.shared()),
        Err(e) => {
            eprintln!("hello-audio-rt: chime.flac failed to decode: {e}");
            None
        }
    }
}

/// Upper bound on a single `render` invoke's block count. The pump is
/// synchronous on the dispatch thread, so an unbounded count would freeze both
/// paint and RPC; `render` is a fine-grained step verb (demos pump 1 block at a
/// time), so this ceiling only guards a fat-fingered / hostile `render 1e11`.
const MAX_RENDER_BLOCKS: u64 = 4096;

/// Parse the `render` step-verb's block count: `Null` → 1 block (the
/// cpal-callback-per-frame default), an `Int` → that many, clamped to
/// `[0, MAX_RENDER_BLOCKS]` (a negative → 0, an over-large → the ceiling),
/// anything else → a type error. Shared by both demo wrappers so the one
/// `render` verb contract has a single home.
fn parse_render_blocks(args: &IntrospectValue) -> Result<u64, InvokeError> {
    match args {
        IntrospectValue::Null => Ok(1),
        IntrospectValue::Int(n) => Ok(u64::try_from(*n).unwrap_or(0).min(MAX_RENDER_BLOCKS)),
        _ => Err(InvokeError::TypeMismatch),
    }
}

/// Demo-glue [`External`] co-locating the audio-thread [`AudioRenderer`] with
/// the control-thread [`AudioControllerExternal`] on one dispatch thread, plus a
/// scratch render buffer.
///
/// Every `query` / `intervene` / `invoke` verb the RT surface declares
/// delegates verbatim to the inner controller-External — so the wire contract
/// this binary proves is *exactly* [`AudioControllerExternal`]'s, untouched. The
/// wrapper adds one verb the real cpal callback thread supplies implicitly: the
/// deterministic **`render` step-verb** (see the module docs), which pumps N
/// blocks synchronously so a queued command's effect is observable on the next
/// `query` with no background thread and no sleeps (ZERO-FLAKE).
struct RtAudioDemoExternal {
    /// The §2 #7 RT surface — the sole home of the wire contract.
    inner: AudioControllerExternal,
    /// The audio-thread mixer handle, pumped by the `render` step-verb.
    renderer: AudioRenderer,
    /// Caller-owned render scratch (the cpal callback's `&mut [f32]` analogue).
    buf: Vec<f32>,
    /// §5.20 intents forwarded from the inner controller (e.g. `audio.play`).
    pending_intents: Vec<Intent>,
}

impl std::fmt::Debug for RtAudioDemoExternal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RtAudioDemoExternal")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl RtAudioDemoExternal {
    /// Build the RT channel, register the demo clips, and co-locate the
    /// renderer with the controller-External. No thread is spawned — the audio
    /// thread is *this* thread, stepped by the `render` verb.
    fn new() -> Self {
        let (controller, renderer) = realtime_channel(AudioEngine::new(RATE), RING_CAP, MAX_VOICES);
        let mut inner = AudioControllerExternal::new(controller)
            // Long tones stay live across render blocks so `voices` reads see
            // them; `blip` is a 2-frame one-shot that finishes in one block.
            .with_clip("bell", AudioClip::sine(RATE, 880.0, 1.0, 0.9).shared())
            .with_clip("waves", AudioClip::sine(RATE, 220.0, 1.0, 0.9).shared())
            .with_clip("blip", AudioClip::new(RATE, 1, vec![0.9; 2]).shared());
        // A real decoded FLAC asset over the RT path — §5.54 decode + the
        // shipping RT mixer proven together over the wire.
        if let Some(chime) = chime_clip() {
            inner = inner.with_clip("chime", chime);
        }
        Self {
            inner,
            renderer,
            buf: vec![0.0; BLOCK_SAMPLES],
            pending_intents: Vec::new(),
        }
    }
}

impl ExternalIntrospect for RtAudioDemoExternal {
    fn schema(&self) -> IntrospectSchema {
        // The wire contract is the inner RT surface's, verbatim.
        self.inner.schema()
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        self.inner.query(path)
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        self.inner.intervene(path, value)
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        if path == "render" {
            // Pump exactly N render blocks (default 1) synchronously — the wire
            // form of the crate tests' `renderer.render(&mut out)` between
            // `invoke`/`query`. A queued command is applied and its snapshot
            // published by the very next block. No background thread → the demo
            // is deterministic (ZERO-FLAKE).
            let blocks = parse_render_blocks(&args)?;
            for _ in 0..blocks {
                // `renderer` and `buf` are disjoint fields — a normal split borrow.
                self.renderer.render(&mut self.buf);
            }
            return Ok(IntrospectValue::Int(
                i64::try_from(blocks).unwrap_or(i64::MAX),
            ));
        }
        // Every other verb is the inner RT surface's, verbatim. Forward any
        // intent it queued (e.g. `audio.play`) so the wrapper's §5.20 drain
        // surfaces it exactly like the single-thread engine does.
        let result = self.inner.invoke(path, args);
        forward_intents(&mut self.inner, &mut self.pending_intents);
        result
    }
}

// Paints nothing (the binding's `view` is the paint scene), RPC backend only,
// draining `self.pending_intents` — the same skeleton both audio Externals use.
pinion_core::intent_query_external_impl!(RtAudioDemoExternal);

/// Demo-glue [`External`] over the retained single-thread [`AudioEngineExternal`]
/// — the engine peer of [`RtAudioDemoExternal`].
///
/// Every declared verb delegates verbatim to the inner engine-External (so the
/// wire contract is *exactly* [`AudioEngineExternal`]'s), and the wrapper adds
/// the same deterministic **`render` step-verb**. The retained engine plays
/// synchronously — a `play` shows in `voice_count` with no render — but it *reaps*
/// finished / stopped voices only in [`AudioEngine::render`], and rendering is
/// the backend's job, not part of the §2 #7 introspection surface. So the
/// wrapper holds a second clone of the same `Rc<RefCell<AudioEngine>>` the inner
/// External drives and exposes `render` to step it: `stop` then `render N` reaps,
/// exactly as a real capture / device pull loop would — the symmetric peer of the
/// RT `render` verb, so both surfaces demonstrate the full play → stop → reap
/// lifecycle over the wire.
struct EngineDemoExternal {
    /// The §2 #7 retained-engine surface — the sole home of the wire contract.
    inner: AudioEngineExternal,
    /// A second handle to the same engine the inner External drives, used only
    /// by the `render` step-verb to pull the mixer (produce + reap).
    engine: Rc<RefCell<AudioEngine>>,
    /// Caller-owned render scratch (the pull loop's `&mut [f32]` analogue).
    buf: Vec<f32>,
    /// §5.20 intents forwarded from the inner engine (e.g. `audio.play`).
    pending_intents: Vec<Intent>,
}

impl std::fmt::Debug for EngineDemoExternal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineDemoExternal")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl EngineDemoExternal {
    /// Build the shared engine once, hand one clone to the introspectable
    /// External and keep one for the `render` step-verb, and register the demo
    /// clips.
    fn new() -> Self {
        let engine = Rc::new(RefCell::new(AudioEngine::new(RATE)));
        let inner = AudioEngineExternal::new(engine.clone())
            .with_clip("bell", AudioClip::sine(RATE, 880.0, 0.4, 0.9).shared())
            .with_clip("waves", AudioClip::sine(RATE, 220.0, 0.8, 0.9).shared());
        Self {
            inner,
            engine,
            buf: vec![0.0; BLOCK_SAMPLES],
            pending_intents: Vec::new(),
        }
    }
}

impl ExternalIntrospect for EngineDemoExternal {
    fn schema(&self) -> IntrospectSchema {
        self.inner.schema()
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        self.inner.query(path)
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        self.inner.intervene(path, value)
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        if path == "render" {
            // Step the retained mixer N blocks: produces PCM and reaps finished /
            // stopped voices (`AudioEngine::render` -> `reap_finished`), exactly
            // as a capture / device pull loop would. Synchronous, no thread.
            let blocks = parse_render_blocks(&args)?;
            for _ in 0..blocks {
                self.engine.borrow_mut().render(&mut self.buf);
            }
            return Ok(IntrospectValue::Int(
                i64::try_from(blocks).unwrap_or(i64::MAX),
            ));
        }
        let result = self.inner.invoke(path, args);
        forward_intents(&mut self.inner, &mut self.pending_intents);
        result
    }
}

pinion_core::intent_query_external_impl!(EngineDemoExternal);

/// The binding unit type.
struct HelloAudioRt;

impl WidgetCore for HelloAudioRt {
    /// Live voice count — read back so the shell repaints when it changes.
    type State = u16;
    /// No keyboard affordances: this is an RPC harness, so there is no typed
    /// widget event (the honest alternative to advertising keys that, without a
    /// running audio thread, would appear to do nothing).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(RtAudioDemoExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // The retained single-thread engine on the same wire, addressed at
        // `/engine/external/...` (wrapped by [`EngineDemoExternal`] for its own
        // `render` step-verb). This is the "GUI variant" the debt anticipated for
        // the single-thread path (the TUI `hello-audio` paints to stdout, which
        // collides with RPC-on-stdout). Its wire contract differs from the RT
        // primary's in one substantial way worth proving: its WRITES go through
        // `intervene` (`master_gain` / `listener` / `attenuation` /
        // `voice.<id>.{gain,pan,position}`), not `invoke` — the retained engine
        // has no lock-free split, so the §2 #7 read twins are directly writable.
        vec![ExtraExternal::new(
            ENGINE_TAG,
            Box::new(EngineDemoExternal::new()),
        )]
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> u16 {
        // Shape-agnostic (bare External or Container([primary, …])) via
        // `primary_external`, so this keeps reading the RT surface if the
        // binding ever grows sibling Externals.
        scene
            .primary_external()
            .and_then(|node| node.handle.introspect())
            .and_then(|intro| intro.query("voice_count"))
            .and_then(|v| v.as_i64())
            .map_or(0, |n| {
                u16::try_from(n.clamp(0, i64::from(u16::MAX))).unwrap_or(0)
            })
    }

    fn view(state: u16, _frame: &Frame) -> Scene {
        let mut children: Vec<Scene> = Vec::new();
        let mut y = 16u32;

        children.push(text_row(
            format!("hello-audio-rt — real-time device audio over RPC ({state} live voice(s))"),
            y,
        ));
        y += 30;
        children.push(text_row(
            "The shipping RT path (AudioController + AudioRenderer) on the wire.".to_string(),
            y,
        ));
        y += 24;
        children.push(text_row(
            "Driven over JSON-RPC — no keyboard. `invoke render N` steps the mixer.".to_string(),
            y,
        ));
        y += 30;
        children.push(text_row(
            "query: voice_count / max_voices / peak / frames_rendered / rejected / stolen / voices"
                .to_string(),
            y,
        ));
        y += 22;
        children.push(text_row(
            "     / listener / attenuation / voice_policy".to_string(),
            y,
        ));
        y += 22;
        children.push(text_row(
            "invoke: play / stop / stop_all / set_master_gain / set_voice_{gain,pan,position}"
                .to_string(),
            y,
        ));
        y += 22;
        children.push(text_row(
            "     / set_listener / set_attenuation / set_voice_policy / render".to_string(),
            y,
        ));
        y += 26;
        children.push(text_row(
            "Also: the retained single-thread engine at /engine/external/* (intervene-write)."
                .to_string(),
            y,
        ));
        y += 26;

        // R55.G.17 — the paint scene carries a node tagged `tag()` so AI-side
        // path-routing / `rect_for_tag` resolve, even though this harness drives
        // the RT surface through the `/external/` state-scene path.
        let mut root = ContainerNode::new(children).with_tag(TAG);
        root.rect = Rect::new(0, 0, 560, y + 16);
        Scene::Container(root)
    }

    fn event_name((): ()) -> &'static str {
        // No externally-drivable widget event — the RT surface is driven through
        // the introspect verbs, not the typed `send` channel.
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-audio-rt — real-time device audio over RPC (§5.54 / §2 #2)"
    }
}

fn text_row(content: String, y: u32) -> Scene {
    Scene::Text(TextNode::new(content, Rect::new(16, y, 540, 16)))
}

impl WidgetA11y for HelloAudioRt {
    fn access_node(state: &u16, focused: Option<&str>) -> Vec<AccessNode> {
        vec![
            AccessNode::new(TAG, AriaRole::List)
                .with_name(format!("real-time audio — {state} live voice(s)"))
                .with_focused(focused == Some(TAG)),
        ]
    }
}

impl WidgetView for HelloAudioRt {
    type Renderer = HelloAudioRtRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: 560,
            height: 320,
        }
    }
}

fn main() {
    pinion_shell::run::<HelloAudioRt>();
}

#[cfg(test)]
mod tests {
    //! In-process contract tests for [`RtAudioDemoExternal`] — the fast
    //! (no-subprocess, no-GPU) peer of `tools/demos/hello_audio_rt.py`. The demo
    //! proves the same contract over the real JSON-RPC wire; these lock the
    //! wrapper's own logic (the `render` step-verb + verbatim delegation) so a
    //! regression is caught in `cargo test` without spawning the shell.
    use super::{EngineDemoExternal, RtAudioDemoExternal};
    use pinion_core::external::{
        External, ExternalIntrospect, InterveneError, IntrospectValue, InvokeError,
    };

    #[test]
    fn render_step_verb_applies_queued_play_and_delegates_reads() {
        let mut ext = RtAudioDemoExternal::new();
        // A queued play is not applied until a render pumps the audio thread —
        // the whole point of the step-verb.
        let id = ext.invoke("play", IntrospectValue::Text("bell".to_string()));
        assert!(
            matches!(id, Ok(IntrospectValue::Int(1))),
            "control-minted id"
        );
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(0))
        ));
        // One render block applies it and publishes the snapshot the reads see.
        assert!(matches!(
            ext.invoke("render", IntrospectValue::Int(1)),
            Ok(IntrospectValue::Int(1))
        ));
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(1))
        ));
        assert!(matches!(
            ext.query("frames_rendered"),
            Some(IntrospectValue::Int(256))
        ));
        // The inner controller's `audio.play` intent is forwarded to the
        // wrapper's own §5.20 drain.
        assert!(ext.is_dirty(), "audio.play intent forwarded from the inner");
    }

    #[test]
    fn render_default_is_one_block_and_wrong_type_is_rejected() {
        let mut ext = RtAudioDemoExternal::new();
        // Null args -> one block (the cpal-callback-per-frame default).
        assert!(matches!(
            ext.invoke("render", IntrospectValue::Null),
            Ok(IntrospectValue::Int(1))
        ));
        assert!(matches!(
            ext.query("frames_rendered"),
            Some(IntrospectValue::Int(256))
        ));
        // A non-int render count is a type error, not a silent no-op.
        assert!(matches!(
            ext.invoke("render", IntrospectValue::Text("two".to_string())),
            Err(InvokeError::TypeMismatch)
        ));
    }

    #[test]
    fn unknown_and_read_only_paths_delegate_to_the_inner_rt_surface() {
        let mut ext = RtAudioDemoExternal::new();
        // A verb the RT surface does not define is the inner surface's UnknownPath.
        assert!(matches!(
            ext.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        ));
        // The RT surface is read-via-query, write-via-invoke: an intervene of a
        // declared read-only field is ReadOnly (delegated verbatim).
        assert!(matches!(
            ext.intervene("voice_count", IntrospectValue::Int(3)),
            Err(InterveneError::ReadOnly)
        ));
    }

    #[test]
    fn engine_render_verb_reaps_a_stopped_voice() {
        // The engine wrapper's distinctive path: a second `Rc<RefCell<AudioEngine>>`
        // clone stepped by `render` while the inner External holds the other clone.
        // A retained `play` is synchronous (no render needed); the wrapper's own
        // `render` verb reaps the stopped voice — the exact behaviour the wire demo
        // exercises, locked here without a subprocess.
        let mut ext = EngineDemoExternal::new();
        let id = match ext.invoke("play", IntrospectValue::Text("bell".to_string())) {
            Ok(IntrospectValue::Int(id)) => id,
            other => panic!("expected a minted id, got {other:?}"),
        };
        // Retained play applies synchronously — no render.
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(1))
        ));
        // Stop marks it finished but does not reap; the count holds until render.
        assert!(matches!(
            ext.invoke("stop", IntrospectValue::Int(id)),
            Ok(IntrospectValue::Bool(true))
        ));
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(1))
        ));
        // The wrapper's `render` steps AudioEngine::render, which reaps it (no
        // double-borrow of the shared engine RefCell).
        assert!(matches!(
            ext.invoke("render", IntrospectValue::Int(1)),
            Ok(IntrospectValue::Int(1))
        ));
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(0))
        ));
    }
}
