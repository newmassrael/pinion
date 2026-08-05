//! `hello-key-sequence` — R1569 §5.39 §5.20 a keymap-preferences pane, and the
//! forcing consumer of the **accelerator-shadow** axis (Qt
//! `QKeySequenceEdit` + `QEvent::ShortcutOverride`).
//!
//! The window is deliberately built to be hostile to its own editor: it paints
//! a `&File` / `&Save` menubar (so <kbd>Alt</kbd>+<kbd>F</kbd> and
//! <kbd>Alt</kbd>+<kbd>S</kbd> are §5.20 mnemonics) and declares a
//! `keybinding` map claiming the bare characters `r` / `c` / `d` / `e`. Both
//! layers fire from anywhere in the window regardless of focus — that is what
//! an accelerator IS — so before R1569 there was no chord the editor could
//! record without the window acting on it instead.
//!
//! What the demo drives, and what each step proves:
//!
//! * idle, the accelerators work: <kbd>Alt</kbd>+<kbd>F</kbd> activates the
//!   File title and `r` starts a recording, because an idle editor claims
//!   nothing;
//! * recording, they do not: the same two keystrokes are RECORDED, and
//!   `scene/accelerators` names this widget as the one taking them;
//! * a bare modifier is a published prefix rather than a silent drop;
//! * the sequence FILLS and commits itself, emitting the same intent an
//!   explicit commit emits;
//! * `scene/accelerators {"chord": …}` answers what a chord would collide with
//!   BEFORE it is recorded — the question that makes a keymap editor usable and
//!   the one `QKeySequenceEdit` cannot ask.
//!
//! Visual contract: a menubar strip, the editor's recorded / in-flight
//! spelling, and a status line naming the statechart state.

use pinion_a11y::{AccessNode, AccessValue, AriaRole};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::key_sequence::{
    KeySequenceEditExternal, KeySequenceEvent, KeySequenceState, use_key_sequence_display,
};
use pinion_core::{Frame, WidgetCore, WidgetStateName};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloKeySequenceRenderer, HelloKeySequenceRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 300;
const THEME_TAG: &str = "app";
/// The editor's paint / focus / dispatch tag.
const KS_TAG: &str = "key_sequence";
/// The value label INSIDE the field.
///
/// A composite sub-tag (`primary#sub`, R51.42) rather than a free name: the
/// deepest tag under the cursor is what click-to-focus resolves, and
/// `FocusManager::resolve_focusable` maps a sub-tag back to its primary. A
/// free tag here would make the field unfocusable BY CLICK while still being
/// in the tab order — reachable by Tab and dead to the mouse.
const KS_VALUE_TAG: &str = "key_sequence#value";
/// The two menubar titles, in Qt's `&`-marked source spelling.
const MENU_TITLES: [(&str, &str); 2] = [("menu#file", "&File"), ("menu#save", "&Save")];

/// view-fn (§6.3): pure sync mapping `KeySequenceState -> Scene`.
///
/// A [`WidgetCore::State`] is `Copy`, and a chord spelling is `String`-shaped,
/// so the text rides the tag-keyed [`use_key_sequence_display`] handle the
/// External writes — the same split `hello-textfield` uses for its buffer.
fn view(state: KeySequenceState) -> Scene {
    let display = use_key_sequence_display(KS_TAG);
    let snap = (display.accepted(), display.in_flight(), display.pending());
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let label = TextStyle::new().with_size_px(14).with_fg(on_surface);

    // The menubar. `mnemonic_styled` is the ONE declaration R1543 derives the
    // underline, the Alt binding and the AT `accesskey` from, so what this
    // window's accelerators are is decided here and nowhere else.
    let menubar = Scene::Container(
        ContainerNode::new(
            MENU_TITLES
                .iter()
                .map(|(tag, source)| {
                    Scene::Container(
                        ContainerNode::new(vec![Scene::Text(TextNode::mnemonic_styled(
                            source,
                            Rect::default(),
                            label.clone(),
                        ))])
                        .with_tag(*tag)
                        .with_layout(LayoutStyle::new().with_padding(Rect::new(8, 8, 8, 8))),
                    )
                })
                .collect(),
        )
        .with_tag("menubar")
        .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    // The editor's own face. While recording it shows the run in flight (plus
    // any held prefix); at rest it shows what was accepted.
    let (accepted, in_flight, pending) = snap;
    let shown = if state == KeySequenceState::Recording {
        let mut text = in_flight;
        if !pending.is_empty() {
            if !text.is_empty() {
                text.push_str(", ");
            }
            text.push_str(&pending);
        }
        if text.is_empty() {
            "press a chord".to_string()
        } else {
            text
        }
    } else if accepted.is_empty() {
        "unbound".to_string()
    } else {
        accepted
    };

    let field = Scene::Container(
        ContainerNode::new(vec![Scene::Text(
            TextNode::styled(&shown, Rect::default(), label.clone()).with_tag(KS_VALUE_TAG),
        )])
        .with_tag(KS_TAG)
        .with_style(BoxStyle::filled(theme.resolve(
            if state == KeySequenceState::Recording {
                ColorRole::SurfaceContainerHigh
            } else {
                ColorRole::SurfaceContainer
            },
        )))
        .with_layout(
            LayoutStyle::new()
                .with_padding(Rect::new(12, 12, 12, 12))
                .with_align_items(AlignItems::Center)
                // §5.39 R1020 — the editor is the window's focus stop, and the
                // shadow is resolved against whatever holds focus, so a field
                // that could not take focus could never claim a chord.
                .with_focusable(true),
        ),
    );

    let status = Scene::Text(
        TextNode::styled(
            format!("state: {}", state.as_name()),
            Rect::default(),
            label,
        )
        .with_tag("ks_status"),
    );

    Scene::Container(
        ContainerNode::new(vec![menubar, field, status])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(12),
            ),
    )
}

/// Project the editor's statechart state + revision out of the state scene.
///
/// The revision is what makes the projection a faithful CHANGE signal: the
/// spelling lives behind the shared handle, so two frames with the same
/// statechart state but different chords compare equal without it and the
/// shell skips the repaint. `hello-textfield` carries its caret offset in this
/// slot for exactly the same reason.
fn read_state(scene: &Scene) -> (KeySequenceState, u32) {
    let Some(intro) = scene
        .find_external_with_tag(KS_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return (KeySequenceState::Idle, 0);
    };
    let state = match intro.query("state") {
        Some(IntrospectValue::Text(name)) => KeySequenceState::from_name_or_default(&name),
        _ => KeySequenceState::Idle,
    };
    let revision = match intro.query("revision") {
        Some(IntrospectValue::Int(n)) => u32::try_from(n.max(0)).unwrap_or(u32::MAX),
        _ => 0,
    };
    (state, revision)
}

struct KeySequenceView;

impl WidgetCore for KeySequenceView {
    type State = (KeySequenceState, u32);
    type Event = KeySequenceEvent;

    fn tag() -> &'static str {
        KS_TAG
    }

    fn create_external() -> Box<dyn External> {
        // Two chords, not Qt's four: a two-chord bound is what makes the
        // fill-commit reachable in a demo without four keystrokes of setup,
        // and `maximumSequenceLength` is a per-widget declaration precisely so
        // a binding can say so.
        Box::new(
            KeySequenceEditExternal::new()
                .with_max_len(2)
                .attach_display(use_key_sequence_display(KS_TAG)),
        )
    }

    fn read_state(scene: &Scene) -> Self::State {
        read_state(scene)
    }

    fn view(state: Self::State, _frame: &Frame) -> Scene {
        view(state.0)
    }

    fn event_name(event: Self::Event) -> &'static str {
        pinion_core::WidgetEventName::as_name(&event)
    }

    fn title() -> &'static str {
        "pinion hello-key-sequence (R1569 §5.39 accelerator shadow)"
    }

    /// The window's `keybinding` accelerator layer — bare characters, which is
    /// the shape R1543 found 96 bindings had hand-rolled. Kept bare on purpose:
    /// `r` and `c` are exactly the characters a user would want to RECORD, so
    /// this map is what makes the round's claim testable rather than asserted.
    fn keybinding(key: &str) -> Option<Self::Event> {
        match key {
            "r" => Some(KeySequenceEvent::Record),
            "c" => Some(KeySequenceEvent::Cancel),
            "d" => Some(KeySequenceEvent::Disable),
            "e" => Some(KeySequenceEvent::Enable),
            _ => None,
        }
    }

    /// Every key that reaches the focused editor is a chord to record.
    ///
    /// The dispatch that gets here is precisely the dispatch the accelerator
    /// layers declined — either because nothing declares the chord or because
    /// [`pinion_core::external::External::shadows_accelerator`]
    /// answered `true`. So this method never has to re-decide the precedence;
    /// it only has to record.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(KS_TAG) {
            return false;
        }
        // Escape leaves a recording without accepting it — the exit Qt's
        // `QKeySequenceEdit` does not have, and the reason `cancel` is an
        // event in the statechart rather than a timer.
        let payload = if key == "Escape" {
            IntrospectValue::Text("Cancel".to_string())
        } else {
            let chord = pinion_core::accelerator::Chord::new(key, modifiers);
            let Some(intro) = scene
                .find_external_with_tag_mut(KS_TAG)
                .and_then(|n| n.handle.introspect_mut())
            else {
                return false;
            };
            return intro
                .invoke("record", IntrospectValue::Text(chord.portable()))
                .is_ok();
        };
        scene
            .find_external_with_tag_mut(KS_TAG)
            .and_then(|n| n.handle.introspect_mut())
            .is_some_and(|intro| intro.invoke("send", payload).is_ok())
    }

    fn fmt_state_log(state: &Self::State) -> String {
        format!("{} @{}", state.0.as_name(), state.1)
    }
}

impl pinion_a11y::WidgetA11y for KeySequenceView {
    /// The editor announces as a `textbox` whose value is the chord spelling —
    /// which is also what a `QKeySequenceEdit` reaches an AT as, since it wraps
    /// a `QLineEdit`. What is not Qt's: the announced value is the string the
    /// paint shows, read from the same snapshot, so the two cannot diverge.
    fn access_node(state: &Self::State, focused: Option<&str>) -> Vec<AccessNode> {
        let accepted = use_key_sequence_display(KS_TAG).accepted();
        let mut node = AccessNode::new(KS_TAG, AriaRole::TextInput);
        node.value = Some(AccessValue::Text(if accepted.is_empty() {
            "unbound".to_string()
        } else {
            accepted
        }));
        node.state.focused = focused == Some(KS_TAG);
        node.state.disabled = state.0 == KeySequenceState::Disabled;
        vec![node]
    }
}

impl WidgetView for KeySequenceView {
    type Renderer = HelloKeySequenceRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<KeySequenceView>();
}

#[cfg(test)]
mod tests {
    use super::{KS_TAG, KeySequenceState, KeySequenceView, view};
    use pinion_core::Frame;
    use pinion_core::accelerator::Chord;
    use pinion_core::external::External;
    use pinion_core::mnemonic::scene_mnemonics;
    use pinion_core::reactive::Owner;
    use pinion_core::widgets::key_sequence::{
        KeySequenceEditExternal, KeySequenceEvent, use_key_sequence_display,
    };

    /// Paint under a fresh `Owner` seeded with `(accepted, in_flight, pending)`
    /// through the SAME external the binding builds, so the test cannot show a
    /// spelling the widget could not produce.
    fn painted(seed: impl FnOnce(&mut KeySequenceEditExternal), state: KeySequenceState) -> String {
        let owner = Owner::new();
        owner.run(|| {
            let mut ext = KeySequenceEditExternal::new()
                .with_max_len(2)
                .attach_display(use_key_sequence_display(KS_TAG));
            seed(&mut ext);
            format!("{:?}", view(state))
        })
    }

    fn chord(key: &str, ctrl: bool, alt: bool) -> Chord {
        Chord::new(
            key,
            pinion_core::Modifiers {
                ctrl,
                alt,
                ..pinion_core::Modifiers::empty()
            },
        )
    }

    #[test]
    fn the_window_declares_the_two_mnemonics_the_editor_must_be_able_to_shadow() {
        // The demo's whole claim rests on these existing: if the window stopped
        // declaring Alt+F, "the editor recorded Alt+F instead of opening File"
        // would pass for the wrong reason.
        let owner = Owner::new();
        let scene = owner.run(|| view(KeySequenceState::Idle));
        let keys: Vec<char> = scene_mnemonics(&scene).iter().map(|b| b.key).collect();
        assert_eq!(keys, vec!['F', 'S']);
    }

    #[test]
    fn the_keybinding_layer_claims_the_characters_the_demo_records() {
        for key in ["r", "c", "d", "e"] {
            assert!(
                <KeySequenceView as pinion_core::WidgetCore>::keybinding(key).is_some(),
                "{key} must be an accelerator for the shadow to be worth testing",
            );
        }
        assert!(<KeySequenceView as pinion_core::WidgetCore>::keybinding("k").is_none());
    }

    #[test]
    fn a_recording_field_paints_the_run_in_flight_and_the_held_prefix() {
        let text = painted(
            |ext| {
                ext.send(KeySequenceEvent::Record);
                ext.record(&chord("j", true, false));
                ext.record(&chord("Alt", false, true));
            },
            KeySequenceState::Recording,
        );
        assert!(
            text.contains("Ctrl+j, Alt+"),
            "in-flight run plus prefix: {text}"
        );
    }

    #[test]
    fn an_idle_field_paints_the_accepted_run_and_names_the_unbound_case() {
        let accepted = painted(
            |ext| {
                ext.send(KeySequenceEvent::Record);
                ext.record(&chord("k", true, false));
                ext.send(KeySequenceEvent::Commit);
            },
            KeySequenceState::Idle,
        );
        assert!(accepted.contains("Ctrl+k"), "{accepted}");
        assert!(
            painted(|_| {}, KeySequenceState::Idle).contains("unbound"),
            "an empty binding says so rather than painting an empty box",
        );
    }

    #[test]
    fn the_editor_shadows_only_while_recording() {
        // The binding-level mirror of the widget unit test: what the SHELL will
        // ask is `shadows_accelerator`, and the answer must follow the state
        // this binding's own `keybinding` map can drive it into.
        let mut ext = KeySequenceEditExternal::new().with_max_len(2);
        let alt_f = chord("f", false, true);
        assert!(!ext.shadows_accelerator(&alt_f), "idle claims nothing");
        ext.send(KeySequenceEvent::Record);
        assert!(
            ext.shadows_accelerator(&alt_f),
            "recording claims everything"
        );
        ext.send(KeySequenceEvent::Cancel);
        assert!(!ext.shadows_accelerator(&alt_f), "cancel gives it back");
    }

    #[test]
    fn r55_g20_view_contains_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<KeySequenceView>(
            (KeySequenceState::Idle, 0),
            &Frame::new(),
        );
    }

    #[test]
    fn r1360_2_view_paints_an_opaque_root() {
        pinion_core::test_fixtures::assert_widget_view_paints_opaque_root::<KeySequenceView>(
            (KeySequenceState::Idle, 0),
            &Frame::new(),
        );
    }

    #[test]
    fn the_editor_owns_the_focus_tag_the_shadow_is_resolved_against() {
        assert_eq!(<KeySequenceView as pinion_core::WidgetCore>::tag(), KS_TAG);
    }
}
