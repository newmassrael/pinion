//! the-tide **2D map** — a relation place-graph solved to coordinates.
//!
//! The spatial peer of `hello-narrative-walk`, and the home of the field
//! report's F3: the place-graph carries only *relations* (containment /
//! adjacency / direction), and [`pinion_narrative::solve_layout`]
//! deterministically **solves** the coordinates the author never wrote.
//! [`pinion_narrative::place_map_scene`] projects the solved layout into a
//! queryable scene of boxes (places), enclosing boxes (containers), and
//! lines (adjacencies).
//!
//! The scene is backend-agnostic (§2 #6). A box-and-line map is a graphical
//! artifact — its adjacency **lines** render on the GUI (Vello) backend; the
//! TUI shell here draws the place boxes + labels (it has no `Scene::Path`
//! cell rasteriser). The full geometry is the same data on either, readable
//! over RPC via the [`PlaceLayout`]
//! `QuerySource` wrapped in `QueryOnlyIntrospect`.
//!
//! Run against the bundled the-tide map, or a live spatial report:
//!
//! ```bash
//! cargo run -p hello-place-map
//! PINION_PLACE_GRAPH=/path/to/place.json cargo run -p hello-place-map
//! ```

use std::io::Stdout;
use std::rc::Rc;

use pinion_a11y::{AccessNode, WidgetA11y};
use pinion_core::external::{External, QueryOnlyIntrospect};
use pinion_core::reactive::Owner;
use pinion_core::scene::Scene;
use pinion_core::{Frame, WidgetCore};
use pinion_narrative::{
    PlaceGraph, PlaceLayout, place_map_access_nodes, place_map_scene, resolve_place_graph,
    solve_layout,
};
use pinion_tui::ratatui::backend::CrosstermBackend;
use pinion_tui::{TuiRenderer, WidgetViewTui};

/// `Owner::cache` key for the solved layout — the one-Rc SSOT.
const LAYOUT_KEY: &str = "the_tide.place_layout";
/// The paint-focus / External tag.
const TAG: &str = "place_map";
/// Env var pointing at a live spatial report; unset = use the bundled map.
const GRAPH_ENV: &str = "PINION_PLACE_GRAPH";
/// The bundled the-tide place-graph.
const SAMPLE_GRAPH: &str = include_str!("../assets/place.json");

/// The solved layout, computed once per Owner scope.
fn place_layout() -> Rc<PlaceLayout> {
    Owner::current()
        .expect("place_layout requires an active Owner scope")
        .cache(LAYOUT_KEY, || solve_layout(&load_graph()))
}

/// Resolve the place-graph: an env-pointed live file, else the bundled map.
fn load_graph() -> PlaceGraph {
    resolve_place_graph(GRAPH_ENV, SAMPLE_GRAPH)
        .unwrap_or_else(|e| panic!("hello-place-map: could not load place graph: {e}"))
}

/// The binding unit type. A static map view (no navigation this round).
struct HelloPlaceMap;

impl WidgetCore for HelloPlaceMap {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        // The solved layout is a `QuerySource`; wrap the shared `Rc` (no
        // clone) in pinion-core's read-only introspection substrate.
        Box::new(QueryOnlyIntrospect::new(place_layout()))
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(_state: (), _frame: &Frame) -> Scene {
        place_map_scene(&place_layout())
    }

    fn event_name(_event: ()) -> &'static str {
        ""
    }

    fn title() -> &'static str {
        "pinion hello-place-map — the-tide relation place-graph solved to 2D"
    }
}

impl WidgetA11y for HelloPlaceMap {
    fn access_node(_state: &(), focused: Option<&str>) -> Vec<AccessNode> {
        match Owner::current().and_then(|o| o.cache_get_by_str::<PlaceLayout>(LAYOUT_KEY)) {
            Some(layout) => place_map_access_nodes(TAG, &layout, focused),
            None => Vec::new(),
        }
    }
}

impl WidgetViewTui for HelloPlaceMap {
    type Renderer = TuiRenderer<CrosstermBackend<Stdout>>;
}

fn main() {
    if let Err(e) = pinion_tui::run::<HelloPlaceMap>() {
        eprintln!("hello-place-map: shell error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_tui::ShellCoreTui;

    fn count(scene: &Scene) -> (usize, usize, usize) {
        // (box count, path count, text count)
        match scene {
            Scene::Box(_) => (1, 0, 0),
            Scene::Path(_) => (0, 1, 0),
            Scene::Text(_) => (0, 0, 1),
            Scene::Container(c) => c.children.iter().fold((0, 0, 0), |acc, ch| {
                let (b, p, t) = count(ch);
                (acc.0 + b, acc.1 + p, acc.2 + t)
            }),
            _ => (0, 0, 0),
        }
    }

    /// The bundled map solves and projects through the real shell,
    /// headlessly. 7 places (2 containers) + 5 adjacencies.
    #[test]
    fn bundled_map_solves_and_projects() {
        let core: ShellCoreTui<HelloPlaceMap> = ShellCoreTui::new();
        let (boxes, paths, texts) = count(&core.compute_paint_scene(80, 24));
        // 7 place boxes + 2 container boxes (village, mudflat).
        assert_eq!(boxes, 9, "place + container boxes");
        // 5 adjacency lines.
        assert_eq!(paths, 5, "one line per adjacency");
        // At least one label per place.
        assert!(texts >= 7, "labels present: {texts}");
    }

    /// R1344 §5.21 §5.41 — the solved map keeps its GEOMETRY, not just its
    /// node count.
    ///
    /// `bundled_map_solves_and_projects` above counts node *kinds*, which is
    /// invariant under total geometric collapse — when the TUI gained its
    /// layout pass, `place_map_scene`'s authored rects were overwritten and
    /// every box flattened to `h = 0` (the 2-D map became a label list) while
    /// that test stayed green. Exactly the repo's unit-green / demo-red
    /// pattern, so the geometry gets its own pin.
    #[test]
    fn r1344_solved_map_keeps_its_geometry_through_the_layout_pass() {
        fn boxes_of(s: &Scene, out: &mut Vec<pinion_core::scene::Rect>) {
            match s {
                Scene::Box(b) => out.push(b.rect),
                Scene::Container(c) => c.children.iter().for_each(|ch| boxes_of(ch, out)),
                _ => {}
            }
        }
        let core: ShellCoreTui<HelloPlaceMap> = ShellCoreTui::new();
        let scene = core.compute_paint_scene(80, 24);
        let mut rects = Vec::new();
        boxes_of(&scene, &mut rects);
        assert_eq!(rects.len(), 9, "9 solved boxes");
        for r in &rects {
            assert!(r.w > 0 && r.h > 0, "a solved box has real extent: {r:?}");
        }
        // A 2-D map: the boxes must NOT all share one column/row (which is
        // exactly what block-flow collapse produces).
        let distinct_x = rects
            .iter()
            .map(|r| r.x)
            .collect::<std::collections::HashSet<_>>();
        let distinct_y = rects
            .iter()
            .map(|r| r.y)
            .collect::<std::collections::HashSet<_>>();
        assert!(distinct_x.len() > 1, "boxes spread across x: {rects:?}");
        assert!(distinct_y.len() > 1, "boxes spread across y: {rects:?}");
    }
}
