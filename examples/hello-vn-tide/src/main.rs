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
//! same choice `hello-audio-rt`'s `render` verb made: the north-star driver
//! is the game-loop `scene/tick {dt}`, but that fan-out reaches only
//! `Scene::ImmediateModeNode` paint drivers, and this runner is deliberately a
//! retained structured-scene `Scene::External`. Frame-driven real-time play
//! (wiring the runner to the shell's frame delta) and the presentation layer
//! it needs (sprites / transitions) are the acknowledged follow-ups; this
//! round proves the VN control + presentation surface over the wire.
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
use pinion_core::external::{External, ExternalIntrospect, IntrospectValue};
use pinion_core::reactive::Owner;
use pinion_core::scene::Scene;
use pinion_core::{Frame, WidgetCore};
use pinion_narrative::vn::state::VnCursor;
use pinion_narrative::{VnExternal, VnOption, VnScript, VnState, VnStep, use_vn_state, vn_scene};
use pinion_shell::{WidgetView, vello_renderer_impl};

// pinion-forge codegen output: `pub struct HelloVnTideRenderer` + async `new`
// + sync `render` / `resize`, `::vello::*`-qualified so the include is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// Bridge the inherent renderer methods into the `pinion_shell::VelloRenderer`
// trait so the generic `AppShell<V>` can construct + render + resize it.
vello_renderer_impl!(HelloVnTideRenderer, HelloVnTideRendererError);

/// `Owner::cache` key for the shared runner — the one-Rc SSOT.
const VN_KEY: &str = "the_tide.vn";
/// The External / paint-focus tag.
const TAG: &str = "vn_tide";

/// The shared VN runner, built once per Owner scope.
fn vn_state() -> Rc<VnState> {
    use_vn_state(VN_KEY, tide_script)
}

/// The-tide "부름 급박 세트피스" — a small authored VN script. Two timed
/// choices: the first the demo lets time out (→ its default), the second the
/// demo answers in time — so one linear play-through exercises the typewriter,
/// `advance`, a countdown timeout, and an explicit `choose`.
fn tide_script() -> VnScript {
    VnScript::new(vec![
        VnStep::narration("밀물이 갯벌을 삼킨다. 등 뒤에서 목소리가 너를 부른다."),
        VnStep::line("무녀", "돌아오지 마라. 물때가 널 데려간다."),
        VnStep::timed_choice(
            "다시, 네 이름이 불린다 — 어쩔 텐가?",
            vec![
                VnOption::new("돌아본다", "turn"),
                VnOption::new("버틴다", "endure"),
            ],
            4000,
            1, // default on timeout = 버틴다 / endure
        ),
        VnStep::narration("너는 이를 악문다. 검은 물이 무릎까지 찬다."),
        VnStep::timed_choice(
            "마지막 부름. 차가운 손이 어깨에 닿는다.",
            vec![
                VnOption::new("뿌리친다", "shake_off"),
                VnOption::new("잡는다", "take_hand"),
            ],
            3000,
            0, // default on timeout = 뿌리친다 / shake_off
        ),
        VnStep::narration("손을 잡자, 물이 잔잔해진다. 물때가 멈춘다."),
    ])
}

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
        Box::new(VnExternal::new(vn_state()))
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
        vn_scene(&vn_state())
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
