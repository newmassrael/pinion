//! the-tide **Stage 0** — a TUI scene walk over a Mnemosyne playable-world
//! report.
//!
//! This is the first pinion-side consumer of the R1 narrative seam
//! (`claudedocs/the-tide-2d-pipeline-requirements.md`). It proves the whole
//! text-stage pipeline with the minimum vertical slice: read a
//! `report-playable-world`-shaped report → project one scene at a time into
//! a retained, queryable scene → navigate scenes and world-lines.
//!
//! All the substance lives in the reusable [`pinion_narrative`] crate; this
//! binary is the thin `WidgetCore` / `WidgetViewTui` binding that wires it
//! into the shell:
//!
//! - [`pinion_narrative::NarrativeExternal`] is the AI-first drive surface
//!   (the same `next_scene` / `goto` verbs a keyboard press and an RPC
//!   `scene/invoke` both reach).
//! - [`pinion_narrative::narrative_scene`] is the retained projection.
//! - [`pinion_narrative::use_narrative_state`] is the one-Rc SSOT the view
//!   and the External share.
//!
//! This deliberately runs on the **retained** shell (idle tick), not the
//! immediate-mode game loop: a tide/turn exploration walk needs a low-freq
//! logical tick, not a per-frame simulation lockstep (that is Phase C).
//!
//! Run against the bundled sample:
//!
//! ```bash
//! cargo run -p hello-narrative-walk
//! ```
//!
//! Or against a live report exported elsewhere (e.g. by
//! `mnemosyne-cli report-playable-world --telling reader --json > r.json`):
//!
//! ```bash
//! PINION_NARRATIVE_REPORT=/path/to/r.json cargo run -p hello-narrative-walk
//! ```
//!
//! Keys: `n`/`j` next scene · `p`/`k` prev scene · `]` next world-line ·
//! `[` prev world-line · `Esc` quit.

use std::io::Stdout;
use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::{External, ExternalIntrospect, IntrospectValue};
use pinion_core::reactive::Owner;
use pinion_core::scene::Scene;
use pinion_core::{Frame, WidgetCore};
use pinion_narrative::{
    NarrativeCursor, NarrativeExternal, NarrativeState, PlayableWorld, narrative_access_nodes,
    narrative_scene, resolve_report, use_narrative_state,
};
use pinion_tui::ratatui::backend::CrosstermBackend;
use pinion_tui::{TuiRenderer, WidgetViewTui};

/// `Owner::cache` key for the shared read-model — the one-Rc SSOT.
const NARRATIVE_KEY: &str = "the_tide.narrative";
/// The External / paint-focus tag.
const TAG: &str = "narrative_walk";
/// Env var pointing at a live report file; unset = use the bundled sample.
const REPORT_ENV: &str = "PINION_NARRATIVE_REPORT";
/// The bundled the-tide sample report (reader telling).
const SAMPLE_REPORT: &str = include_str!("../assets/report.json");

/// The shared narrative read-model, built once per Owner scope.
fn narrative_state() -> Rc<NarrativeState> {
    use_narrative_state(NARRATIVE_KEY, load_world)
}

/// Resolve the report: an env-pointed live file, else the bundled sample.
/// A bad *explicit* path is a hard error (the operator asked for it).
fn load_world() -> PlayableWorld {
    resolve_report(REPORT_ENV, SAMPLE_REPORT)
        .unwrap_or_else(|e| panic!("hello-narrative-walk: could not load report: {e}"))
}

/// Navigation events the keyboard maps to.
#[derive(Clone, Copy)]
enum Nav {
    NextScene,
    PrevScene,
    NextWorld,
    PrevWorld,
}

/// The binding unit type.
struct HelloNarrativeWalk;

impl WidgetCore for HelloNarrativeWalk {
    /// The cursor — read back from the External each frame so the shell's
    /// change detection repaints when navigation moves it.
    type State = NarrativeCursor;
    type Event = Nav;

    fn create_external() -> Box<dyn External> {
        Box::new(NarrativeExternal::new(narrative_state()))
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> NarrativeCursor {
        if let Scene::External(node) = scene
            && let Some(intro) = node.handle.introspect()
        {
            return NarrativeCursor {
                world: query_u16(intro, "world"),
                scene: query_u16(intro, "scene"),
            };
        }
        NarrativeCursor::default()
    }

    fn view(_state: NarrativeCursor, _frame: &Frame) -> Scene {
        narrative_scene(&narrative_state())
    }

    fn event_name(event: Nav) -> &'static str {
        match event {
            Nav::NextScene => "next_scene",
            Nav::PrevScene => "prev_scene",
            Nav::NextWorld => "next_world",
            Nav::PrevWorld => "prev_world",
        }
    }

    fn title() -> &'static str {
        "pinion hello-narrative-walk — the-tide Stage 0 scene walk"
    }

    fn keybinding(key: &str) -> Option<Nav> {
        match key {
            "n" | "j" => Some(Nav::NextScene),
            "p" | "k" => Some(Nav::PrevScene),
            "]" => Some(Nav::NextWorld),
            "[" => Some(Nav::PrevWorld),
            _ => None,
        }
    }
}

/// Read a `u16` cursor field from the External's introspection channel,
/// defaulting to `0` when absent or out of range.
fn query_u16(intro: &dyn ExternalIntrospect, path: &str) -> u16 {
    match intro.query(path) {
        Some(IntrospectValue::Int(n)) => {
            u16::try_from(n.clamp(0, i64::from(u16::MAX))).unwrap_or(0)
        }
        _ => 0,
    }
}

impl WidgetA11y for HelloNarrativeWalk {
    fn access_node(_state: &NarrativeCursor, focused: Option<&str>) -> Vec<AccessNode> {
        // Prefer the shared read-model (the SSOT) via the Owner cache; fall
        // back to a bare container node if we are outside an Owner scope.
        match Owner::current().and_then(|o| o.cache_get_by_str::<NarrativeState>(NARRATIVE_KEY)) {
            Some(state) => narrative_access_nodes(TAG, &state, focused),
            None => vec![AccessNode::new(TAG, AriaRole::List)],
        }
    }
}

impl WidgetViewTui for HelloNarrativeWalk {
    type Renderer = TuiRenderer<CrosstermBackend<Stdout>>;
}

fn main() {
    if let Err(e) = pinion_tui::run::<HelloNarrativeWalk>() {
        eprintln!("hello-narrative-walk: shell error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_tui::ShellCoreTui;

    fn collect_text(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => {
                for child in &c.children {
                    collect_text(child, out);
                }
            }
            _ => {}
        }
    }

    fn paint_text(core: &ShellCoreTui<HelloNarrativeWalk>) -> String {
        let mut text = Vec::new();
        collect_text(&core.compute_paint_scene(80, 24), &mut text);
        text.join("\n")
    }

    /// End-to-end binding wiring, headlessly: keyboard `keybinding →
    /// forward → NarrativeExternal → read_state → view`, driven through the
    /// renderer-agnostic TUI substrate (no terminal). Proves the full R1
    /// consumption seam, not just the crate in isolation.
    #[test]
    fn walk_drives_scenes_and_worlds() {
        let mut core: ShellCoreTui<HelloNarrativeWalk> = ShellCoreTui::new();

        // First paint = scene 0 of the trunk world-line.
        assert_eq!(core.cached_state(), &NarrativeCursor { world: 0, scene: 0 });
        assert!(paint_text(&core).contains("물때표"));

        // `n` advances to scene 1; the cursor round-trips through the
        // External and the projection follows.
        assert!(core.dispatch_key("n", pinion_core::Modifiers::empty()));
        assert_eq!(core.cached_state().scene, 1);
        assert!(paint_text(&core).contains("문설주의 동그라미"));

        // `]` switches to the next world-line and resets to its scene 0.
        assert!(core.dispatch_key("]", pinion_core::Modifiers::empty()));
        assert_eq!(core.cached_state(), &NarrativeCursor { world: 1, scene: 0 });
        assert!(paint_text(&core).contains("떠나는 발"));

        // `p` at scene 0 is a clamped no-op (visible state unchanged).
        assert!(!core.dispatch_key("p", pinion_core::Modifiers::empty()));
        assert_eq!(core.cached_state().scene, 0);
    }
}
