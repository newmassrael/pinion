//! `hello-undo` — R748 §5.52 **undo / redo command stack**.
//!
//! A counter editor sitting on the [`UndoStack`] substrate (the `QUndoStack`
//! peer). The `+` / `-` buttons do not mutate the counter directly: each
//! records a reversible [`SignalEdit`] onto a shared [`UndoStack`], so the
//! stack is the single mutation path. `Undo` / `Redo` step the cursor; both
//! grey out **reactively** at the history boundaries (the stack's revision
//! `Signal` repaints them). The history is queryable as data through the
//! [`UndoStackExternal`] — an AI agent reads `index` / `count` /
//! `undo_label` without touching pixels (§2 #7).
//!
//! ## The witness (§2 #7 scene-as-data)
//!
//! Boot shows `0`, Undo + Redo disabled. Click `+` twice → `2`, the history
//! is `[Increment, Increment]` with the cursor at the top, Undo enabled /
//! Redo disabled. `invoke "undo"` (or the button) → `1`, cursor steps back,
//! Redo enables. A fresh `+` after an undo truncates the redo branch (the
//! single-branch `QUndoStack` model). The whole proof is data (see
//! `tools/demos/r748_undo.py`).
//!
//! ## The four buttons
//!
//! - **Primary** `ButtonExternal` at [`INC_TAG`] (`+`) — the static Tab stop.
//! - **Extra** `ButtonExternal`s at [`DEC_TAG`] (`-`), [`UNDO_TAG`],
//!   [`REDO_TAG`] — pointer / RPC / AT reachable.
//! - **Extra** [`UndoStackExternal`] at [`STACK_TAG`] — the AI-first history
//!   surface (`query` / `invoke "undo"` / `invoke "redo"`).

use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::undo::{use_undo_stack, SignalEdit, UndoStack, UndoStackExternal};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::button::{
    button_a11y_state, read_button_focused, read_button_state, ButtonColors, ButtonStyle,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloUndoRenderer, HelloUndoRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 280;
const THEME_TAG: &str = "app";

/// `-` button (decrement). Records a reversible `Decrement` edit.
const DEC_TAG: &str = "dec";
/// `+` button (increment) — the primary external + static Tab stop.
const INC_TAG: &str = "inc";
/// `Undo` button — steps the stack cursor back.
const UNDO_TAG: &str = "undo";
/// `Redo` button — steps the stack cursor forward.
const REDO_TAG: &str = "redo";
/// The [`UndoStackExternal`] anchor (the AI-first history surface).
const STACK_TAG: &str = "undo_stack";

/// `Owner::cache` key for the counter `Signal`.
const COUNTER_KEY: &str = "undo.counter";
/// `use_undo_stack` key for the shared history.
const UNDO_KEY: &str = "undo.stack";

const DEC_HOVER_KEY: &str = "dec_hover";
const INC_HOVER_KEY: &str = "inc_hover";
const UNDO_HOVER_KEY: &str = "undo_hover";
const REDO_HOVER_KEY: &str = "redo_hover";

/// `Owner::cache`-keyed counter — the single reactive value the edits move.
/// Read by the view (subscribes → repaints on every undo/redo) and by the
/// reducer (to snapshot the `before` for a new edit).
fn counter() -> Rc<Signal<i64>> {
    Owner::current()
        .expect("counter requires an active Owner scope")
        .cache(COUNTER_KEY, || Signal::new(0i64))
}

/// The shared [`UndoStack`] for this view scope — reached identically by the
/// reducer (records / steps), the view (reactive `can_undo`), and the
/// [`UndoStackExternal`].
fn stack() -> Rc<UndoStack> {
    use_undo_stack(UNDO_KEY)
}

/// Render one button via the [`pinion_widget_paint::button`] substrate.
fn button_scene(
    tag: &'static str,
    label: &str,
    state: ButtonState,
    focused: bool,
    hover_key: &'static str,
    colors: &ButtonColors,
) -> Scene {
    pinion_widget_paint::button::button_scene(
        label,
        state,
        focused,
        hover_key,
        colors,
        &ButtonStyle::m3_default(tag)
            .with_size(Size::px(72, 40))
            .with_corner_radius(20)
            .with_label_font_size_px(15),
    )
}

/// Posture (state + focus) of the four buttons, ordered `[dec, inc, undo,
/// redo]`. The counter value + the stack's reactive history live in the
/// `Signal` / [`UndoStack`] the owner-scoped view + `access_node` read
/// directly (the snackbar pattern).
type UndoViewState = ([ButtonState; 4], [bool; 4]);

/// view-fn (§6.3): pure sync mapping `(button postures) -> Scene`, reading
/// the reactive counter + the [`UndoStack`]'s history boundaries.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: UndoViewState, _frame: &Frame) -> Scene {
    let ([dec_s, inc_s, undo_s, redo_s], [dec_f, inc_f, undo_f, redo_f]) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let value = counter().get();
    let st = stack();
    let can_undo = st.can_undo();
    let can_redo = st.can_redo();

    // The big counter readout.
    let value_text = Scene::Text(TextNode::styled(
        format!("{value}"),
        Rect::default(),
        TextStyle::new()
            .with_size_px(56)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    // History status line — surfaced for the human; RPC reads the same
    // facts off the `UndoStackExternal`.
    let undo_l = st.undo_label();
    let redo_l = st.redo_label();
    let status = Scene::Text(TextNode::styled(
        format!(
            "{} edit(s), cursor {}   \u{00B7}   Undo: {}   Redo: {}",
            st.len(),
            st.index(),
            undo_l.as_deref().unwrap_or("\u{2014}"),
            redo_l.as_deref().unwrap_or("\u{2014}"),
        ),
        Rect::default(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    // Undo / Redo paint Disabled at the history boundaries (clicking is a
    // harmless no-op there too — `UndoStack::undo`/`redo` return false).
    let undo_state = if can_undo { undo_s } else { ButtonState::Disabled };
    let redo_state = if can_redo { redo_s } else { ButtonState::Disabled };

    let tonal = ButtonColors::filled_tonal(&theme);
    let accent = ButtonColors::accent(&theme);

    let buttons = Scene::Container(
        ContainerNode::new(vec![
            button_scene(DEC_TAG, "\u{2212}", dec_s, dec_f, DEC_HOVER_KEY, &tonal),
            button_scene(INC_TAG, "+", inc_s, inc_f, INC_HOVER_KEY, &tonal),
            button_scene(UNDO_TAG, "Undo", undo_state, undo_f, UNDO_HOVER_KEY, &accent),
            button_scene(REDO_TAG, "Redo", redo_state, redo_f, REDO_HOVER_KEY, &accent),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_gap(10),
        ),
    );

    let content = Scene::Container(
        ContainerNode::new(vec![value_text, buttons, status]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_flex_grow(1.0)
                .with_gap(20),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![content])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

struct UndoView;

impl WidgetCore for UndoView {
    type State = UndoViewState;
    type Event = ();

    /// Primary = the `+` button (the static Tab stop).
    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    /// Extras: the `-` / `Undo` / `Redo` buttons + the history coordinator.
    /// The `UndoStackExternal` wraps the **same** shared [`UndoStack`] the
    /// reducer records onto and the view reads `can_undo` from.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(DEC_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(UNDO_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(REDO_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(STACK_TAG, Box::new(UndoStackExternal::new(stack()))),
        ]
    }

    fn tag() -> &'static str {
        INC_TAG
    }

    fn read_state(scene: &Scene) -> UndoViewState {
        (
            [
                read_button_state(scene, DEC_TAG),
                read_button_state(scene, INC_TAG),
                read_button_state(scene, UNDO_TAG),
                read_button_state(scene, REDO_TAG),
            ],
            [
                read_button_focused(scene, DEC_TAG),
                read_button_focused(scene, INC_TAG),
                read_button_focused(scene, UNDO_TAG),
                read_button_focused(scene, REDO_TAG),
            ],
        )
    }

    fn view(state: UndoViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-undo (R748 §5.52 undo/redo command stack)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// All four buttons are Tab stops; the focused one activates on
    /// Space / Enter.
    fn focusable_tags() -> Vec<&'static str> {
        vec![INC_TAG, DEC_TAG, UNDO_TAG, REDO_TAG]
    }

    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: Modifiers) -> bool {
        [INC_TAG, DEC_TAG, UNDO_TAG, REDO_TAG]
            .iter()
            .any(|tag| apply_aria_activate(scene, focused, key, tag))
    }

    /// R748 reducer: `+` / `-` record a reversible [`SignalEdit`] onto the
    /// shared [`UndoStack`] (which applies it); `Undo` / `Redo` step the
    /// cursor. Every mutation flows through the stack — the single path.
    fn update(_state: UndoViewState, intent: &pinion_core::Intent) -> Vec<pinion_core::command::Command> {
        let st = stack();
        match intent.tag_str() {
            "inc.click" => {
                let c = counter();
                st.record(SignalEdit::to(&c, c.get() + 1, "Increment"));
            }
            "dec.click" => {
                let c = counter();
                st.record(SignalEdit::to(&c, c.get() - 1, "Decrement"));
            }
            "undo.click" => {
                st.undo();
            }
            "redo.click" => {
                st.redo();
            }
            _ => {}
        }
        Vec::new()
    }
}

impl WidgetA11y for UndoView {
    /// Four [`AriaRole::Button`]s; `Undo` / `Redo` carry `disabled` at the
    /// history boundaries (the same posture the view paints).
    fn access_node(state: &UndoViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let ([dec_s, inc_s, undo_s, redo_s], _) = *state;
        let st = stack();
        let undo_state = if st.can_undo() { undo_s } else { ButtonState::Disabled };
        let redo_state = if st.can_redo() { redo_s } else { ButtonState::Disabled };
        vec![
            AccessNode::new(DEC_TAG, AriaRole::Button)
                .with_name("Decrement")
                .with_state(button_a11y_state(dec_s, focused == Some(DEC_TAG))),
            AccessNode::new(INC_TAG, AriaRole::Button)
                .with_name("Increment")
                .with_state(button_a11y_state(inc_s, focused == Some(INC_TAG))),
            AccessNode::new(UNDO_TAG, AriaRole::Button)
                .with_name("Undo")
                .with_state(button_a11y_state(undo_state, focused == Some(UNDO_TAG))),
            AccessNode::new(REDO_TAG, AriaRole::Button)
                .with_name("Redo")
                .with_state(button_a11y_state(redo_state, focused == Some(REDO_TAG))),
        ]
    }
}

impl WidgetView for UndoView {
    type Renderer = HelloUndoRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<UndoView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the reducer in a fresh `Owner` scope (so the cached counter +
    /// stack resolve), returning `(counter, cursor, len)` after a sequence
    /// of intent tags.
    fn drive(tags: &[&str]) -> (i64, usize, usize) {
        Owner::new().run(|| {
            for tag in tags {
                let intent = pinion_core::Intent::new_owned((*tag).to_string(), pinion_core::external::IntrospectValue::Null);
                let _ = UndoView::update(([ButtonState::Idle; 4], [false; 4]), &intent);
            }
            let st = stack();
            (counter().get(), st.index(), st.len())
        })
    }

    #[test]
    fn increments_record_and_undo_reverts() {
        // +, +, + → 3; undo → 2; redo → 3.
        assert_eq!(drive(&["inc.click", "inc.click", "inc.click"]), (3, 3, 3));
        assert_eq!(drive(&["inc.click", "inc.click", "undo.click"]), (1, 1, 2));
        assert_eq!(drive(&["inc.click", "inc.click", "undo.click", "redo.click"]), (2, 2, 2));
    }

    #[test]
    fn decrement_is_a_distinct_reversible_edit() {
        // +, -, undo → back to 1 (the decrement reverts), cursor 1 of 2.
        assert_eq!(drive(&["inc.click", "dec.click", "undo.click"]), (1, 1, 2));
    }

    #[test]
    fn new_edit_truncates_the_redo_branch() {
        // +, +, undo (→1, redo branch holds the 2nd inc), + (→2, branch
        // dropped): len is 2, cursor 2, no redo available.
        assert_eq!(drive(&["inc.click", "inc.click", "undo.click", "inc.click"]), (2, 2, 2));
    }

    #[test]
    fn undo_at_bottom_is_a_noop() {
        assert_eq!(drive(&["undo.click", "undo.click"]), (0, 0, 0));
    }
}
