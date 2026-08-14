//! `hello-image` — R740 §5.16 **raster image paint** (clearing the
//! `Scene::Image` no-op). The first consumer of the asset pipeline:
//! decode a bundled PNG once and paint it under each of the four
//! [`Fit`] policies.
//!
//! ## What this proves
//!
//! Before R740, `Scene::Image` was a paint no-op — the data model,
//! layout, hashing and `scene/snapshot` were all complete, only the
//! rasterizer arm was missing. R740 adds the decode substrate
//! (`pinion_asset`), a decode-once cache
//! (`pinion_runtime::image_cache`), and the `paint_image` arm in both
//! paint walkers. This binding bundles a 16x16 four-quadrant PNG (red /
//! green / blue / white) and draws it four times so each [`Fit`] reads
//! differently *because the cells are non-square* (a square source in a
//! square rect makes Fill / Contain / Cover identical):
//!
//! * **Fill** — non-uniform stretch; the quadrants fill the whole cell.
//! * **Contain** — uniform fit-inside; the square image is centred with
//!   left/right letterbox margins.
//! * **Cover** — uniform fill-the-cell; the square image overflows top /
//!   bottom and is clipped to the cell.
//! * **Tile** — the 16x16 image repeats across the cell.
//!
//! The colored quadrants make the live-pixel check decisive: a solid
//! fill cannot produce four different corner colours, so sampling the
//! Fill cell's corners proves the image really decoded and rendered.
//!
//! ## a11y / interaction
//!
//! The four images are decorative; the focusable element is a
//! [`ToggleExternal`] "show cell bounds" switch. Toggling it outlines
//! each image's cell rect so a viewer can see how each fit places the
//! source inside the identical cell. The images themselves never move,
//! so the live-pixel samples are stable regardless of the toggle.
//!
//! ## Source model (first slice)
//!
//! `ImageNode::source` is a filesystem path. `main` writes the
//! `include_bytes!`-embedded PNG to a temp file once and stashes the
//! path; the cache reads + decodes it on the first paint. A binding-
//! registered in-memory store and `https://` are additive later axes.

use std::sync::OnceLock;

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{BoxNode, ContainerNode, ImageNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Fit, FlexDirection, ImageStyle, JustifyContent, LayoutStyle, Size,
    TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloImageRenderer, HelloImageRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 300;
const THEME_TAG: &str = "app";
const TOGGLE_TAG: &str = "main_toggle";

// Non-square cell so Fill / Contain / Cover / Tile read differently.
const CELL_W: u32 = 150;
const CELL_H: u32 = 80;
const GAP: u32 = 12;

/// The bundled icon, written to a temp file once at startup so the
/// path-based `pinion_runtime::image_cache` can read it. `None` until
/// `main` runs (view/a11y unit tests then paint an empty source, which
/// the cache skips gracefully).
static ICON_PATH: OnceLock<String> = OnceLock::new();

fn icon_source() -> &'static str {
    ICON_PATH.get().map_or("", String::as_str)
}

/// One labelled cell: the icon under `fit` in a non-square box, with the
/// fit name beneath. `bounds` outlines the cell so the fit placement is
/// visible against the identical rect.
fn fit_cell(
    fit: Fit,
    label: &str,
    tag: &'static str,
    highlight: bool,
    theme: &pinion_core::theme::Theme,
) -> Scene {
    // The cell bounds (destination rect) are always outlined so each fit's
    // placement reads against an identical box — Contain's inner margins,
    // Cover's clipped overflow, Tile's repeats. The "highlight bounds"
    // switch only changes the outline colour (muted → accent) so the demo
    // is fully informative even before any interaction.
    let outline = if highlight {
        theme.resolve(ColorRole::Accent)
    } else {
        theme.resolve(ColorRole::Outline)
    };
    let cell_style = BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh))
        .with_corner_radius(6)
        .with_border(pinion_core::style::Border::new(outline, 1));
    let frame = Scene::Box(
        BoxNode::new(Rect::default(), cell_style)
            .with_layout(LayoutStyle::new().with_size(Size::px(CELL_W, CELL_H))),
    );
    let image = Scene::Image(
        ImageNode::styled(
            icon_source(),
            Rect::default(),
            ImageStyle::default().with_fit(fit),
        )
        .with_tag(tag)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(0, 0)
                .with_size(Size::px(CELL_W, CELL_H)),
        ),
    );
    // The image is absolutely positioned over the cell background so both
    // share the CELL_W x CELL_H rect.
    let stack = Scene::Container(
        ContainerNode::new(vec![frame, image])
            .with_layout(LayoutStyle::new().with_size(Size::px(CELL_W, CELL_H))),
    );
    let caption = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![stack, caption]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Center)
                .with_gap(4),
        ),
    )
}

fn row(cells: Vec<Scene>) -> Scene {
    Scene::Container(
        ContainerNode::new(cells).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_gap(GAP),
        ),
    )
}

/// view-fn (§6.3): the 2x2 fit grid plus the "show bounds" toggle.
fn view(state: ToggleState, bounds: bool, _frame: Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(TextNode::styled(
        "Image — Fit policies",
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let grid = Scene::Container(
        ContainerNode::new(vec![
            row(vec![
                fit_cell(Fit::Fill, "Fill", "img_fill", bounds, &theme),
                fit_cell(Fit::Contain, "Contain", "img_contain", bounds, &theme),
            ]),
            row(vec![
                fit_cell(Fit::Cover, "Cover", "img_cover", bounds, &theme),
                fit_cell(Fit::Tile, "Tile", "img_tile", bounds, &theme),
            ]),
        ])
        .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(GAP)),
    );

    // The focusable toggle: a small switch box echoing the bounds state.
    let knob = if bounds { "accent" } else { "muted" };
    let toggle = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(if bounds {
                theme.resolve(ColorRole::Accent)
            } else {
                theme.resolve(ColorRole::SurfaceContainerHighest)
            })
            .with_corner_radius(10),
        )
        .with_tag(TOGGLE_TAG)
        .with_layout(
            LayoutStyle::new()
                .with_focusable(true)
                .with_size(Size::px(120, 28)),
        ),
    );
    let toggle_caption = Scene::Text(TextNode::styled(
        format!("highlight bounds: {knob} ({})", state.as_name()),
        Rect::default(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, grid, toggle, toggle_caption])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(GAP),
            ),
    )
}

#[widget(
    tag = "main_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-image (R740 §5.16 raster image paint)",
    renderer = HelloImageRenderer,
    initial_size = (WIN_W, WIN_H),
    external = ToggleExternal::new,
    role = Switch,
    // R1692 §5.40 — the toggle is a plain filled box with a caption BESIDE it,
    // so there is nothing under its tag for the scene derivation to read and a
    // reader was told "switch" and no more. Authored here, where the role is.
    access_name = "Highlight image bounds",
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
        checked = bool_field(1),
    ),
    access_value = bool_field(1),
    apply_key = aria_activate,
    event_name_derive,
)]
struct ImageView;

impl ImageView {
    /// Tuple-state introspect: SCXML state name + the bounds sidecar.
    fn read_state(scene: &Scene) -> (ToggleState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Ok(IntrospectValue::Text(name)) = intro.query("state") {
                    ToggleState::from_name_or_default(&name)
                } else {
                    ToggleState::Idle
                };
                let bounds = matches!(intro.query("value"), Ok(IntrospectValue::Bool(true)));
                return (state, bounds);
            }
        }
        (ToggleState::Idle, false)
    }

    fn view(state: (ToggleState, bool), frame: Frame) -> Scene {
        view(state.0, state.1, frame)
    }
}

fn main() {
    // Write the bundled icon to a stable temp path so the path-based
    // image cache can read it. Content is constant, so a concurrent
    // sweep writing the same bytes is harmless.
    let path = std::env::temp_dir().join("pinion-hello-image-quadrants.png");
    std::fs::write(&path, include_bytes!("../assets/icon.png"))
        .expect("write bundled icon to temp");
    let _ = ICON_PATH.set(path.to_string_lossy().into_owned());
    pinion_shell::run::<ImageView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::{AriaRole, WidgetA11y};
    use pinion_core::Owner;

    fn rendered(bounds: bool) -> Scene {
        Owner::new().run(|| view(ToggleState::Idle, bounds, Frame::new()))
    }

    #[test]
    fn grid_carries_all_four_fit_tags() {
        let scene = rendered(false);
        for tag in ["img_fill", "img_contain", "img_cover", "img_tile"] {
            assert!(scene.contains_tag(tag), "{tag} image present");
        }
        assert!(scene.contains_tag(TOGGLE_TAG), "toggle present");
    }

    #[test]
    fn images_carry_distinct_fit_policies() {
        // Walk the scene collecting (tag -> fit) for the image nodes.
        fn collect(scene: &Scene, out: &mut Vec<(String, Fit)>) {
            match scene {
                Scene::Image(i) => {
                    if let Some(tag) = i.tag.as_deref() {
                        out.push((tag.to_string(), i.style.fit));
                    }
                }
                Scene::Container(c) => c.children.iter().for_each(|ch| collect(ch, out)),
                _ => {}
            }
        }
        let scene = rendered(false);
        let mut fits = Vec::new();
        collect(&scene, &mut fits);
        assert_eq!(fits.len(), 4, "four image nodes");
        let find = |t: &str| fits.iter().find(|(tag, _)| tag == t).map(|(_, f)| *f);
        assert_eq!(find("img_fill"), Some(Fit::Fill));
        assert_eq!(find("img_contain"), Some(Fit::Contain));
        assert_eq!(find("img_cover"), Some(Fit::Cover));
        assert_eq!(find("img_tile"), Some(Fit::Tile));
    }

    #[test]
    fn cell_bounds_always_outlined_highlight_changes_color() {
        // The cell bounds are always outlined (informative without
        // interaction); the highlight switch only changes the outline
        // colour, so a border is present in both states.
        fn first_border_color(scene: &Scene) -> Option<pinion_core::style::Color> {
            match scene {
                Scene::Box(b) => b.style.border.map(|x| x.color),
                Scene::Container(c) => c.children.iter().find_map(first_border_color),
                _ => None,
            }
        }
        let off = first_border_color(&rendered(false)).expect("border off-state present");
        let on = first_border_color(&rendered(true)).expect("border on-state present");
        assert_ne!(off, on, "highlight switch changes the outline colour");
    }

    #[test]
    fn emits_switch_node() {
        let nodes =
            <ImageView as WidgetA11y>::access_node(&(ToggleState::Idle, true), Some(TOGGLE_TAG));
        assert!(!nodes.is_empty());
        assert_eq!(nodes[0].role, AriaRole::Switch);
    }

    /// ★★★★ R1692 — and it says WHICH switch. The toggle is a plain filled box
    /// with its caption beside it, so there is nothing under its tag for the
    /// scene derivation to read: before `access_name` this control announced
    /// "switch" and no more, which is the floor's exact failure.
    ///
    /// A counterfactual asked for this test: making the macro accept
    /// `access_name` and emit nothing left every crate green — the
    /// declared-and-dropped shape this tree has been bitten by before.
    #[test]
    fn r1692_the_switch_says_which_switch_it_is() {
        let nodes =
            <ImageView as WidgetA11y>::access_node(&(ToggleState::Idle, true), Some(TOGGLE_TAG));
        assert_eq!(nodes[0].name.as_deref(), Some("Highlight image bounds"));
        assert!(
            pinion_core::voice::NameFault::judge(
                &nodes[0].tag,
                nodes[0].name.as_deref().unwrap_or_default(),
                nodes[0].role.name_required(),
            )
            .is_none(),
            "and it is a name a reader can use",
        );
    }
}
