//! `hello-filedrop` — R770 §5.15 **OS file drag-drop first consumer**,
//! R1437 §5.16 **two peer drop targets in two windows**.
//!
//! Drag a file from the OS file manager onto either window: that window's
//! drop-zone highlights while a file hovers over it (winit `HoveredFile`),
//! clears if the drag leaves (`HoveredFileCancelled`), and appends the
//! dropped path to *its own* list on release (`DroppedFile`). The same
//! three transitions are drivable head-less through the
//! `scene/hover_file`, `scene/hover_file_cancel`, and `scene/drop_file`
//! RPC peers — §2 invariant #2 (every input a human makes has an RPC
//! peer) — each scoped by the shared `{window: "<id>"}` param.
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
//! ## Why two windows (R1437)
//!
//! "The drop target is the window" is only a *contract* if the binding can
//! tell the windows apart. Both windows here are peers running the same
//! view — an inbox and an archive — with one drop list each, keyed by
//! [`WindowSpec::id`]. A drop on the archive must land in the archive even
//! when the inbox is the focused window, which is exactly the case a
//! focus-based fallback gets wrong: X11 / Wayland DND does not focus a
//! window before delivering the drop.
//!
//! ## AI-first witness (§2 #7 scene-as-data)
//!
//! No pixels required: `scene/snapshot` reports the drop-zone container's
//! fill (idle `SurfaceContainerHighest` vs hovering `Accent`) and the
//! dropped paths as `Text` nodes, per window. The R770 demo drives the
//! full hover → cancel → drop arc over RPC and reads the result as data;
//! the R1437 demo drives both windows and asserts neither leaks into the
//! other.

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
use pinion_shell::{SizeStrategy, WidgetView, WindowSpec, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloFileDropRenderer, HelloFileDropRendererError);

/// The primary window — the id `DEFAULT_WINDOW` resolves to, so an RPC
/// frame that names no window addresses this one.
const MAIN: &str = "main";
/// The peer window. Not a dialog or an inspector *of* the main window: a
/// second, equal drop target, which is what makes a mis-routed drop
/// observable (its list is the one that must stay empty).
const ARCHIVE: &str = "archive";
/// Paint tag for the drop-zone container (the demo reads its fill +
/// dropped-path `Text` children via `scene/snapshot`). The same tag in
/// both windows — each window's snapshot is its own tree, and R813
/// contributes AT nodes per window, so the tag stays the stable address
/// of "the drop zone of the window you asked about".
const DROP_ZONE_TAG: &str = "drop_zone";
/// Shared [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key.
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

/// `Owner::cache`-keyed accessor for one window's [`DropState`] —
/// resolves the same `Rc` in the view fn and in the file hooks (both run
/// owner-wrapped), so the painted tree and the hook mutations share one
/// state. R1437: the key carries `window_id`, so each window owns an
/// independent hover flag + path list. `Owner::cache` takes
/// `Cow<'static, str>`, so the runtime-built key allocates and needs no
/// `Box::leak`.
fn use_drop_state(window_id: &str) -> Rc<DropState> {
    Owner::current()
        .expect("use_drop_state requires an active Owner scope")
        .cache(format!("filedrop:{window_id}"), || DropState {
            hovering: Signal::new(false),
            paths: Signal::new(Vec::new()),
        })
}

/// Human label for a window. Unknown ids read as the inbox — the shell
/// only ever passes an id from [`WidgetView::windows`], so this is a
/// fail-open default, not a routing decision.
fn window_label(window_id: &str) -> &'static str {
    if window_id == ARCHIVE {
        "Archive"
    } else {
        "Inbox"
    }
}

/// View: a title naming the window, the drop-zone container (fill +
/// content reflect that window's live hover / dropped state), and a
/// status line counting that window's drops.
fn view_window(window_id: &str) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let drop = use_drop_state(window_id);
    let hovering = drop.hovering.get();
    let paths = drop.paths.get();

    let title = Scene::Text(TextNode::styled(
        format!("File drop zone — {}", window_label(window_id)),
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

    /// The single-window view is the primary window's view — the shell's
    /// [`WidgetView::view_for_window`] default forwards here for any
    /// binding that does not split per window, so `MAIN` is the honest
    /// answer rather than a window-less variant that paints neither list.
    fn view(_state: (), _frame: &Frame) -> Scene {
        view_window(MAIN)
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
        zone_access_node(MAIN)
    }
}

/// R813 §5.40 §5.16 — one window's AT contribution. Each window paints
/// its own zone, so each must name its own hover state: an AT user on the
/// archive window must not hear the inbox's "release to drop".
fn zone_access_node(window_id: &str) -> Vec<AccessNode> {
    let label = window_label(window_id);
    let name = if use_drop_state(window_id).hovering.get() {
        format!("{label} file drop zone (release to drop)")
    } else {
        format!("{label} file drop zone")
    };
    vec![AccessNode::new(DROP_ZONE_TAG, AriaRole::Group).with_name(name)]
}

impl WidgetView for FileDropView {
    type Renderer = HelloFileDropRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }

    fn windows() -> Vec<WindowSpec> {
        let size = SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        };
        vec![
            WindowSpec::new(MAIN, "hello-filedrop — Inbox", size),
            WindowSpec::new(ARCHIVE, "hello-filedrop — Archive", size),
        ]
    }

    fn view_for_window(window_id: &str, _state: Self::State, _frame: &Frame) -> Scene {
        view_window(window_id)
    }

    fn access_node_for_window(
        window_id: &str,
        _state: &(),
        _focused: Option<&str>,
    ) -> Vec<AccessNode> {
        zone_access_node(window_id)
    }

    /// A file is dragged over `window_id` — light up *that* window's zone.
    fn on_file_hover(window_id: &str, _state: &(), _path: &str) -> bool {
        use_drop_state(window_id).hovering.set(true);
        true
    }

    /// The drag left `window_id` without dropping — clear its affordance.
    /// The other window's zone is untouched: a drag that crosses from one
    /// window to the other cancels on the first and hovers on the second.
    fn on_file_hover_cancel(window_id: &str, _state: &()) -> bool {
        use_drop_state(window_id).hovering.set(false);
        true
    }

    /// A file was dropped on `window_id` — append its path to that
    /// window's list and clear that window's hover state.
    fn on_file_drop(window_id: &str, _state: &(), path: &str) -> bool {
        let drop = use_drop_state(window_id);
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
    use super::{ARCHIVE, FileDropView, MAIN, use_drop_state, view_window, zone_access_node};
    use pinion_core::Scene;
    use pinion_core::reactive::Owner;
    use pinion_shell::WidgetView;

    #[test]
    fn r770_view_renders_without_panicking() {
        let owner = Owner::new();
        owner.run(|| {
            let n = collect_texts(&view_window(MAIN)).len();
            assert!(n >= 2, "title + zone hint + status render");
        });
    }

    #[test]
    fn r770_on_file_drop_appends_path_and_clears_hover() {
        let owner = Owner::new();
        owner.run(|| {
            let drop = use_drop_state(MAIN);
            assert!(FileDropView::on_file_hover(MAIN, &(), "/tmp/a.txt"));
            assert!(drop.hovering.get(), "hover lights the zone");
            assert!(FileDropView::on_file_drop(MAIN, &(), "/tmp/a.txt"));
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
            let drop = use_drop_state(MAIN);
            FileDropView::on_file_hover(MAIN, &(), "/tmp/b.txt");
            assert!(drop.hovering.get());
            assert!(FileDropView::on_file_hover_cancel(MAIN, &()));
            assert!(!drop.hovering.get(), "cancel clears hover");
            assert!(drop.paths.get().is_empty(), "cancel drops nothing");
        });
    }

    /// R1437 — the routing claim itself: a drop carrying the archive's id
    /// lands in the archive and NOWHERE else. Pre-R1437 the hook could not
    /// have made this distinction, because the id never reached it.
    #[test]
    fn r1437_drop_lands_only_in_the_addressed_window() {
        let owner = Owner::new();
        owner.run(|| {
            let inbox = use_drop_state(MAIN);
            let archive = use_drop_state(ARCHIVE);

            assert!(FileDropView::on_file_drop(ARCHIVE, &(), "/tmp/old.log"));
            assert_eq!(
                archive.paths.get(),
                vec!["/tmp/old.log".to_string()],
                "the addressed window received the drop"
            );
            assert!(
                inbox.paths.get().is_empty(),
                "the peer window received nothing"
            );

            assert!(FileDropView::on_file_drop(MAIN, &(), "/tmp/new.txt"));
            assert_eq!(
                inbox.paths.get(),
                vec!["/tmp/new.txt".to_string()],
                "the inbox keeps its own list"
            );
            assert_eq!(
                archive.paths.get().len(),
                1,
                "the archive list did not grow from the inbox drop"
            );
        });
    }

    /// R1437 — the hover affordance is per window too: lighting one zone
    /// leaves the other idle, and cancelling one does not clear the other.
    #[test]
    fn r1437_hover_and_cancel_are_scoped_to_their_window() {
        let owner = Owner::new();
        owner.run(|| {
            let inbox = use_drop_state(MAIN);
            let archive = use_drop_state(ARCHIVE);

            FileDropView::on_file_hover(MAIN, &(), "/tmp/a.txt");
            FileDropView::on_file_hover(ARCHIVE, &(), "/tmp/a.txt");
            assert!(inbox.hovering.get(), "inbox lit");
            assert!(archive.hovering.get(), "archive lit");

            assert!(FileDropView::on_file_hover_cancel(MAIN, &()));
            assert!(!inbox.hovering.get(), "cancel cleared the inbox");
            assert!(
                archive.hovering.get(),
                "the archive stayed lit — cancel is window-scoped"
            );
        });
    }

    /// R1437 — the paint scene and the AT tree read the same per-window
    /// state, so the window an AI (or a screen reader) inspects reports
    /// the drops that window received.
    #[test]
    fn r1437_paint_and_at_agree_per_window() {
        let owner = Owner::new();
        owner.run(|| {
            FileDropView::on_file_drop(ARCHIVE, &(), "/tmp/only-archive.bin");

            let archive_texts = collect_texts(&view_window(ARCHIVE));
            let inbox_texts = collect_texts(&view_window(MAIN));
            assert!(
                archive_texts.iter().any(|t| t == "/tmp/only-archive.bin"),
                "archive paints its dropped path"
            );
            assert!(
                !inbox_texts.iter().any(|t| t == "/tmp/only-archive.bin"),
                "inbox does not paint the archive's path"
            );
            assert!(
                archive_texts.iter().any(|t| t == "1 file(s) dropped"),
                "archive counts one drop"
            );
            assert!(
                inbox_texts.iter().any(|t| t == "0 file(s) dropped"),
                "inbox counts none"
            );
            assert!(
                archive_texts.iter().any(|t| t.contains("Archive")),
                "each window titles itself"
            );

            FileDropView::on_file_hover(ARCHIVE, &(), "/tmp/x");
            let archive_at = zone_access_node(ARCHIVE);
            let inbox_at = zone_access_node(MAIN);
            assert_eq!(archive_at.len(), 1, "one zone node per window");
            assert_eq!(
                archive_at[0].name.as_deref(),
                Some("Archive file drop zone (release to drop)"),
                "the AT name reports the hovered window's own state"
            );
            assert_eq!(
                inbox_at[0].name.as_deref(),
                Some("Inbox file drop zone"),
                "the peer window's AT name stays idle"
            );
        });
    }

    fn walk_texts(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => {
                for child in &c.children {
                    walk_texts(child, out);
                }
            }
            _ => {}
        }
    }

    fn collect_texts(scene: &Scene) -> Vec<String> {
        let mut out = Vec::new();
        walk_texts(scene, &mut out);
        out
    }
}
