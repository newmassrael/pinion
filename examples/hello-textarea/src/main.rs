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
//! whose content carries `\n` and whose `TextFieldStyle::m3_multiline`
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
//! - **Soft-wrap at width** (R765) — `TextFieldStyle::m3_multiline`
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
//! [`StyleRun`] spans (the leading word of
//! each line) into
//! [`TextEditState::set_style_runs`](pinion_core::widgets::text_edit::TextEditState::set_style_runs).
//! The runs ride along through edits (insert shifts them, deleting a
//! styled word drops its run) via the `TextEditState` `FormatRange`
//! maintenance, and [`tf_paint::view_field`] threads them through the
//! `field_shaping` SSOT so paint and caret/hit-test geometry shape one
//! identical run-aware `Layout`.
//!
//! R768 adds **apply-to-selection**: a colour-swatch toolbar under the field.
//! Select a range, click a swatch → that span takes the swatch's colour via
//! [`TextEditState::apply_style_run`](pinion_core::widgets::text_edit::TextEditState::apply_style_run) (the
//! toolkit `setCharFormat` semantics — the overlay carves existing runs and merges
//! abutting identical spans); the clear swatch strips formatting. The same
//! operation is the AI-first `apply-style` / `clear-style` invoke funnel on [`TextFieldExternal`], so a `scene/invoke` drives
//! the identical path. A runs-only change (no text edit) repaints through the
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
//! R798 adds **undo / redo**: the textarea is the 2nd
//! [`TextEditState::attach_undo`](pinion_core::widgets::text_edit::TextEditState::attach_undo)
//! consumer after hello-textfield, so `Ctrl+Z` / `Ctrl+Shift+Z` (`Ctrl+Y`)
//! undo / redo with word-level coalescing through the same substrate path
//! (`forward_key_to_field` recognises the chords once a stack is attached).
//! The stack is attached *after* the boot seed, so the seeded multi-line
//! rich-text document is the undo floor. R796.1 made the command granular —
//! it carries the deleted `removed_runs` — so undoing a deleted coloured
//! word restores both its text **and** its style run, exercising run
//! reversal against real multi-line rich text.
//!
//! R799 makes the formatting toolbar a **framework-routed, non-focusable
//! [`Toolbar`](pinion_core::widgets::toolbar::Toolbar) widget** instead of
//! hand-rolled decoration. The R768/R769 toolbar was a row of tagged `BoxNode`s that
//! the caret press hook scanned by hand (`hit_tag` / `try_toolbar_press`) — an application
//! re-implementing the `InputRouter`. Now the strip is a [`ToolbarExternal`] sibling: its six controls
//! paint with the composite tags `fmt_toolbar#<i>`, so the router dispatches a click to the
//! External, which emits a `"command"` intent [`TextAreaView::update`] maps to the same `merge_style_run` / `apply_style_run` / `clear_style_runs`
//! op on the live selection. Two framework axes compose to give the editor's
//! canonical behaviour with no new substrate: *routing* (the `InputRouter`, by
//! composite tag) applies the format, while *focus* (the scene-derived `.with_focusable(true)`
//! Tab-stop set) leaves the strip out, so the W3C / the toolkit-`NoFocus` rule the
//! shell encodes means a control click never steals the field's focus — the
//! selection survives. The B / I "pressed" state is reflective: the cell
//! paints a tonal fill read from the selection's `style_at`, so the toolbar mirrors
//! the document rather than owning a toggle bit.
//!
//! R928 makes **formatting itself undoable**. R798's undo covered typing
//! and run-reversal-on-delete, but the toolbar's `apply_style_run` /
//! `clear_style_runs` / `merge_style_run` wrote the run list directly,
//! bypassing the [`UndoStack`](pinion_core::undo::UndoStack) — Bold was the
//! one editable mutation `Ctrl+Z` never reversed. The three mutators now
//! journal a granular `StyleRunCommand` (the run-delta peer of the text
//! splice), so toggling Bold or recolouring a word is a discrete `Ctrl+Z`
//! step — distinct from the surrounding typing — through the GUI toolbar and
//! the `scene/invoke` formatting funnel alike (both call the same mutators).
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
//! swatch clears formatting. `Ctrl+Z` undoes the last edit (a typing run
//! reverts as one word, or — R928 — a Bold / recolour reverts as one
//! discrete step), `Ctrl+Shift+Z` / `Ctrl+Y` redoes; deleting a coloured
//! word and undoing brings its colour back with it.

use pinion_a11y::{AccessNode, ToolbarControl, WidgetA11y, toolbar_button_nodes};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::intent::Intent;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, ScrollNode, StyleRun, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, FontStyle, FontWeight, JustifyContent,
    LayoutStyle, Size, TextAlign, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::undo::use_undo_stack;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::text_edit::{FormatField, use_text_edit_state};
use pinion_core::widgets::text_field::{
    TextFieldEvent, TextFieldExternal, TextFieldSendKey, TextFieldState,
};
use pinion_core::widgets::toolbar::{ToolItem, ToolbarExternal};
use pinion_core::{Command, Frame, Scene, WidgetCore, WidgetStateName, intent_tag};
use pinion_platform_clipboard::use_app_clipboard;
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_text::{CaretRect, VisualLineMetric, logical_line_span};
use pinion_widget_paint::coord::saturating_f32_to_u32;
use pinion_widget_paint::text_field as tf_paint;
use pinion_widget_paint::toolbar::composite_item_tag;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTextAreaRenderer, HelloTextAreaRendererError);

/// Paint-root + input-router + reactive-cache tag for the textarea.
const TA_TAG: &str = "main_textarea";
/// R956 — the line-number gutter box (container) tag. R959 — each number is
/// tagged under the *field's* own composite namespace `main_textarea#gl<n>`
/// (`TextFieldSendKey::gutter_line_tag`), not this box, so a click routes
/// through the `InputRouter` to the field's `send` wire (`go_to_line`) —
/// focus-independent, no caret-drag arm (see [`TextFieldSendKey`]'s B1/B2 doc).
const GUTTER_TAG: &str = "ta_gutter";
/// R957 — the current-line highlight band tag (the gutter row the caret's
/// logical line sits on); one per frame, moves with the caret.
const GUTTER_CURRENT_TAG: &str = "ta_gutter_current";
/// R957 — alpha of the current-line gutter band (a faint Accent tint, the
/// caret / selection hue, so a palette swap restains it coherently).
const GUTTER_CURRENT_ALPHA: u8 = 0x33;
/// R962 — alpha of the *body* current-line band (`TextFieldStyle::current_line_alpha`).
/// Fainter than the gutter band: it washes a whole editor-width row, so a
/// stronger tint would swamp the glyphs. The VS Code current-line weight.
const BODY_CURRENT_ALPHA: u8 = 0x14;
/// Shared [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key (matches the gallery `"app"`
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

// R767 §5.36 — the three seed colours (leading word of each line) shared
// by `create_external`'s run seed and the toolbar swatches so the
// "apply to selection" affordance recolours text into the same palette.
const INK_RED: Rgb = (0xD0, 0x28, 0x28);
const INK_GREEN: Rgb = (0x1F, 0x8A, 0x34);
const INK_BLUE: Rgb = (0x26, 0x4C, 0xD8);

// R799 §5.36 §5.22 §5.38 — the formatting toolbar is a *framework-routed*
// control strip (a non-focusable [`ToolbarExternal`] extra-external), not the hand-rolled
// decoration the pre-R799 binding scanned by hand. Each control is painted
// with the composite tag `fmt_toolbar#<i>`, so the InputRouter hit-tests it and dispatches
// the click to the toolbar External — which emits a `"command"` intent the reducer
// maps to a format op on the live selection. The old manual `hit_tag` / `try_toolbar_press`
// rect-scan (an application re-implementing the router) is gone. Because the
// strip is *not* marked `.with_focusable(true)`, clicking a control applies formatting without
// stealing the field's focus — the W3C / the toolkit `NoFocus` rule the shell
// already encodes (only focusable tags focus on press) supplies the
// no-focus-steal the decoration used to need by construction, so the selection
// survives the click.
//
// Control indices (parallel to the `ToolItem::Command` strip + the paint):
//   0 bold · 1 italic · 2 red · 3 green · 4 blue · 5 clear
const FMT_TAG: &str = "fmt_toolbar";
const FMT_BOLD: usize = 0;
const FMT_ITALIC: usize = 1;
/// Index of the first colour swatch; swatches occupy `[FMT_SWATCH_BASE..]`.
const FMT_SWATCH_BASE: usize = 2;
/// The swatch inks in control order (`None` = the clear swatch).
const FMT_SWATCHES: [Option<Rgb>; 4] = [Some(INK_RED), Some(INK_GREEN), Some(INK_BLUE), None];
/// Total control count: bold + italic + the four swatches.
const FMT_CONTROLS: usize = FMT_SWATCH_BASE + FMT_SWATCHES.len();
/// R800 §5.40 — explicit accessible names per control (the colour swatches
/// have no glyph to enrich a name from). The `[_; FMT_CONTROLS]` size ties
/// this to the control count: adding a control is a compile error until it
/// is named.
const FMT_CONTROL_NAMES: [&str; FMT_CONTROLS] =
    ["Bold", "Italic", "Red", "Green", "Blue", "Clear formatting"];
/// The §5.20 intent the toolbar External emits on activation, prefixed by
/// its scene tag (`fmt_toolbar` + `command`), matched in [`TextAreaView::update`].
const FMT_CMD_INTENT: &str = intent_tag!("fmt_toolbar", "command");
const SWATCH_SIZE: u32 = 28;
const SWATCH_GAP: u32 = 10;

/// R764 §5.22 — single source of truth for the textarea's
/// `TextFieldStyle`. The view fn, the pointer hooks, and the
/// vertical-nav `apply_key` arm all shape against this *identical*
/// style so the painted Layout and the hit-tested / line-moved Layout
/// stay one cache entry (the R762.1 `field_shaping` SSOT discipline).
fn ta_style() -> tf_paint::TextFieldStyle {
    // R962 — opt into the body current-line band. This editor is a code
    // surface (it already carries a line-number gutter), so the row the
    // caret sits on gets the VS Code current-line wash spanning the full
    // field width — the body sibling of the R957 gutter band.
    tf_paint::TextFieldStyle {
        current_line_alpha: BODY_CURRENT_ALPHA,
        ..tf_paint::TextFieldStyle::m3_multiline(TA_ROWS)
    }
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

/// R769 §5.36 / R770.1 — the field's default char format: the base style
/// unstyled text paints with. Passed to `TextEditState::merge_style_run`
/// so a bold/italic toggle over *unstyled* bytes resolves their colour
/// from the field's real base. R770.1 — reuse `tf_paint::field_text_style`
/// (the SSOT `field_shaping` itself shapes against) instead of re-guessing
/// a `ColorRole`, so the merge base and the paint base can never diverge.
fn base_text_style(theme: &pinion_core::theme::Theme, interaction: TextFieldState) -> TextStyle {
    tf_paint::field_text_style(theme, interaction, &ta_style())
}

/// R799 §5.36 §5.22 §5.38 — paint the formatting toolbar. Bold + italic
/// toggle buttons (field-level `mergeCharFormat`) then a row of colour
/// swatches (wholesale `setCharFormat`) + a clear swatch. Each control
/// carries the composite tag `fmt_toolbar#<i>` so the `InputRouter` routes a
/// click to the [`ToolbarExternal`] registered in
/// [`TextAreaView::create_extra_externals`]; the press is dispatched there,
/// not scanned by the binding. `bold_active` / `italic_active` are the
/// *reflective* toggle state read from the selection (`style_at`) — the
/// toolbar owns no format state, it mirrors the document — so the B / I
/// cell paints a tonal fill when the selection start already carries that
/// style (the toolkit's "the bold action is checked when the cursor is in bold
/// text"). `view_toolbar` (the label-only strip) is deliberately not
/// reused: the colour swatches diverge in paint, so the shared substrate
/// is the External routing + intent model, not the label paint (R773).
fn toolbar(theme: &pinion_core::theme::Theme, bold_active: bool, italic_active: bool) -> Scene {
    let surface = theme.resolve(ColorRole::SurfaceContainerHighest);
    let accent = theme.resolve(ColorRole::Accent);
    // Reflective toggle fill: a tonal blend toward the accent while the
    // selection already carries the style (the `aria-pressed` affordance).
    let toggle_fill = |active: bool| {
        if active {
            surface.lerp(accent, 0.16)
        } else {
            surface
        }
    };

    let cell = |index: usize, fill: Color, border: bool, child: Vec<Scene>| {
        let mut style = BoxStyle::filled(fill).with_corner_radius(6);
        if border {
            style = style.with_border(Border::new(theme.resolve(ColorRole::OnSurfaceMuted), 1));
        }
        Scene::Container(
            ContainerNode::new(child)
                .with_tag(composite_item_tag(FMT_TAG, index))
                .with_style(style)
                .with_layout(
                    // `.flex(...)` is required: `justify_content` /
                    // `align_items` are flex properties, ignored unless the
                    // container is a flex box. Without it the cell fell back
                    // to block layout (child top-left), so the "B" / "I"
                    // glyph hugged the top — only `text_align` had been
                    // centring it horizontally.
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_size(Size::px(SWATCH_SIZE, SWATCH_SIZE))
                        .with_justify(JustifyContent::Center)
                        .with_align_items(AlignItems::Center),
                ),
        )
    };
    // Bold / italic toggle buttons, labelled with a "B" / "I" glyph
    // rendered in the very style they toggle (no icon font). The glyph's
    // Text box spans the full cell width, so `JustifyContent` cannot centre
    // it — the glyph must centre *itself* via `text_align`.
    let label = |text: &'static str, style: TextStyle| {
        vec![Scene::Text(TextNode::styled(
            text,
            Rect::default(),
            style.with_align(TextAlign::Center),
        ))]
    };
    let on = theme.resolve(ColorRole::OnSurface);
    let bold_btn = cell(
        FMT_BOLD,
        toggle_fill(bold_active),
        true,
        label(
            "B",
            TextStyle::new().with_fg(on).with_weight(FontWeight::BOLD),
        ),
    );
    let italic_btn = cell(
        FMT_ITALIC,
        toggle_fill(italic_active),
        true,
        label(
            "I",
            TextStyle::new().with_fg(on).with_style(FontStyle::Italic),
        ),
    );
    let mut children = vec![bold_btn, italic_btn];
    children.extend(FMT_SWATCHES.iter().enumerate().map(|(j, ink)| {
        let index = FMT_SWATCH_BASE + j;
        match ink {
            // Clear swatch: a neutral bordered surface (the bordered empty
            // box reads as "no fill / remove").
            None => cell(index, surface, true, Vec::new()),
            Some(rgb) => cell(index, Color::rgb(rgb.0, rgb.1, rgb.2), false, Vec::new()),
        }
    }));
    Scene::Container(
        ContainerNode::new(children)
            // The strip's primary tag = the toolbar External's scene tag
            // (the router's Toolbar scope; per-control tags are `#<i>`).
            .with_tag(FMT_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_gap(SWATCH_GAP),
            ),
    )
}

/// R799 §5.36 §5.22 — apply formatting control `index` to the live
/// selection. The reducer ([`TextAreaView::update`]) calls this when the
/// toolbar External emits its `"command"` intent (so the *router*, not the
/// binding, decides which control was hit). *Policy* lives here (a
/// toggle's direction is read from the selection start via
/// `TextEditState::style_at`); the substrate owns the *mechanics*. No-op
/// when nothing is selected — formatting needs a range.
fn apply_format(index: usize, interaction: TextFieldState) {
    let edit = use_text_edit_state(TA_TAG);
    // R951 §5.36 — the toolbar works selected-or-not. With a selection the
    // format lands on the runs (the R768/R769 path); at a collapsed caret it
    // arms an active typing mark so the *next* typed text is styled (Word's
    // "press Bold, then type"). The selection range, if any, threads through to
    // the swatch overlays (which need a range); the toggle delegates the branch
    // to `format_at_caret_or_selection`.
    let selection = edit.selection_range();
    match index {
        FMT_BOLD | FMT_ITALIC => {
            // R967 §5.36 — the toggle direction (read the selection-start / caret
            // style, flip the one field) + the mergeCharFormat apply now live in
            // the substrate ([`TextEditState::toggle_format`]) — the SSOT the
            // AI-first `toggle-format` RPC verb shares, so the toolbar **B** / **I**
            // and the AI channel flip identically (divergence-is-a-bug). Unstyled
            // bytes still resolve against the theme field base.
            let theme = use_theme(THEME_TAG).theme_animated();
            let base = base_text_style(&theme, interaction);
            let field = if index == FMT_BOLD {
                FormatField::Bold
            } else {
                FormatField::Italic
            };
            edit.toggle_format(field, &base);
        }
        // Colour swatches: wholesale `setCharFormat` over the selection; the
        // clear swatch (`None`) strips it. At a collapsed caret the swatch arms
        // a pending colour mark (and clear drops the mark) — the colour peer of
        // the Bold / Italic caret path.
        _ => match FMT_SWATCHES.get(index - FMT_SWATCH_BASE) {
            Some(Some(rgb)) => match selection {
                Some((start, end)) => edit.apply_style_run(start, end, swatch_text_style(*rgb)),
                None => edit.set_pending_style(Some(swatch_text_style(*rgb))),
            },
            Some(None) => match selection {
                Some((start, end)) => edit.clear_style_runs(start, end),
                None => edit.set_pending_style(None),
            },
            None => {} // index out of range — ignore.
        },
    }
}

// R790 §5.22 — the `Owner::cache`-keyed clipboard hook (`AppClipboard`
// wrapper + Arboard-with-InMemory-fallback) lifted to
// `pinion_platform_clipboard::use_app_clipboard`, shared by every
// text-field binding (the lifted wrapper also forwards the
// selection-aware `copy_to` / `paste_from`).

/// R956 — decimal digit count of `n` (clamped to ≥ 1), sizing the gutter
/// wide enough for the highest line number it shows.
fn digit_count(n: usize) -> u32 {
    n.max(1).ilog10() + 1
}

/// R956 §5.36 §5.22 — the left line-number gutter, structurally a mirror
/// of the field's own multi-line `Scene::Scroll`: a fixed `gutter_w ×
/// field_h` box whose padded inner scroll viewport carries one
/// right-aligned number per **logical** line, absolute-positioned at that
/// line's layout-space `y` and scrolled by the *same*
/// [`tf_paint::field_scroll_offset`] the field applies — so the numbers
/// track the scrolled text rows pixel-for-pixel.
///
/// `lines` is the [`tf_paint::field_visual_lines`] readout (the painted
/// layout's per-visual-line metrics): soft-wrapped continuation rows have
/// [`VisualLineMetric::starts_logical_line`] `false` and so get no number,
/// the canonical "one number per logical line, on its first row" gutter.
///
/// The gutter mirrors the field's scroll offset *manually*
/// (`with_offset(scroll_y)`) rather than via the R859 linked-scroll
/// follower substrate (`from_state` + `as_follower`): the field's own
/// `Scene::Scroll` is a manual-offset, *untagged* node (built inside
/// `view_field`, not a `from_state` primary), and `from_state` would
/// derive the field's `ScrollState` tag (`main_textarea`) onto the gutter
/// node — colliding with the field container's tag. A `state: None` scroll
/// is already skipped by `update_scroll_state_bounds`, so this gutter is a
/// passive slider (it never publishes bounds) exactly like a follower,
/// without the tag tangle — the path consistent with how the field itself
/// scrolls.
fn gutter(
    theme: &pinion_core::theme::Theme,
    style: &tf_paint::TextFieldStyle,
    lines: &[VisualLineMetric],
    scroll_y: u32,
    caret_line: usize,
) -> Scene {
    let pad = style.field_pad;
    let logical_count = lines.iter().filter(|l| l.starts_logical_line).count();
    // ~0.6em per digit + one digit of right margin so the numbers never
    // touch the field edge.
    let digit_w = (style.font_size_px * 3) / 5;
    let inner_w = (digit_count(logical_count) + 1) * digit_w;
    let gutter_w = inner_w + 2 * pad;
    let inner_h = style.field_h.saturating_sub(2 * pad);
    // The scroll content must be as tall as the field's content (the last
    // line's bottom) so the gutter's `Scene::Scroll` has the same scroll
    // range as the field — an all-absolute child contributes no flow
    // height, so the explicit size is what lets it scroll in lock-step.
    let content_h = lines
        .last()
        .map_or(0, |m| saturating_f32_to_u32(m.y + m.height));

    // R957 — the caret's line number reads in the brighter `OnSurface`
    // ink; every other line stays muted. The active-line band (below)
    // carries the same hue family as the field's caret / selection.
    let muted_style = TextStyle::new()
        .with_size_px(style.font_size_px)
        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted))
        .with_align(TextAlign::End);
    let current_style = muted_style
        .clone()
        .with_fg(theme.resolve(ColorRole::OnSurface));
    let current_band_fill = theme
        .resolve(ColorRole::Accent)
        .with_alpha(GUTTER_CURRENT_ALPHA);

    let mut logical_n: u32 = 0;
    let mut numbers: Vec<Scene> = Vec::new();
    for m in lines {
        if !m.starts_logical_line {
            continue;
        }
        logical_n += 1;
        let top = saturating_f32_to_u32(m.y);
        let row_h = saturating_f32_to_u32(m.height).max(style.font_size_px);
        // R957 — the caret's logical line (0-based) highlights its number.
        let is_current = logical_n as usize - 1 == caret_line;
        if is_current {
            // The band paints BEFORE the number (so the glyph sits on top),
            // spanning the gutter inner width. R987 — it covers the caret's
            // whole LOGICAL line (every soft-wrapped row, via the shared
            // `logical_line_span`), aligned with the body's current-line band
            // rather than just the number's first row; the number itself stays
            // on the first row (`top` / `row_h`). Tagged so the AI side reads
            // which line is active from the rendered frame.
            let (band_top, band_h) = logical_line_span(lines, m.y)
                .map_or((top, row_h), |(t, h)| {
                    (saturating_f32_to_u32(t), saturating_f32_to_u32(h))
                });
            numbers.push(Scene::Box(
                BoxNode::new(Rect::default(), BoxStyle::filled(current_band_fill))
                    .with_tag(GUTTER_CURRENT_TAG.to_owned())
                    .with_layout(
                        LayoutStyle::new()
                            .with_size(Size::px(inner_w, band_h))
                            .with_absolute_position(0, band_top),
                    ),
            ));
        }
        let ink = if is_current {
            &current_style
        } else {
            &muted_style
        };
        numbers.push(Scene::Text(
            // R959 — tag each number under the *field's* composite namespace
            // (`main_textarea#gl<n>`), so a click routes through the
            // InputRouter to the field's `send` wire (`go_to_line`) instead of
            // the geometry press hook: focus-independent + no caret-drag arm.
            TextNode::styled(format!("{logical_n}"), Rect::default(), ink.clone())
                .with_tag(TextFieldSendKey::gutter_line_tag(
                    TA_TAG,
                    logical_n as usize,
                ))
                .with_layout(
                    LayoutStyle::new()
                        .with_size(Size::px(inner_w, row_h))
                        .with_absolute_position(0, top),
                ),
        ));
    }

    let content = Scene::Container(
        ContainerNode::new(numbers)
            .with_layout(LayoutStyle::new().with_size(Size::px(inner_w, content_h))),
    );
    #[allow(
        clippy::cast_possible_wrap,
        reason = "scroll_y <= content_h; UI content height never approaches i32::MAX"
    )]
    let offset_y = scroll_y as i32;
    let scroll = Scene::Scroll(
        ScrollNode::new(Rect::new(0, 0, inner_w, inner_h), content).with_offset(0, offset_y),
    );

    Scene::Container(
        ContainerNode::new(vec![scroll])
            .with_tag(GUTTER_TAG.to_owned())
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHigh),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Start)
                    .with_size(Size::px(gutter_w, style.field_h))
                    .with_padding(Rect::new(pad, pad, pad, pad)),
            ),
    )
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

    // R956 §5.36 §5.22 — the line-number gutter. Built AFTER `view_field`
    // so `field_scroll_offset` reads the offset the paint just stored, and
    // from `field_visual_lines` (the same shaped layout `view_field`
    // painted) so the numbers align row-for-row with the text. Placed left
    // of the field in a top-aligned Row.
    let lines = tf_paint::field_visual_lines(TA_TAG, interaction, caret_byte, &theme, &ta_style());
    let scroll_y = tf_paint::field_scroll_offset(TA_TAG, &ta_style());
    // R957 — the caret's logical line drives the gutter current-line band.
    let caret_line = use_text_edit_state(TA_TAG).caret_line();
    let editor = Scene::Container(
        ContainerNode::new(vec![
            gutter(&theme, &ta_style(), &lines, scroll_y, caret_line),
            field,
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Start),
        ),
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
    // R799 — reflective toggle state for the B / I cells: the toolbar *mirrors*
    // the document (it owns no format state). R967.1 — via the substrate
    // `reflective_style` SSOT (the SAME read `toggle_format` uses for its flip
    // direction, so pressed-state + flip cannot diverge), so the cells also light
    // for a collapsed-caret armed mark / inherited style, not only a selection.
    let (bold_active, italic_active) = text_state.reflective_style().map_or((false, false), |st| {
        (FormatField::Bold.is_on(&st), FormatField::Italic.is_on(&st))
    });
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
        ContainerNode::new(vec![
            title,
            editor,
            toolbar(&theme, bold_active, italic_active),
            status,
        ])
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
        // R798 §5.52 — journal edits onto a shared undo stack so Ctrl+Z /
        // Ctrl+Shift+Z (Ctrl+Y) undo / redo with word-level coalescing, the
        // textarea (multi-line + rich-text) being the 2nd `attach_undo`
        // consumer after hello-textfield. Attached on the *state* (the single
        // edit write path the view fn and the binding's apply_key share), and
        // *after* the boot seed above so the initial document is the undo
        // floor (you cannot undo past it). The R796.1 granular command
        // restores deleted `removed_runs`, so undoing a deleted coloured word
        // brings back both its text and its style run.
        text_state.attach_undo(use_undo_stack(TA_TAG));
        let blink = use_caret_blink(TA_TAG);
        let clipboard = use_app_clipboard(TA_TAG);
        Box::new(
            TextFieldExternal::new()
                .attach_state(text_state)
                .attach_blink(blink)
                .attach_clipboard(clipboard),
        )
    }

    /// R799 §5.16 §5.38 — register the formatting toolbar as a
    /// *non-focusable* routed [`ToolbarExternal`] sibling. Six one-shot
    /// commands (bold / italic / red / green / blue / clear); the view
    /// paints the controls with the matching `fmt_toolbar#<i>` composite
    /// tags so the `InputRouter` dispatches a click here, and [`Self::update`]
    /// maps the emitted `"command"` intent to a format op. The strip is
    /// not marked `.with_focusable(true)` (the textarea is the only
    /// scene-derived §5.39 Tab stop), so clicking a control never steals
    /// the field's focus.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(
            FMT_TAG,
            Box::new(ToolbarExternal::new(vec![ToolItem::Command; FMT_CONTROLS])),
        )]
    }

    fn tag() -> &'static str {
        TA_TAG
    }

    fn read_state(scene: &Scene) -> (TextFieldState, u32) {
        tf_paint::read_text_field_state(scene, TA_TAG)
    }

    /// R799 §5.36 §5.20 — apply a formatting-toolbar command to the live
    /// selection. The routed [`ToolbarExternal`] emits `"command"` with the
    /// control index as its payload (the runtime prefixes it to
    /// `fmt_toolbar.command`); [`apply_format`] maps it to a merge / overlay
    /// / clear. Fire-and-forget: the format mutates `TextEditState`
    /// directly, so no [`Command`] flows back to the externals.
    fn update(state: (TextFieldState, u32), intent: &Intent) -> Vec<Command> {
        if intent.tag.as_ref() == FMT_CMD_INTENT {
            if let IntrospectValue::Text(idx) = &intent.payload {
                if let Ok(index) = idx.parse::<usize>() {
                    apply_format(index, state.0);
                }
            }
        }
        Vec::new()
    }

    fn view(state: (TextFieldState, u32), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(event: TextFieldEvent) -> &'static str {
        pinion_core::WidgetEventName::as_name(&event)
    }

    fn title() -> &'static str {
        "pinion hello-textarea (R799 §5.36 §5.22)"
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
                // R1268 §5.22 — route through the newline SSOT. This prose
                // textarea does not opt into `set_auto_indent`, so it inserts a
                // plain `\n` (byte-identical to the pre-R1268 `insert("\n")`);
                // a code editor opts in and the same entry point copies indent.
                use_text_edit_state(TA_TAG).insert_newline();
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
                    if end { edit.text().len() } else { 0 }
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
        pinion_core::forward_key_to_field(scene, TA_TAG, key, modifiers)
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
    /// `AccessValue::Text`. (The multi-line `aria-multiline` refinement
    /// is an additive a11y axis — deferred until a 2nd textarea consumer
    /// per [[abstraction-needs-second-consumer]].)
    fn access_node(state: &(TextFieldState, u32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, _caret) = state;
        let edit = use_text_edit_state(TA_TAG);
        let text = edit.text();
        let tag = <Self as WidgetCore>::tag();
        // R790 §5.40 — the lifted textbox node SSOT. The deferred
        // `aria-multiline` refinement above becomes an additive param to
        // this helper once a 2nd textarea consumer surfaces.
        let mut nodes = vec![tf_paint::text_field_a11y_node(
            tag,
            text,
            *interaction,
            focused == Some(tag),
        )];

        // R800 §5.40 — the formatting toolbar's a11y, via the lifted SSOT
        // ([`pinion_a11y::toolbar_button_nodes`], shared with hello-toolbar).
        // B / I are *reflective* `aria-pressed` toggles read from the
        // selection's style (not an owned bit, matching the painted fill);
        // the colour swatches are command buttons named explicitly (no glyph
        // to enrich from). The strip is non-focusable, so it rings nothing
        // (`focused_control = None`).
        let (bold_active, italic_active) = edit.reflective_style().map_or((false, false), |st| {
            (FormatField::Bold.is_on(&st), FormatField::Italic.is_on(&st))
        });
        let tags: Vec<String> = (0..FMT_CONTROLS)
            .map(|i| composite_item_tag(FMT_TAG, i))
            .collect();
        let controls: Vec<ToolbarControl<'_>> = (0..FMT_CONTROLS)
            .map(|i| ToolbarControl {
                tag: &tags[i],
                name: Some(FMT_CONTROL_NAMES[i]),
                checked: match i {
                    FMT_BOLD => Some(bold_active),
                    FMT_ITALIC => Some(italic_active),
                    _ => None, // colour swatch / clear = one-shot command
                },
                disabled: false, // formatting controls are always operable (R989)
            })
            .collect();
        nodes.extend(toolbar_button_nodes(
            FMT_TAG,
            "Formatting toolbar",
            &controls,
            None,
        ));
        nodes
    }
}

impl WidgetView for TextAreaView {
    type Renderer = HelloTextAreaRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
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
        // ★ R1684.1 — the scene walk is the lifted helper's now.
        let theme = use_theme(THEME_TAG).theme_animated();
        tf_paint::ime_caret_rect_in_scene(
            TA_TAG,
            interaction,
            caret_byte,
            scene,
            &theme,
            &ta_style(),
        )
    }

    /// R762 / R763 reused verbatim against the multi-line layout: the
    /// hit-test (`byte_for_field_point`) picks the line by `y`, so a
    /// click / drag / Shift-click on any line resolves correctly.
    fn position_caret_for_point(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
        hit_tag: Option<&str>,
        x: f32,
        y: f32,
        extend: bool,
    ) -> Option<usize> {
        if focused != Some(TA_TAG) {
            return None;
        }
        // R801 — the shell fires this hook for *every* press (so a binding
        // can handle non-caret presses), so a press that landed on the
        // routed formatting toolbar (a sibling External below the field)
        // reaches here too while the field keeps focus. Reject any press
        // the router routed elsewhere by tag identity: only a press whose
        // resolved hit-target is the field itself becomes a caret move —
        // otherwise a toolbar press would clear the selection its command
        // needs. The router already hit-tested the press; the field no
        // longer re-scans its own rect (the pre-R801 workaround).
        // R959 — a gutter line-number click is NOT handled in this geometry
        // hook. The number is tagged under the field's own composite namespace
        // (`main_textarea#gl<n>`, `TextFieldSendKey::gutter_line_tag`), so the InputRouter
        // routes the click to the field's `send` wire (→ `go_to_line`) and
        // click-to-focus resolves the composite to this field (focusing the
        // editor). That `main_textarea#gl<n>` hit-tag is a composite, never the
        // bare `TA_TAG`, so it falls through the gate below to `None`: the
        // press hook arms no caret text-drag for it. (The R957 first cut
        // decoded the gutter here, which armed a char text-drag on the
        // smallest pointer jitter and no-op'd unless the field already held
        // focus — the B1 / B2 defects this round retired.)
        if hit_tag != Some(TA_TAG) {
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
    use super::{FMT_CMD_INTENT, FMT_CONTROLS, FMT_TAG, TA_TAG, TextAreaView, view};
    use pinion_a11y::{AriaRole, WidgetA11y};
    use pinion_core::external::IntrospectValue;
    use pinion_core::intent::Intent;
    use pinion_core::reactive::Owner;
    use pinion_core::scene::ExternalNode;
    use pinion_core::style::{FontStyle, FontWeight};
    use pinion_core::widgets::text_edit::use_text_edit_state;
    use pinion_core::widgets::text_field::TextFieldState;
    use pinion_core::{Frame, Scene, WidgetCore};
    use pinion_shell::WidgetView;
    use pinion_widget_paint::toolbar::composite_item_tag;

    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    #[test]
    fn r764_view_carries_textarea_tag() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            assert!(
                scene.contains_tag(TA_TAG),
                "paint scene carries the textarea tag"
            );
        });
    }

    #[test]
    fn r764_enter_inserts_newline() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            edit.set_text("ab".to_owned());
            edit.set_caret(1);
            let mut scene = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
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
            let mut scene = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
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
            assert_eq!(
                edit.caret(),
                5,
                "ArrowDown lands on the next line at the same column"
            );
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
            let mut scene = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
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
            let mut scene = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
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
            assert!(
                (9..=11).contains(&m1),
                "first ArrowDown lands on the short line 1 (got {m1})"
            );
            assert!(down(&mut scene));
            let m2 = edit.caret();
            assert!(
                m2 >= 12,
                "second ArrowDown lands on the long line 2 (got {m2})"
            );
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
            let mut scene = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
            let _ = view((TextFieldState::Focused, 6), &Frame::default());
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "Home",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(
                edit.caret(),
                4,
                "Home moves to the start of the current visual line"
            );
            assert!(
                edit.goal_column().is_none(),
                "Home (a horizontal move) clears the goal column"
            );
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "End",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(
                edit.caret(),
                7,
                "End moves to the end of the current visual line"
            );
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
            let mut scene = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
            let _ = view((TextFieldState::Focused, 5), &Frame::default());
            let ctrl = pinion_core::Modifiers {
                ctrl: true,
                ..pinion_core::Modifiers::empty()
            };
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "Home",
                ctrl
            ));
            assert_eq!(edit.caret(), 0, "Ctrl+Home moves to the document start");
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "End",
                ctrl
            ));
            assert_eq!(edit.caret(), 7, "Ctrl+End moves to the document end");
            // Ctrl+Shift+Home selects from the caret back to byte 0.
            let ctrl_shift = pinion_core::Modifiers {
                ctrl: true,
                shift: true,
                ..pinion_core::Modifiers::empty()
            };
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "Home",
                ctrl_shift
            ));
            assert_eq!(
                edit.selection_range(),
                Some((0, 7)),
                "Ctrl+Shift+Home selects from the document end back to the start",
            );
        });
    }

    /// R801 §5.36 §5.35 — the caret press hook moves the caret only for a
    /// press the `InputRouter` routed to the field itself. A press the
    /// router routed to a *sibling* (a foreign `hit_tag` — the formatting
    /// toolbar keeps the field focused, so `focused` alone cannot tell it
    /// apart), or one arriving while the field is unfocused, is rejected
    /// before any caret write — so the live selection a toolbar command
    /// needs survives the click. This pins the framework seam the round
    /// replaced (the old per-binding rect re-scan).
    #[test]
    fn r801_press_routed_to_sibling_keeps_caret_and_selection() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            edit.set_text("first second".to_owned());
            edit.set_selection(0, 5); // the range a toolbar command would act on
            let scene = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
            let toolbar_tag = composite_item_tag(FMT_TAG, 0);
            // Press routed to the toolbar sibling while the field keeps focus.
            assert_eq!(
                TextAreaView::position_caret_for_point(
                    &(TextFieldState::Focused, 0),
                    &scene,
                    Some(TA_TAG),
                    Some(toolbar_tag.as_str()),
                    10.0,
                    10.0,
                    false,
                ),
                None,
                "a press the router routed to the toolbar is not a caret move",
            );
            assert_eq!(
                (edit.selection_anchor(), edit.caret()),
                (Some(0), 5),
                "the selection survives a toolbar-routed press",
            );
            // A press arriving while the field is unfocused is likewise rejected.
            assert_eq!(
                TextAreaView::position_caret_for_point(
                    &(TextFieldState::Focused, 0),
                    &scene,
                    None,
                    Some(TA_TAG),
                    10.0,
                    10.0,
                    false,
                ),
                None,
                "an unfocused press is not a caret move",
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
            let mut scene = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
            let _ = view((TextFieldState::Focused, 6), &Frame::default());
            let mods = pinion_core::Modifiers {
                shift: true,
                ..pinion_core::Modifiers::empty()
            };
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "Home",
                mods
            ));
            assert_eq!(
                edit.caret(),
                4,
                "Shift+Home moves the caret to the line start"
            );
            assert_eq!(
                edit.selection_range(),
                Some((4, 6)),
                "Shift+Home selects from the line start to the original caret",
            );
        });
    }

    /// R798 §5.52 — the textarea journals edits onto the undo stack
    /// (attached after the boot seed), so `Ctrl+Z` reverts an edit back to
    /// the seeded document floor and `Ctrl+Shift+Z` redoes it.
    #[test]
    fn r798_ctrl_z_reverts_to_seed_floor_then_redoes() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            let mut scene = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
            let seed = edit.text();
            edit.set_caret(seed.len());
            edit.insert("Z"); // a content edit recorded on the stack
            assert_eq!(edit.text(), format!("{seed}Z"), "insert appended a char");
            let ctrl = pinion_core::Modifiers {
                ctrl: true,
                ..pinion_core::Modifiers::empty()
            };
            assert!(TextAreaView::apply_key(&mut scene, Some(TA_TAG), "z", ctrl));
            assert_eq!(
                edit.text(),
                seed,
                "Ctrl+Z reverts the insert to the seed floor"
            );
            // Cannot undo past the floor: the seed was below the attach point.
            assert!(TextAreaView::apply_key(&mut scene, Some(TA_TAG), "z", ctrl));
            assert_eq!(
                edit.text(),
                seed,
                "no further undo past the seeded document"
            );
            let ctrl_shift = pinion_core::Modifiers {
                ctrl: true,
                shift: true,
                ..pinion_core::Modifiers::empty()
            };
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "z",
                ctrl_shift
            ));
            assert_eq!(
                edit.text(),
                format!("{seed}Z"),
                "Ctrl+Shift+Z redoes the insert"
            );
        });
    }

    /// R798 §5.52 / R796.1 — the granular command restores `removed_runs`:
    /// deleting the seeded coloured leading word and undoing brings back both
    /// its text *and* its style run (the multi-line rich-text 2nd consumer
    /// that exercises run reversal end-to-end through the binding).
    #[test]
    fn r798_undo_restores_a_deleted_style_run() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            let mut scene = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
            // The seed styles "first" (bytes 0..5) red.
            assert!(
                edit.style_at(0).is_some(),
                "the leading word seeds a style run"
            );
            edit.set_selection(0, 5);
            edit.backspace(); // selection-delete drops "first" and clips its run
            assert!(!edit.text().starts_with("first"), "the word is gone");
            assert!(edit.style_at(0).is_none(), "its style run went with it");
            let ctrl = pinion_core::Modifiers {
                ctrl: true,
                ..pinion_core::Modifiers::empty()
            };
            assert!(TextAreaView::apply_key(&mut scene, Some(TA_TAG), "z", ctrl));
            assert!(
                edit.text().starts_with("first"),
                "undo restored the deleted word"
            );
            assert!(
                edit.style_at(0).is_some(),
                "undo restored its style run (R796.1 removed_runs reversal)",
            );
        });
    }

    /// R799 §5.36 §5.20 — the reducer maps a routed toolbar `command`
    /// intent (control index in the payload) to a format op on the live
    /// selection. Bolding unstyled bytes materialises a bold run; the
    /// reducer returns no [`Command`] (fire-and-forget).
    #[test]
    fn r799_toolbar_command_bolds_the_selection() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            let _ = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
            edit.set_selection(5, 10); // " line" after the seed's red "first"
            assert!(edit.style_at(7).is_none(), "byte 7 starts unstyled");
            let intent = Intent::new_static(FMT_CMD_INTENT, IntrospectValue::Text("0".to_owned()));
            let cmds = TextAreaView::update((TextFieldState::Focused, 5), &intent);
            assert!(
                cmds.is_empty(),
                "format is fire-and-forget — no Command back to externals"
            );
            let st = edit
                .style_at(7)
                .expect("bolding materialised a run over the gap");
            assert_eq!(
                st.font_weight,
                FontWeight::BOLD,
                "the command bolded the selection"
            );
            assert_eq!(
                st.font_style,
                FontStyle::Normal,
                "italic untouched (merge is field-level)"
            );
        });
    }

    /// R799 — a toolbar `command` with no active selection is a no-op
    /// (formatting needs a range), and an unrelated intent is ignored.
    #[test]
    fn r799_toolbar_command_without_selection_is_noop() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            let _ = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
            edit.set_caret(5); // collapsed — no selection range
            assert!(edit.selection_range().is_none(), "no active selection");
            let runs_before = edit.style_runs().len();
            let intent = Intent::new_static(FMT_CMD_INTENT, IntrospectValue::Text("0".to_owned()));
            let _ = TextAreaView::update((TextFieldState::Focused, 5), &intent);
            assert_eq!(
                edit.style_runs().len(),
                runs_before,
                "a no-selection format command changes nothing",
            );
            // An intent the toolbar did not emit is ignored.
            let other = Intent::new_static("something.else", IntrospectValue::Text("0".to_owned()));
            let cmds = TextAreaView::update((TextFieldState::Focused, 5), &other);
            assert!(cmds.is_empty(), "an unrelated intent is ignored");
        });
    }

    /// R800 — the `intent_tag!` macro needs string literals, so
    /// `FMT_CMD_INTENT` duplicates the `"fmt_toolbar"` literal. Pin it to
    /// `FMT_TAG` so renaming one without the other fails at test time
    /// ([[wire-vocab-canon-pin-not-fold]]).
    #[test]
    fn r800_intent_tag_pins_to_the_bar_tag() {
        assert!(
            FMT_CMD_INTENT.starts_with(FMT_TAG),
            "the intent is scoped to the bar tag"
        );
        assert_eq!(
            FMT_CMD_INTENT,
            format!("{FMT_TAG}.command"),
            "intent = <bar>.command"
        );
    }

    /// R800 §5.40 — `access_node` exposes the formatting toolbar (lifted
    /// SSOT): a `toolbar` node + one `button` per control, with B / I
    /// carrying *reflective* `aria-pressed` (read from the selection) and
    /// the colour swatches carrying an explicit name + no `aria-pressed`.
    #[test]
    fn r800_access_node_exposes_reflective_toolbar() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            let _ = Scene::External(
                ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG),
            );
            // No selection → B / I report aria-pressed = false.
            edit.set_caret(0);
            let nodes = TextAreaView::access_node(&(TextFieldState::Focused, 0), Some(TA_TAG));
            assert_eq!(
                nodes.len(),
                2 + FMT_CONTROLS,
                "textbox + toolbar + N control buttons"
            );
            let toolbar = nodes
                .iter()
                .find(|n| n.role == AriaRole::Toolbar)
                .expect("a toolbar node");
            assert_eq!(
                toolbar.children.len(),
                FMT_CONTROLS,
                "toolbar references every control"
            );
            // nodes = [textbox, toolbar, bold, italic, red, green, blue, clear]
            assert_eq!(nodes[2].role, AriaRole::Button);
            assert_eq!(nodes[2].name.as_deref(), Some("Bold"));
            assert_eq!(
                nodes[2].state.checked,
                Some(false),
                "no selection -> Bold not pressed"
            );
            assert_eq!(
                nodes[4].name.as_deref(),
                Some("Red"),
                "a glyph-less swatch is named"
            );
            assert_eq!(
                nodes[4].state.checked, None,
                "a colour swatch carries no aria-pressed"
            );

            // Bold the selection → the reflective B node flips to pressed.
            edit.set_selection(0, 5);
            let intent = Intent::new_static(FMT_CMD_INTENT, IntrospectValue::Text("0".to_owned()));
            let _ = TextAreaView::update((TextFieldState::Focused, 0), &intent);
            let after = TextAreaView::access_node(&(TextFieldState::Focused, 5), Some(TA_TAG));
            assert_eq!(
                after[2].state.checked,
                Some(true),
                "selection now bold -> Bold aria-pressed = true (reflective)",
            );
        });
    }
}
