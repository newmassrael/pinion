//! `hello-window-focus-multi` — R1419/R1420 §5.39 §5.16.
//!
//! TWO OS windows (`main` + `inspector`) share ONE binding, and each dims only
//! when ITS OWN window loses OS keyboard focus. That is the whole point: the
//! paint-path OS-focus read is a window **IDENTITY**, not a binding-wide bool.
//!
//! ```ignore
//! // inside view_for_window(window_id, …):
//! let focused = window_focus_state::os_focused_window().as_deref() == Some(window_id);
//! ```
//!
//! `os_focused_window()` is binding-wide (one value naming WHICH of the binding's
//! OS windows the window manager has activated), so each window compares it
//! against its own id to answer "is *my* window focused". Had the read been a
//! bare `bool` (the shape PINION-PR73 first proposed), a two-window binding could
//! not tell its windows apart — this example is the concrete reason the shipped
//! read is `Option<String>`.
//!
//! Drive it headlessly with `scene/window_focus {window, focused}` (R1419) —
//! `tools/demos/r1419_window_focus_multi.py`: focus main → only inspector dims;
//! focus inspector → only main dims; blur → both dim.
//!
//! Multi-window scaffolding is the manual `WidgetView::windows()` +
//! `view_for_window` path (hello-multi-window's shape), since the `#[widget]`
//! macro has no multi-window override.
//!
//! R1427 §5.41 §5.39 — each window also carries a small terminal grid with a
//! visible BLINKING block cursor ([`CURSOR_GRID_TAG`]). The shell renders the
//! cursor filled and blinking on the OS-focused window, and as a steady HOLLOW
//! outline box on the unfocused one — the universal unfocused-terminal indicator
//! — driven purely by the per-window `cursor_focused` paint flag, so the VIEW is
//! identical in both windows. `tools/demos/r1427_cursor_focus.py` proves the
//! blink MODE is unchanged by focus (§2 #7 — OS focus is separate data via
//! `scene/input_state`, never folded into the cursor).

use pinion_core::CellMetric;
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, TextGridNode, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::term_grid::{CursorShape, GridBuffer, GridCursor, TermCell, TermColor};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, WindowSpec, vello_renderer_impl};
use pinion_widget_paint::button::{ButtonColors, ButtonStyle, use_hover_progress, view_button};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(
    HelloWindowFocusMultiRenderer,
    HelloWindowFocusMultiRendererError
);

const WIN_W: u32 = 300;
const WIN_H: u32 = 180;
const THEME_TAG: &str = "app";
const MAIN_BTN_TAG: &str = "main_btn";
const HOVER_ANIM_KEY: &str = "hello_window_focus_multi::hover";

/// The per-window status label tag. Both windows paint it; the RPC demo reads
/// each window's copy with `snapshot(window="…")`.
const STATUS_TAG: &str = "os_focus_status";

const MAIN: &str = "main";
const INSPECTOR: &str = "inspector";

/// The per-window terminal-cursor grid tag (R1427). Both windows paint it; the
/// RPC demo reads each window's copy to prove the blink MODE is unchanged by
/// focus (§2 #7 — focus is separate data via `scene/input_state`).
const CURSOR_GRID_TAG: &str = "focus_cursor_grid";

/// R1427 §5.41 §5.39 — a small terminal-cursor grid carrying a visible BLINKING
/// block cursor. The render (filled + blinking on the focused window, HOLLOW +
/// steady on the unfocused one) is driven entirely by the shell's per-window
/// `cursor_focused` paint flag — the view is identical in both windows, so this
/// example demonstrates the focus-gated cursor beside the R1419 per-window
/// dimming without the view reading focus for the cursor at all. The cursor mode
/// (`blink: true`) is the same in both windows; only the paint differs.
fn cursor_grid() -> Scene {
    let cell = |c: char| TermCell::new(c.to_string(), TermColor::Default, TermColor::Default);
    let cells = GridBuffer::new(6, 1)
        .with_row(0, "$ vim".chars().map(cell))
        // A visible blinking block cursor on the input column (col 5).
        .with_cursor(GridCursor::new(5, 0, CursorShape::Block, true).with_blink(true));
    Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(CURSOR_GRID_TAG)
            .with_cells(cells)
            .with_layout(LayoutStyle::new().with_size(Size::px(48, 16))),
    )
}

/// The label a window paints from the OS-focus read, keyed to WHETHER THIS
/// window holds OS focus. Free fn so the demo + tests share one spelling.
#[must_use]
fn status_text(window_id: &str, focused: bool) -> String {
    if focused {
        format!("{window_id}: OS-ACTIVE")
    } else {
        format!("{window_id}: blurred")
    }
}

/// The per-window view. Reads the binding-wide OS-focus mirror and compares it
/// against THIS window's id — the identity read that makes per-window dimming
/// possible.
fn view_window(window_id: &str, state: ButtonState) -> Scene {
    // R1419 §5.39 §5.16 — is MY window the OS-focused one?
    let focused =
        pinion_core::window_focus_state::os_focused_window().as_deref() == Some(window_id);

    let theme = use_theme(THEME_TAG).theme_animated();
    let surface = theme.resolve(ColorRole::Surface);
    let panel_bg = if focused {
        surface
    } else {
        surface.lerp(theme.resolve(ColorRole::OnSurface), 0.12)
    };
    let label_fg = if focused {
        theme.resolve(ColorRole::OnSurface)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };

    let status = Scene::Text(
        TextNode::styled(
            status_text(window_id, focused),
            Rect::default(),
            TextStyle::new().with_size_px(18).with_fg(label_fg),
        )
        .with_tag(STATUS_TAG),
    );

    // R1427 §5.41 §5.39 — both windows carry the blinking terminal cursor; the
    // shell renders it filled + blinking on the focused window and hollow +
    // steady on the unfocused one, purely from the per-window OS-focus fact.
    let mut children = vec![status, cursor_grid()];
    // The main window carries the focusable Button primary (the binding's one
    // routable surface); the inspector window is display-only.
    if window_id == MAIN {
        let hover = use_hover_progress(matches!(state, ButtonState::Hover), HOVER_ANIM_KEY);
        children.push(view_button(
            "focusable button",
            state,
            hover,
            &ButtonColors::filled_tonal(&theme),
            &ButtonStyle::m3_default(MAIN_BTN_TAG)
                .with_size(Size::px(180, 52))
                .with_label_font_size_px(15),
        ));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(panel_bg))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(16),
            ),
    )
}

struct WindowFocusMultiView;

impl WidgetCore for WindowFocusMultiView {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn tag() -> &'static str {
        MAIN_BTN_TAG
    }

    fn title() -> &'static str {
        "pinion hello-window-focus-multi (R1419/R1420 per-window OS focus)"
    }

    fn create_external() -> Box<dyn pinion_core::external::External> {
        Box::new(ButtonExternal::new())
    }

    fn read_state(scene: &Scene) -> Self::State {
        if let Some(node) = scene.find_external_with_tag(MAIN_BTN_TAG)
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return <Self::State as pinion_core::WidgetStateName>::from_name_or_default(&name);
        }
        ButtonState::Idle
    }

    fn event_name(event: Self::Event) -> &'static str {
        <Self::Event as pinion_core::WidgetEventName>::as_name(&event)
    }

    fn view(state: Self::State, _frame: &Frame) -> Scene {
        // Single-window fallback (tests / unscoped RPC) — the AppShell live
        // loop always calls `view_for_window`.
        view_window(MAIN, state)
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }
}

impl pinion_a11y::WidgetA11y for WindowFocusMultiView {}

impl WidgetView for WindowFocusMultiView {
    type Renderer = HelloWindowFocusMultiRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }

    fn windows() -> Vec<WindowSpec> {
        vec![
            WindowSpec::new(
                MAIN,
                "hello-window-focus-multi — Main",
                SizeStrategy::Fixed {
                    width: WIN_W,
                    height: WIN_H,
                },
            ),
            WindowSpec::new(
                INSPECTOR,
                "hello-window-focus-multi — Inspector",
                SizeStrategy::Fixed {
                    width: WIN_W,
                    height: WIN_H,
                },
            ),
        ]
    }

    fn view_for_window(window_id: &str, state: Self::State, _frame: &Frame) -> Scene {
        view_window(window_id, state)
    }
}

fn main() {
    pinion_shell::run::<WindowFocusMultiView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene_status(scene: &Scene) -> Option<String> {
        match scene {
            Scene::Text(t) if t.tag.as_deref() == Some(STATUS_TAG) => Some(t.content.clone()),
            Scene::Container(c) => c.children.iter().find_map(scene_status),
            _ => None,
        }
    }

    /// The identity read makes per-window dimming: with OS focus on `main`, the
    /// main window reads ACTIVE and the inspector reads blurred FROM THE SAME
    /// binding-wide mirror — because each compares the id against its own window.
    #[test]
    fn each_window_reflects_only_its_own_os_focus() {
        let owner = pinion_core::Owner::new();
        owner.os_focused_window_signal().set(Some(MAIN.to_owned()));

        let main_scene = owner.run(|| view_window(MAIN, ButtonState::Idle));
        let insp_scene = owner.run(|| view_window(INSPECTOR, ButtonState::Idle));
        assert_eq!(
            scene_status(&main_scene).as_deref(),
            Some("main: OS-ACTIVE")
        );
        assert_eq!(
            scene_status(&insp_scene).as_deref(),
            Some("inspector: blurred"),
            "the inspector dims while main holds OS focus — per-window identity",
        );

        // Move OS focus to the inspector: the roles swap, same mirror.
        owner
            .os_focused_window_signal()
            .set(Some(INSPECTOR.to_owned()));
        let main_scene = owner.run(|| view_window(MAIN, ButtonState::Idle));
        let insp_scene = owner.run(|| view_window(INSPECTOR, ButtonState::Idle));
        assert_eq!(scene_status(&main_scene).as_deref(), Some("main: blurred"));
        assert_eq!(
            scene_status(&insp_scene).as_deref(),
            Some("inspector: OS-ACTIVE"),
        );

        // Whole-application blur: both dim.
        owner.os_focused_window_signal().set(None);
        let main_scene = owner.run(|| view_window(MAIN, ButtonState::Idle));
        let insp_scene = owner.run(|| view_window(INSPECTOR, ButtonState::Idle));
        assert_eq!(scene_status(&main_scene).as_deref(), Some("main: blurred"));
        assert_eq!(
            scene_status(&insp_scene).as_deref(),
            Some("inspector: blurred")
        );
    }

    #[test]
    fn main_window_carries_the_button_primary() {
        let owner = pinion_core::Owner::new();
        let main_scene = owner.run(|| view_window(MAIN, ButtonState::Idle));
        assert!(
            main_scene.contains_tag(MAIN_BTN_TAG),
            "the main window paints the routable Button primary",
        );
        let insp_scene = owner.run(|| view_window(INSPECTOR, ButtonState::Idle));
        assert!(
            !insp_scene.contains_tag(MAIN_BTN_TAG),
            "the inspector window is display-only",
        );
    }

    #[test]
    fn both_windows_carry_the_same_blinking_cursor_regardless_of_focus() {
        // R1427 — the terminal cursor's MODE (blinking block) is identical in
        // both windows and independent of OS focus: the focus-driven hollow /
        // stop-blink is a paint concern, never scene data (§2 #7). So the view
        // is byte-identical whether or not the window holds focus.
        use pinion_core::scene::Scene as S;
        fn grid_cursor(scene: &S) -> Option<GridCursor> {
            match scene {
                S::TextGrid(n) if n.tag.as_deref() == Some(CURSOR_GRID_TAG) => {
                    Some(n.cells().cursor())
                }
                S::Container(c) => c.children.iter().find_map(grid_cursor),
                _ => None,
            }
        }
        let owner = pinion_core::Owner::new();
        // Focus main: BOTH windows still carry the blinking block cursor mode.
        owner.os_focused_window_signal().set(Some(MAIN.to_owned()));
        let main_cur = grid_cursor(&owner.run(|| view_window(MAIN, ButtonState::Idle)))
            .expect("main carries the cursor grid");
        let insp_cur = grid_cursor(&owner.run(|| view_window(INSPECTOR, ButtonState::Idle)))
            .expect("inspector carries the cursor grid");
        assert!(
            main_cur.blink && main_cur.visible,
            "focused window: blinking mode"
        );
        assert!(
            insp_cur.blink && insp_cur.visible,
            "unfocused window carries the SAME blinking mode — focus is not scene data",
        );
        assert_eq!(main_cur, insp_cur, "the cursor data is focus-independent");
        assert_eq!(main_cur.shape, CursorShape::Block);
    }
}
