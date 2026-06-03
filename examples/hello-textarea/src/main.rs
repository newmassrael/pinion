//! `hello-textarea` — R764 §5.22 §5.36 multi-line `TextField` (textarea)
//! consumer.
//!
//! ## What it demonstrates
//!
//! The multi-line text-editing substrate landed in R764 on top of the
//! existing single-line `TextField` (R56.1.b.1) + click-caret (R762) +
//! pointer-selection (R763) stack. The same [`TextFieldExternal`] +
//! [`TextEditState`](pinion_core::widgets::text_edit::TextEditState) +
//! [`tf_paint`] substrate drives both — a textarea is a `TextField`
//! whose content carries `\n` and whose [`TextFieldStyle::m3_multiline`]
//! style top-aligns a `rows`-tall box. The pieces R764 adds:
//!
//! - **Per-line selection bands** — [`tf_paint::view_field`] decomposes
//!   a selection range into one rect per visual line
//!   (`pinion_text::selection_rects_for_range` → parley
//!   `Selection::geometry`), so a drag / Shift-select that spans hard
//!   line breaks paints the partial-first / full-middle / partial-last
//!   band shape a text editor draws.
//! - **Vertical caret navigation** — `ArrowUp` / `ArrowDown` resolve the
//!   adjacent-line byte via
//!   [`tf_paint::byte_for_field_vertical_move`]
//!   (`pinion_text::byte_offset_for_line_move` → parley
//!   `Selection::move_lines`). This is geometry-dependent so it runs in
//!   the binding's `apply_key` (which holds the layout cache), not on
//!   the geometry-free `TextEditState` path the horizontal arrows use.
//! - **`Enter` inserts a newline** — the one binding-level divergence
//!   from a single-line field (where `Enter` would submit / be ignored).
//! - **Soft-wrap at width** (R765) — [`TextFieldStyle::m3_multiline`]
//!   sets `soft_wrap = true`, so a long line with no `\n` breaks onto
//!   additional *visual* lines at the box's inner width. This needed no
//!   binding change: the `field_shaping` SSOT threads the wrap width to
//!   paint, the caret rect, the hit-test, and vertical nav, so parley
//!   resolves wrapped-line caret/selection/point geometry for free.
//!
//! Click-to-position, drag-select, and Shift-click all reuse the R762 /
//! R763 hooks unchanged — `byte_for_field_point` hit-tests against the
//! multi-line layout (parley `Cursor::from_point` already picks the line
//! by `y`), so a click on line 3 lands on line 3 with no extra work.
//! Soft-wrapped lines are visual lines like any other, so `ArrowUp` /
//! `ArrowDown` and pointer hits cross wrap boundaries with no extra work.
//!
//! R766 adds the **goal column** (`ArrowUp` / `ArrowDown` hold the
//! horizontal column across a short line) + **visual-line `Home` / `End`**
//! (with `Ctrl+Home` / `Ctrl+End` for the document boundaries).
//!
//! R767 adds **rich-text editing**: the textarea seeds three colour
//! [`StyleRun`](pinion_core::scene::StyleRun) spans (the leading word of
//! each line) into
//! [`TextEditState::set_style_runs`](pinion_core::widgets::text_edit::TextEditState::set_style_runs).
//! The runs ride along through edits (insert shifts them, deleting a
//! styled word drops its run) via the `TextEditState` `FormatRange`
//! maintenance, and [`tf_paint::view_field`] threads them through the
//! `field_shaping` SSOT so paint and caret/hit-test geometry shape one
//! identical run-aware `Layout`.
//!
//! R768 adds **apply-to-selection**: a colour-swatch toolbar under the
//! field. Select a range, click a swatch → that span takes the swatch's
//! colour via
//! [`TextEditState::apply_style_run`](pinion_core::widgets::text_edit::TextEditState::apply_style_run)
//! (Qt `setCharFormat` semantics — the overlay carves existing runs and
//! merges abutting identical spans); the clear swatch strips formatting.
//! The same operation is the AI-first `apply-style` / `clear-style`
//! invoke funnel on [`TextFieldExternal`], so a `scene/invoke` drives the
//! identical path. A runs-only change (no text edit) repaints through the
//! reactive `style_runs` `Signal`.
//!
//! R769 adds **field-level merge** (`mergeCharFormat`): **B** / **I**
//! toggle buttons. Selecting a coloured word and clicking **B** makes it
//! bold *while keeping its colour* — the toggle changes only the weight,
//! via
//! [`TextEditState::merge_style_run`](pinion_core::widgets::text_edit::TextEditState::merge_style_run)
//! (covered bytes transform their run; unstyled bytes resolve against the
//! field base). The toggle direction is read from the selection start
//! with `style_at`. Bold / italic are metric-affecting runs, so the
//! `field_shaping` SSOT keeps the caret + hit-test geometry exact.
//!
//! ## Try it
//!
//! ```text
//! cargo run --release -p hello-textarea
//! ```
//!
//! Tab in → caret blinks. Type → text inserts; `Enter` starts a new
//! line. Keep typing on one line past the box edge → it soft-wraps to
//! the next visual line. `ArrowUp` / `ArrowDown` move between lines
//! (hold `Shift` to select); drag across lines to select a multi-line
//! band. Select a word and click **B** / **I** to bold / italicise it
//! (keeping its colour), or a colour swatch to recolour it; the bordered
//! swatch clears formatting.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::clipboard::{Clipboard, InMemoryClipboard};
use pinion_core::external::External;
use pinion_core::reactive::Owner;
use pinion_core::scene::{ContainerNode, Rect, StyleRun, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, FontStyle, FontWeight, JustifyContent,
    LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldEvent, TextFieldExternal, TextFieldState};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_platform_clipboard::ArboardClipboard;
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_text::CaretRect;
use pinion_widget_paint::text_field as tf_paint;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTextAreaRenderer, HelloTextAreaRendererError);

/// Paint-root + input-router + reactive-cache tag for the textarea.
const TA_TAG: &str = "main_textarea";
/// Shared [`ThemeProvider`] cache key (matches the gallery `"app"`
/// convention so a host binding shares one provider).
const THEME_TAG: &str = "app";
/// Visible rows for the textarea box.
const TA_ROWS: u32 = 5;

const WIN_W: u32 = 480;
const WIN_H: u32 = 320;

const TITLE_FONT_SIZE_PX: u32 = 18;
const STATUS_FONT_SIZE_PX: u32 = 12;
const ROW_GAP: u32 = 16;

/// R768 — an (R, G, B) ink triple shared by the seed runs + toolbar.
type Rgb = (u8, u8, u8);
/// R768 — a toolbar swatch: its tag + ink (`None` = the clear swatch).
type Swatch = (&'static str, Option<Rgb>);

// R767 §5.36 — the three seed colours (leading word of each line) shared
// by `create_external`'s run seed and the R768 toolbar swatches so the
// "apply to selection" affordance recolours text into the same palette.
const INK_RED: Rgb = (0xD0, 0x28, 0x28);
const INK_GREEN: Rgb = (0x1F, 0x8A, 0x34);
const INK_BLUE: Rgb = (0x26, 0x4C, 0xD8);

// R768 §5.36 §5.22 — colour-apply toolbar. Each swatch is a tagged,
// non-focusable `BoxNode` (a decoration — clicking it never steals focus
// from the field, so the selection survives); the press hook routes a
// hit to `TextEditState::apply_style_run` over the live selection. The
// last swatch (`None`) clears formatting back to the base style.
const SW_RED: &str = "swatch_red";
const SW_GREEN: &str = "swatch_green";
const SW_BLUE: &str = "swatch_blue";
const SW_CLEAR: &str = "swatch_clear";
const SWATCH_SIZE: u32 = 28;
const SWATCH_GAP: u32 = 10;
const SWATCHES: [Swatch; 4] = [
    (SW_RED, Some(INK_RED)),
    (SW_GREEN, Some(INK_GREEN)),
    (SW_BLUE, Some(INK_BLUE)),
    (SW_CLEAR, None),
];

// R769 §5.36 §5.22 — field-level merge (mergeCharFormat) toggle buttons:
// bold / italic over the selection, *preserving* its colour. Unlike the
// colour swatches (wholesale setCharFormat) these route through
// `TextEditState::merge_style_run`, which keeps untouched fields.
const TB_BOLD: &str = "toggle_bold";
const TB_ITALIC: &str = "toggle_italic";

/// R769 — which text field a toggle button flips.
#[derive(Clone, Copy)]
enum ToggleField {
    Bold,
    Italic,
}

const TOGGLES: [(&str, ToggleField); 2] =
    [(TB_BOLD, ToggleField::Bold), (TB_ITALIC, ToggleField::Italic)];

/// R764 §5.22 — single source of truth for the textarea's
/// [`TextFieldStyle`]. The view fn, the pointer hooks, and the
/// vertical-nav `apply_key` arm all shape against this *identical*
/// style so the painted Layout and the hit-tested / line-moved Layout
/// stay one cache entry (the R762.1 `field_shaping` SSOT discipline).
fn ta_style() -> tf_paint::TextFieldStyle {
    tf_paint::TextFieldStyle::m3_multiline(TA_ROWS)
}

/// R768 §5.36 — the colour-only [`TextStyle`] a swatch (and the R767
/// seed) applies: the field's base font size with the given ink, every
/// other field at its default so the run is metric-neutral (no glyph-
/// metric shift, so paint and caret/hit-test geometry stay one layout).
fn swatch_text_style(rgb: Rgb) -> TextStyle {
    TextStyle::new()
        .with_size_px(ta_style().font_size_px)
        .with_fg(Color::rgb(rgb.0, rgb.1, rgb.2))
}

/// R769 §5.36 — the field's default char format: the base style unstyled
/// text paints with. Passed to [`TextEditState::merge_style_run`] so a
/// bold/italic toggle over *unstyled* bytes resolves their colour from
/// here (M3 `OnSurface`, the field text ink) rather than dropping to a
/// hard-coded default.
fn base_text_style(theme: &pinion_core::theme::Theme) -> TextStyle {
    TextStyle::new()
        .with_size_px(ta_style().font_size_px)
        .with_fg(theme.resolve(ColorRole::OnSurface))
}

/// R768 §5.36 §5.22 / R769 — the formatting toolbar: bold + italic toggle
/// buttons (field-level `mergeCharFormat`) then a row of colour swatches
/// (wholesale `setCharFormat`) + a clear swatch. Each control is a tagged
/// decoration; the press hook ([`TextAreaView::position_caret_for_point`])
/// resolves a click to its tag and applies it to the live selection.
fn toolbar(theme: &pinion_core::theme::Theme) -> Scene {
    let cell = |tag: &'static str, fill: Color, border: bool, child: Vec<Scene>| {
        let mut style = BoxStyle::filled(fill).with_corner_radius(6);
        if border {
            style = style.with_border(Border::new(theme.resolve(ColorRole::OnSurfaceMuted), 1));
        }
        Scene::Container(
            ContainerNode::new(child)
                .with_tag(tag)
                .with_style(style)
                .with_layout(
                    LayoutStyle::new()
                        .with_size(Size::px(SWATCH_SIZE, SWATCH_SIZE))
                        .with_justify(JustifyContent::Center)
                        .with_align_items(AlignItems::Center),
                ),
        )
    };
    // R769 — bold / italic toggle buttons, labelled with a "B" / "I"
    // glyph rendered in the very style they toggle (no icon font).
    let label = |text: &'static str, style: TextStyle| {
        vec![Scene::Text(TextNode::styled(text, Rect::default(), style))]
    };
    let on = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::SurfaceContainerHighest);
    let bold_btn = cell(
        TB_BOLD,
        surface,
        true,
        label("B", TextStyle::new().with_fg(on).with_weight(FontWeight::BOLD)),
    );
    let italic_btn = cell(
        TB_ITALIC,
        surface,
        true,
        label("I", TextStyle::new().with_fg(on).with_style(FontStyle::Italic)),
    );
    let mut children = vec![bold_btn, italic_btn];
    children.extend(SWATCHES.iter().map(|&(tag, ink)| match ink {
        // Clear swatch: a neutral bordered surface (the bordered empty
        // box reads as "no fill / remove").
        None => cell(tag, surface, true, Vec::new()),
        Some(rgb) => cell(tag, Color::rgb(rgb.0, rgb.1, rgb.2), false, Vec::new()),
    }));
    Scene::Container(
        ContainerNode::new(children).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_gap(SWATCH_GAP),
        ),
    )
}

/// R768/R769 — does `(x, y)` land on the toolbar control tagged `tag`?
/// One rect-contains test shared by the swatch + toggle press routing.
fn hit_tag(scene: &Scene, tag: &'static str, x: f32, y: f32) -> bool {
    let Some(rect) = pinion_shell::rect_for_tag(scene, tag) else {
        return false;
    };
    #[allow(
        clippy::cast_precision_loss,
        reason = "toolbar rect coords are small screen pixels; f32 is exact below 2^23"
    )]
    {
        x >= rect.x as f32
            && x < (rect.x + rect.w) as f32
            && y >= rect.y as f32
            && y < (rect.y + rect.h) as f32
    }
}

/// R769 §5.36 §5.22 — flip `field` over the live selection via
/// [`TextEditState::merge_style_run`]. *Policy* lives here (the toggle
/// direction is read from the selection start via
/// [`TextEditState::style_at`]); the substrate owns the *mechanics*. The
/// other style fields are preserved, so bolding a coloured word keeps its
/// colour. No-op when nothing is selected.
fn apply_toggle(field: ToggleField) {
    let edit = use_text_edit_state(TA_TAG);
    let Some((start, end)) = edit.selection_range() else {
        return;
    };
    let theme = use_theme(THEME_TAG).theme_animated();
    let base = base_text_style(&theme);
    let at_start = edit.style_at(start);
    match field {
        ToggleField::Bold => {
            let now_bold = at_start.is_some_and(|st| st.font_weight == FontWeight::BOLD);
            let target = if now_bold { FontWeight::NORMAL } else { FontWeight::BOLD };
            edit.merge_style_run(start, end, &base, move |st| st.font_weight = target);
        }
        ToggleField::Italic => {
            let now_italic = at_start.is_some_and(|st| st.font_style == FontStyle::Italic);
            let target = if now_italic { FontStyle::Normal } else { FontStyle::Italic };
            edit.merge_style_run(start, end, &base, move |st| st.font_style = target);
        }
    }
}

/// R768 §5.36 §5.22 / R769 — toolbar press router. Returns `true` when
/// `(x, y)` landed on a toolbar control (the press is swallowed — no
/// caret move): a toggle button flips bold/italic over the selection
/// (merge), a colour swatch sets its ink (overlay), the clear swatch
/// strips formatting. A press with no active selection is still
/// swallowed (the toolbar owns that pixel) but is a no-op — formatting
/// needs a range. Returns `false` when nothing was hit, so the caller
/// falls through to caret positioning.
fn try_toolbar_press(scene: &Scene, x: f32, y: f32) -> bool {
    for (tag, field) in TOGGLES {
        if hit_tag(scene, tag, x, y) {
            apply_toggle(field);
            return true;
        }
    }
    for (tag, ink) in SWATCHES {
        if hit_tag(scene, tag, x, y) {
            if let Some((start, end)) = use_text_edit_state(TA_TAG).selection_range() {
                let edit = use_text_edit_state(TA_TAG);
                match ink {
                    Some(rgb) => edit.apply_style_run(start, end, swatch_text_style(rgb)),
                    None => edit.clear_style_runs(start, end),
                }
            }
            return true;
        }
    }
    false
}

/// R56.2.b §5.22 — `Sized` wrapper around `Box<dyn Clipboard>` so the
/// [`use_clipboard`] hook parks either an [`ArboardClipboard`] or an
/// [`InMemoryClipboard`] in one `Owner::cache` slot (mirror of the
/// hello-textfield pattern).
struct AppClipboard(Box<dyn Clipboard>);

impl Clipboard for AppClipboard {
    fn copy(&self, text: String) {
        self.0.copy(text);
    }
    fn paste(&self) -> Option<String> {
        self.0.paste()
    }
}

/// `Owner::cache`-keyed clipboard hook — platform `arboard` with an
/// in-memory fallback when the platform daemon is unreachable.
fn use_clipboard(key: &'static str) -> Rc<dyn Clipboard> {
    let cb: Rc<AppClipboard> = Owner::current()
        .expect("use_clipboard requires an active Owner scope")
        .cache(key, || {
            AppClipboard(match ArboardClipboard::try_new() {
                Ok(arboard) => Box::new(arboard) as Box<dyn Clipboard>,
                Err(e) => {
                    eprintln!(
                        "hello-textarea: ArboardClipboard init failed ({e}); \
                         falling back to InMemoryClipboard",
                    );
                    Box::new(InMemoryClipboard::new()) as Box<dyn Clipboard>
                }
            })
        });
    cb
}

/// View: title + the multi-line field + a status line mirroring the
/// live `(state, caret, selection)` so the AI side verifies the same
/// data the visible field renders via `scene/query`.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "view-fn shape mirrors hello-textfield"
)]
fn view(state: (TextFieldState, u32), _frame: &Frame) -> Scene {
    let (interaction, caret_byte) = state;
    let theme = use_theme(THEME_TAG).theme_animated();

    let field = tf_paint::view_field(
        TA_TAG,
        interaction,
        caret_byte,
        &theme,
        &ta_style(),
        "Multi-line text input",
    );

    let title = Scene::Text(TextNode::styled(
        "TextArea",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_SIZE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let text_state = use_text_edit_state(TA_TAG);
    let text = text_state.text();
    let lines = text.split('\n').count();
    let selection = match text_state.selection_range() {
        Some((start, end)) => format!(" | sel={start}..{end}"),
        None => String::new(),
    };
    let status_str = format!(
        "{} | caret={} | lines={}{}",
        interaction.as_name(),
        caret_byte,
        lines,
        selection,
    );
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_SIZE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, field, toolbar(&theme), status])
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

struct TextAreaView;

impl WidgetCore for TextAreaView {
    type State = (TextFieldState, u32);
    type Event = TextFieldEvent;

    fn create_external() -> Box<dyn External> {
        let text_state = use_text_edit_state(TA_TAG);
        // Seed multi-line content so the textarea reads as multi-line at
        // boot (parley breaks on the explicit `\n`).
        if text_state.text().is_empty() {
            text_state.set_text("first line\nsecond line\nthird line".to_owned());
            text_state.set_caret(0);
            // R767 §5.36 — seed rich styled runs over the leading word of
            // each line (colour-only: same size/weight as the field base,
            // so the runs read as rich formatting without changing glyph
            // metrics). The runs ride along through edits — typing before
            // "first" shifts all three right, deleting a styled word drops
            // its run — via the TextEditState FormatRange maintenance.
            text_state.set_style_runs(vec![
                StyleRun::new(0, 5, swatch_text_style(INK_RED)), // "first"  red
                StyleRun::new(11, 17, swatch_text_style(INK_GREEN)), // "second" green
                StyleRun::new(23, 28, swatch_text_style(INK_BLUE)), // "third"  blue
            ]);
        }
        let blink = use_caret_blink(TA_TAG);
        let clipboard = use_clipboard(TA_TAG);
        Box::new(
            TextFieldExternal::new()
                .attach_state(text_state)
                .attach_blink(blink)
                .attach_clipboard(clipboard),
        )
    }

    fn tag() -> &'static str {
        TA_TAG
    }

    fn read_state(scene: &Scene) -> (TextFieldState, u32) {
        tf_paint::read_text_field_state(scene, TA_TAG)
    }

    fn view(state: (TextFieldState, u32), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(event: TextFieldEvent) -> &'static str {
        pinion_core::WidgetEventName::as_name(&event)
    }

    fn title() -> &'static str {
        "pinion hello-textarea (R769 §5.36 §5.22)"
    }

    /// R764 §5.22 / R766 — multi-line key handling. Five keys diverge
    /// from the single-line field; everything else forwards to the
    /// External's modifier-aware `invoke("key", ...)` channel exactly
    /// like hello-textfield.
    ///
    /// - `Enter` inserts a newline (a single-line field would submit).
    /// - `ArrowUp` / `ArrowDown` move the caret one visual line, holding
    ///   the **goal column** across the run (R766), via the layout-
    ///   geometry helper ([`tf_paint::byte_for_field_vertical_move`]).
    ///   Shift extends the selection from the retained anchor. These run
    ///   here (not on the `TextEditState` `apply_key` path) because
    ///   vertical movement needs the shaped layout the binding's cache
    ///   holds.
    /// - `Home` / `End` move the caret to the start / end of the current
    ///   **visual** line (R766, soft-wrap aware) via
    ///   [`tf_paint::byte_for_field_line_boundary`] — not the buffer
    ///   ends the single-line field uses. `Ctrl+Home` / `Ctrl+End`
    ///   promote them to the **document** start / end (byte `0` /
    ///   `text.len()`), the canonical editor pairing. Shift extends from
    ///   the anchor (so `Ctrl+Shift+Home` selects to the document start).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(TA_TAG) {
            return false;
        }
        match key {
            "Enter" => {
                use_text_edit_state(TA_TAG).insert("\n");
                return true;
            }
            "ArrowUp" | "ArrowDown" => {
                let (interaction, _caret) = Self::read_state(scene);
                let theme = use_theme(THEME_TAG).theme_animated();
                let delta = if key == "ArrowUp" { -1 } else { 1 };
                let (new_byte, goal_x) = tf_paint::byte_for_field_vertical_move(
                    TA_TAG,
                    interaction,
                    delta,
                    &theme,
                    &ta_style(),
                );
                let edit = use_text_edit_state(TA_TAG);
                if modifiers.shift_key() {
                    let anchor = edit.selection_anchor().unwrap_or_else(|| edit.caret());
                    edit.set_selection(anchor, new_byte);
                } else {
                    edit.set_caret(new_byte);
                }
                // R766 — re-arm the goal column *after* the caret write
                // (which cleared it) so a run of ArrowUp/ArrowDown holds
                // the original column across a short line.
                edit.set_goal_column(Some(goal_x));
                return true;
            }
            "Home" | "End" => {
                // R766 §5.22 — Home/End move to the current **visual**
                // row's start / end (soft-wrap aware), not the buffer ends
                // the single-line field uses. The Ctrl modifier promotes
                // them to **document** start / end (the canonical editor
                // pairing — Home=row, Ctrl+Home=document), keeping the
                // buffer boundaries reachable in one keystroke. Shift
                // extends the selection from the retained anchor (so
                // Ctrl+Shift+Home selects to the document start), exactly
                // like the vertical moves above.
                let end = key == "End";
                let edit = use_text_edit_state(TA_TAG);
                let new_byte = if modifiers.control_key() {
                    if end {
                        edit.text().len()
                    } else {
                        0
                    }
                } else {
                    let (interaction, _caret) = Self::read_state(scene);
                    let theme = use_theme(THEME_TAG).theme_animated();
                    tf_paint::byte_for_field_line_boundary(
                        TA_TAG,
                        interaction,
                        end,
                        &theme,
                        &ta_style(),
                    )
                };
                if modifiers.shift_key() {
                    let anchor = edit.selection_anchor().unwrap_or_else(|| edit.caret());
                    edit.set_selection(anchor, new_byte);
                } else {
                    edit.set_caret(new_byte);
                }
                return true;
            }
            _ => {}
        }
        // R764.1 — forward everything else through the lifted SSOT
        // (modifier-aware: Shift+Arrow / Ctrl+A reach the substrate's
        // selection arms).
        tf_paint::forward_key_to_field(scene, TA_TAG, key, modifiers)
    }

    /// R56.2.a §5.13 — platform IME composition (mirror of
    /// hello-textfield; the textarea reuses the same substrate funnel).
    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        if focused != Some(TA_TAG) {
            return false;
        }
        // R764.1 §5.38 §5.13 — reformat + forward through the lifted SSOT.
        tf_paint::forward_composition_to_field(scene, TA_TAG, event)
    }

    fn fmt_state_log(state: &(TextFieldState, u32)) -> String {
        format!("{} / caret={}", state.0.as_name(), state.1)
    }
}

impl WidgetA11y for TextAreaView {
    /// R764 §5.40 — ARIA `textbox` carrying the live multi-line text as
    /// [`AccessValue::Text`]. (The multi-line `aria-multiline` refinement
    /// is an additive a11y axis — deferred until a 2nd textarea consumer
    /// per [[abstraction-needs-second-consumer]].)
    fn access_node(state: &(TextFieldState, u32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, _caret) = state;
        let text = use_text_edit_state(TA_TAG).text();
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            disabled: matches!(interaction, TextFieldState::Disabled),
            hovered: false,
            pressed: false,
            checked: None,
        };
        vec![
            AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::TextInput)
                .with_value(AccessValue::Text(text))
                .with_state(access_state),
        ]
    }
}

impl WidgetView for TextAreaView {
    type Renderer = HelloTextAreaRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }

    fn ime_caret_rect(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
    ) -> Option<CaretRect> {
        if focused != Some(TA_TAG) {
            return None;
        }
        let (interaction, caret_byte) = *state;
        let field_rect = pinion_shell::rect_for_tag(scene, TA_TAG)?;
        let theme = use_theme(THEME_TAG).theme_animated();
        Some(tf_paint::ime_caret_rect_for(
            TA_TAG,
            interaction,
            caret_byte,
            field_rect,
            &theme,
            &ta_style(),
        ))
    }

    /// R762 / R763 reused verbatim against the multi-line layout: the
    /// hit-test (`byte_for_field_point`) picks the line by `y`, so a
    /// click / drag / Shift-click on any line resolves correctly.
    fn position_caret_for_point(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
        x: f32,
        y: f32,
        extend: bool,
    ) -> Option<usize> {
        if focused != Some(TA_TAG) {
            return None;
        }
        // R768 §5.36 §5.22 — toolbar swatch router runs before caret
        // positioning: a press on a colour swatch applies it to the live
        // selection (and is swallowed, leaving caret + selection intact)
        // — the click peer of the AI-first `apply-style` invoke funnel,
        // both reaching `TextEditState::apply_style_run`.
        if try_toolbar_press(scene, x, y) {
            return None;
        }
        let byte = hit_test_area_byte(*state, scene, x, y)?;
        let edit = use_text_edit_state(TA_TAG);
        if extend {
            let anchor = edit.selection_anchor().unwrap_or_else(|| edit.caret());
            edit.set_selection(anchor, byte);
            Some(anchor)
        } else {
            edit.set_caret(byte);
            Some(byte)
        }
    }

    fn select_drag_to_point(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
        anchor: usize,
        x: f32,
        y: f32,
    ) -> bool {
        if focused != Some(TA_TAG) {
            return false;
        }
        let Some(byte) = hit_test_area_byte(*state, scene, x, y) else {
            return false;
        };
        let edit = use_text_edit_state(TA_TAG);
        let before = (edit.caret(), edit.selection_anchor());
        edit.set_selection(anchor, byte);
        before != (edit.caret(), edit.selection_anchor())
    }
}

/// R764 §5.36 §5.22 — shared pointer hit-test for the press + drag
/// hooks (mirror of hello-textfield's `hit_test_field_byte`): resolve a
/// window-local pixel point to a byte via the `byte_for_field_point`
/// SSOT against the *multi-line* `ta_style()` layout.
fn hit_test_area_byte(
    state: (TextFieldState, u32),
    scene: &Scene,
    x: f32,
    y: f32,
) -> Option<usize> {
    let (interaction, _caret_byte) = state;
    let field_rect = pinion_shell::rect_for_tag(scene, TA_TAG)?;
    let theme = use_theme(THEME_TAG).theme_animated();
    Some(tf_paint::byte_for_field_point(
        TA_TAG,
        interaction,
        x,
        y,
        field_rect,
        &theme,
        &ta_style(),
    ))
}

fn main() {
    pinion_shell::run::<TextAreaView>();
}

#[cfg(test)]
mod tests {
    use super::{view, TextAreaView, TA_TAG};
    use pinion_core::reactive::Owner;
    use pinion_core::scene::ExternalNode;
    use pinion_core::widgets::text_edit::use_text_edit_state;
    use pinion_core::widgets::text_field::TextFieldState;
    use pinion_core::{Frame, Scene, WidgetCore};

    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    #[test]
    fn r764_view_carries_textarea_tag() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            assert!(scene.contains_tag(TA_TAG), "paint scene carries the textarea tag");
        });
    }

    #[test]
    fn r764_enter_inserts_newline() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            edit.set_text("ab".to_owned());
            edit.set_caret(1);
            let mut scene =
                Scene::External(ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG));
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "Enter",
                pinion_core::Modifiers::empty(),
            ));
            // create_external seeds when empty; we set non-empty above so
            // the buffer is "ab" and Enter splits it at the caret.
            assert_eq!(edit.text(), "a\nb", "Enter inserts a newline at the caret");
            assert_eq!(edit.caret(), 2, "caret advances past the inserted newline");
        });
    }

    #[test]
    fn r764_arrow_down_moves_to_next_line() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            edit.set_text("abc\nxyz".to_owned());
            edit.set_caret(1); // after 'a' on line 0
            let mut scene =
                Scene::External(ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG));
            // Paint once so the layout cache holds the shaped multi-line
            // Layout the vertical-move helper reads.
            let _ = view((TextFieldState::Focused, 1), &Frame::default());
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "ArrowDown",
                pinion_core::Modifiers::empty(),
            ));
            // Line 1 starts at byte 4 ("abc\n" = 4 bytes); column 1 → byte 5.
            assert_eq!(edit.caret(), 5, "ArrowDown lands on the next line at the same column");
        });
    }

    /// R766 — `apply_key` for a vertical move re-arms the goal column so
    /// a subsequent vertical move can reuse it.
    #[test]
    fn r766_arrow_down_arms_goal_column() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            edit.set_text("abc\nxyz".to_owned());
            edit.set_caret(1);
            let mut scene =
                Scene::External(ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG));
            let _ = view((TextFieldState::Focused, 1), &Frame::default());
            assert!(edit.goal_column().is_none(), "goal column starts unarmed");
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "ArrowDown",
                pinion_core::Modifiers::empty(),
            ));
            assert!(
                edit.goal_column().is_some(),
                "ArrowDown re-arms the goal column for the next move",
            );
        });
    }

    /// R766 — the goal column survives a vertical pass through a short
    /// line: `ArrowDown` into the short line clamps the column,
    /// `ArrowDown` again into the long line restores a column wider than
    /// the short line could hold (proving the goal rode along the run).
    #[test]
    fn r766_goal_column_restores_column_across_short_line() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            // line 0 "aaaaaaaa" (0..8), '\n' 8, line 1 "bb" (9..11),
            // '\n' 11, line 2 "cccccccc" (12..20).
            edit.set_text("aaaaaaaa\nbb\ncccccccc".to_owned());
            edit.set_caret(5); // line 0, column 5
            let mut scene =
                Scene::External(ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG));
            let _ = view((TextFieldState::Focused, 5), &Frame::default());
            let down = |scene: &mut Scene| {
                TextAreaView::apply_key(
                    scene,
                    Some(TA_TAG),
                    "ArrowDown",
                    pinion_core::Modifiers::empty(),
                )
            };
            assert!(down(&mut scene));
            let m1 = edit.caret();
            assert!((9..=11).contains(&m1), "first ArrowDown lands on the short line 1 (got {m1})");
            assert!(down(&mut scene));
            let m2 = edit.caret();
            assert!(m2 >= 12, "second ArrowDown lands on the long line 2 (got {m2})");
            assert!(
                m2 - 12 > m1 - 9,
                "goal column restores a column wider than the short line allowed \
                 (line2 col {} > line1 col {})",
                m2 - 12,
                m1 - 9,
            );
        });
    }

    /// R766 — `Home` / `End` move to the visual line boundary, not the
    /// buffer ends, and the caret-write clears the goal column.
    #[test]
    fn r766_home_end_move_to_visual_line_boundary() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            edit.set_text("abc\nxyz".to_owned()); // line 1 = bytes 4..7
            edit.set_caret(6); // line 1, between 'y' and 'z'
            let mut scene =
                Scene::External(ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG));
            let _ = view((TextFieldState::Focused, 6), &Frame::default());
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "Home",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(edit.caret(), 4, "Home moves to the start of the current visual line");
            assert!(edit.goal_column().is_none(), "Home (a horizontal move) clears the goal column");
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "End",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(edit.caret(), 7, "End moves to the end of the current visual line");
        });
    }

    /// R766 — `Ctrl+Home` / `Ctrl+End` move to the document boundaries
    /// (byte 0 / `text.len()`), not the current visual row.
    #[test]
    fn r766_ctrl_home_end_move_to_document_boundary() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            edit.set_text("abc\nxyz".to_owned());
            edit.set_caret(5); // line 1
            let mut scene =
                Scene::External(ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG));
            let _ = view((TextFieldState::Focused, 5), &Frame::default());
            let ctrl = pinion_core::Modifiers { ctrl: true, ..pinion_core::Modifiers::empty() };
            assert!(TextAreaView::apply_key(&mut scene, Some(TA_TAG), "Home", ctrl));
            assert_eq!(edit.caret(), 0, "Ctrl+Home moves to the document start");
            assert!(TextAreaView::apply_key(&mut scene, Some(TA_TAG), "End", ctrl));
            assert_eq!(edit.caret(), 7, "Ctrl+End moves to the document end");
            // Ctrl+Shift+Home selects from the caret back to byte 0.
            let ctrl_shift = pinion_core::Modifiers {
                ctrl: true,
                shift: true,
                ..pinion_core::Modifiers::empty()
            };
            assert!(TextAreaView::apply_key(&mut scene, Some(TA_TAG), "Home", ctrl_shift));
            assert_eq!(
                edit.selection_range(),
                Some((0, 7)),
                "Ctrl+Shift+Home selects from the document end back to the start",
            );
        });
    }

    /// R766 — `Shift+Home` extends the selection from the retained
    /// anchor to the visual line start (mirror of the vertical shift
    /// path).
    #[test]
    fn r766_shift_home_extends_selection_to_line_start() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            edit.set_text("abc\nxyz".to_owned());
            edit.set_caret(6); // line 1
            let mut scene =
                Scene::External(ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG));
            let _ = view((TextFieldState::Focused, 6), &Frame::default());
            let mods = pinion_core::Modifiers { shift: true, ..pinion_core::Modifiers::empty() };
            assert!(TextAreaView::apply_key(&mut scene, Some(TA_TAG), "Home", mods));
            assert_eq!(edit.caret(), 4, "Shift+Home moves the caret to the line start");
            assert_eq!(
                edit.selection_range(),
                Some((4, 6)),
                "Shift+Home selects from the line start to the original caret",
            );
        });
    }
}
