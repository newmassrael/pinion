//! `hello-disclosure` — R696 §5.38 §5.50 WAI-ARIA disclosure
//! (show/hide) widget on the pinion-shell substrate.
//!
//! The disclosure is the atomic unit of an accordion: a header button
//! that reveals or hides an associated content panel. Per the WAI-ARIA
//! APG "Disclosure (Show/Hide)" pattern the header is a `button`
//! carrying `aria-expanded` — pinion's R696 `AccessState::expanded`
//! axis, distinct from a checkbox's `aria-checked` value.
//!
//! This binary is the first consumer of two R696 substrate pieces:
//! the `pinion-core::widgets::disclosure` statechart (a Checkbox-shape
//! four-state button-toggle whose sidecar tracks `expanded` and whose
//! activate emits an `"expanded"` intent) and the `#[widget]`
//! `state_flags(expanded = bool_field(1))` derive extension, which
//! lowers the sidecar bool into `aria-expanded` with no hand-written
//! `access_node`.
//!
//! Visual contract: a 320-px header row (`U+25B6`/`U+25BC` twisty +
//! summary label) on an M3 `SurfaceContainerHigh` state-layer ramp,
//! with the content panel appearing beneath only while expanded.

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{Rect, TextNode};
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle};
use pinion_core::theme::{use_theme, ColorRole};
#[cfg(test)]
use pinion_core::theme::Theme;
use pinion_core::widgets::disclosure::{DisclosureEvent, DisclosureExternal, DisclosureState};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
#[cfg(test)]
use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::disclosure::{view_disclosure, DisclosureStyle};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloDisclosureRenderer, HelloDisclosureRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 320;
/// [`ThemeProvider`] cache key — matches the `"app"` convention shared
/// across the example gallery.
const THEME_TAG: &str = "app";
const SUMMARY: &str = "Advanced settings";
const BODY: &str = "Auto-save drafts every 30 seconds.";

/// view-fn (§6.3): pure sync mapping `(DisclosureState, bool) -> Scene`.
///
/// The M3 disclosure composition (twisty + summary header + revealed
/// panel + state-layer ramp) lives in
/// [`pinion_widget_paint::disclosure::view_disclosure`]; the binding
/// resolves the theme, supplies the dispatch tag, the M3 style
/// defaults, the summary, and the panel body, then centers the section
/// in the window with the binding-owned surface fill.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: DisclosureState, expanded: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let body = Scene::Text(
        TextNode::styled(
            BODY,
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        // Tagged so the AI-first demo can confirm the panel body is
        // present in the scene only while expanded (`scene/bbox`).
        .with_tag("section_body"),
    );
    let section = view_disclosure(
        "main_disclosure",
        state,
        expanded,
        &theme,
        &DisclosureStyle::m3(),
        SUMMARY,
        body,
    );
    Scene::Container(
        pinion_core::scene::ContainerNode::new(vec![section])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// R696 §5.16 — `#[widget]` derive. `role = Button` +
/// `state_flags(expanded = bool_field(1))` derives the single-node
/// `WidgetCore` / `WidgetA11y` / `WidgetView` trio with `aria-expanded`
/// taken from the `(DisclosureState, bool)` sidecar. No `access_value`:
/// a disclosure button has no *value*, only the `aria-expanded` state
/// axis (distinct from Checkbox, which lowers `aria-checked` through
/// both `state.checked` and an `AccessValue::Bool`). No reducer —
/// the disclosure carries no theme-flip side effect.
#[widget(
    tag = "main_disclosure",
    state = (DisclosureState, bool),
    event = DisclosureEvent,
    title = "pinion hello-disclosure (R696 §5.16 aria-expanded)",
    renderer = HelloDisclosureRenderer,
    initial_size = (WIN_W, WIN_H),
    external = DisclosureExternal::new,
    role = Button,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
        expanded = bool_field(1),
    ),
    apply_key,
    keybinding,
    fmt_state_log,
)]
struct DisclosureView;

impl DisclosureView {
    fn read_state(scene: &Scene) -> (DisclosureState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    DisclosureState::from_name_or_default(&name)
                } else {
                    DisclosureState::Idle
                };
                let expanded =
                    matches!(intro.query("expanded"), Some(IntrospectValue::Bool(true)));
                return (state, expanded);
            }
        }
        (DisclosureState::Idle, false)
    }

    fn view(state: (DisclosureState, bool), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
    }

    fn event_name(event: DisclosureEvent) -> &'static str {
        // R699 §5.16 — route the forward Event->name mapping through the
        // WidgetEventName SSOT (`as_name`), retiring the hand-written
        // match table. `as_name` is total over internal variants too;
        // only external events reach this path via `ShellCore::forward`.
        pinion_core::WidgetEventName::as_name(&event)
    }

    fn keybinding(key: &str) -> Option<DisclosureEvent> {
        match key {
            "d" => Some(DisclosureEvent::Disable),
            "e" => Some(DisclosureEvent::Enable),
            _ => None,
        }
    }

    /// WAI-ARIA APG disclosure keyboard model: when the header button
    /// has focus, **both** `Enter` and `Space` toggle the disclosure
    /// (unlike a pure ARIA checkbox, which accepts only `Space`).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(Self::tag()) {
            return false;
        }
        if key != "Space" && key != "Enter" {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        intro
            .invoke("send", IntrospectValue::Text("KeyboardActivate".to_string()))
            .is_ok()
    }

    fn fmt_state_log(state: (DisclosureState, bool)) -> String {
        format!(
            "{} / {}",
            state.0.as_name(),
            if state.1 { "expanded" } else { "collapsed" },
        )
    }
}

fn main() {
    pinion_shell::run::<DisclosureView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn enriched(state: (DisclosureState, bool), focused: Option<&str>) -> Vec<AccessNode> {
        let (s, e) = state;
        let scene = pinion_core::Owner::new().run(|| view(s, e, &Frame::new()));
        let mut nodes = DisclosureView::access_node(&state, focused);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        nodes
    }

    #[test]
    fn collapsed_idle_emits_button_role_with_expanded_false() {
        let nodes = enriched((DisclosureState::Idle, false), None);
        assert_eq!(nodes[0].role, AriaRole::Button);
        assert_eq!(nodes[0].name.as_deref(), Some(SUMMARY));
        // aria-expanded present and false (the WAI-ARIA disclosure axis).
        assert_eq!(nodes[0].expanded, Some(false));
        // A disclosure carries no aria-checked value.
        assert_eq!(nodes[0].state.checked, None);
        assert!(nodes[0].value.is_none());
    }

    #[test]
    fn expanded_idle_sets_aria_expanded_true() {
        let nodes = DisclosureView::access_node(&(DisclosureState::Idle, true), None);
        assert_eq!(nodes[0].expanded, Some(true));
    }

    #[test]
    fn hover_state_does_not_change_expanded() {
        let nodes = DisclosureView::access_node(&(DisclosureState::Hover, true), None);
        assert!(nodes[0].state.hovered);
        assert_eq!(nodes[0].expanded, Some(true));
    }

    #[test]
    fn focused_tag_sets_focused_flag() {
        let nodes = DisclosureView::access_node(
            &(DisclosureState::Idle, false),
            Some("main_disclosure"),
        );
        assert!(nodes[0].state.focused);
    }

    #[test]
    fn twisty_is_presentational_name_lands_on_summary() {
        // The `U+25B6`/`U+25BC` twisty carries TextRole::Presentational
        // so enrich_names_from_scene skips it and the disclosure's
        // accessible name is the summary label, even when expanded.
        let nodes = enriched((DisclosureState::Idle, true), None);
        assert_eq!(nodes[0].name.as_deref(), Some(SUMMARY));
    }

    #[test]
    fn r55_g20_view_contains_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<DisclosureView>(
            (DisclosureState::Idle, false),
            &Frame::new(),
        );
    }

    #[test]
    fn r696_header_ramp_idle_uses_surface_container_high_role() {
        let light = Theme::light();
        assert_eq!(
            pinion_widget_paint::disclosure::disclosure_header_for(
                &light,
                DisclosureState::Idle,
            ),
            light.resolve(ColorRole::SurfaceContainerHigh),
        );
    }
}
