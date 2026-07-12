//! `hello-no-primary` — R1308 PR-51 forcing consumer for the
//! optional-primary seam (R1303 / R1306 / R1307).
//!
//! Every other `hello-*` binding has a natural **primary** interactive
//! surface (a button, a text field, an editor viewport) that
//! [`WidgetCore::primary_surface`] returns and the substrate composes at
//! state-scene index 0. This binding has **none**: it is a two-pane
//! dashboard whose only interactive surfaces are dynamic *extras*
//! ([`WidgetCore::create_extra_externals`]). It is the minimal in-repo
//! dogfood of the shape the self-hosted editor converges on — a dock of
//! panes with no single canonical primary — and the forcing consumer that
//! proves the seam over the **real** JSON-RPC wire (`tools/demos/`), not
//! just in-process crate tests.
//!
//! What it exercises end-to-end in a real `pinion_shell` shell:
//!
//! * [`WidgetCore::primary_surface`] returns `None`, so `CoreShell`
//!   composes the state scene from the extras alone — a
//!   `Scene::Container([External(pane_0), External(pane_1)])` marked
//!   no-primary-head (R1307). `create_external` / `tag` are `unreachable!`
//!   markers the substrate never reaches.
//! * Over JSON-RPC (§2 #2, the AI-primary path) each pane is reachable and
//!   mutable by its explicit tag (`/pane_0/external/count`,
//!   `/pane_0/external/increment`), while the bare `/external` shorthand —
//!   which names "the primary" — rejects cleanly with `NoExternalAtPath`
//!   (§2 #7 self-describing) instead of silently resolving an arbitrary
//!   pane.
//! * The GUI paint / focus path is unaffected (focus is paint-derived, not
//!   `tag()`-derived), so a no-primary binding is a first-class shell
//!   citizen.

use pinion_a11y::WidgetA11y;
use pinion_core::external::{CountedExternal, External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widget_core::{ExtraExternal, PrimarySurface};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

// pinion-forge codegen output — defines `HelloNoPrimaryRenderer` +
// `HelloNoPrimaryRendererError` (the Vello wrapper). Same emit template as
// hello-button; `include!` is bare because the template uses `::vello::*`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloNoPrimaryRenderer, HelloNoPrimaryRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 160;
const PANE_W: u32 = 150;
const PANE_H: u32 = 100;
/// Shared `ThemeProvider` cache key (the `"app"` gallery convention).
const THEME_TAG: &str = "app";
/// State-scene routing tags for the two extra panes — the addresses AI
/// clients drive over the wire (`/pane_0/external/...`).
const PANE_TAGS: [&str; 2] = ["pane_0", "pane_1"];
/// Paint-scene tags for the two pane boxes (distinct from the state-scene
/// external tags; the paint tree is a separate axis).
const PANE_BOX_TAGS: [&str; 2] = ["pane_0_box", "pane_1_box"];

/// Cached projection: the current count of each pane, read from the live
/// state scene by [`HelloNoPrimary::read_state`] every paint cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PaneCounts {
    pane_0: i64,
    pane_1: i64,
}

/// Read a pane's `count` slot from the state scene via the §5.15 introspect
/// channel — the same path an RPC `scene/query /<tag>/external/count` walks.
fn read_pane_count(scene: &Scene, tag: &str) -> i64 {
    scene
        .find_external_with_tag(tag)
        .and_then(|node| node.handle.introspect())
        .and_then(|intro| intro.query("count"))
        .and_then(|value| match value {
            IntrospectValue::Int(n) => Some(n),
            _ => None,
        })
        .unwrap_or(0)
}

/// view-fn (§6.3): pure sync `PaneCounts -> Scene`. Paints the two panes
/// side by side, each a titled box showing its live count. No primary tag —
/// R55.G.17 does not apply to a no-primary binding.
//
// `&Frame` intentional per the §6.3 view-fn signature contract even though
// `Frame` is presently a `Copy` ZST — matches the `WidgetCore::view` trait
// slot so the by-value bridge is unnecessary (same allow as hello-button).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: PaneCounts, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let pane = |label: &str, count: i64, box_tag: &'static str| -> Scene {
        Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                format!("{label}: {count}"),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(18)
                    .with_fg(theme.resolve(ColorRole::OnSurface)),
            ))])
            .with_tag(box_tag)
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest))
                    .with_corner_radius(12),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(PANE_W, PANE_H)),
            ),
        )
    };
    Scene::Container(
        ContainerNode::new(vec![
            pane("Pane 0", state.pane_0, PANE_BOX_TAGS[0]),
            pane("Pane 1", state.pane_1, PANE_BOX_TAGS[1]),
        ])
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center),
        ),
    )
}

/// The no-primary binding. Hand-written (not `#[widget]`-derived) because
/// the derive macro always emits a primary `create_external` — the exact
/// mandatory-primary contract this binding opts out of.
struct HelloNoPrimary;

impl WidgetCore for HelloNoPrimary {
    type State = PaneCounts;
    /// No typed widget events — the panes are driven through their own
    /// tags, never a primary statechart. `event_name` is inert.
    type Event = ();

    /// (R1306 PR-51) The opt-out: no single canonical primary. Every
    /// interactive surface is a dynamic extra below. This is the
    /// editor-archetype shape.
    fn primary_surface() -> Option<PrimarySurface> {
        None
    }

    fn create_external() -> Box<dyn External> {
        unreachable!("hello-no-primary has no primary surface — see primary_surface()")
    }

    fn tag() -> &'static str {
        unreachable!("hello-no-primary has no primary surface — see primary_surface()")
    }

    /// The whole interactive surface set: two independent counter panes,
    /// each a routable [`CountedExternal`] addressed over the wire by its
    /// tag. Static (frozen at boot) — `external_set_is_dynamic` stays
    /// `false`.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(PANE_TAGS[0], Box::new(CountedExternal::new(0))),
            ExtraExternal::new(PANE_TAGS[1], Box::new(CountedExternal::new(0))),
        ]
    }

    fn read_state(scene: &Scene) -> Self::State {
        PaneCounts {
            pane_0: read_pane_count(scene, PANE_TAGS[0]),
            pane_1: read_pane_count(scene, PANE_TAGS[1]),
        }
    }

    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-no-primary (PR-51 primary_surface() == None)"
    }
}

// Default a11y surface — the panes expose no bespoke AT nodes in this
// minimal dogfood (a real editor pane would; the seam under test is
// composition + routing, not a11y).
impl WidgetA11y for HelloNoPrimary {}

impl WidgetView for HelloNoPrimary {
    type Renderer = HelloNoPrimaryRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<HelloNoPrimary>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    /// The binding declares no primary — the opt-out this example exists to
    /// dogfood.
    #[test]
    fn primary_surface_is_none() {
        assert!(<HelloNoPrimary as WidgetCore>::primary_surface().is_none());
    }

    /// Its whole interactive surface set is two tagged extras.
    #[test]
    fn extras_are_two_counter_panes() {
        let extras = <HelloNoPrimary as WidgetCore>::create_extra_externals();
        let tags: Vec<&str> = extras.iter().map(|e| e.tag.as_ref()).collect();
        assert_eq!(tags, vec!["pane_0", "pane_1"]);
    }

    /// `read_state` projects each pane's count from the composed state
    /// scene (the shape `compose_root(None, [..])` produces — a
    /// no-primary-head container of the two panes).
    #[test]
    fn read_state_extracts_pane_counts() {
        let a = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(3))).with_tag(PANE_TAGS[0]),
        );
        let b = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(7))).with_tag(PANE_TAGS[1]),
        );
        let scene = Scene::Container(ContainerNode::new(vec![a, b]).without_primary_head());
        assert_eq!(
            <HelloNoPrimary as WidgetCore>::read_state(&scene),
            PaneCounts {
                pane_0: 3,
                pane_1: 7
            },
        );
        // The marked no-primary-head container resolves no primary — the
        // bare `/external` shorthand rejects rather than hitting pane_0.
        assert!(scene.primary_external().is_none());
    }

    /// The view paints both panes (by their paint tags), with no primary
    /// tag required (R55.G.17 is inapplicable).
    #[test]
    fn view_renders_both_panes() {
        let owner = pinion_core::Owner::new();
        let scene = owner.run(|| {
            view(
                PaneCounts {
                    pane_0: 1,
                    pane_1: 2,
                },
                &Frame::new(),
            )
        });
        assert!(scene.contains_tag(PANE_BOX_TAGS[0]));
        assert!(scene.contains_tag(PANE_BOX_TAGS[1]));
    }
}
