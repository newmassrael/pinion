// Example bindings tolerate looser doc-markdown lints than substrate crates;
// the narrative carries many proper-noun identifiers (JSON-RPC, VN, …).
#![allow(clippy::doc_markdown)]

//! `hello-vn-tide` — the-tide **VN Stage 0** set-piece, brought onto the real
//! JSON-RPC wire.
//!
//! ## What this proves
//!
//! `claudedocs/the-tide-vn-renpy-parity-requirements.md` §7 asks for the
//! "부름 급박 세트피스" — the heart of a Ren'Py-parity VN runner:
//! typewriter dialogue + a **timed choice** with a countdown ("급박 모달리티").
//! The reusable runner lives in [`pinion_narrative::vn`] (a retained
//! structured-scene surface: the dialogue is queryable text, not opaque
//! paint — §2 #1 / #7); this binary is the thin `pinion_shell` binding that
//! hosts its [`VnExternal`] on the GUI shell's built-in stdin JSON-RPC reader,
//! so `tools/demos/hello_vn_tide.py` drives the whole set-piece over the same
//! wire an AI agent uses: `tick` reveals the typewriter and drains the
//! countdown, `advance` snaps / steps a line, `choose` takes an option — all
//! as real `scene/invoke` + `scene/query` requests.
//!
//! ## The `tick` step-verb — honestly scoped
//!
//! On real hardware a VN's typewriter and countdown advance from the frame
//! clock. Reproducing that with a background thread + sleeps in a demo is the
//! exact [[zero-flake-policy]] hazard, so the runner is stepped by a
//! deterministic **`tick {ms}` step-verb** that advances *logical* time read
//! from the argument (the wire form of a fixed-timestep frame). This is the
//! same choice `hello-audio-rt`'s `render` verb made: the game-loop
//! `scene/tick {dt}` fan-out reaches only `Scene::ImmediateModeNode` paint
//! drivers, and this runner is deliberately a retained structured-scene
//! `Scene::External`, so `scene/tick` cannot reach it and a zero-flake demo
//! must not depend on the wall clock. Live real-time play (typewriter /
//! countdown on the wall clock) is a small retained `Tickable` (the
//! `CaretBlink` pattern, existing substrate — NOT Phase-C, NOT an
//! immediate-mode node); it is a follow-up round, kept out of this wire-proof
//! harness so the demo stays deterministic.
//!
//! ## Why the GUI shell, not the TUI
//!
//! `pinion_tui` paints the terminal to stdout — the same channel the JSON-RPC
//! server replies on — so a TUI binary cannot be cleanly driven over
//! stdin/stdout RPC. `pinion_shell` paints to a GPU window instead, leaving
//! stdout a clean RPC channel; under `PINION_HIDDEN_WINDOW=1` (the harness
//! default) the window is created unmapped, so the full real pipeline runs
//! headless.
//!
//! Run headless over RPC:
//!
//! ```bash
//! cargo build -p hello-vn-tide --release
//! python3 tools/demos/hello_vn_tide.py
//! ```

use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::{
    External, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
};
use pinion_core::intent::Intent;
use pinion_core::reactive::Owner;
use pinion_core::scene::Scene;
use pinion_core::storage::Storage;
use pinion_core::{Frame, WidgetCore};
use pinion_narrative::vn::state::{VnCursor, VnSave};
use pinion_narrative::{
    SpritePos, VnExternal, VnOption, VnScript, VnSprite, VnState, VnStep, use_vn_clock,
    use_vn_state, vn_scene,
};
use pinion_platform_storage::{AppStorage, use_app_storage};
use pinion_shell::{WidgetView, vello_renderer_impl};

// pinion-forge codegen output: `pub struct HelloVnTideRenderer` + async `new`
// + sync `render` / `resize`, `::vello::*`-qualified so the include is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// Bridge the inherent renderer methods into the `pinion_shell::VelloRenderer`
// trait so the generic `AppShell<V>` can construct + render + resize it.
vello_renderer_impl!(HelloVnTideRenderer, HelloVnTideRendererError);

/// `Owner::cache` key for the shared runner — the one-Rc SSOT.
const VN_KEY: &str = "the_tide.vn";
/// Animation-registry key for the real-time clock (the `CaretBlink` pattern).
const VN_CLOCK_KEY: &str = "the_tide.vn.clock";

/// `true` in an interactive window (real-time clock on), `false` under the
/// headless `PINION_HIDDEN_WINDOW` harness (the deterministic `tick` verb
/// drives time instead — a registered wall-clock clock drifts the offscreen
/// play-head run-to-run, even under `scene/set_fps 0`, because RPC-induced
/// repaints still tick the animation clock by wall-clock; verified).
fn real_time_enabled() -> bool {
    std::env::var("PINION_HIDDEN_WINDOW")
        .map(|v| v.is_empty() || v == "0")
        .unwrap_or(true)
}
/// The External / paint-focus tag.
const TAG: &str = "vn_tide";

/// The shared VN runner, built once per Owner scope.
fn vn_state() -> Rc<VnState> {
    use_vn_state(VN_KEY, tide_script)
}

/// The-tide "부름 급박 세트피스" — a small **branching** VN script. One
/// decisive timed choice forks the telling into two endings, so a choice
/// actually changes what happens (R1296): 돌아본다 → the peril branch
/// (steps 3–4), 버틴다 → the endure branch (steps 5–6). The peril ending's
/// line jumps past the endure branch to the End (`with_goto`), so the two
/// branches do not bleed into each other. Timeout takes the default
/// (돌아본다 → peril) — hesitation is punished, the "급박함" of the tide.
///
/// Steps: 0 narration · 1 무녀 line · 2 timed choice · 3–4 peril branch ·
/// 5–6 endure branch (len = 7, so `with_goto(7)` is the End).
fn tide_script() -> VnScript {
    const END: u16 = 7;
    VnScript::new(vec![
        VnStep::narration("밀물이 갯벌을 삼킨다. 등 뒤에서 목소리가 너를 부른다."),
        VnStep::line("무녀", "돌아오지 마라. 물때가 널 데려간다."),
        VnStep::timed_choice(
            "네 이름이 다시 불린다 — 어쩔 텐가?",
            vec![
                VnOption::new("돌아본다", "turn").goto(3),
                VnOption::new("버틴다", "endure").goto(5),
            ],
            4000,
            0, // default on timeout = 돌아본다 / turn (the peril branch)
        ),
        // Peril branch (3–4): the terminal line jumps to the End.
        VnStep::narration("돌아보자, 아무도 없다. 검은 물이 목까지 차오른다."),
        VnStep::line("무녀", "물때가 너를 데려간다. — 나쁜 끝.").with_goto(END),
        // Endure branch (5–6): flows off the last step into the End.
        VnStep::narration("이를 악물고 버틴다. 검은 물이 무릎에서 멈춘다."),
        VnStep::line("무녀", "물때가 잦아든다. 너는 살아 남는다. — 버틴 끝."),
    ])
}

/// App name for the on-disk save-slot store (honours `PINION_STORAGE_DIR`).
const APP: &str = "pinion-vn-tide";
/// `Owner::cache` key for the shared save-slot store.
const STORAGE_KEY: &str = "the_tide.vn.storage";

/// Demo-glue [`External`] adding **file-backed save slots** on top of the
/// crate's pure state save/load — the persistence half of save/load, the peer
/// of `hello-audio-rt`'s `EngineDemoExternal`.
///
/// Every crate verb (`tick` / `advance` / `choose` / `save` / `load` / query /
/// intervene) delegates verbatim to the inner [`VnExternal`], so the wire
/// contract this binary proves is exactly the crate's. The wrapper adds two
/// verbs the crate deliberately leaves out (it stays storage-free): `save_slot`
/// / `load_slot`, which persist the crate's [`VnState::save`] blob to a
/// `pinion-platform-storage` file so a save survives a process restart. The
/// wrapper holds a second clone of the same `Rc<VnState>` the inner External
/// drives, so a slot save reads the live play-head and a slot load mutates it.
struct VnSaveDemoExternal {
    inner: VnExternal,
    state: Rc<VnState>,
    storage: Rc<AppStorage>,
    pending_intents: Vec<Intent>,
}

impl std::fmt::Debug for VnSaveDemoExternal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VnSaveDemoExternal")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

/// Absolute paths (resolved at compile time from the crate dir, present at run
/// time in a checkout) to the bundled placeholder art, so the shell's
/// `ImageCache` — which reads `ImageNode.source` as a filesystem path — draws
/// real pixels. (A logical-name → file asset registry is the Phase-C asset
/// pipeline; these paths are the honest stand-in that proves the render path.)
const BG_ASSET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/tideflat.png");
const MUDANG_ASSET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/mudang.png");

impl VnSaveDemoExternal {
    fn new() -> Self {
        let state = vn_state();
        // Author the opening stage with REAL assets: the tide-flat background
        // fills the stage and the 무녀 stands centre — both resolvable image
        // files, so a screenshot shows actual positioned pixels, not just
        // queryable data. (The demo then directs further sprites over the wire.)
        state.stage().set_background(BG_ASSET);
        state
            .stage()
            .show(VnSprite::new("mudang", MUDANG_ASSET, SpritePos::Center, 1));
        Self {
            inner: VnExternal::new(state.clone()),
            state,
            storage: use_app_storage(STORAGE_KEY, APP),
            pending_intents: Vec::new(),
        }
    }
}

/// The slot name a `save_slot` / `load_slot` arg carries (a plain `Text`).
fn slot_name(args: &IntrospectValue) -> Result<String, InvokeError> {
    match args {
        IntrospectValue::Text(s) => Ok(format!("slot.{s}")),
        _ => Err(InvokeError::TypeMismatch),
    }
}

impl ExternalIntrospect for VnSaveDemoExternal {
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
        match path {
            // Persist the live play-head to a named on-disk slot.
            "save_slot" => {
                let key = slot_name(&args)?;
                let blob =
                    serde_json::to_vec(&self.state.save()).map_err(|_| InvokeError::Rejected)?;
                self.storage.save(&key, &blob);
                Ok(IntrospectValue::json(&self.state.save()))
            }
            // Restore the play-head from a named on-disk slot. A missing slot
            // is Rejected (well-typed arg, nothing to load).
            "load_slot" => {
                let key = slot_name(&args)?;
                match self.storage.load(&key) {
                    Some(bytes) => {
                        let save: VnSave =
                            serde_json::from_slice(&bytes).map_err(|_| InvokeError::Rejected)?;
                        self.state.load(save);
                        Ok(IntrospectValue::json(&self.state.save()))
                    }
                    None => Err(InvokeError::Rejected),
                }
            }
            // Every other verb is the inner crate surface's, verbatim; forward
            // any intent it queued so the wrapper's own §5.20 drain surfaces it.
            _ => {
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
    }
}

// Paints nothing (the binding's `view` is the paint scene), draining
// `self.pending_intents` — the same skeleton the crate External uses.
pinion_core::intent_query_external_impl!(VnSaveDemoExternal);

/// The binding unit type.
struct HelloVnTide;

impl WidgetCore for HelloVnTide {
    /// The visible play-head — read back each frame so the shell repaints when
    /// the typewriter / countdown moves.
    type State = VnCursor;
    /// No keyboard affordances: this is an RPC harness, and the typewriter /
    /// countdown advance only through the `tick` verb (there is no
    /// frame-driven clock here), so advertising keys would be misleading.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(VnSaveDemoExternal::new())
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> VnCursor {
        scene
            .primary_external()
            .and_then(|node| node.handle.introspect())
            .map_or_else(VnCursor::default, |intro| VnCursor {
                step: query_u16(intro, "step"),
                revealed_chars: query_u16(intro, "revealed_chars"),
                remaining_ms: query_u32(intro, "remaining_ms"),
            })
    }

    fn view(_state: VnCursor, _frame: &Frame) -> Scene {
        let state = vn_state();
        // Real-time clock — but ONLY in an interactive window. In a live GUI the
        // registered `Tickable` advances the typewriter / countdown on the wall
        // clock (felt urgency). The headless RPC harness deliberately does NOT
        // register it: the shared animation loop paints the offscreen window on
        // the wall clock too, which would drift the play-head and defeat the
        // zero-flake demo — so the demo drives time by the deterministic `tick`
        // verb instead. (Verified: with the clock on, a hidden window's boot
        // `revealed_chars` drifts run-to-run.)
        if real_time_enabled() {
            let _clock = use_vn_clock(VN_CLOCK_KEY, state.clone());
        }
        vn_scene(&state)
    }

    fn event_name((): ()) -> &'static str {
        // No externally-drivable widget event — the runner is driven through
        // the introspect verbs, not the typed `send` channel.
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-vn-tide — the-tide VN Stage 0 (typewriter + timed choice) over RPC"
    }
}

/// Read a `u16` field from the External's introspection channel, defaulting to
/// `0` when absent or out of range.
fn query_u16(intro: &dyn ExternalIntrospect, path: &str) -> u16 {
    match intro.query(path) {
        Some(IntrospectValue::Int(n)) => {
            u16::try_from(n.clamp(0, i64::from(u16::MAX))).unwrap_or(0)
        }
        _ => 0,
    }
}

/// Read a `u32` field from the External's introspection channel, defaulting to
/// `0` when absent or out of range.
fn query_u32(intro: &dyn ExternalIntrospect, path: &str) -> u32 {
    match intro.query(path) {
        Some(IntrospectValue::Int(n)) => {
            u32::try_from(n.clamp(0, i64::from(u32::MAX))).unwrap_or(0)
        }
        _ => 0,
    }
}

impl WidgetA11y for HelloVnTide {
    fn access_node(state: &VnCursor, focused: Option<&str>) -> Vec<AccessNode> {
        // Prefer the shared runner (the SSOT) via the Owner cache for a
        // descriptive label; fall back to a bare node outside an Owner scope.
        let name = match Owner::current().and_then(|o| o.cache_get_by_str::<VnState>(VN_KEY)) {
            Some(vn) => format!("the-tide VN — {} · 스텝 {}", vn.mode().as_str(), state.step),
            None => "the-tide VN".to_string(),
        };
        vec![
            AccessNode::new(TAG, AriaRole::List)
                .with_name(name)
                .with_focused(focused == Some(TAG)),
        ]
    }
}

impl WidgetView for HelloVnTide {
    type Renderer = HelloVnTideRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: 800,
            height: 320,
        }
    }
}

fn main() {
    pinion_shell::run::<HelloVnTide>();
}
