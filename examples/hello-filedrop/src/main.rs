//! `hello-filedrop` — R770 §5.15 **OS file drag-drop first consumer**.
//!
//! Drag a file from the OS file manager onto the window: the drop-zone
//! highlights while a file hovers over it (winit `HoveredFile`), clears
//! if the drag leaves (`HoveredFileCancelled`), and appends the dropped
//! path to a list on release (`DroppedFile`). The same three transitions
//! are drivable head-less through the `scene/hover_file`,
//! `scene/hover_file_cancel`, and `scene/drop_file` RPC peers — §2
//! invariant #2 (every input a human makes has an RPC peer).
//!
//! ## Why a dedicated binding
//!
//! winit's file drag-drop is **window-scoped**: the OS reports the dragged
//! path but no drop coordinate, so the drop target is the window, not a
//! sub-widget (unlike pointer drag-drop, R660/R742). The framework surfaces
//! this as three [`WidgetView`] hooks — `on_file_hover` /
//! `on_file_hover_cancel` / `on_file_drop` — each run in the root-owner
//! scope so a binding mutates reactive state and repaints.
//!
//! ## AI-first witness (§2 #7 scene-as-data)
//!
//! No pixels required: `scene/snapshot` reports the drop-zone container's
//! fill (idle `SurfaceContainerHighest` vs hovering `Accent`) and the
//! dropped paths as `Text` nodes. The R770 demo drives the full
//! hover → cancel → drop arc over RPC and reads the result as data.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::{External, StubExternal};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloFileDropRenderer, HelloFileDropRendererError);

/// Reactive-cache key for the drop-zone state.
const DROP_KEY: &str = "filedrop";
/// Paint tag for the drop-zone container (the demo reads its fill +
/// dropped-path `Text` children via `scene/snapshot`).
const DROP_ZONE_TAG: &str = "drop_zone";
/// Shared [`ThemeProvider`] cache key.
const THEME_TAG: &str = "app";

const WIN_W: u32 = 480;
const WIN_H: u32 = 320;
const TITLE_FONT_SIZE_PX: u32 = 18;
const BODY_FONT_SIZE_PX: u32 = 14;
const STATUS_FONT_SIZE_PX: u32 = 12;
const ROW_GAP: u32 = 12;

/// R770 §5.15 — reactive drop-zone state: whether a file is currently
/// hovering, and the list of paths dropped so far. Both are `Signal`s so
/// the view fn re-runs when a file hook mutates them (the same content-
/// state shape the text-edit widgets use).
struct DropState {
    hovering: Signal<bool>,
    paths: Signal<Vec<String>>,
}

/// `Owner::cache`-keyed accessor for the [`DropState`] — resolves the
/// same `Rc` in the view fn and in the file hooks (both run owner-
/// wrapped), so the painted tree and the hook mutations share one state.
fn use_drop_state(key: &'static str) -> Rc<DropState> {
    Owner::current()
        .expect("use_drop_state requires an active Owner scope")
        .cache(key, || DropState {
            hovering: Signal::new(false),
            paths: Signal::new(Vec::new()),
        })
}

/// View: a title, the drop-zone container (fill + content reflect the
/// live hover / dropped state), and a status line counting drops.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "view-fn shape mirrors the WidgetCore::view(&Frame) trait signature"
)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let drop = use_drop_state(DROP_KEY);
    let hovering = drop.hovering.get();
    let paths = drop.paths.get();

    let title = Scene::Text(TextNode::styled(
        "File drop zone",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_SIZE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    // Zone fill diverges idle vs hovering so the hover state is
    // introspectable as scene data (no pixels needed) and visible.
    let (zone_fill, body_fg) = if hovering {
        (
            theme.resolve(ColorRole::Accent),
            theme.resolve(ColorRole::OnAccent),
        )
    } else {
        (
            theme.resolve(ColorRole::SurfaceContainerHighest),
            theme.resolve(ColorRole::OnSurface),
        )
    };

    let zone_children: Vec<Scene> = if hovering {
        vec![zone_text("Release to drop", body_fg)]
    } else if paths.is_empty() {
        vec![zone_text(
            "Drag a file here",
            theme.resolve(ColorRole::OnSurfaceMuted),
        )]
    } else {
        paths.iter().map(|p| zone_text(p, body_fg)).collect()
    };

    let zone = Scene::Container(
        ContainerNode::new(zone_children)
            .with_tag(DROP_ZONE_TAG)
            .with_style(
                BoxStyle::filled(zone_fill)
                    .with_corner_radius(10)
                    .with_border(pinion_core::style::Border::new(
                        theme.resolve(ColorRole::Outline),
                        2,
                    )),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_size(Size::px(WIN_W - 48, WIN_H - 130))
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(6),
            ),
    );

    let status = Scene::Text(TextNode::styled(
        format!("{} file(s) dropped", paths.len()),
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_SIZE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, zone, status])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    )
}

/// A body-styled `Text` line inside the drop zone.
fn zone_text(content: impl Into<String>, fg: Color) -> Scene {
    Scene::Text(TextNode::styled(
        content.into(),
        Rect::default(),
        TextStyle::new().with_size_px(BODY_FONT_SIZE_PX).with_fg(fg),
    ))
}

struct FileDropView;

impl WidgetCore for FileDropView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    fn tag() -> &'static str {
        DROP_ZONE_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-filedrop (R770 §5.15)"
    }
}

impl WidgetA11y for FileDropView {
    /// The drop zone is a WAI-ARIA `group` named for its purpose; the
    /// dropped paths are conveyed as the painted `Text` children
    /// (scene-as-data), so no per-path node is synthesised this slice.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let drop = use_drop_state(DROP_KEY);
        let name = if drop.hovering.get() {
            "File drop zone (release to drop)"
        } else {
            "File drop zone"
        };
        vec![AccessNode::new(DROP_ZONE_TAG, AriaRole::Group).with_name(name)]
    }
}

impl WidgetView for FileDropView {
    type Renderer = HelloFileDropRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }

    /// A file is dragged over the window — light up the zone.
    fn on_file_hover(_state: &(), _path: &str) -> bool {
        use_drop_state(DROP_KEY).hovering.set(true);
        true
    }

    /// The drag left without dropping — clear the affordance.
    fn on_file_hover_cancel(_state: &()) -> bool {
        use_drop_state(DROP_KEY).hovering.set(false);
        true
    }

    /// A file was dropped — append its path and clear the hover state.
    fn on_file_drop(_state: &(), path: &str) -> bool {
        let drop = use_drop_state(DROP_KEY);
        let mut paths = drop.paths.get();
        paths.push(path.to_owned());
        pinion_core::reactive::batch(|| {
            drop.paths.set(paths);
            drop.hovering.set(false);
        });
        true
    }
}

fn main() {
    pinion_shell::run::<FileDropView>();
}

#[cfg(test)]
mod tests {
    use super::{DROP_KEY, FileDropView, use_drop_state, view};
    use pinion_core::reactive::Owner;
    use pinion_core::{Frame, Scene};
    use pinion_shell::WidgetView;

    fn count_text_nodes(scene: &Scene, out: &mut usize) {
        if let Scene::Text(_) = scene {
            *out += 1;
        }
        if let Scene::Container(c) = scene {
            for child in &c.children {
                count_text_nodes(child, out);
            }
        }
    }

    #[test]
    fn r770_view_renders_without_panicking() {
        let owner = Owner::new();
        owner.run(|| {
            let scene = view((), &Frame::default());
            let mut n = 0;
            count_text_nodes(&scene, &mut n);
            assert!(n >= 2, "title + zone hint + status render");
        });
    }

    #[test]
    fn r770_on_file_drop_appends_path_and_clears_hover() {
        let owner = Owner::new();
        owner.run(|| {
            let drop = use_drop_state(DROP_KEY);
            assert!(FileDropView::on_file_hover(&(), "/tmp/a.txt"));
            assert!(drop.hovering.get(), "hover lights the zone");
            assert!(FileDropView::on_file_drop(&(), "/tmp/a.txt"));
            assert_eq!(
                drop.paths.get(),
                vec!["/tmp/a.txt".to_string()],
                "path appended"
            );
            assert!(!drop.hovering.get(), "drop clears the hover state");
        });
    }

    #[test]
    fn r770_on_file_hover_cancel_clears_without_dropping() {
        let owner = Owner::new();
        owner.run(|| {
            let drop = use_drop_state(DROP_KEY);
            FileDropView::on_file_hover(&(), "/tmp/b.txt");
            assert!(drop.hovering.get());
            assert!(FileDropView::on_file_hover_cancel(&()));
            assert!(!drop.hovering.get(), "cancel clears hover");
            assert!(drop.paths.get().is_empty(), "cancel drops nothing");
        });
    }
}
