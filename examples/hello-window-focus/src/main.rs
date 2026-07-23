//! `hello-window-focus` — R1419 §5.39 §5.16 first consumer of the
//! OS-window-focus reactive read.
//!
//! The whole point of this binding is one line in [`view`]:
//!
//! ```ignore
//! let os_focus = pinion_core::window_focus_state::os_focused_window();
//! ```
//!
//! `os_focused_window()` is the paint-path READ of which OS window holds the
//! OS keyboard focus (the peer of `focus_state::focused()` on the OS-focus
//! axis). Reading it inside the view fn auto-subscribes this binding, so when
//! the application gains or loses OS focus — the user alt-tabs away and back —
//! the view re-runs and repaints: the panel dims on blur and the status label
//! re-renders. This is the OS-focus counterpart of a widget dimming when its
//! window is deactivated (vim's `FocusLost`, a file manager greying its
//! selection), expressed as a pure reactive read rather than a callback.
//!
//! In production the mirror is driven by winit `WindowEvent::Focused`. Headless
//! (the RPC demo), the `scene/window_focus` drive method simulates that edge so
//! the reactive read can be exercised without a window manager — see
//! `tools/demos/r1419_window_focus.py`.
//!
//! The [`ButtonExternal`] primary is scaffolding (a focusable surface to satisfy
//! `WidgetCore`); the demonstration is the OS-focus-reactive panel around it.

use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::button::{ButtonColors, ButtonStyle, use_hover_progress, view_button};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloWindowFocusRenderer, HelloWindowFocusRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 220;
const THEME_TAG: &str = "app";
const BTN_W: u32 = 200;
const BTN_H: u32 = 64;
const HOVER_ANIM_KEY: &str = "hello_window_focus::hover_progress";

/// The status-label tag the RPC demo reads back to observe the reactive
/// re-render (`scene/snapshot` / `scene/query`).
const STATUS_TAG: &str = "os_focus_status";

/// The label a binding paints from the OS-focus read. Kept a free fn so the
/// demo and the regression test assert against the same spelling.
#[must_use]
fn status_text(os_focus: Option<&str>) -> String {
    match os_focus {
        Some(window) => format!("OS focus: {window}"),
        None => "OS focus: (blurred)".to_owned(),
    }
}

/// view-fn (§6.3): pure sync `ButtonState` → `Scene`, plus the R1419 reactive
/// read. `os_focused_window()` auto-subscribes this view, so an OS focus / blur
/// edge re-runs it.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    // R1419 §5.39 §5.16 — the reactive OS-window-focus read. `Some(window)` when
    // this application holds OS keyboard focus (named by window id), `None` when
    // it is blurred (the user alt-tabbed away) or the OS-focus state is unknown.
    let os_focus = pinion_core::window_focus_state::os_focused_window();
    let focused = os_focus.is_some();

    let theme = use_theme(THEME_TAG).theme_animated();
    let surface = theme.resolve(ColorRole::Surface);
    // Blur dims the panel toward the foreground tone — the textbook "this window
    // is inactive" cue. Focused = full Surface.
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
            status_text(os_focus.as_deref()),
            Rect::default(),
            TextStyle::new().with_size_px(18).with_fg(label_fg),
        )
        .with_tag(STATUS_TAG),
    );

    let hover_progress = use_hover_progress(matches!(state, ButtonState::Hover), HOVER_ANIM_KEY);
    let button = view_button(
        "focusable button",
        state,
        hover_progress,
        focused,
        &ButtonColors::filled_tonal(&theme),
        &ButtonStyle::m3_default("main_btn")
            .with_size(Size::px(BTN_W, BTN_H))
            .with_label_font_size_px(16),
    );

    Scene::Container(
        ContainerNode::new(vec![status, button])
            .with_style(BoxStyle::filled(panel_bg))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(20),
            ),
    )
}

/// `WidgetView` binding. The Button primary is scaffolding; the R1419
/// demonstration lives in [`view`]'s `os_focused_window()` read.
#[widget(
    tag = "main_btn",
    state = ButtonState,
    event = ButtonEvent,
    title = "pinion hello-window-focus (R1419 §5.39 §5.16 OS-focus reactive read)",
    renderer = HelloWindowFocusRenderer,
    initial_size = (WIN_W, WIN_H),
    external = ButtonExternal::new,
    role = Button,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
    ),
    apply_key,
    // Derive `read_state` + `event_name` from ButtonState/ButtonEvent's
    // WidgetStateName / WidgetEventName impls (R643). Without this the macro
    // forwards `read_state` to an inherent method this binding does not define,
    // which resolves back to the trait method → infinite recursion at boot.
    state_name_derive,
)]
struct WindowFocusView;

impl WindowFocusView {
    fn view(state: ButtonState, frame: Frame) -> Scene {
        view(state, &frame)
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

fn main() {
    pinion_shell::run::<WindowFocusView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reactive read drives the label: an unfocused binding (no OS focus
    /// published) reads `None` → the blurred label; publishing a window id →
    /// the focused label naming it. Reads through a real `Owner` scope, the way
    /// the shell runs the view.
    #[test]
    fn view_reflects_os_focus_edges() {
        let owner = pinion_core::Owner::new();
        // Boot: mirror is empty → blurred label.
        let blurred = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        assert!(scene_has_status(&blurred, "OS focus: (blurred)"));

        // Publish OS focus for "main" (what the shell's `set_os_focused_window`
        // does on a winit `Focused(true)`); the view names it.
        owner
            .os_focused_window_signal()
            .set(Some("main".to_owned()));
        let focused = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        assert!(scene_has_status(&focused, "OS focus: main"));

        // Blur again → back to the blurred label.
        owner.os_focused_window_signal().set(None);
        let reblurred = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        assert!(scene_has_status(&reblurred, "OS focus: (blurred)"));
    }

    #[test]
    fn status_text_spelling() {
        assert_eq!(status_text(Some("inspector")), "OS focus: inspector");
        assert_eq!(status_text(None), "OS focus: (blurred)");
    }

    fn scene_has_status(scene: &Scene, expected: &str) -> bool {
        match scene {
            Scene::Text(t) => t.tag.as_deref() == Some(STATUS_TAG) && t.content == expected,
            Scene::Container(c) => c.children.iter().any(|c| scene_has_status(c, expected)),
            _ => false,
        }
    }
}
