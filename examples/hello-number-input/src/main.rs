// R736 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (WAI-ARIA, TextFieldExternal, ButtonExternal, SpinButton, …).
#![allow(clippy::doc_markdown)]

//! `hello-number-input` — R736 §5.38 §5.40 §5.50 **editable number input**
//! (the typeable WAI-ARIA `spinbutton`: the HTML `<input type=number>` /
//! GTK editable `SpinButton` / Qt `QSpinBox` form factor): a single-line
//! numeric text field flanked by `−` / `+` stepper buttons. The user can
//! either *type* a value directly or step it with the buttons / arrow keys.
//!
//! ## 2nd consumer of the editable-field value-coordination pattern
//!
//! R717 `hello-combobox-editable` established the textbook shape for an
//! editable composite: a [`TextFieldExternal`] primary (the focusable,
//! editable text) + extra externals coordinated through the binding's
//! [`NumberView::update`] reducer, with **no** new coordinator substrate
//! (`[[abstraction-needs-second-consumer]]`). This number input is the 2nd
//! consumer of that pattern; it swaps the combobox's listbox popup for two
//! [`ButtonExternal`] steppers and the typeahead filter for numeric parse /
//! clamp / reformat:
//!
//! * **input** — a [`TextFieldExternal`] (`num_input`, the primary,
//!   focusable external) owning the editable numeric text + caret
//!   (`use_text_edit_state` / `use_caret_blink`, the same hooks
//!   `hello-textfield` + the editable combobox use). **The field text *is*
//!   the value** — the numeric value is the pure projection
//!   [`parse_clamp`]`(text)`, exactly as the editable combobox's value is
//!   its field text. There is no separate `f32` value store to keep in
//!   sync (a dual-SSOT trap): the text buffer is the single source.
//! * **steppers** — two [`ButtonExternal`]s (`num_dec` / `num_inc`) as
//!   extra externals. A stepper click emits the `"click"` §5.20 intent the
//!   reducer turns into a `±STEP` commit (the same arithmetic the arrow
//!   keys drive). The buttons carry real `Idle` / `Hover` / `Pressed`
//!   interaction state machines, so the steppers get full desktop-grade
//!   pointer feedback for free.
//!
//! ## Why not reuse `SpinButtonExternal` as the value store
//!
//! R734 `SpinButtonExternal` is a *poll-only* value holder (it emits no
//! intents — its value is observed through the introspect channel and
//! stepped through `invoke`). For a stepper-only spinbutton that is exactly
//! right. But the *editable* variant's value lives in the editable text
//! buffer — that is the whole point of "type a number". Mirroring the
//! value into a `SpinButtonExternal` would create two stores (the text and
//! the f32) that must be kept in sync in both directions, and the
//! poll-only spinbutton could not notify the reducer when a stepper fired.
//! The textbook editable spinbutton stores the value in its text, and the
//! stepping arithmetic ([`clamp`] / `±STEP` / `±PAGE`) is a handful of pure
//! functions — so this binding uses them directly (the editable combobox
//! likewise minted no `ComboBoxExternal`). "Similar widget class is not the
//! same value store" — the editable variant's store is fundamentally the
//! text buffer.
//!
//! ## Keyboard model (WAI-ARIA spinbutton, editable)
//!
//! `ArrowUp` / `ArrowDown` step by `STEP`; `PageUp` / `PageDown` by
//! `PAGE`; `Enter` normalises the typed text in place (parse → clamp →
//! reformat, e.g. `"007"` → `"7"`, `"999"` → the max). `Home` / `End` /
//! `ArrowLeft` / `ArrowRight` / `Backspace` / `Delete` move the *caret* /
//! edit the text (the defining difference from R734's stepper-only
//! spinbutton, where every arrow stepped). Only numeric keystrokes
//! (digits / `-` / `.`) reach the field — letters are dropped at the
//! binding so the buffer never holds non-numeric junk (the HTML
//! `<input type=number>` keystroke gate).
//!
//! ## a11y (R736 §5.40) — no new axis
//!
//! One [`AriaRole::SpinButton`] node carrying the value as
//! [`AccessValue::Float`] (`aria-valuenow` / `aria-valuemin` /
//! `aria-valuemax`) — the same lowering R734 + `Slider` + `ProgressBar`
//! use, **no new a11y axis** (R734 already minted the SpinButton role +
//! Float-value path). The `−` / `+` steppers are presentational (no
//! separate AT nodes); AT users reach the value through the operable
//! spinbutton's Increment / Decrement actions the arrow keys drive
//! (R734 precedent).
//!
//! ## Known gaps (honest carry)
//!
//! - **Blur does not reformat.** HTML / GTK number inputs normalise the
//!   typed text when the field loses focus; here you commit with `Enter`,
//!   a stepper, or an arrow. A binding-observable plain-blur
//!   (`Focused → Idle`) hook does not exist — `WidgetCore` has no
//!   focus-change callback and the field's `"text_committed"` intent fires
//!   only on IME-composition blur (R56.1.a detect rule), not plain blur.
//!   Blur-commit is an additive substrate axis (a `WidgetCore` focus-loss
//!   reducer hook), deferred until a 2nd consumer needs it
//!   (`[[abstraction-needs-second-consumer]]`).
//! - IME composition is not wired (numeric input is ASCII; the
//!   `hello-textfield` `apply_composition` shape is an additive carry).
//! - The input paints no keyboard-focus ring (the shared shell-focus-paint
//!   axis R690 deferred; focus is real + RPC-observable).

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::text_field as tf_paint;

use pinion_widget_paint::state_layer::{HOVER, PRESSED};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloNumberInputRenderer, HelloNumberInputRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 200;
const THEME_TAG: &str = "app";

/// The editable numeric text input (primary external) — the focusable
/// spinbutton. Its text buffer is the value SSOT.
const INPUT_TAG: &str = "num_input";
/// Decrement / increment stepper buttons (extra externals). Each emits a
/// `"click"` intent the reducer turns into a `∓STEP` / `±STEP` commit.
const DEC_TAG: &str = "num_dec";
const INC_TAG: &str = "num_inc";
/// The status line echoing the committed numeric value.
const STATUS_TAG: &str = "num_status";

/// Font-size property field: 8..=72 px, single step 1, page step 12, start
/// 16 — a recognisable DCC / IDE property-panel number input.
const MIN: f32 = 8.0;
const MAX: f32 = 72.0;
const STEP: f32 = 1.0;
const PAGE: f32 = 12.0;
const START: f32 = 16.0;

const FIELD_W: u32 = 96;
const FIELD_H: u32 = 44;
const STEP_W: u32 = 44;

const STATUS_PX: u32 = 15;
const LABEL_PX: u32 = 14;

/// U+2212 MINUS SIGN / U+002B PLUS SIGN — the stepper glyphs (named const
/// with escape per [[non-ascii-literal-named-const-escape]]; raw glyph in
/// docs only).
const MINUS_GLYPH: &str = "\u{2212}";
const PLUS_GLYPH: &str = "+";

/// Clamp a value into `[MIN, MAX]`; `NaN` saturates to `MIN` (a malformed
/// parse can never poison the field). The single numeric invariant —
/// every commit funnels through here, mirroring `SpinButtonExternal`'s
/// internal re-clamp (R734) without owning a separate value store.
fn clamp(v: f32) -> f32 {
    if v.is_nan() {
        MIN
    } else {
        v.clamp(MIN, MAX)
    }
}

/// Format a committed value as the canonical field text. Integer display
/// (the field steps in whole units); the parse round-trips it exactly so
/// the text buffer stays the value SSOT.
fn format_value(v: f32) -> String {
    format!("{v:.0}")
}

/// Parse the field text into a clamped numeric value. The numeric value is
/// a **pure projection** of the text buffer (the SSOT) — empty / malformed
/// text reads as `MIN` so `aria-valuenow` + the steppers always have a
/// defined base. This is the editable spinbutton's "the text *is* the
/// value" model (mirror of the editable combobox, whose value is its
/// field text).
fn parse_clamp(text: &str) -> f32 {
    clamp(text.trim().parse::<f32>().unwrap_or(MIN))
}

/// The committed numeric value = `parse_clamp` of the live field text.
/// Requires an active Owner scope (the hook resolves the cached
/// [`TextEditState`]); the reducer / `apply_key` / view paths are all
/// owner-wrapped.
fn current_value() -> f32 {
    parse_clamp(&use_text_edit_state(INPUT_TAG).text())
}

/// Commit `v`: clamp it and write the canonical text back into the field
/// (caret parked at the end). Because the text buffer is the value SSOT,
/// committing *is* a `set_text` — there is no second store to update. Used
/// by Enter (normalise in place), the steppers, and the arrow keys.
fn commit_value(v: f32) {
    let c = clamp(v);
    let text = format_value(c);
    let ts = use_text_edit_state(INPUT_TAG);
    ts.seed(text);
}

/// Whether `key` is a numeric keystroke allowed into the field: a digit,
/// the sign `-`, or the decimal point `.`. Letters and symbols are dropped
/// at the binding so the buffer never holds non-numeric junk (the HTML
/// `<input type=number>` keystroke gate). Multi-char (IME / named) strings
/// are not numeric keystrokes.
fn is_numeric_char(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(c) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        return false;
    }
    c.is_ascii_digit() || c == '-' || c == '.'
}

/// Cached posture for the paint fn. The text + caret are reactive (read via
/// hooks); this `Copy` snapshot carries the field + stepper interaction
/// states per [[update-by-value-snapshot]].
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct NumberViewState {
    /// Text-field interaction posture (SCXML state) for the field paint.
    input: TextFieldState,
    /// Caret byte offset for the field paint.
    caret: u32,
    /// Decrement stepper interaction state (hover / pressed paint).
    dec: ButtonState,
    /// Increment stepper interaction state.
    inc: ButtonState,
}

impl NumberViewState {
    #[cfg(test)]
    fn idle() -> Self {
        Self {
            input: TextFieldState::Idle,
            caret: 0,
            dec: ButtonState::Idle,
            inc: ButtonState::Idle,
        }
    }
}

/// Read the stepper [`ButtonState`] from an extra external's introspect
/// `state` slot (mirror of the editable combobox's per-option state read).
fn read_button_state(scene: &Scene, tag: &str) -> ButtonState {
    let Some(node) = scene.find_external_with_tag(tag) else {
        return ButtonState::Idle;
    };
    let Some(intro) = node.handle.introspect() else {
        return ButtonState::Idle;
    };
    match intro.query("state") {
        Some(IntrospectValue::Text(name)) => ButtonState::from_name_or_default(&name),
        _ => ButtonState::Idle,
    }
}

/// Read the full posture: the text field's interaction state + caret, then
/// the two steppers' interaction states.
fn read_number_state(scene: &Scene) -> NumberViewState {
    let (input, caret) = tf_paint::read_text_field_state(scene, INPUT_TAG);
    NumberViewState {
        input,
        caret,
        dec: read_button_state(scene, DEC_TAG),
        inc: read_button_state(scene, INC_TAG),
    }
}

/// A circular `−` / `+` stepper affordance, tagged for hit-testing as its
/// own [`ButtonExternal`]. The fill layers the shared M3 hover (0.08) /
/// pressed (0.12) `OnSurface` state overlay (the same overlay
/// `hello-spinbutton` / `hello-radio-group` use) so the stepper has
/// desktop-grade pointer feedback.
fn stepper(tag: &'static str, glyph: &str, state: ButtonState, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            glyph,
            Rect::default(),
            TextStyle::new()
                .with_size_px(22)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(tag)
        .with_style(
            BoxStyle::filled(stepper_fill(theme, state)).with_corner_radius(FIELD_H / 2),
        )
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(STEP_W, FIELD_H)),
        ),
    )
}

/// Stepper fill = the `SurfaceContainerHighest` base with the shared M3
/// hover / pressed `OnSurface` state-layer (0.08 / 0.12).
fn stepper_fill(theme: &Theme, state: ButtonState) -> pinion_core::Color {
    let base = theme.resolve(ColorRole::SurfaceContainerHighest);
    match state {
        ButtonState::Idle | ButtonState::Disabled => base,
        ButtonState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), HOVER),
        ButtonState::Pressed => base.lerp(theme.resolve(ColorRole::OnSurface), PRESSED),
    }
}

/// view-fn (§6.3): pure sync `(postures) -> Scene`. A label, then a
/// `[ − ][ field ][ + ]` flex row built from the lifted TextField paint +
/// two stepper buttons, then a status line — all in a column-centred root.
/// Mirrors `hello-spinbutton`'s proven flex layout (a plain flex row of
/// steppers around the value surface); the `view_field` replaces the
/// readout. No absolute positioning — flex children of an absolutely-
/// positioned container do not lay out (the boot-pixel guard reads the
/// computed rects either way).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &NumberViewState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let value = current_value();

    let label = Scene::Text(TextNode::styled(
        "Font size",
        Rect::default(),
        TextStyle::new()
            .with_size_px(LABEL_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    // The lifted TextField paint keeps the `num_input` hit-test tag.
    let field_style = tf_paint::TextFieldStyle {
        field_w: FIELD_W,
        field_h: FIELD_H,
        ..tf_paint::TextFieldStyle::m3_filled()
    };
    let field_inner = tf_paint::view_field(
        INPUT_TAG,
        state.input,
        state.caret,
        &theme,
        &field_style,
        "",
    );

    // `[ − ][ field ][ + ]` flex row (hello-spinbutton layout).
    let row = Scene::Container(
        ContainerNode::new(vec![
            stepper(DEC_TAG, MINUS_GLYPH, state.dec, &theme),
            field_inner,
            stepper(INC_TAG, PLUS_GLYPH, state.inc, &theme),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(6),
        ),
    );

    let status = Scene::Text(
        TextNode::styled(
            format!("Value: {value:.0} px"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(STATUS_TAG),
    );

    Scene::Container(
        ContainerNode::new(vec![label, row, status])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_gap(14),
            ),
    )
}

struct NumberView;

impl WidgetCore for NumberView {
    type State = NumberViewState;
    // Text editing flows through the pointer router + apply_key; stepper
    // clicks flow through the intent reducer. No keybinding-channel events.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        // Same hooks hello-textfield / the editable combobox use;
        // `create_external` runs inside `root_owner.run(...)` so the seeded
        // text resolves to the instance the view fn reads later (Owner::cache
        // dedup). Seed the field with the START value so the boot frame
        // shows a real number (the text buffer is the value SSOT).
        let text_state = use_text_edit_state(INPUT_TAG);
        text_state.set_text(format_value(START));
        let blink = use_caret_blink(INPUT_TAG);
        Box::new(
            TextFieldExternal::new()
                .attach_state(text_state)
                .attach_blink(blink),
        )
    }

    /// The two stepper buttons as extra externals (both persist in the
    /// state scene; each emits a `"click"` intent the reducer commits).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(DEC_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(INC_TAG, Box::new(ButtonExternal::new())),
        ]
    }

    fn tag() -> &'static str {
        INPUT_TAG
    }

    fn read_state(scene: &Scene) -> NumberViewState {
        read_number_state(scene)
    }

    fn view(state: NumberViewState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-number-input (R736 §5.38 editable spinbutton)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// The number input is a single Tab stop — only the field is focusable.
    /// The steppers are click targets only (the spinbutton owns the
    /// Increment / Decrement AT actions).
    fn focusable_tags() -> Vec<&'static str> {
        vec![INPUT_TAG]
    }

    /// WAI-ARIA editable-spinbutton keyboard model. Up/Down step, Page
    /// keys page-step, Enter normalises the typed text; caret / edit keys
    /// flow to the field; non-numeric printable keys are dropped.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(INPUT_TAG) {
            return false;
        }
        match key {
            "ArrowUp" => {
                commit_value(current_value() + STEP);
                true
            }
            "ArrowDown" => {
                commit_value(current_value() - STEP);
                true
            }
            "PageUp" => {
                commit_value(current_value() + PAGE);
                true
            }
            "PageDown" => {
                commit_value(current_value() - PAGE);
                true
            }
            // Enter normalises the typed text in place (parse → clamp →
            // reformat), the HTML number-input commit-on-Enter behaviour.
            "Enter" => {
                commit_value(current_value());
                true
            }
            // Caret motion + text deletion always reach the field; the
            // held modifiers ride along so `Shift+Arrow` / `Shift+Home`
            // extend the field's selection (the modifier-aware path that
            // the pre-R804 bare-`Text` hand-roll silently dropped).
            "ArrowLeft" | "ArrowRight" | "Home" | "End" | "Backspace" | "Delete" => {
                pinion_core::forward_key_to_field(scene, INPUT_TAG, key, modifiers)
            }
            // Printable keys reach the field only if numeric; letters /
            // symbols drop (the <input type=number> keystroke gate).
            other => {
                if is_numeric_char(other) {
                    pinion_core::forward_key_to_field(scene, INPUT_TAG, other, modifiers)
                } else {
                    false
                }
            }
        }
    }

    /// R736 §5.40 — bridge the stepper `"click"` intents into a `±STEP`
    /// commit (the same arithmetic the arrow keys drive). The committed
    /// value writes back into the field text (the value SSOT).
    fn update(
        _state: NumberViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "num_dec.click" => commit_value(current_value() - STEP),
            "num_inc.click" => commit_value(current_value() + STEP),
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &NumberViewState) -> String {
        format!("input={:?} caret={} dec={:?} inc={:?}", state.input, state.caret, state.dec, state.inc)
    }
}

impl WidgetA11y for NumberView {
    /// R736 §5.40 — one `spinbutton` node carrying the value as
    /// [`AccessValue::Float`] (`aria-valuenow` / `aria-valuemin` /
    /// `aria-valuemax`). No new a11y axis: R734 minted the SpinButton
    /// role plus the Float-value path. The decrement / increment
    /// steppers are presentational (no separate AT nodes); AT users
    /// reach the value through the operable spinbutton's Increment /
    /// Decrement actions the arrow keys drive.
    fn access_node(_state: &NumberViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let value = current_value();
        vec![AccessNode::new(INPUT_TAG, AriaRole::SpinButton)
            .with_name("Font size")
            .with_value(AccessValue::Float { value, min: MIN, max: MAX })
            .with_state(AccessState {
                focused: focused == Some(INPUT_TAG),
                ..AccessState::default()
            })]
    }
}

impl WidgetView for NumberView {
    type Renderer = HelloNumberInputRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<NumberView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;
    use pinion_core::scene::ExternalNode;

    fn idle() -> NumberViewState {
        NumberViewState::idle()
    }

    fn intent_with(tag: &str, payload: IntrospectValue) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(tag.to_string(), payload)
    }

    /// Build the multi-external state scene the way the shell does so
    /// `apply_key` has real input + stepper externals.
    fn boot_scene() -> Scene {
        let mut children = vec![Scene::External(
            ExternalNode::new(NumberView::create_external()).with_tag(INPUT_TAG),
        )];
        for extra in NumberView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }

    // ----- pure numeric helpers -----

    #[test]
    fn r736_clamp_bounds_and_nan() {
        assert!((clamp(5.0) - MIN).abs() < f32::EPSILON, "below min → min");
        assert!((clamp(100.0) - MAX).abs() < f32::EPSILON, "above max → max");
        assert!((clamp(20.0) - 20.0).abs() < f32::EPSILON, "in range unchanged");
        assert!((clamp(f32::NAN) - MIN).abs() < f32::EPSILON, "NaN → min");
    }

    #[test]
    fn r736_parse_clamp_handles_garbage_and_range() {
        assert!((parse_clamp("16") - 16.0).abs() < f32::EPSILON, "plain integer");
        assert!((parse_clamp("  24 ") - 24.0).abs() < f32::EPSILON, "whitespace trimmed");
        assert!((parse_clamp("007") - 7.0).abs() > f32::EPSILON, "007 = 7, clamped to min 8");
        assert!((parse_clamp("007") - MIN).abs() < f32::EPSILON, "7 clamps up to min 8");
        assert!((parse_clamp("999") - MAX).abs() < f32::EPSILON, "over-max clamps to max");
        assert!((parse_clamp("") - MIN).abs() < f32::EPSILON, "empty → min");
        assert!((parse_clamp("abc") - MIN).abs() < f32::EPSILON, "garbage → min");
    }

    #[test]
    fn r736_format_value_is_integer() {
        assert_eq!(format_value(16.0), "16");
        assert_eq!(format_value(8.0), "8");
    }

    #[test]
    fn r736_is_numeric_char_gate() {
        assert!(is_numeric_char("0"));
        assert!(is_numeric_char("9"));
        assert!(is_numeric_char("-"));
        assert!(is_numeric_char("."));
        assert!(!is_numeric_char("a"), "letter rejected");
        assert!(!is_numeric_char("$"), "symbol rejected");
        assert!(!is_numeric_char("Enter"), "named key rejected");
        assert!(!is_numeric_char(""), "empty rejected");
    }

    // ----- value round-trip through the text buffer (the SSOT) -----

    #[test]
    fn r736_boot_seeds_start_value() {
        Owner::new().run(|| {
            // Build the external (seeds the field text).
            let _ = boot_scene();
            assert_eq!(use_text_edit_state(INPUT_TAG).text(), "16", "field seeded to START");
            assert!((current_value() - START).abs() < f32::EPSILON, "value reads START");
        });
    }

    #[test]
    fn r736_commit_value_writes_text_and_clamps() {
        Owner::new().run(|| {
            let _ = boot_scene();
            commit_value(30.0);
            assert_eq!(use_text_edit_state(INPUT_TAG).text(), "30");
            commit_value(999.0);
            assert_eq!(use_text_edit_state(INPUT_TAG).text(), "72", "clamped to max");
            commit_value(1.0);
            assert_eq!(use_text_edit_state(INPUT_TAG).text(), "8", "clamped to min");
        });
    }

    // ----- reducer (stepper clicks) -----

    #[test]
    fn r736_stepper_clicks_step_the_value() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let _ = NumberView::update(idle(), &intent_with("num_inc.click", IntrospectValue::Null));
            assert!((current_value() - 17.0).abs() < f32::EPSILON, "inc click 16 → 17");
            let _ = NumberView::update(idle(), &intent_with("num_dec.click", IntrospectValue::Null));
            let _ = NumberView::update(idle(), &intent_with("num_dec.click", IntrospectValue::Null));
            assert!((current_value() - 15.0).abs() < f32::EPSILON, "dec ×2 17 → 15");
        });
    }

    #[test]
    fn r736_stepper_clicks_clamp_at_bounds() {
        Owner::new().run(|| {
            let _ = boot_scene();
            commit_value(MAX);
            let _ = NumberView::update(idle(), &intent_with("num_inc.click", IntrospectValue::Null));
            assert!((current_value() - MAX).abs() < f32::EPSILON, "inc at max clamps");
            commit_value(MIN);
            let _ = NumberView::update(idle(), &intent_with("num_dec.click", IntrospectValue::Null));
            assert!((current_value() - MIN).abs() < f32::EPSILON, "dec at min clamps");
        });
    }

    // ----- keyboard -----

    #[test]
    fn r736_arrows_step_and_page_keys_page() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let m = pinion_core::Modifiers::empty();
            assert!(NumberView::apply_key(&mut scene, Some(INPUT_TAG), "ArrowUp", m));
            assert!((current_value() - 17.0).abs() < f32::EPSILON, "ArrowUp 16 → 17");
            assert!(NumberView::apply_key(&mut scene, Some(INPUT_TAG), "ArrowDown", m));
            assert!((current_value() - 16.0).abs() < f32::EPSILON, "ArrowDown → 16");
            assert!(NumberView::apply_key(&mut scene, Some(INPUT_TAG), "PageUp", m));
            assert!((current_value() - 28.0).abs() < f32::EPSILON, "PageUp +12 → 28");
            assert!(NumberView::apply_key(&mut scene, Some(INPUT_TAG), "PageDown", m));
            assert!((current_value() - 16.0).abs() < f32::EPSILON, "PageDown -12 → 16");
        });
    }

    #[test]
    fn r736_enter_normalises_typed_text() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Simulate the user typing a verbose / out-of-range value.
            use_text_edit_state(INPUT_TAG).set_text("099".to_string());
            assert!(NumberView::apply_key(
                &mut scene,
                Some(INPUT_TAG),
                "Enter",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(use_text_edit_state(INPUT_TAG).text(), "72", "Enter clamps 99 → max + reformats");
        });
    }

    #[test]
    fn r736_typing_digits_reaches_field_letters_dropped() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            use_text_edit_state(INPUT_TAG).set_text(String::new());
            use_text_edit_state(INPUT_TAG).set_caret(0);
            let m = pinion_core::Modifiers::empty();
            assert!(NumberView::apply_key(&mut scene, Some(INPUT_TAG), "2", m), "digit consumed");
            assert!(NumberView::apply_key(&mut scene, Some(INPUT_TAG), "4", m), "digit consumed");
            assert_eq!(use_text_edit_state(INPUT_TAG).text(), "24", "digits inserted");
            assert!(!NumberView::apply_key(&mut scene, Some(INPUT_TAG), "x", m), "letter dropped");
            assert_eq!(use_text_edit_state(INPUT_TAG).text(), "24", "letter did not insert");
        });
    }

    #[test]
    fn r736_keys_ignored_when_input_not_focused() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(!NumberView::apply_key(
                &mut scene,
                None,
                "ArrowUp",
                pinion_core::Modifiers::empty(),
            ));
            assert!((current_value() - START).abs() < f32::EPSILON, "value unchanged");
        });
    }

    #[test]
    fn r736_focusable_tags_lists_only_input() {
        assert_eq!(NumberView::focusable_tags(), vec![INPUT_TAG]);
    }

    // ----- a11y -----

    #[test]
    fn r736_emits_one_spinbutton_node_with_float_value() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let nodes = NumberView::access_node(&idle(), None);
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].role, AriaRole::SpinButton);
            assert_eq!(nodes[0].name.as_deref(), Some("Font size"));
            assert_eq!(
                nodes[0].value,
                Some(AccessValue::Float { value: START, min: MIN, max: MAX }),
                "aria-valuenow / valuemin / valuemax",
            );
        });
    }

    #[test]
    fn r736_focused_flag_tracks_the_input_tag() {
        Owner::new().run(|| {
            let _ = boot_scene();
            assert!(NumberView::access_node(&idle(), Some(INPUT_TAG))[0].state.focused);
            assert!(!NumberView::access_node(&idle(), None)[0].state.focused);
        });
    }

    // ----- view / paint -----

    #[test]
    fn r736_view_carries_input_and_stepper_tags() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let scene = view(&idle(), &Frame::new());
            assert!(scene.contains_tag(INPUT_TAG), "input painted");
            assert!(scene.contains_tag(DEC_TAG), "decrement stepper painted");
            assert!(scene.contains_tag(INC_TAG), "increment stepper painted");
            assert!(scene.contains_tag(STATUS_TAG), "status line painted");
        });
    }

    #[test]
    fn r736_view_contains_input_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<NumberView>(
            idle(),
            &Frame::default(),
        );
    }
}
