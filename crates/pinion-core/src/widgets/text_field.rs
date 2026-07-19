//! R56.1 §5.38 — `TextField` widget catalogue entry.
//!
//! First slice of the R56 axis: §5.38 SCXML statechart + Rust binding
//! ([`TextField`] / [`TextFieldEvent`] / [`TextFieldState`] /
//! [`TextFieldExternal`]). Subsequent sub-rounds layer on top:
//! caret rendering (R56.1.b), blink animation (R56.1.c), key input
//! (R56.1.d — see [`apply_key`]), clipboard (R56.1.e), selection
//! (R56.1.f), and IME composition (R56.1.g). The R56.1.h
//! focus-lifecycle wire — shell focus mgr ↔ `External::on_focus_change`
//! ↔ statechart focus/blur drive ↔ [`CaretBlink`]
//! `set_enabled` — splits off so the keystroke surface lands without
//! the cross-cutting shell substrate change. R55.D.1 → R55.D.2
//! cascade mirror — land the interaction substrate first so the
//! following sub-rounds compose without re-engineering it.
//!
//! ## State model
//!
//! Four-state interaction machine (file `widgets/text_field.scxml`):
//! Idle / Focused / Editing / Disabled.
//!
//! | State | Meaning |
//! |---|---|
//! | `Idle` | not focused; caret hidden; no key dispatch. |
//! | `Focused` | focused; caret visible (R56.1.b); key dispatch active (R56.1.d). |
//! | `Editing` | focused + IME composition active; preedit text visible (R56.1.g). |
//! | `Disabled` | absorbing class until [`TextFieldEvent::Enable`] arrives. |
//!
//! Unlike Slider's typed f32 sidecar (R51.39 §5.38), `TextField`
//! carries **no per-state value sidecar on this first slice** — the
//! text content (`String` + caret position) and the IME preedit
//! buffer arrive with R56.1.b / R56.1.g respectively, mirroring the
//! R55.D.2 `ScrollBar` staging (statechart first, content sidecar in
//! a later sub-round). The introspect schema therefore declares only
//! `state` (read) and `send` (invoke) on R56.1.a.
//!
//! ## Intent contract
//!
//! Emits the §5.20 `"text_committed"` intent on the two SCXML
//! commit-raise transitions:
//!
//! - `Editing → Focused` via [`TextFieldEvent::CommitEdit`] — IME
//!   commit (e.g. Enter during composition; Wayland
//!   `text-input-v3` `commit_string`).
//! - `Editing → Idle` via [`TextFieldEvent::Blur`] — focus loss
//!   during composition. IME canonical commit-on-blur (matches macOS
//!   `NSTextInputContext` / GTK `IBus` / Wayland `text-input-v3` /
//!   Windows TSF). Applications that want the cancel-on-blur variant
//!   intercept `Blur` before forwarding.
//!
//! The cancel paths stay silent:
//!
//! - `Editing → Focused` via [`TextFieldEvent::CancelEdit`] — IME
//!   cancel preedit (e.g. Escape during composition; preedit
//!   discarded without committing).
//! - `Editing → Disabled` via [`TextFieldEvent::Disable`] — widget
//!   disabled during composition; preedit dropped on the floor
//!   (matches Wayland IME disable contract).
//!
//! Intent payload is [`IntrospectValue::Null`] on R56.1.a — the
//! committed-text payload (`IntrospectValue::Text`) arrives with
//! R56.1.g once the IME preedit buffer substrate lands.

#[allow(
    non_snake_case,
    unused_imports,
    dead_code,
    unused_variables,
    unused_mut,
    unused_labels,
    unreachable_patterns,
    unreachable_code,
    unused_assignments,
    clippy::style,
    clippy::complexity,
    clippy::pedantic,
    clippy::all
)]
mod sm {
    include!(concat!(env!("OUT_DIR"), "/text_field_sm.rs"));
}

pub use sm::{TextFieldEvent, TextFieldState};

// SCE-002 §5.16 — the `WidgetStateName` / `WidgetEventName` impls for the
// sce-generated `TextFieldState` / `TextFieldEvent` enums are injected as
// `#[derive]`s by `build.rs` (`compile_scxml_with_derives`), reconstructed
// from the codegen's `#[default]` state + `EXTERNALLY_DRIVABLE_EVENTS`
// const (see `pinion-derive`); the per-widget `widget_{state,event}_name!`
// macros are retired. The external set is focus/edit-lifecycle (not
// pointer); `TextfieldCommit` (internal raise) + `Null` are excluded from
// `EXTERNALLY_DRIVABLE_EVENTS`, so `from_name` rejects them. The
// pinion-widget-paint read path uses `from_name_or_default`.
use sm::TextFieldPolicy;

use std::rc::Rc;

use crate::clipboard::{Clipboard, ClipboardSelection};
use crate::composite_tag::split_send_payload;
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use crate::input::is_activation_event;
use crate::intent::Intent;
use crate::scene::Rect;
use crate::style::{
    Color, FontStyle, FontWeight, LineHeight, TextAlign, TextDecoration, TextOverflow, TextStyle,
};
use crate::widget_core::ExtraExternal;
use crate::widgets::caret_blink::{CaretBlink, use_caret_blink};
use crate::widgets::text_edit::{FormatField, TextEditState, use_text_edit_state};
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};
use crate::{WidgetEventName, WidgetStateName};

/// R56.1.b §5.38 §5.21 — closed-form caret rectangle derivation.
///
/// Pure function of pre-shaped inputs — no font shaping, no
/// [`TextEditState`] borrow. The caller computes `caret_x_offset`
/// (the horizontal distance from the line origin to the caret
/// position in painted-pixel units, derived from a §5.36 shaped
/// glyph run) and supplies the line geometry; this helper assembles
/// the [`Rect`] in the paint coordinate frame.
///
/// Mirrors the R55.D.1
/// [`scrollbar_thumb_rect`](crate::widgets::scrollbar::scrollbar_thumb_rect)
/// closed-form split — caret geometry lives separately from the
/// SCXML statechart (R56.1.a) and the reactive [`TextEditState`]
/// (R56.1.b above), so a paint backend can call into it without
/// touching either. Shaped-run integration (parley + caret-x lookup
/// against the text shaping cache) is a R56.1.b follow-up
/// (`carret_x_for_position(&shaped_run, caret_pos) -> u32`); landing
/// the geometry primitive standalone keeps that integration a pure
/// caller-side translation.
///
/// `line_origin` is the **top-left** of the text line in the paint
/// frame (so `line_origin.1` is the y of the line top, not the
/// baseline — matches the [`Rect`] convention used by every other
/// paint helper in pinion). `line_height` is the full line box
/// extent; the caret rect spans the entire line so that the user
/// sees a full-height blink, the canonical text-input caret shape
/// shared by every platform.
///
/// `caret_width` is typically `1` or `2` paint pixels (1 px on Hi-DPI
/// displays where AA softens single-pixel lines, 2 px on
/// integer-scaled Lo-DPI displays). The blink animation (R56.1.c)
/// toggles paint opacity, not extent, so the width stays constant
/// across the visible / hidden frames.
#[must_use]
pub fn caret_rect(
    line_origin: (u32, u32),
    caret_x_offset: u32,
    caret_width: u32,
    line_height: u32,
) -> Rect {
    Rect::new(
        line_origin.0.saturating_add(caret_x_offset),
        line_origin.1,
        caret_width,
        line_height,
    )
}

/// R56.1.d §5.38 §5.22 — W3C UI Events key dispatch into a
/// [`TextEditState`]. Maps the canonical key-string surface
/// (the W3C `KeyboardEvent.key` values that every other pinion
/// widget consumes through
/// [`WidgetCore::apply_key`](crate::widget_core::WidgetCore::apply_key))
/// to caret-relative edit operations on the attached state.
///
/// R56.1.f.0 §5.13 — `modifiers` carries the W3C `KeyboardEvent`
/// four-bit modifier surface. On R56.1.d every recognized key
/// branch ignores the parameter (the canonical no-modifier
/// `Backspace` / `ArrowLeft` / printable-char dispatch); R56.1.f
/// layers selection-extension semantics on top by branching on
/// [`Modifiers::shift_key`](crate::input::Modifiers::shift_key) inside the arrow / Home / End arms.
///
/// Pure function: no statechart drive, no [`Owner`](crate::reactive::Owner)
/// access, no IME composition path. Focus/blur statechart drive +
/// caret-blink lifecycle land with the R56.1.h follow-up; IME
/// preedit handling lands with R56.1.g. Splitting the keystroke
/// surface from the lifecycle wire mirrors the R55.D.1 →
/// R55.D.3 `ScrollBar` cascade — the closed-form keystroke helper
/// lands first so the next sub-round composes the lifecycle wire
/// on top of a stable mapping.
///
/// ## Recognized keys
///
/// | Key string | Effect |
/// |---|---|
/// | `"Backspace"`  | [`TextEditState::backspace`] — delete char left of caret. |
/// | `"Delete"`     | [`TextEditState::delete_forward`] — delete char at caret. |
/// | `"ArrowLeft"`  | [`TextEditState::move_left`] — caret one char left. |
/// | `"ArrowRight"` | [`TextEditState::move_right`] — caret one char right. |
/// | `"Home"`       | [`TextEditState::move_home`] — caret to position 0. |
/// | `"End"`        | [`TextEditState::move_end`] — caret to end of text. |
/// | `"Space"`      | [`TextEditState::insert`]`(" ")` — single space. |
/// | single non-control char (`"a"`, `"A"`, `"ㄱ"`, `"!"`) | [`TextEditState::insert`]`(key)` — verbatim insert. |
///
/// Returns `true` if the key was recognized (the caret-at-edge
/// no-ops — `"Backspace"` at caret 0, `"Delete"` at caret end,
/// `"ArrowLeft"` at caret 0, `"ArrowRight"` at end — still
/// return `true` because the key was *consumed*; the W3C
/// `KeyboardEvent` `defaultPrevented` semantics gate on
/// recognition, not on visible mutation, and the application
/// `apply_key` contract follows the same shape).
///
/// ## Unrecognized keys (returns `false`)
///
/// - Named keys not in the explicit recognized set (`"ArrowUp"`,
///   `"ArrowDown"`, `"PageUp"`, `"PageDown"`, `"F1"`..`"F12"`,
///   `"Escape"`).
///
///   R1364 — this list used to include `"Enter"` and `"Tab"`, and to explain
///   that they, with `"Escape"`, "are shell-reserved upstream … and never reach
///   this hook in practice". Every part of that has since become false:
///
///   * `"Tab"` has had its own arm since the code-editor indent work — it
///     returns `false` unless the field opted in via `set_tab_indents`, and
///     otherwise indents / dedents the selection.
///   * `"Enter"` has had its own arm since R1268 (the auto-indented newline).
///   * `"Escape"` genuinely still lands here and is genuinely rejected — but by
///     the catch-all's single-codepoint test, not by an upstream reservation.
///     RPC `scene/key` carries no allowlist, so an injected `"Escape"` reaches
///     this hook however "reserved" the winit path calls it. The defensive
///     reject is therefore the LIVE contract, not a belt-and-braces margin.
/// - Empty string.
/// - Single-codepoint control chars (e.g. raw `"\t"` / `"\n"` —
///   the framework converts these to named keys at the input
///   boundary, so reaching `apply_key` with a raw control byte
///   is a bug fixture path; rejection here is the defensive
///   stance).
///
/// ## R56.1.h carry — vertical caret navigation
///
/// `"ArrowUp"` and `"ArrowDown"` are W3C-recognized text-input
/// keys but require a shaped multi-line layout (`caret_x` ↔
/// line-y mapping) to translate "move to same x on adjacent
/// line". R56.1.b ships only a single-line [`caret_rect`]
/// helper; R56.1.h is the natural slice to add multi-line
/// shaping + vertical navigation. Returning `false` here on
/// `"ArrowUp"` / `"ArrowDown"` lets the application's
/// [`WidgetCore::apply_key`](crate::widget_core::WidgetCore::apply_key)
/// fall through to the focus manager's Tab traversal — matches
/// the W3C ARIA `textbox` single-line convention that vertical
/// arrows do not consume.
///
/// ## R56.1.g carry — IME composition
///
/// `KeyboardEvent.key` may carry multi-char strings during IME
/// composition (e.g. the Korean syllable `"안"` after three jamo
/// combine). On R56.1.d the multi-char path returns `false` —
/// IME preedit input flows through the R56.1.g preedit-buffer
/// substrate, not this synchronous keystroke hook. Single-char
/// CJK ideographs (already-composed) insert correctly via the
/// printable-char branch.
#[must_use]
pub fn apply_key(state: &TextEditState, key: &str, modifiers: crate::input::Modifiers) -> bool {
    match key {
        "Backspace" => {
            // R56.1.f.1 — `backspace` is already selection-aware:
            // an active selection is drained wholesale; collapsed
            // (no-selection) caret falls back to the original
            // R56.1.d single-char prev-boundary delete.
            state.backspace();
            true
        }
        "Delete" => {
            state.delete_forward();
            true
        }
        // R56.1.f.2 §5.22 — caret-motion arrows branch on Shift:
        //  - plain `ArrowLeft` collapses any active selection to the
        //    leading edge (or moves caret one char left when there
        //    is no selection).
        //  - Shift+ArrowLeft extends the selection one char left
        //    (latches an anchor at the current caret if none exists).
        // Mirrors macOS / iOS / GTK / Web canonical text-input.
        "ArrowLeft" => {
            if modifiers.shift_key() {
                state.select_left();
            } else {
                state.move_left();
            }
            true
        }
        "ArrowRight" => {
            if modifiers.shift_key() {
                state.select_right();
            } else {
                state.move_right();
            }
            true
        }
        "Home" => {
            if modifiers.shift_key() {
                state.select_home();
            } else {
                state.move_home();
            }
            true
        }
        "End" => {
            if modifiers.shift_key() {
                state.select_end();
            } else {
                state.move_end();
            }
            true
        }
        // U+0020 SPACE arrives through the W3C named-key channel
        // (`NamedKey::Space → "Space"` on winit, `KeyCode::Char(' ')
        // → "Space"` on crossterm — see R51.111 pinion-tui/input.rs
        // bridge). Explicit handler avoids depending on the
        // printable-char branch interpreting `" "` (which would
        // require shell change to bypass the named-key conversion).
        "Space" => {
            // R56.1.f.1 — `insert` is selection-aware: an active
            // selection is replaced wholesale before the literal
            // space lands.
            state.insert(" ");
            true
        }
        // R938 §5.22 — Tab / Shift+Tab indent / dedent for a multi-line code
        // editor that opted in via `set_tab_indents`. Lives here in the keymap
        // SSOT (a TextEditState-only key, like the arrows / Backspace / Ctrl+Z
        // above), NOT in `dispatch_key` (which pre-empts only keys needing the
        // External's clipboard). A field that did not opt in returns `false`,
        // so the shell's focus-traversal default still advances focus (the
        // `app.rs` Tab arm). The key is "handled" whenever the editor opted in
        // — even when a dedent finds no leading whitespace (a no-op) — the W3C
        // `defaultPrevented` discipline every arm here follows; the caller's
        // handled-key bookkeeping (caret-blink reset, PRIMARY publish) then
        // runs once via `dispatch_key`'s post-`apply_key` path.
        "Tab" => {
            if !state.tab_indents() {
                return false;
            }
            if modifiers.shift_key() {
                state.dedent_selection(crate::widgets::text_edit::INDENT_WIDTH);
            } else {
                state.indent_selection(crate::widgets::text_edit::INDENT_UNIT);
            }
            true
        }
        // R1268 §5.22 — Enter inserts an auto-indented newline for a multi-line
        // code editor that opted in via `set_auto_indent`. The keymap-SSOT peer
        // of the Tab arm above (a TextEditState-only key, not a `dispatch_key`
        // clipboard pre-empt): a field that did NOT opt in returns `false`, so
        // Enter keeps the field's own policy — a single-line field submits, and
        // a prose multi-line field drives its own plain-newline handler — every
        // pre-R1268 field is byte-unchanged. The modifier state is intentionally
        // ignored: Shift+Enter is the same indent-aware newline (a hard newline;
        // a soft-break variant is a later slice, not a missed case here).
        //
        // R1270 F5 (audit — honest boundary): `auto_indent` deliberately
        // doubles here as "capture Enter in the shared keymap", because the
        // ONLY shared-keymap Enter consumer today IS an auto-indenting code
        // editor. Enter's full policy is 3-way — single-line submit / prose
        // plain-newline / code auto-indent-newline — but modelling that as a
        // first-class `EnterAction` is 2nd-consumer-gated: a prose multi-line
        // field that wants *keymap-driven* plain newline (rather than its own
        // handler, as hello-textarea does) is the missing consumer that would
        // force the split. Until then the `tab_indents`-style single opt-in is
        // the honest shape, not a conflation to abstract away speculatively.
        "Enter" => {
            if !state.auto_indent() {
                return false;
            }
            state.insert_newline();
            true
        }
        other => {
            // R56.1.f.2 §5.22 — Ctrl+A / Cmd+A select-all. The W3C
            // `KeyboardEvent.key` value for the lowercase letter
            // arrives verbatim from every platform (winit converts
            // `Key::Character` to the printable string; crossterm
            // emits `KeyCode::Char('a')` which the R51.111 bridge
            // maps to `"a"`). Both Ctrl (Linux/Win) and Meta (macOS)
            // count so the same binding fires on every desktop.
            if modifiers.command_key() && !modifiers.alt_key() && other == "a" {
                let len = state.text().len();
                state.set_selection(0, len);
                return true;
            }
            // R796 §5.52 — undo / redo chords. Ctrl/Cmd+Z undoes,
            // Ctrl/Cmd+Shift+Z or Ctrl/Cmd+Y redoes — the cross-platform
            // text-editor convention. Engaged only when a binding attached an
            // undo stack (`attach_undo`); otherwise the chord falls through
            // to the generic Ctrl-reject below so an application shortcut
            // handler can claim it. Consumed (returns `true`) even at a stack
            // boundary, so an attached field's Ctrl+Z never bubbles to a
            // global undo.
            // R932.1 — the chord → verb decision via the shared
            // [`undo_redo_verb`](crate::undo::undo_redo_verb) SSOT (lifted to
            // pinion-core), so the text field shares one editor keybinding with
            // the node-graph / data-grid / tree-grid keymaps. The `!alt_key`
            // gate stays local (AltGr + key is character composition in a text
            // field, not a chord). This also fixes a latent bug the hand-rolled
            // `other == "z"` match had: the platform delivers uppercase `"Z"`
            // when Shift is held, so `Ctrl+Shift+Z` redo silently did nothing;
            // `undo_redo_verb` case-folds the key.
            if modifiers.command_key() && !modifiers.alt_key() && state.undo_stack().is_some() {
                if let Some(verb) = crate::undo::undo_redo_verb(other, modifiers) {
                    if verb == "redo" {
                        state.redo();
                    } else {
                        state.undo();
                    }
                    return true;
                }
            }
            // R939 §5.22 — Ctrl/Cmd+/ toggle line comment, for a code editor
            // that opted in via `set_line_comment`. Same Ctrl-OR-Meta-not-Alt
            // gate as select-all / undo above. A field with no configured
            // marker (`line_comment() == None`) falls through to the generic
            // Ctrl-reject below, so `/` is never inserted as a literal under
            // Ctrl and the application keeps the chord — the keymap SSOT
            // placement (a `TextEditState`-only key, R938.1).
            if modifiers.command_key() && !modifiers.alt_key() && other == "/" {
                if let Some(marker) = state.line_comment() {
                    state.toggle_line_comment(marker);
                    return true;
                }
            }
            // R56.1.e §5.22 — Ctrl-OR-Meta-modified printable chars
            // are clipboard / shortcut chords (Ctrl+C / Ctrl+V /
            // Ctrl+Z / etc.). The text-input layer must NOT treat
            // them as literal inserts — Ctrl+C inserting a 'c' is
            // the wrong-UX trap every text-input layer guards
            // against. Returning `false` lets the higher-level
            // dispatcher (`TextFieldExternal::dispatch_key` for the
            // clipboard arm, or the application's own shortcut
            // handler) consume the chord; if nobody consumes it the
            // key drops with `defaultPrevented = false` per the W3C
            // KeyboardEvent contract.
            //
            // Alt-without-Ctrl is treated as a printable variant
            // (macOS Option+letter ↔ special character; the OS-
            // side keyboard layout already encoded the variation
            // into `KeyboardEvent.key`). AltGr (Ctrl+Alt on Windows
            // / Linux) collapses into the Ctrl branch and rejects;
            // applications relying on raw AltGr printable variants
            // route them via a shell shortcut instead of through
            // text-input apply_key.
            if modifiers.command_key() {
                return false;
            }
            match is_printable_key(other) {
                Some(c) => {
                    // 4-byte UTF-8 buffer covers every Unicode code
                    // point through U+10FFFF; `encode_utf8` returns
                    // the populated subslice that
                    // `TextEditState::insert` splices into the
                    // reactive text. `insert` is selection-aware so
                    // a printable keystroke with an active selection
                    // replaces the selected range (W3C
                    // type-to-replace canonical).
                    let mut buf = [0u8; 4];
                    state.insert(c.encode_utf8(&mut buf));
                    true
                }
                None => false,
            }
        }
    }
}

/// R56.1.d §5.38 — W3C UI Events printable-key predicate.
///
/// Returns `Some(c)` when `key` is a single non-control codepoint
/// suitable for verbatim text insert; `None` for named keys
/// (multi-char strings), multi-char IME composition output (R56.1.g
/// path), the empty string, and single-codepoint control chars.
///
/// Distinct from the listbox
/// [`is_typeahead_char`](crate::widgets) predicate (R51.106) in
/// that text input accepts *any* non-control printable codepoint —
/// punctuation (`","`), symbols (`"$"`), math (`"≠"`), and CJK
/// ideographs (`"漢"`) all flow through. The typeahead predicate
/// gates on [`char::is_alphanumeric`] specifically because option
/// labels are letter-prefixed; text input has no such precondition.
#[must_use]
fn is_printable_key(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let first = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if first.is_control() {
        return None;
    }
    Some(first)
}

/// `TextField` widget state machine.
///
/// Four-state interaction model (Idle / Focused / Editing / Disabled)
/// generated from `widgets/text_field.scxml`. R56.1.b composes an
/// optional [`TextEditState`] handle for the text-content sidecar —
/// the SCXML statechart owns interaction state (focus, IME compose
/// gate), the [`TextEditState`] owns text content + caret. Mirrors
/// the R55.D.3 [`ScrollBar`](crate::widgets::scrollbar::ScrollBar)
/// composition (visible peer's statechart + orthogonal reactive
/// state).
pub struct TextField {
    inner: Widget<TextFieldPolicy>,
    /// R56.1.b §5.38 — composition handle to the authoritative
    /// [`TextEditState`]. Optional — a bare `TextField::new()`
    /// carries no handle and the introspect `text` / `caret` query
    /// slots return `None`, matching the
    /// [`ScrollBar`](crate::widgets::scrollbar::ScrollBar)
    /// paint-only convention until the application explicitly
    /// attaches reactive state.
    text_state: Option<Rc<TextEditState>>,
    /// R56.1.h §5.38 §5.28 — optional caret blink animation handle.
    /// `None` means the binding hasn't attached a blink (the paint
    /// backend renders the caret as solid). When attached, every
    /// statechart transition ([`Self::send`]) syncs the blink's
    /// enabled gate: `Focused` / `Editing` → enabled (caret blinks),
    /// `Idle` / `Disabled` → disabled (the [`CaretBlink::tick`](crate::animation::Tickable::tick) no-op
    /// holds the off frame so the caret is hidden whenever the
    /// widget is unfocused).
    blink: Option<Rc<CaretBlink>>,
    /// R56.1.e §5.22 — optional clipboard handle for Ctrl+C / Ctrl+X
    /// / Ctrl+V dispatch through the `invoke("key", ...)` channel.
    /// `None` means the binding has not attached a clipboard; the
    /// `key` invoke surface returns `Bool(false)` for the three
    /// clipboard keystrokes (mirror of the
    /// `text_state.is_none()` no-op shape). The handle is shared
    /// (`Rc<dyn Clipboard>`) so the same instance can back multiple
    /// `TextField`s in a future multi-widget binding without each
    /// one owning its own paste buffer.
    clipboard: Option<Rc<dyn Clipboard>>,
}

impl TextField {
    /// Construct a `TextField` in the [`TextFieldState::Idle`] state.
    /// W3C ARIA `textbox` role canonical (un-focused) default. No
    /// [`TextEditState`] attached — call [`Self::attach_state`] to
    /// wire reactive text content.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Widget::new(),
            text_state: None,
            blink: None,
            clipboard: None,
        }
    }

    /// R56.1.b §5.38 — attach a [`TextEditState`] handle
    /// (composition). The `TextField` becomes the interaction-state
    /// peer for the reactive text container; subsequent introspect
    /// `text` / `caret` queries read through this handle. Builder-
    /// style: chain after [`Self::new`] for the fluent
    /// `TextField::new().attach_state(rc)` shape.
    ///
    /// Detaching is not supported — drop the `TextField` and build
    /// a fresh one. The `TextEditState` handle outlives the
    /// `TextField` (refcounted by `Rc`), so detach/reattach mid-life
    /// would complicate the contract without unlocking a real use
    /// case. Mirrors the R55.D.3
    /// [`ScrollBar::attach_state`](crate::widgets::scrollbar::ScrollBar::attach_state)
    /// contract.
    #[must_use]
    pub fn attach_state(mut self, state: Rc<TextEditState>) -> Self {
        self.text_state = Some(state);
        self
    }

    /// Read-only access to the attached [`TextEditState`] handle.
    /// `None` until [`Self::attach_state`] fires. Diagnostic / test
    /// surface; production callers reach the state through the same
    /// `Rc<TextEditState>` they passed in (the R56.1.b
    /// [`use_text_edit_state`] hook returns the canonical shared handle).
    #[must_use]
    pub fn text_state(&self) -> Option<&Rc<TextEditState>> {
        self.text_state.as_ref()
    }

    /// R56.1.h §5.38 §5.28 — attach a [`CaretBlink`] animation handle.
    /// After attachment, every statechart transition
    /// ([`Self::send`]) syncs the blink's enabled gate via
    /// `Self::sync_blink`: `Focused` / `Editing` → enabled,
    /// `Idle` / `Disabled` → disabled. Builder-style; chain after
    /// [`Self::new`] for the fluent
    /// `TextField::new().attach_state(text).attach_blink(blink)`
    /// shape.
    ///
    /// The handle is shared (`Rc`) — the same `CaretBlink` instance
    /// is queried by the paint backend (`visible()`) and ticked by
    /// the binding's animation loop (Tickable per R56.1.c). Drop the
    /// `TextField` to detach; mid-life detach/reattach is not
    /// supported (mirror of [`Self::attach_state`] contract).
    ///
    /// Initial sync runs immediately so attaching after a `Focus`
    /// event still propagates the enabled gate (e.g. attaching a
    /// blink to a `TextField` that the application has already
    /// driven into `Focused` via an external `send(Focus)` will
    /// enable the blink right away).
    #[must_use]
    pub fn attach_blink(mut self, blink: Rc<CaretBlink>) -> Self {
        self.blink = Some(blink);
        self.sync_blink();
        self
    }

    /// Read-only access to the attached [`CaretBlink`] handle.
    /// `None` until [`Self::attach_blink`] fires. Diagnostic / test
    /// surface; production callers reach the blink through the same
    /// `Rc<CaretBlink>` they passed in.
    #[must_use]
    pub fn blink(&self) -> Option<&Rc<CaretBlink>> {
        self.blink.as_ref()
    }

    /// R56.1.e §5.22 — attach a [`Clipboard`] handle. After
    /// attachment, the `invoke("key", ...)` channel routes
    /// Ctrl+C / Ctrl+X / Ctrl+V keystrokes through this handle
    /// (Ctrl/Meta gate mirrors the R56.1.f.2 Ctrl/Cmd+A select-all
    /// binding so the same modifier set fires on every desktop).
    /// Builder-style; chain after [`Self::new`] for the fluent
    /// `TextField::new().attach_state(text).attach_clipboard(cb)`
    /// shape.
    ///
    /// The handle is shared (`Rc<dyn Clipboard>`) — the same
    /// implementation backs every attached field, so an
    /// [`InMemoryClipboard`](crate::clipboard::InMemoryClipboard)
    /// shared across multiple `TextField`s gives them a common paste
    /// buffer (the canonical "system clipboard" UX). Drop the
    /// `TextField` to detach; mid-life detach/reattach is not
    /// supported (mirror of [`Self::attach_state`] /
    /// [`Self::attach_blink`] contracts).
    #[must_use]
    pub fn attach_clipboard(mut self, clipboard: Rc<dyn Clipboard>) -> Self {
        self.clipboard = Some(clipboard);
        self
    }

    /// Read-only access to the attached [`Clipboard`] handle. `None`
    /// until [`Self::attach_clipboard`] fires. Diagnostic / test
    /// surface.
    #[must_use]
    pub fn clipboard(&self) -> Option<&Rc<dyn Clipboard>> {
        self.clipboard.as_ref()
    }

    /// Drive a [`TextFieldEvent`] through the SCXML. Pure state
    /// transition — text-content mutation flows through
    /// [`TextEditState`] (R56.1.b composition handle), not through
    /// this widget.
    ///
    /// R56.1.h §5.38 §5.28 — every send syncs the attached
    /// [`CaretBlink`] enabled gate to the post-transition state, so
    /// the lifecycle ordering matches the W3C `FocusEvent` contract:
    /// state transition fires first, then the blink animation gate
    /// settles into the new state's posture (Focused/Editing
    /// enabled, Idle/Disabled disabled). No-op on bare `TextField`s
    /// without an attached blink.
    pub fn send(&mut self, event: TextFieldEvent) {
        self.inner.send(event);
        self.sync_blink();
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> TextFieldState {
        self.inner.state()
    }

    /// R56.1.h §5.38 §5.28 — sync the attached [`CaretBlink`] enabled
    /// gate to the current statechart state. Called automatically
    /// from [`Self::send`] (and from [`Self::attach_blink`] for the
    /// initial post-attach sync); production callers do not need to
    /// invoke this directly. No-op on bare `TextField`s without an
    /// attached blink.
    ///
    /// Enabled gate policy: `Focused` / `Editing` → enabled,
    /// `Idle` / `Disabled` → disabled. The `Editing` state inherits
    /// the enabled gate from `Focused` because IME composition still
    /// shows a caret (the blinking insertion point sits at the end
    /// of the preedit run in most platforms' IME UI — Wayland text-
    /// input-v3, `GTK` `IBus`, macOS `NSTextInputContext` all paint
    /// the same caret during composition).
    fn sync_blink(&self) {
        if let Some(blink) = &self.blink {
            let enabled = matches!(
                self.state(),
                TextFieldState::Focused | TextFieldState::Editing,
            );
            blink.set_enabled(enabled);
        }
    }
}

impl Default for TextField {
    fn default() -> Self {
        Self::new()
    }
}

/// R56.1.a §5.38 — `TextField` transition contract. The snapshot is
/// just the SCXML state (no value sidecar on this slice). Detection
/// emits a single `"text_committed"` intent on the two commit-raise
/// transitions out of [`TextFieldState::Editing`]
/// ([`TextFieldEvent::CommitEdit`] → `Focused`,
/// [`TextFieldEvent::Blur`] → `Idle`). The
/// [`TextFieldEvent::CancelEdit`] / [`TextFieldEvent::Disable`] exit
/// paths stay silent — matches the IME canonical
/// cancel-discards-preedit + disable-drops-preedit behaviour.
///
/// State-pair-only detection (the `ScrollBar` R55.D.2 / Slider
/// R51.14 idiom) does not work here — `Editing → Focused` is
/// reachable via
/// **both** [`TextFieldEvent::CommitEdit`] (commit) and
/// [`TextFieldEvent::CancelEdit`] (silent). The `event` argument is
/// the R51.54 §5.39 surface added exactly for this case: distinguish
/// commit from cancel when the state-pair signature is ambiguous.
impl WidgetTransition for TextField {
    type Event = TextFieldEvent;
    type Snapshot = TextFieldState;

    fn snapshot(&self) -> Self::Snapshot {
        self.state()
    }

    fn drive(&mut self, event: Self::Event) {
        self.send(event);
    }

    fn detect(before: Self::Snapshot, event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
        let commit_via_event = matches!(event, TextFieldEvent::CommitEdit | TextFieldEvent::Blur,);
        let exited_editing =
            matches!(before, TextFieldState::Editing) && !matches!(after, TextFieldState::Editing);
        if commit_via_event && exited_editing {
            vec![Intent::new_static(
                crate::widgets::commit::TEXT_COMMITTED_EVENT,
                IntrospectValue::Null,
            )]
        } else {
            Vec::new()
        }
    }
}

/// R959 / R961 §5.36 §5.22 — the field's **`send`-wire sub-target** grammar:
/// the closed set of `"<field>#<key>"` composite click targets a multi-line
/// editor's gutter routes to its own [`TextFieldExternal`]'s
/// `invoke("send", "<key>:PointerUp")` wire. A closed sum type over the field's
/// two send sub-targets, so one [`parse`](Self::parse) + one encode-per-kind
/// keep the paint producer and the `send` decoder from drifting.
///
/// R961.1 honesty note: this is a thin codec, NOT the
/// [`GridSendKey`](crate::composite_tag::GridSendKey) pattern in full —
/// `GridSendKey` earns its enum via a *polymorphic* `row()` projection that
/// several row-only coordinators consume without caring about the variant; this
/// has no such polymorphic consumer (its sole decoder, `invoke_send`,
/// immediately matches and dispatches per-kind). The enum is justified as the
/// one home of the two-kind send grammar (and the exhaustive `match` forces a
/// future kind to be handled), not by shared downstream behaviour — the two
/// kinds bifurcate into unrelated actions (`go_to_line` vs `toggle_fold`).
///
/// A gutter click routes here instead of through the geometry press hook
/// (`position_caret_for_point`):
/// focus-independent and arming no caret text-drag, because click-to-focus
/// resolves the composite to the primary focusable field and the press hook
/// rejects the `!= field` composite tag (R959 B1/B2). The discrete tagged
/// node IS the addressed line — there is no pixel → line geometry.
///
/// Two kinds today, both `"<prefix><line>"`:
///
/// * [`GutterLine`](Self::GutterLine) (`"gl<n>"`) — a line-number click;
///   `<n>` is the **1-based** logical line (the
///   [`go_to_line`](crate::widgets::text_edit::TextEditState::go_to_line)
///   convention). R959.
/// * [`FoldToggle`](Self::FoldToggle) (`"fold<n>"`) — a fold-chevron click;
///   `<n>` is the **0-based** opener `start_line` (the
///   [`toggle_fold`](crate::widgets::text_edit::TextEditState::toggle_fold) /
///   `toggle-fold` convention). The R955 deferred click-to-fold, landed as
///   the 2nd consumer of this grammar (R961).
///
/// Each kind carries its line in its substrate method's native base (the two
/// RPC peers `go-to-line` / `toggle-fold` already differ), so the `send` wire
/// and the RPC agree per kind.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TextFieldSendKey {
    /// A line-number gutter click — `"gl<n>"`.
    GutterLine {
        /// 1-based logical line (the `go_to_line` target).
        line: usize,
    },
    /// A fold-chevron click — `"fold<n>"`.
    FoldToggle {
        /// 0-based opener `start_line` (the `toggle_fold` target).
        line: usize,
    },
}

impl TextFieldSendKey {
    /// The line-number sub-target prefix (`"gl"`).
    const GUTTER_LINE_PREFIX: &'static str = "gl";
    /// The fold-chevron sub-target prefix (`"fold"`). Disjoint from
    /// [`GUTTER_LINE_PREFIX`](Self::GUTTER_LINE_PREFIX) — neither prefixes the
    /// other — so the [`parse`](Self::parse) order is irrelevant.
    const FOLD_PREFIX: &'static str = "fold";

    /// Decode a `'#'`-split sub-target (the part after `"<field>#"`). `None`
    /// for any sub that addresses neither kind (no field `send` target).
    #[must_use]
    pub fn parse(sub: &str) -> Option<Self> {
        if let Some(n) = sub.strip_prefix(Self::GUTTER_LINE_PREFIX) {
            return n.parse().ok().map(|line| Self::GutterLine { line });
        }
        if let Some(n) = sub.strip_prefix(Self::FOLD_PREFIX) {
            return n.parse().ok().map(|line| Self::FoldToggle { line });
        }
        None
    }

    /// Build a line-number gutter click target — `"<field_tag>#gl<line>"`
    /// (1-based `line`). The encode twin of [`parse`](Self::parse) for the
    /// [`GutterLine`](Self::GutterLine) kind; the `'#'` join is the R51.42
    /// §5.35 frozen-separator idiom (R803).
    #[must_use]
    pub fn gutter_line_tag(field_tag: &str, line: usize) -> String {
        format!("{field_tag}#{}{line}", Self::GUTTER_LINE_PREFIX)
    }

    /// Build a fold-chevron click target — `"<field_tag>#fold<line>"`
    /// (0-based opener `start_line`). The encode twin of [`parse`](Self::parse)
    /// for the [`FoldToggle`](Self::FoldToggle) kind.
    #[must_use]
    pub fn fold_toggle_tag(field_tag: &str, line: usize) -> String {
        format!("{field_tag}#{}{line}", Self::FOLD_PREFIX)
    }
}

/// `External` adapter wrapping a [`TextField`]. Emits a single
/// intent kind on R56.1.a:
///
/// * `"text_committed"` ([`IntrospectValue::Null`] payload) on
///   `Editing → Focused` ([`TextFieldEvent::CommitEdit`]) or
///   `Editing → Idle` ([`TextFieldEvent::Blur`]).
///
/// R56.1.d §5.38 §5.22 — `invoke("key", Text(k))` RPC path dispatches
/// W3C UI Events keystrokes (`"Backspace"`, `"ArrowLeft"`, single
/// printable chars, etc.) into the attached [`TextEditState`] via
/// [`apply_key`]; returns `Bool(true)` on recognized keys,
/// `Bool(false)` on unrecognized or bare-`TextField` paths.
/// Future sub-rounds extend the surface — R56.1.g attaches the
/// preedit-buffer payload, R56.1.f wires the ARIA `textbox`
/// accessible name/value, R56.1.h adds the focus-lifecycle wire
/// (statechart focus/blur drive + caret-blink `set_enabled` sync).
pub struct TextFieldExternal {
    em: IntentEmitter<TextField>,
    /// R793 §5.38 §5.39 — opt-in: emit a `"blur"` §5.20 intent on every
    /// focus loss (the W3C DOM `focusout` mirror). Off by default — a plain
    /// field stays silent on blur (only the IME `"text_committed"` path
    /// fires). An inline editor (todomvc row edit / file-manager rename)
    /// opts in via [`Self::with_blur_intent`] so its binding's reducer can
    /// **commit-on-blur** (click-away saves the edit, the Files/Explorer /
    /// `TodoMVC` convention).
    emit_blur_intent: bool,
}

impl TextFieldExternal {
    /// Construct a `TextFieldExternal` wrapping a fresh
    /// [`TextField`] in [`TextFieldState::Idle`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            em: IntentEmitter::default(),
            emit_blur_intent: false,
        }
    }

    /// R793 §5.38 §5.39 — opt into emitting a `"blur"` §5.20 intent on every
    /// focus loss (the DOM `focusout` mirror). A binding wires this on an
    /// **inline editor** field so its reducer commits the edit when focus
    /// leaves (click-away). Plain fields leave it off and stay silent on
    /// blur. Builder-style; chain after [`Self::new`].
    #[must_use]
    pub fn with_blur_intent(mut self) -> Self {
        self.emit_blur_intent = true;
        self
    }

    /// R56.1.b §5.38 — attach a [`TextEditState`] handle to the
    /// inner [`TextField`] (composition). Builder-style; chain after
    /// [`Self::new`] for the fluent shape. See
    /// [`TextField::attach_state`] for the contract. Mirror of the
    /// R55.D.3
    /// [`ScrollBarExternal::attach_state`](crate::widgets::scrollbar::ScrollBarExternal::attach_state)
    /// pattern.
    #[must_use]
    pub fn attach_state(mut self, state: Rc<TextEditState>) -> Self {
        // `mem::take` keeps the builder shape (consume-and-return)
        // through the wrapping `IntentEmitter`; `TextField::default`
        // is the cheap round-trip (no observable state — text_state
        // None, SCXML freshly initialized to Idle).
        self.em.inner = std::mem::take(&mut self.em.inner).attach_state(state);
        self
    }

    /// R56.1.b §5.38 — attached [`TextEditState`] handle (delegates
    /// to [`TextField::text_state`]). `None` until
    /// [`Self::attach_state`] fires.
    #[must_use]
    pub fn text_state(&self) -> Option<&Rc<TextEditState>> {
        self.em.inner.text_state()
    }

    /// R56.1.h §5.38 §5.28 — attach a [`CaretBlink`] handle to the
    /// inner [`TextField`] (composition; delegates to
    /// [`TextField::attach_blink`]). Builder-style; chain after
    /// [`Self::new`] for the fluent shape. Statechart transitions
    /// driven through [`Self::send`] / [`Self::on_focus_change`]
    /// sync the blink's enabled gate automatically.
    #[must_use]
    pub fn attach_blink(mut self, blink: Rc<CaretBlink>) -> Self {
        // `mem::take` keeps the builder shape (consume-and-return)
        // through the wrapping `IntentEmitter`; `TextField::default`
        // is the cheap round-trip (no observable state — blink None,
        // SCXML freshly initialized to Idle).
        self.em.inner = std::mem::take(&mut self.em.inner).attach_blink(blink);
        self
    }

    /// R56.1.h §5.38 §5.28 — attached [`CaretBlink`] handle
    /// (delegates to [`TextField::blink`]). `None` until
    /// [`Self::attach_blink`] fires.
    #[must_use]
    pub fn blink(&self) -> Option<&Rc<CaretBlink>> {
        self.em.inner.blink()
    }

    /// R56.1.e §5.22 — attach a [`Clipboard`] handle to the inner
    /// [`TextField`] (composition; delegates to
    /// [`TextField::attach_clipboard`]). Builder-style; chain after
    /// [`Self::new`] for the fluent shape.
    #[must_use]
    pub fn attach_clipboard(mut self, clipboard: Rc<dyn Clipboard>) -> Self {
        self.em.inner = std::mem::take(&mut self.em.inner).attach_clipboard(clipboard);
        self
    }

    /// R56.1.e §5.22 — attached [`Clipboard`] handle (delegates to
    /// [`TextField::clipboard`]). `None` until
    /// [`Self::attach_clipboard`] fires.
    #[must_use]
    pub fn clipboard(&self) -> Option<&Rc<dyn Clipboard>> {
        self.em.inner.clipboard()
    }

    /// Drive a [`TextFieldEvent`] and queue a `"text_committed"`
    /// intent on the two commit-raise transitions. Pipeline lives on
    /// [`IntentEmitter::dispatch`]; the detection rule lives on the
    /// [`WidgetTransition`] impl for [`TextField`].
    pub fn send(&mut self, event: TextFieldEvent) {
        self.em.dispatch(event);
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> TextFieldState {
        self.em.inner.state()
    }

    /// R56.1.g §5.38 §5.22 — begin an IME composition. Seeds the
    /// attached [`TextEditState`] preedit buffer (via
    /// [`TextEditState::preedit_start`]) AND drives
    /// [`TextFieldEvent::BeginEdit`] through the SCXML.
    ///
    /// The two layers stay loosely coupled: `preedit_start` mutates
    /// the reactive sidecar (4-axis batched write when a selection
    /// was active, simple 1-axis write otherwise); the SCXML
    /// transition advances `Focused` → `Editing` so the caret-blink
    /// gate + the introspect `state` slot reflect "composition in
    /// flight". The blink resets to fully-visible on
    /// composition-start to mirror the canonical macOS / iOS / GTK /
    /// Web "user interacted, snap the caret solid" UX
    /// ([`r56_1_j` carry close](#r56_1_j)).
    ///
    /// No-op on bare `TextFieldExternal`s without an attached
    /// [`TextEditState`] — the preedit buffer side silently skips,
    /// the SCXML transition still fires so the AI client can
    /// observe the state.
    pub fn apply_composition_start(&mut self) {
        if let Some(state) = self.em.inner.text_state() {
            state.preedit_start();
        }
        self.send(TextFieldEvent::BeginEdit);
        if let Some(blink) = self.em.inner.blink() {
            blink.reset();
        }
    }

    /// R56.1.g §5.38 §5.22 — update the active IME preedit string
    /// (the W3C `compositionupdate` mirror, where `data` carries the
    /// current preedit text). Forwards into
    /// [`TextEditState::preedit_update`]; the SCXML stays in
    /// `Editing` (no transition fires) because the canonical IME
    /// contract is one start + many updates + one commit-or-cancel.
    ///
    /// The blink resets on every update so the caret stays solid
    /// while the user is composing (matches the platform IME UX:
    /// macOS `NSTextInputContext` blinks the caret only when the
    /// preedit pauses).
    ///
    /// No-op on bare `TextFieldExternal`s and on defensive
    /// out-of-order delivery (`preedit_update` without a prior
    /// `preedit_start`). The substrate idempotence is documented on
    /// [`TextEditState::preedit_update`].
    pub fn apply_composition_update(&mut self, preedit: &str) {
        if let Some(state) = self.em.inner.text_state() {
            state.preedit_update(preedit);
        }
        if let Some(blink) = self.em.inner.blink() {
            blink.reset();
        }
    }

    /// R56.1.g §5.38 §5.22 — commit the active IME composition.
    /// Inserts `committed` into the attached [`TextEditState`] at
    /// the current caret (via [`TextEditState::preedit_commit`]),
    /// drives [`TextFieldEvent::CommitEdit`] through the SCXML, and
    /// queues a `"text_committed"` intent with the
    /// [`IntrospectValue::Text`] payload carrying the committed
    /// string.
    ///
    /// The intent payload **upgrades** from the R56.1.a
    /// [`IntrospectValue::Null`] (legacy plain
    /// `send(TextFieldEvent::CommitEdit)`) to
    /// [`IntrospectValue::Text`] on this composition path — the
    /// W3C `CompositionEvent.data` shape the AI client expects when
    /// observing IME commits. Plain `send(TextFieldEvent::CommitEdit)`
    /// (no composition layer) continues to emit
    /// [`IntrospectValue::Null`] for backward compatibility — the
    /// detect rule on the [`WidgetTransition`] impl handles that
    /// path unchanged.
    ///
    /// The SCXML drive bypasses [`IntentEmitter::dispatch`] (no
    /// `detect` invocation) so the `Editing → Focused` transition
    /// does **not** also push the legacy `Null`-payload intent on
    /// top of the upgraded `Text` payload — one composition commit
    /// emits exactly one intent.
    ///
    /// Empty `committed` is the cancel-shaped commit (the W3C
    /// `compositionend` with empty `data` after a cancel): the
    /// preedit buffer clears, the SCXML transitions, but **no**
    /// intent is queued (an empty commit semantically committed no
    /// text). Applications that want a marker intent on empty
    /// commit use [`Self::apply_composition_cancel`] instead.
    ///
    /// No-op on bare `TextFieldExternal`s without an attached
    /// [`TextEditState`] — both the buffer mutation and the intent
    /// push are skipped, but the SCXML transition still fires so
    /// the AI client can observe the state-pair flip.
    pub fn apply_composition_commit(&mut self, committed: &str) {
        // Sample the composition-active predicate before
        // `preedit_commit` clears the buffer — the post-clear read
        // would always return `false` and gate the intent off.
        let was_composing = self.em.inner.text_state().is_some_and(|s| s.is_composing());
        if let Some(state) = self.em.inner.text_state() {
            state.preedit_commit(committed);
        }
        self.em.inner.send(TextFieldEvent::CommitEdit);
        if was_composing && !committed.is_empty() {
            self.em.push(Intent::new_static(
                crate::widgets::commit::TEXT_COMMITTED_EVENT,
                IntrospectValue::Text(committed.to_string()),
            ));
        }
        if let Some(blink) = self.em.inner.blink() {
            blink.reset();
        }
    }

    /// R56.1.g §5.38 §5.22 — cancel the active IME composition.
    /// Clears the attached [`TextEditState`] preedit buffer (via
    /// [`TextEditState::preedit_cancel`]) AND drives
    /// [`TextFieldEvent::CancelEdit`] through the SCXML. The SCXML
    /// cancel transition is silent (the detect rule does not emit a
    /// `text_committed` intent on `CancelEdit` — matches the IME
    /// canonical "cancel-discards-preedit" path: Escape during
    /// composition, or Wayland text-input-v3 cancel).
    ///
    /// The blink resets so the caret stays solid for the
    /// post-cancel keystroke the user is presumably about to type
    /// (matches the macOS `NSTextInputContext` UX).
    ///
    /// No-op on bare `TextFieldExternal`s without an attached
    /// [`TextEditState`] (buffer mutation skipped; SCXML transition
    /// still fires).
    pub fn apply_composition_cancel(&mut self) {
        if let Some(state) = self.em.inner.text_state() {
            state.preedit_cancel();
        }
        self.send(TextFieldEvent::CancelEdit);
        if let Some(blink) = self.em.inner.blink() {
            blink.reset();
        }
    }
}

impl Default for TextFieldExternal {
    fn default() -> Self {
        Self::new()
    }
}

/// R1250 §5.45 §5.38 — the standard **commit-on-blur inline editor** sibling
/// extra: a [`TextFieldExternal`] keyed by `tag`, sharing the Owner-cached
/// [`TextEditState`] + [`CaretBlink`] the view fn / edit lifecycle resolve, with
/// the R793 [`with_blur_intent`](TextFieldExternal::with_blur_intent) commit
/// signal. Lifted from four byte-identical `create_extra_externals` hand-rolls
/// (`hello-property-grid` / `hello-data-grid` / `hello-node-editor` /
/// `hello-inspector`) — the mechanical `ExtraExternal::new(tag,
/// Box::new(TextFieldExternal::new().attach_state(use_text_edit_state(tag))
/// .attach_blink(use_caret_blink(tag)).with_blur_intent()))` registration with
/// no per-binding opinion (a mechanical 4-site duplication, R727/R732 3b lift).
///
/// Call inside the root [`Owner`](crate::reactive::Owner) scope (where
/// [`WidgetCore::create_extra_externals`](crate::widget_core::WidgetCore::create_extra_externals)
/// runs), so the `use_*` hooks resolve the same `Rc`s the view fn later reads.
///
/// The rename-editor variants (`hello-file-manager` / `todomvc`) additionally
/// `attach_clipboard` — a different capability, deliberately NOT folded in here
/// (a `_with_clipboard` peer is the answer when its 3rd consumer surfaces).
#[must_use]
pub fn blur_committing_field_extra(tag: &'static str) -> ExtraExternal {
    ExtraExternal::new(
        tag,
        Box::new(
            TextFieldExternal::new()
                .attach_state(use_text_edit_state(tag))
                .attach_blink(use_caret_blink(tag))
                .with_blur_intent(),
        ),
    )
}

impl core::fmt::Debug for TextFieldExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextFieldExternal")
            .field("state", &self.state())
            .finish()
    }
}

impl External for TextFieldExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        self.em.drain(sink);
    }

    fn is_dirty(&self) -> bool {
        self.em.is_dirty()
    }

    /// R56.1.h §5.38 §5.39 — shell focus change ↔ SCXML statechart
    /// drive. The shell's
    /// `notify_focus_change`
    /// calls this hook on the outgoing widget (`focused=false`)
    /// before the incoming widget (`focused=true`) — mirrors the
    /// W3C DOM `FocusEvent` dispatch order (`blur` then `focus`).
    ///
    /// `focused=true` drives [`TextFieldEvent::Focus`]; `focused=false`
    /// drives [`TextFieldEvent::Blur`]. The SCXML transition graph
    /// (R56.1.a `widgets/text_field.scxml`) handles every reachable
    /// state pair:
    ///
    /// - `Idle → Focused` (focus): caret appears, blink enables.
    /// - `Focused → Idle` (blur): caret hides, blink disables.
    /// - `Editing → Idle` (blur): IME canonical commit-on-blur —
    ///   raises `textfield.commit`, emits the `"text_committed"`
    ///   intent (matches Wayland text-input-v3 / `GTK` `IBus` / macOS
    ///   `NSTextInputContext` / Windows TSF). The application's drain
    ///   loop picks up the intent on the next dispatch tail.
    /// - `Disabled + focus|blur`: no transition (SCXML rejects), the
    ///   focus mgr still tracks the tag but the widget stays inert.
    ///
    /// R56.1.g §5.38 §5.22 — when the focus loss arrives while a
    /// composition is in flight (preedit buffer `Some(s)` with
    /// non-empty `s`), the substrate forwards through
    /// [`Self::apply_composition_commit`] before driving the
    /// `Blur` event so the committed text + the upgraded
    /// `Intent(Text(s))` payload fire in the canonical order. Empty
    /// preedit (composition active but no preedit text yet — the
    /// `compositionstart`-before-`compositionupdate` window) cancels
    /// instead of committing, matching the platform "no-data
    /// compositionend is a cancel" convention. The legacy
    /// `Intent(Null)` payload from the `Editing → Idle` detect rule
    /// stays in place for the plain `send(Blur)` path (no
    /// composition layer); the commit-on-blur path here pushes the
    /// upgraded `Text` intent through `apply_composition_commit`.
    ///
    /// The blink lifecycle syncs automatically through
    /// `TextField::sync_blink` (called from [`TextField::send`]),
    /// so attaching a [`CaretBlink`] makes the gate flip in lockstep
    /// with the statechart transition.
    fn on_focus_change(&mut self, focused: bool) {
        if focused {
            self.send(TextFieldEvent::Focus);
            return;
        }
        // R56.1.g §5.38 §5.22 — commit-on-blur for in-flight
        // composition. Empty preedit (start without any update yet)
        // cancels instead of committing — matches the W3C
        // "no-data compositionend is a cancel" convention.
        let pending_preedit = self.em.inner.text_state().and_then(|s| s.preedit());
        match pending_preedit {
            Some(text) if !text.is_empty() => {
                self.apply_composition_commit(&text);
            }
            Some(_) => {
                self.apply_composition_cancel();
            }
            None => {}
        }
        self.send(TextFieldEvent::Blur);
        // R793 §5.38 §5.39 — the DOM `focusout` mirror: an opted-in inline
        // editor emits a `"blur"` intent on every focus loss so its binding
        // reducer can commit the edit (click-away saves). Fires unconditionally
        // on focus loss (the reducer gates on its own edit-mode flag); a plain
        // field never opts in, so this is silent for every non-editor field.
        if self.emit_blur_intent {
            self.em
                .push(Intent::new_static("blur", IntrospectValue::Null));
        }
    }
}

impl ExternalIntrospect for TextFieldExternal {
    fn schema(&self) -> IntrospectSchema {
        // R56.1.b §5.38 — schema grows from 2 slots to 4 when the
        // text content sidecar arrives. `text` + `caret` are exposed
        // unconditionally; the introspect query / intervene path
        // returns `None` / `ReadOnly` when no [`TextEditState`] is
        // attached so the AI client sees a stable schema shape across
        // bare and wired-up `TextField`s.
        //
        // R56.1.d §5.38 §5.22 — adds the `key` invoke slot for the
        // W3C UI Events keystroke dispatch surface. The slot exists
        // unconditionally (mirror of the `text` / `caret` policy);
        // invoke returns `Bool(false)` when no [`TextEditState`] is
        // attached so RPC clients distinguish "no state bound" from
        // "key rejected".
        // R56.1.f.3 §5.38 §5.22 — `selection` slot exposes the
        // active selection range as Json `{"start": int, "end": int}`
        // (mirror of the W3C `HTMLInputElement.selectionStart` /
        // `selectionEnd` pair). `Null` when the selection is
        // collapsed (no anchor). Symmetric intervene path accepts
        // the same Json shape (sets both ends atomically through
        // `TextEditState::set_selection`) or `Null` (clears).
        // R56.1.g.2 §5.38 §5.22 — `preedit` query/intervene slot +
        // `composition` invoke slot lift the IME composition surface
        // into the AI-first RPC primary path. `preedit` returns
        // `Text(s)` while composing or `Null` when idle; intervene
        // accepts `Text(s)` (auto-starts + sets) or `Null` (cancels).
        // `composition` invoke takes a Json action surface so the AI
        // client driving an IME flows through one method per W3C
        // lifecycle event.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("state", "string"),
                    SchemaField::new("text", "string"),
                    SchemaField::new("caret", "number"),
                    SchemaField::new("selection", "object"),
                    // R769.1 §5.36 §5.22 — applied rich-text formatting as a JSON
                    // array of `{start, end, style}` runs (the same shape the
                    // field's Text node carries in `scene/snapshot`), so an AI
                    // client reads bold / italic / colour over the buffer directly
                    // without walking the paint tree.
                    SchemaField::new("style_runs", "json"),
                    SchemaField::new("preedit", "string"),
                    SchemaField::new("send", "string"),
                    SchemaField::new("key", "string"),
                    SchemaField::new("composition", "string"),
                    // R56.2.e §5.22 — middle-mouse PRIMARY paste action.
                    // No payload (`Null`); returns `Bool(handled)`. Mirrors
                    // the X11 / Wayland "middle-click pastes the PRIMARY
                    // selection" convention so AI clients can drive the
                    // same code path the shell's middle-click handler hits.
                    SchemaField::new("paste-primary", "boolean"),
                    // R768 §5.36 §5.22 — rich-text formatting actions over a
                    // byte range. `apply-style` takes Json
                    // `{"start": int, "end": int, "fg": "#rrggbb", "size"?: int}`
                    // and overlays one colour [`StyleRun`] on the range
                    // (setCharFormat); `clear-style` takes
                    // `{"start": int, "end": int}` and strips styling back to
                    // the base. Both return `Bool(handled)` (`false` when no
                    // [`TextEditState`] is attached) — the AI-first peer of the
                    // toolbar's apply-to-selection click.
                    //
                    // [`StyleRun`]: crate::scene::StyleRun
                    SchemaField::new("apply-style", "boolean"),
                    SchemaField::new("clear-style", "boolean"),
                    // R967 §5.36 — toggle ONE style field (bold / italic / underline /
                    // strikethrough) over the selection / caret, preserving the run's
                    // other fields (the AI-first peer of the toolbar B / I toggle —
                    // mergeCharFormat). Text arg = the field name; returns the new state.
                    SchemaField::new("toggle-format", "boolean"),
                    // R903 §5.22 — find &amp; replace. `find_query` /
                    // `find_case_sensitive` / `find_whole_word` are query+intervene
                    // (the needle + its flags); `find_matches` is a derived read
                    // (`{query, case_sensitive, whole_word, count, current, ranges}`).
                    // `find-next` / `find-prev` navigate (return the new selection or
                    // `Null`); `replace` (arg = replacement `Text`) replaces the
                    // current match and advances (returns `Bool`); `replace-all` (arg =
                    // replacement `Text`) rewrites every match as one undo step
                    // (returns the count). The AI-first peer of the find bar's
                    // keyboard.
                    SchemaField::new("find_query", "string"),
                    SchemaField::new("find_case_sensitive", "boolean"),
                    SchemaField::new("find_whole_word", "boolean"),
                    SchemaField::new("find_matches", "json"),
                    SchemaField::new("find-next", "object"),
                    SchemaField::new("find-prev", "object"),
                    SchemaField::new("replace", "boolean"),
                    SchemaField::new("replace-all", "number"),
                    // R926 §5.22 — matching-bracket read. Derived from the live
                    // buffer + caret: `{"open": int, "close": int}` when the
                    // caret sits adjacent to a balanced bracket, `Null`
                    // otherwise. The AI-first peer of the editor's
                    // matching-brace highlight — an agent reasoning about code
                    // structure reads where a brace closes without re-scanning.
                    SchemaField::new("bracket_match", "object"),
                    // R933 §5.36 — code-folding surface. `fold_regions` is a
                    // derived read: a JSON array of `{open, close, start_line,
                    // end_line, collapsed}`, one per foldable bracket block (≥ 2
                    // logical lines), opener-ordered. The three actions are the
                    // AI-first peers of the gutter chevron: `toggle-fold` (arg =
                    // the opener's line `Int`) returns `Bool` (did a region
                    // toggle?), `fold-all` / `unfold-all` (arg `Null`) return the
                    // resulting collapsed-region `Int` count.
                    SchemaField::new("fold_regions", "json"),
                    SchemaField::new("toggle-fold", "boolean"),
                    SchemaField::new("fold-all", "number"),
                    SchemaField::new("unfold-all", "number"),
                    // R938 §5.22 — multi-line indent / dedent (the Tab / Shift+Tab
                    // twins). `Null` arg; returns `Bool` (did the lines shift?). R941 —
                    // schema-listed (R938 added the verbs but not the slots — cleared here).
                    SchemaField::new("indent", "boolean"),
                    SchemaField::new("dedent", "boolean"),
                    // R939 §5.22 — line-comment toggle (the Ctrl+/ twin). `Null` arg;
                    // returns `Bool` (R941 — schema-listed, cleared with the above).
                    SchemaField::new("toggle-comment", "boolean"),
                    // R941 §5.22 — go-to-line navigation. `line_count` is the logical
                    // (newline-delimited) line count (the navigation bound + a gutter /
                    // prompt max); `go-to-line` (arg = a 1-based line `Int`) jumps the
                    // caret to that line's start and returns the resolved (clamped) line.
                    SchemaField::new("line_count", "number"),
                    SchemaField::new("go-to-line", "number"),
                    // R945 §5.22 — line manipulation (the Alt+Up / Alt+Down move +
                    // Shift+Alt copy twins). `Null` arg; each returns `Bool` (did the
                    // buffer change? a boundary move — first line up, last line down —
                    // is `false`).
                    SchemaField::new("move-line-up", "boolean"),
                    SchemaField::new("move-line-down", "boolean"),
                    SchemaField::new("duplicate-line-up", "boolean"),
                    SchemaField::new("duplicate-line-down", "boolean"),
                    // R951 §5.36 §5.22 — active typing mark (collapsed-caret formatting,
                    // ProseMirror `storedMarks`). `style_at_caret` reads the style the
                    // next char would carry (armed mark, else inherited-from-left) as
                    // the full TextStyle object (the `apply-style` `style` shape), or
                    // Null for the field base; `pending_style` reads only the *armed*
                    // mark (Null when merely inherited). `mark` (Json = the
                    // `style_at_caret` read shape, bare or `{"style": {...}}`) arms it
                    // so the next typed text is styled; `clear-mark` (Null) drops it.
                    // The AI-first peer of pressing Bold with nothing selected, then
                    // typing — `apply-style` remains the selection path.
                    SchemaField::new("style_at_caret", "json"),
                    SchemaField::new("pending_style", "json"),
                    SchemaField::new("mark", "boolean"),
                    SchemaField::new("clear-mark", "boolean"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(self.state().as_name().to_string())),
            // R56.1.b §5.38 — text + caret read through the attached
            // [`TextEditState`]. `None` when no handle is attached;
            // the AI client treats that as "widget not bound to
            // reactive state" and gates intervene/invoke accordingly.
            "text" => self.text_state().map(|s| IntrospectValue::Text(s.text())),
            "caret" => self.text_state().map(|s| {
                // usize → i64 — caret is bounded by `text.len() <=
                // isize::MAX` on every platform pinion targets, so
                // the cast is lossless. The `try_from` defends
                // against the unreachable 2^63-byte text case.
                IntrospectValue::Int(i64::try_from(s.caret()).unwrap_or(i64::MAX))
            }),
            // R941 §5.22 — the logical (newline-delimited) line count, the
            // upper bound for `go-to-line` + a line-number gutter / prompt max.
            // `None` for a bare field (no attached state), like `caret`.
            "line_count" => self
                .text_state()
                .map(|s| IntrospectValue::Int(i64::try_from(s.line_count()).unwrap_or(i64::MAX))),
            // R56.1.f.3 §5.38 §5.22 — selection range as Json
            // `{"start": int, "end": int}` mirror of W3C
            // `HTMLInputElement.selectionStart` / `selectionEnd`.
            // `Null` when no selection is active (collapsed caret).
            // Bare `TextField` (no attached state) returns `None`
            // (the path is unknown to that instance) so RPC clients
            // distinguish "no state bound" from "no selection".
            // R903 — the `{start, end}|Null` encoding is the
            // [`selection_range_to_value`] SSOT, shared with the
            // `find-next` / `find-prev` invoke return.
            "selection" => self
                .text_state()
                .map(|s| selection_range_to_value(s.selection_range())),
            // R769.1 §5.36 §5.22 — applied formatting runs as a JSON array
            // `[{"start", "end", "style": {...}}]` (the `StyleRun` serde
            // shape, identical to the field Text node's `runs` in
            // `scene/snapshot`). `[]` when the buffer is unstyled. Lets an
            // AI verify bold / italic / colour over the selection directly
            // — the read peer of the `apply-style` / `clear-style` actions.
            "style_runs" => self.text_state().map(|s| {
                IntrospectValue::Json(
                    serde_json::to_value(s.style_runs())
                        .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
                )
            }),
            // R951 §5.36 §5.22 — the style the next inserted char would carry
            // (armed typing mark, else inherited from the char to the left): the
            // full TextStyle object (the `apply-style` `style` shape an agent
            // mutates + writes back via `mark`), or `Null` when the next char is
            // the field base. What a Bold toolbar button lights from.
            "style_at_caret" => self
                .text_state()
                .map(|s| style_to_value(s.style_at_caret())),
            // R951 §5.36 §5.22 — the *armed* typing mark only (`Null` when the
            // next char merely inherits): lets an AI / toolbar distinguish "Bold
            // is armed" from "the text here is already bold".
            "pending_style" => self.text_state().map(|s| style_to_value(s.pending_style())),
            // R56.1.g.2 §5.38 §5.22 — preedit (IME composition) read
            // path. `Text(s)` when composing (mirror of W3C
            // `CompositionEvent.data` observed during
            // `compositionupdate`); `Null` when no composition is
            // active. Bare `TextField` (no attached state) returns
            // `None` so the AI client distinguishes "no state bound"
            // from "not composing".
            "preedit" => self.text_state().map(|s| match s.preedit() {
                Some(content) => IntrospectValue::Text(content),
                None => IntrospectValue::Null,
            }),
            // R903 §5.22 — find &amp; replace read surface (the write peers are
            // the matching `intervene` arms + `find-*` / `replace*` invokes).
            "find_query" => self
                .text_state()
                .map(|s| IntrospectValue::Text(s.find_query())),
            "find_case_sensitive" => self
                .text_state()
                .map(|s| IntrospectValue::Bool(s.find_case_sensitive())),
            "find_whole_word" => self
                .text_state()
                .map(|s| IntrospectValue::Bool(s.find_whole_word())),
            // Derived match state: count + every `[start, end]` range + the
            // index the selection currently coincides with (`current`, null
            // when off a match) — the "{n} of {N}" status + highlight data, all
            // from the one `find_matches` derivation so the wire never disagrees
            // with the paint.
            "find_matches" => self.text_state().map(|s| {
                let matches = s.find_matches();
                let ranges: Vec<[usize; 2]> = matches.iter().map(|&(a, b)| [a, b]).collect();
                IntrospectValue::Json(serde_json::json!({
                    "query": s.find_query(),
                    "case_sensitive": s.find_case_sensitive(),
                    "whole_word": s.find_whole_word(),
                    "count": matches.len(),
                    "current": s.find_current_index(),
                    "ranges": ranges,
                }))
            }),
            // R926 §5.22 — matching-bracket read. `{open, close}` byte
            // offsets when the caret is adjacent to a balanced bracket,
            // `Null` otherwise (caret not next to a bracket, or
            // unbalanced). Reads the SAME `matching_bracket` derivation
            // the paint highlight reads, so the wire and the bands can
            // never report a *different* pair. The wire is, however,
            // focus-independent (it reports the buffer's current pair
            // even on an unfocused field), whereas the paint outline is
            // a caret affordance painted only in the focused posture —
            // so an unfocused field can answer a pair here while showing
            // no box. The buffer truth is the introspection contract;
            // the paint gate is presentation.
            "bracket_match" => self.text_state().map(|s| match s.matching_bracket() {
                Some((open, close)) => IntrospectValue::Json(serde_json::json!({
                    "open": open,
                    "close": close,
                })),
                None => IntrospectValue::Null,
            }),
            // R933 §5.36 — foldable regions as a JSON array
            // `[{open, close, start_line, end_line, collapsed}]`, the read
            // peer of the gutter chevrons + the `toggle-fold` action. Reads
            // the SAME `fold_regions` derivation the paint gutter reads, so
            // the wire and the painted chevrons can never report a different
            // fold set. `[]` for an unfoldable buffer; `None` (path unknown)
            // for a bare field with no attached state.
            "fold_regions" => self.text_state().map(|s| {
                let regions: Vec<serde_json::Value> = s
                    .fold_regions()
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "open": r.open_byte,
                            "close": r.close_byte,
                            "start_line": r.start_line,
                            "end_line": r.end_line,
                            "collapsed": r.collapsed,
                        })
                    })
                    .collect();
                IntrospectValue::Json(serde_json::Value::Array(regions))
            }),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // §5.38 — `state` is SCXML-owned (driven via `send`).
            "state" => Err(InterveneError::ReadOnly),
            // R56.1.b — `text` + `caret` write through the attached
            // [`TextEditState`]. No attached handle → ReadOnly
            // (the slot exists in the schema but cannot be written
            // without a backing reactive store).
            "text" => {
                let Some(state) = self.text_state() else {
                    return Err(InterveneError::ReadOnly);
                };
                let IntrospectValue::Text(s) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.set_text(s);
                Ok(())
            }
            "caret" => {
                let Some(state) = self.text_state() else {
                    return Err(InterveneError::ReadOnly);
                };
                let IntrospectValue::Int(n) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                if n < 0 {
                    return Err(InterveneError::OutOfRange);
                }
                // Lossless cast — TextEditState::set_caret clamps to
                // text.len() internally, so any in-range i64 is
                // accepted; the `try_from` only fails on the
                // unreachable 2^64-overflow path.
                let pos = usize::try_from(n).unwrap_or(usize::MAX);
                state.set_caret(pos);
                Ok(())
            }
            // R56.1.f.3 §5.38 §5.22 — selection write path.
            // `Null` clears any active selection (mirror of W3C
            // `HTMLInputElement.setSelectionRange(null)`). `Json`
            // with `{"start": int, "end": int}` calls
            // `set_selection(start, end)` atomically (3-axis batched
            // write per R56.1.f.1). The two ends may arrive in
            // either order — `set_selection` normalises internally
            // through `min` / `max`; the `end` slot still names the
            // caret-side (W3C focus) for the canonical client read.
            "selection" => {
                let Some(state) = self.text_state() else {
                    return Err(InterveneError::ReadOnly);
                };
                match value {
                    IntrospectValue::Null => {
                        state.clear_selection();
                        // R56.2.e §5.22 — `Null` collapses the
                        // selection; PRIMARY is intentionally NOT
                        // cleared (X11 convention retains the prior
                        // selection until a new one is published).
                        Ok(())
                    }
                    IntrospectValue::Json(obj) => {
                        let Some((start, end)) = parse_selection_intervene_json(&obj) else {
                            return Err(InterveneError::TypeMismatch);
                        };
                        state.set_selection(start, end);
                        // R56.2.e §5.22 — auto-publish the new
                        // selection to PRIMARY so AI-client-driven
                        // selection writes match the canonical Linux
                        // desktop "select + middle-click paste" UX
                        // observable from out-of-process apps.
                        self.publish_primary_selection_if_any();
                        Ok(())
                    }
                    _ => Err(InterveneError::TypeMismatch),
                }
            }
            // R56.1.g.2 §5.38 §5.22 — preedit (IME composition) write
            // path. `Null` cancels the active composition (mirror of
            // a no-data `compositionend`). `Text(s)` auto-starts the
            // composition if not active then sets the preedit to `s`
            // (the AI-client-as-platform-IME use case — driving the
            // substrate directly without a paired `composition`
            // invoke for lifecycle coordination). The SCXML state
            // stays unchanged by intervene (no `BeginEdit` /
            // `CancelEdit` drive); applications that need the SCXML
            // transition use the `composition` invoke surface
            // instead.
            "preedit" => {
                let Some(state) = self.text_state() else {
                    return Err(InterveneError::ReadOnly);
                };
                match value {
                    IntrospectValue::Null => {
                        state.preedit_cancel();
                        Ok(())
                    }
                    IntrospectValue::Text(s) => {
                        // Idempotent — no-op if already composing.
                        state.preedit_start();
                        state.preedit_update(&s);
                        Ok(())
                    }
                    _ => Err(InterveneError::TypeMismatch),
                }
            }
            // R903 §5.22 — find &amp; replace write surface. Pure setters (they
            // never move the caret — `find-next` navigates), so each is the
            // exact write peer of its `query` read.
            "find_query" => {
                let Some(state) = self.text_state() else {
                    return Err(InterveneError::ReadOnly);
                };
                let IntrospectValue::Text(s) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.set_find_query(&s);
                Ok(())
            }
            "find_case_sensitive" => {
                let Some(state) = self.text_state() else {
                    return Err(InterveneError::ReadOnly);
                };
                let IntrospectValue::Bool(b) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.set_find_case_sensitive(b);
                Ok(())
            }
            "find_whole_word" => {
                let Some(state) = self.text_state() else {
                    return Err(InterveneError::ReadOnly);
                };
                let IntrospectValue::Bool(b) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.set_find_whole_word(b);
                Ok(())
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "send" => match args {
                IntrospectValue::Text(ref name) => self.invoke_send(name),
                _ => Err(InvokeError::TypeMismatch),
            },
            // R56.1.d §5.38 §5.22 — W3C UI Events keystroke dispatch.
            // Returns `Bool(true)` if the key was recognized (the
            // visible mutation may be a caret-edge no-op — see the
            // [`apply_key`] doc). Returns `Bool(false)` for
            // unrecognized keys and for the bare-`TextField`
            // (no [`TextEditState`] attached) path, so the AI client
            // distinguishes "key rejected" from "widget not bound to
            // reactive state". `TypeMismatch` on non-Text args mirrors
            // the `send` invoke discipline.
            //
            // R56.1.j §5.38 §5.28 — recognized keys reset the attached
            // [`CaretBlink`] (snap the caret to fully-visible + restart
            // the period timer). Matches the macOS / iOS / GTK / Web
            // canonical UX — the caret stays solid while the user is
            // typing or navigating, then resumes blinking once the
            // user pauses. Unrecognized keys do not reset (the user
            // did not interact with the field). Bare `TextField`s
            // (no attached blink) silently no-op via
            // [`Option::map`].
            //
            // R56.1.f.0 §5.13 — two args shapes for backward + modifier-
            // aware dispatch:
            //
            // - [`IntrospectValue::Text`]`(key)`: no-modifier dispatch
            //   (W3C `KeyboardEvent.key` with empty modifier surface).
            //   Backward-compat with the R56.1.d wire-shape.
            // - [`IntrospectValue::Json`]`({"key": ..., "shift": ..., ...})`:
            //   modifier-aware dispatch. Each `"shift"` / `"ctrl"` /
            //   `"alt"` / `"meta"` field is an optional bool (`false`
            //   by default). The W3C `KeyboardEvent` shape mirrored
            //   verbatim so RPC clients structured as W3C event
            //   serialisers route through one call site.
            //
            // Other variants → `TypeMismatch` per the consistent
            // arg-shape rejection discipline.
            "key" => match args {
                IntrospectValue::Text(ref key_str) => Ok(IntrospectValue::Bool(
                    self.dispatch_key(key_str, crate::input::Modifiers::empty()),
                )),
                IntrospectValue::Json(ref obj) => match parse_key_invoke_json(obj) {
                    Some((key_str, modifiers)) => Ok(IntrospectValue::Bool(
                        self.dispatch_key(&key_str, modifiers),
                    )),
                    None => Err(InvokeError::TypeMismatch),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
            // R56.1.g.2 §5.38 §5.22 — W3C `CompositionEvent` dispatch
            // surface. The Json action vocabulary mirrors the W3C
            // event types:
            //
            // ```json
            // {"action": "start"}                              // compositionstart
            // {"action": "update", "data": "preedit_string"}   // compositionupdate
            // {"action": "end",    "data": "committed_string"} // compositionend
            // {"action": "cancel"}                             // cancel
            // ```
            //
            // The `data` field is required for `update` and `end`;
            // missing or wrong-typed slots return
            // [`InvokeError::TypeMismatch`]. Unrecognized actions
            // return [`InvokeError::Rejected`]. Returns the
            // post-dispatch SCXML state name (mirror of the
            // `send` invoke return shape).
            "composition" => match args {
                IntrospectValue::Json(ref obj) => match parse_composition_invoke_json(obj) {
                    Some(CompositionAction::Start) => {
                        self.apply_composition_start();
                        Ok(IntrospectValue::Text(self.state().as_name().to_string()))
                    }
                    Some(CompositionAction::Update(data)) => {
                        self.apply_composition_update(&data);
                        Ok(IntrospectValue::Text(self.state().as_name().to_string()))
                    }
                    Some(CompositionAction::End(data)) => {
                        self.apply_composition_commit(&data);
                        Ok(IntrospectValue::Text(self.state().as_name().to_string()))
                    }
                    Some(CompositionAction::Cancel) => {
                        self.apply_composition_cancel();
                        Ok(IntrospectValue::Text(self.state().as_name().to_string()))
                    }
                    None => Err(InvokeError::TypeMismatch),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
            // R56.2.e §5.22 — middle-click / RPC PRIMARY paste.
            // `Null` triggers `paste_from_primary` (reads PRIMARY,
            // inserts at caret, resets blink); returns `Bool(true)`
            // when a non-empty payload was inserted, `Bool(false)`
            // when PRIMARY is empty / unavailable / no state /
            // no clipboard attached. Any non-Null arg returns
            // `TypeMismatch` (defensive against a future-arg-shape
            // wire-form drift). Mirrors the shell's middle-click
            // path (R56.2.e.3): both call paths funnel through this
            // invoke slot so the substrate has one source of truth
            // for the PRIMARY paste action.
            "paste-primary" => match args {
                IntrospectValue::Null => Ok(IntrospectValue::Bool(self.paste_from_primary())),
                _ => Err(InvokeError::TypeMismatch),
            },
            // R768 §5.36 §5.22 / R770.1 — apply one styled run over a byte
            // range (`setCharFormat`). The Json wire shape carries
            // `{"start", "end"}` plus the style in one of two forms:
            //
            // - **`"style"` object** (R770.1, canonical) — the *exact*
            //   shape a run's read emits (`scene/snapshot`), decoded by
            //   [`json_to_text_style`]. Round-trippable: an AI reads a
            //   run's full style, mutates any field (bold / italic / size
            //   / colour / decoration), and writes it back. This is what
            //   makes every character format — not just colour —
            //   RPC-settable, the AI-first peer of the toolbar.
            // - **`"fg"` (+ optional `"size"`) shorthand** (R768) — the
            //   common recolour; `"fg"` is a CSS colour
            //   ([`Color::from_hex`]), other fields default.
            //
            // Returns `Bool(true)` when the range was styled, `Bool(false)`
            // when no [`TextEditState`] is attached; `TypeMismatch` on a
            // non-Json arg or an unparseable shorthand payload (mirror of
            // the `key` / `composition` arg-shape discipline).
            //
            // [`Color::from_hex`]: crate::style::Color::from_hex
            // R768 / R967 §5.36 — the styled-run verbs (apply-style wholesale
            // setCharFormat / clear-style clearFormat / toggle-format
            // mergeCharFormat), delegated to keep `invoke` under the
            // `too_many_lines` budget (the R959 `invoke_send` / `invoke_mark`
            // extraction precedent).
            "apply-style" | "clear-style" | "toggle-format" => self.invoke_style(path, &args),
            // R951 §5.36 §5.22 — active typing marks (collapsed-caret formatting,
            // ProseMirror storedMarks). `mark` arms it (so the next typed text is
            // styled), `clear-mark` drops it; split out (SRP, line ceiling) like
            // the find / fold / indent helpers.
            "mark" | "clear-mark" => self.invoke_mark(path, &args),
            // R903 §5.22 — find &amp; replace actions live in a dedicated helper
            // (SRP: keeps the invoke dispatch under the line ceiling).
            "find-next" | "find-prev" | "replace" | "replace-all" => {
                self.invoke_find_replace(path, &args)
            }
            // R933 §5.36 — code-folding actions (split out like find/replace
            // to keep this dispatch under the line ceiling).
            "toggle-fold" | "fold-all" | "unfold-all" => self.invoke_fold(path, &args),
            // R938 §5.22 — indent / dedent the selection (the AI-first twin of
            // the Tab / Shift+Tab keyboard path; split out for SRP).
            "indent" | "dedent" => self.invoke_indent(path, &args),
            // R939 §5.22 — toggle line comments (the AI-first twin of Ctrl+/).
            "toggle-comment" => self.invoke_comment(&args),
            "go-to-line" => self.invoke_go_to_line(&args),
            // R945 §5.22 — move / duplicate the current line block (the AI-first
            // twins of Alt+Up / Alt+Down + Shift+Alt copy).
            "move-line-up" | "move-line-down" | "duplicate-line-up" | "duplicate-line-down" => {
                self.invoke_line_move(path, &args)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

impl TextFieldExternal {
    /// R768 / R967 §5.36 — the styled-run `invoke` verbs, split out of
    /// [`invoke`](ExternalIntrospect::invoke) for SRP + the `too_many_lines`
    /// ceiling (the find / fold / mark precedent): `apply-style` overlays one run
    /// wholesale (setCharFormat), `clear-style` strips a range (clearFormat), and
    /// `toggle-format` flips ONE field preserving the run's others
    /// (mergeCharFormat — the AI-first peer of the toolbar B / I toggle, routed
    /// through the shared [`TextEditState::toggle_format`] SSOT, so the human + AI
    /// channels flip the field identically over already-styled bytes). Each
    /// returns `Bool` (setter-returns-outcome): the styled / cleared flag, or for
    /// `toggle-format` the new on-state. `TypeMismatch` on a malformed arg.
    fn invoke_style(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "apply-style" => match args {
                IntrospectValue::Json(obj) => match parse_apply_style_json(obj) {
                    Some((start, end, style)) => {
                        Ok(IntrospectValue::Bool(self.text_state().is_some_and(|s| {
                            s.apply_style_run(start, end, style);
                            true
                        })))
                    }
                    None => Err(InvokeError::TypeMismatch),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
            "clear-style" => match args {
                IntrospectValue::Json(obj) => match parse_clear_style_json(obj) {
                    Some((start, end)) => {
                        Ok(IntrospectValue::Bool(self.text_state().is_some_and(|s| {
                            s.clear_style_runs(start, end);
                            true
                        })))
                    }
                    None => Err(InvokeError::TypeMismatch),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
            // R967 — toggle ONE field, preserving the run's others. The base is
            // only consumed for bytes with NO existing run; an already-styled
            // byte ignores it (its run resolves the toggle). The generic headless
            // field has no theme handle (the live theme is view-layer + animated),
            // so the base is the caret's effective style, falling back to
            // `TextStyle::default()`. Caveat: for a selection spanning UNSTYLED
            // gap bytes, those bytes materialise against this default base, NOT
            // the toolbar's theme ink — the one place the AI + human channels'
            // OUTPUT can differ (the flip itself is identical). The AI uses
            // `apply-style` when it needs an explicit colour over plain text.
            "toggle-format" => match args {
                IntrospectValue::Text(token) => match FormatField::from_wire(token) {
                    Some(field) => Ok(IntrospectValue::Bool(self.text_state().is_some_and(|s| {
                        let base = s.style_at_caret().unwrap_or_default();
                        s.toggle_format(field, &base)
                    }))),
                    None => Err(InvokeError::TypeMismatch),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }

    /// R903 §5.22 §5.52 — the find &amp; replace `invoke` actions, split out of
    /// [`invoke`](ExternalIntrospect::invoke) for SRP (and to keep that
    /// dispatch under the line ceiling). `path` is one of `find-next` /
    /// `find-prev` / `replace` / `replace-all`; the navigation actions take
    /// `Null` and return the new selection (or `Null`), `replace` takes the
    /// `Text` replacement and returns `Bool`, `replace-all` takes the `Text`
    /// replacement and returns the `Int` count. A bare field (no state) is
    /// inert — `Null` / `Bool(false)` / `Int(0)` — never `UnknownPath`.
    /// R951 §5.36 §5.22 — the active-typing-mark `invoke` actions, split out of
    /// [`invoke`](ExternalIntrospect::invoke) for SRP (and to keep that dispatch
    /// under the line ceiling, like the find / fold / indent helpers). `mark`
    /// (Json = the `style_at_caret` read shape — a bare full-style object or
    /// `{"style": {...}}`) arms the mark so the next typed text is styled; an
    /// agent reads `style_at_caret`, mutates a field, and writes it back.
    /// `clear-mark` (`Null`) drops it. Both return `Bool(true)` when a
    /// [`TextEditState`] is attached, `Bool(false)` otherwise; `TypeMismatch` on
    /// a malformed arg. `apply-style` stays the selection path; this is the
    /// collapsed-caret typing attribute.
    fn invoke_mark(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "mark" => match args {
                IntrospectValue::Json(obj) => match parse_mark_style_json(obj) {
                    Some(style) => Ok(IntrospectValue::Bool(self.text_state().is_some_and(|s| {
                        s.set_pending_style(Some(style));
                        true
                    }))),
                    None => Err(InvokeError::TypeMismatch),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
            "clear-mark" => match args {
                IntrospectValue::Null => {
                    Ok(IntrospectValue::Bool(self.text_state().is_some_and(|s| {
                        s.set_pending_style(None);
                        true
                    })))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }

    fn invoke_find_replace(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // No args (`Null`, mirror of `paste-primary`); returns the newly
            // selected match as `{"start", "end"}` (the `selection` query shape)
            // or `Null` when there are no matches — the read outcome of the move.
            "find-next" => match args {
                IntrospectValue::Null => Ok(selection_range_to_value(
                    self.text_state().and_then(|s| s.find_next()),
                )),
                _ => Err(InvokeError::TypeMismatch),
            },
            "find-prev" => match args {
                IntrospectValue::Null => Ok(selection_range_to_value(
                    self.text_state().and_then(|s| s.find_prev()),
                )),
                _ => Err(InvokeError::TypeMismatch),
            },
            // Replace the current match (when the selection is on one) with the
            // `Text` replacement and advance; `Bool(true)` when a replacement
            // happened, `Bool(false)` when it only selected the next match (or
            // no state is attached). The empty replacement deletes.
            "replace" => match args {
                IntrospectValue::Text(replacement) => Ok(IntrospectValue::Bool(
                    self.text_state()
                        .is_some_and(|s| s.replace_current(replacement)),
                )),
                _ => Err(InvokeError::TypeMismatch),
            },
            // Replace every match with the `Text` replacement as one undo step;
            // returns the count replaced.
            "replace-all" => match args {
                IntrospectValue::Text(replacement) => {
                    Ok(IntrospectValue::Int(self.text_state().map_or(0, |s| {
                        i64::try_from(s.replace_all(replacement)).unwrap_or(i64::MAX)
                    })))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Unreachable: the caller only routes the four paths above here.
            _ => Err(InvokeError::UnknownPath),
        }
    }

    /// R933 §5.36 — the code-folding `invoke` actions, split out of
    /// [`invoke`](ExternalIntrospect::invoke) for SRP (mirror of
    /// [`invoke_find_replace`](Self::invoke_find_replace)). `toggle-fold`
    /// takes the opener's logical line as an `Int` and returns `Bool`
    /// (whether a region toggled); `fold-all` / `unfold-all` take `Null`
    /// and return the resulting collapsed-region count as an `Int` — the
    /// setter-returns-read-outcome contract (the wire reflects the
    /// substrate state *after* the action). A bare field (no state) is
    /// inert: `Bool(false)` / `Int(0)`, never `UnknownPath`.
    fn invoke_fold(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Arg = the opener's logical line. A negative line cannot name a
            // row → `Rejected` (the `usize::try_from` failure path), mirror
            // of the `caret` intervene's range guard.
            "toggle-fold" => match args {
                IntrospectValue::Int(line) => {
                    let l = usize::try_from(*line).map_err(|_| InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(
                        self.text_state().is_some_and(|s| s.toggle_fold(l)),
                    ))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // `Null` arg (mirror of `paste-primary`); returns the collapsed
            // count after the bulk fold / unfold.
            "fold-all" | "unfold-all" => match args {
                IntrospectValue::Null => {
                    Ok(IntrospectValue::Int(self.text_state().map_or(0, |s| {
                        if path == "fold-all" {
                            s.fold_all();
                        } else {
                            s.unfold_all();
                        }
                        i64::try_from(s.fold_regions().iter().filter(|r| r.collapsed).count())
                            .unwrap_or(i64::MAX)
                    })))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Unreachable: the caller only routes the three paths above.
            _ => Err(InvokeError::UnknownPath),
        }
    }

    /// R938 §5.22 — the indent / dedent `invoke` actions, the AI-first twin of
    /// the `Tab` / `Shift+Tab` keyboard path (split out like
    /// [`invoke_fold`](Self::invoke_fold) for SRP). Both take `Null` and
    /// return `Bool` — whether the buffer changed (a dedent on lines with no
    /// leading whitespace is a `Bool(false)` no-op, the setter-returns-read-
    /// outcome contract). The width is the shared
    /// [`INDENT_UNIT`](crate::widgets::text_edit::INDENT_UNIT) default the
    /// keyboard path also uses, so an AI-driven indent and a `Tab` press land
    /// the same edit. A bare field (no state) is inert — `Bool(false)`, never
    /// `UnknownPath`.
    fn invoke_indent(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match args {
            IntrospectValue::Null => {
                Ok(IntrospectValue::Bool(self.text_state().is_some_and(|s| {
                    if path == "indent" {
                        s.indent_selection(crate::widgets::text_edit::INDENT_UNIT)
                    } else {
                        s.dedent_selection(crate::widgets::text_edit::INDENT_WIDTH)
                    }
                })))
            }
            _ => Err(InvokeError::TypeMismatch),
        }
    }

    /// R939 §5.22 — the `toggle-comment` `invoke` action, the AI-first twin of
    /// the `Ctrl+/` keyboard path. Takes `Null` and returns `Bool` — whether
    /// the buffer changed (a toggle over only blank lines is a `Bool(false)`
    /// no-op, the setter-returns-read-outcome contract). The marker comes from
    /// the field's configured [`line_comment`](crate::widgets::text_edit::TextEditState::line_comment),
    /// the same source the keyboard path reads, so an AI-driven toggle and a
    /// `Ctrl+/` press land the same edit. A bare field — no state, or a field
    /// that never called `set_line_comment` — is inert (`Bool(false)`), never
    /// `UnknownPath`.
    fn invoke_comment(&mut self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match args {
            IntrospectValue::Null => {
                Ok(IntrospectValue::Bool(self.text_state().is_some_and(|s| {
                    s.line_comment().is_some_and(|m| s.toggle_line_comment(m))
                })))
            }
            _ => Err(InvokeError::TypeMismatch),
        }
    }

    /// R941 §5.22 — the `go-to-line` action: move the caret to the start of
    /// 1-based logical line `args` (an [`IntrospectValue::Int`]), collapsing any
    /// selection — the AI-first peer of an editor's `Ctrl+G` prompt. Returns the
    /// resolved 1-based line the caret landed on (clamped to `1..=line_count` by
    /// [`TextEditState::go_to_line`](crate::widgets::text_edit::TextEditState::go_to_line)),
    /// so the caller learns the actual destination in one round-trip
    /// (setter-returns-the-read). A negative line cannot name a row → `Rejected`
    /// (the `usize::try_from` failure path, mirror of the `toggle-fold` guard); a
    /// bare `TextField` with no attached state returns `Int(0)` (nothing to
    /// navigate), distinguishing it from a real line-1 landing.
    fn invoke_go_to_line(
        &mut self,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match args {
            IntrospectValue::Int(line) => {
                let l = usize::try_from(*line).map_err(|_| InvokeError::Rejected)?;
                let resolved = self.text_state().map_or(0, |s| s.go_to_line(l));
                Ok(IntrospectValue::Int(
                    i64::try_from(resolved).unwrap_or(i64::MAX),
                ))
            }
            _ => Err(InvokeError::TypeMismatch),
        }
    }

    /// R56.1.a / R959 / R961 §5.38 §5.36 — the `invoke("send", Text(...))`
    /// channel. A bare event name (`"Focus"` / `"Blur"` / …) dispatches the
    /// matching SCXML [`TextFieldEvent`]; a composite sub-target
    /// `"<key>:PointerUp"` is a gutter click decoded through
    /// [`TextFieldSendKey`] — a line-number ([`send_gutter_line`](Self::send_gutter_line))
    /// or a fold-chevron ([`toggle_fold_at_line`](Self::toggle_fold_at_line)).
    /// Returns the post-send state name (the established `send` read-outcome
    /// contract).
    fn invoke_send(&mut self, name: &str) -> Result<IntrospectValue, InvokeError> {
        // R959 / R961 — a gutter affordance routes its clicks to the field's
        // own `send` wire as `"<key>:PointerUp[:mods]"`. Decode through the `:`
        // grammar SSOT, then the `TextFieldSendKey` SSOT, and act once, on the
        // activation edge: a line-number click jumps / Shift-extends to that
        // line, a fold chevron toggles the region opening there. The
        // `PointerDown` / `PointerEnter` / `PointerLeave` edges the router fires
        // around the click are a recognized no-op. Decoded before the bare-event
        // path so the sub never reaches `from_name` (which would reject it).
        if let Some((sub, event, mods)) = split_send_payload(name) {
            if let Some(key) = TextFieldSendKey::parse(sub) {
                if is_activation_event(event) {
                    match key {
                        TextFieldSendKey::GutterLine { line } => {
                            self.send_gutter_line(line, mods.shift_key());
                        }
                        TextFieldSendKey::FoldToggle { line } => self.toggle_fold_at_line(line),
                    }
                }
                return Ok(IntrospectValue::Text(self.state().as_name().to_string()));
            }
        }
        let ev = TextFieldEvent::from_name(name).ok_or(InvokeError::Rejected)?;
        self.send(ev);
        Ok(IntrospectValue::Text(self.state().as_name().to_string()))
    }

    /// R959 §5.36 §5.22 — apply a gutter line-number click (the `send`-wire
    /// peer of [`invoke_go_to_line`](Self::invoke_go_to_line), decoded from
    /// the `"gl<n>:PointerUp"` sub-target): move the caret to the start of
    /// 1-based logical `line` ([`TextEditState::go_to_line`](crate::widgets::text_edit::TextEditState::go_to_line)),
    /// or — when `extend` (a `Shift`+click) — extend the selection from the
    /// live anchor to that line's start
    /// ([`line_start_byte`](crate::widgets::text_edit::TextEditState::line_start_byte),
    /// the pure-positioning peer that does not collapse the selection the way
    /// `go_to_line` would). Inert on a bare field (no attached
    /// [`TextEditState`]).
    fn send_gutter_line(&self, line: usize, extend: bool) {
        let Some(state) = self.text_state() else {
            return;
        };
        if extend {
            let anchor = state.selection_anchor().unwrap_or_else(|| state.caret());
            state.set_selection(anchor, state.line_start_byte(line));
        } else {
            state.go_to_line(line);
        }
    }

    /// R961 §5.36 §5.22 — toggle the fold of the region opening on 0-based
    /// `line` (the `send`-wire peer of the `toggle-fold` invoke + the keyboard
    /// `Enter` path, decoded from a `"fold<n>:PointerUp"` chevron click). The
    /// same [`TextEditState::toggle_fold`](crate::widgets::text_edit::TextEditState::toggle_fold)
    /// SSOT — a collapse reanchors the caret out of the hidden interior. Inert
    /// on a bare field (no attached [`TextEditState`]).
    fn toggle_fold_at_line(&self, line: usize) {
        if let Some(state) = self.text_state() {
            state.toggle_fold(line);
        }
    }

    /// R945 §5.22 — the line-manipulation `invoke` actions, the AI-first twins
    /// of the editor's move-line (`Alt+Up` / `Alt+Down`) and copy-line
    /// (`Shift+Alt+Up` / `Shift+Alt+Down`) chords (split out like
    /// [`invoke_indent`](Self::invoke_indent) for SRP). All take `Null` and
    /// return `Bool` — whether the buffer changed (a boundary move — the first
    /// line up or the last line down — is a `Bool(false)` no-op, the
    /// setter-returns-read-outcome contract; a duplicate always inserts, so it
    /// is `Bool(true)`). A bare field (no state) is inert — `Bool(false)`,
    /// never `UnknownPath`.
    fn invoke_line_move(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match args {
            IntrospectValue::Null => {
                Ok(IntrospectValue::Bool(self.text_state().is_some_and(|s| {
                    match path {
                        "move-line-up" => s.move_lines(false),
                        "move-line-down" => s.move_lines(true),
                        "duplicate-line-up" => s.duplicate_lines(false),
                        "duplicate-line-down" => s.duplicate_lines(true),
                        // Unreachable: the caller routes only the four paths above.
                        _ => false,
                    }
                })))
            }
            _ => Err(InvokeError::TypeMismatch),
        }
    }

    /// R56.1.f.0 §5.13 — single dispatch site shared by the
    /// `IntrospectValue::Text` (no-modifier) and `IntrospectValue::Json`
    /// (modifier-aware) arms of `invoke("key", ...)`. Forwards into
    /// the closed-form [`apply_key`] free fn, then resets the attached
    /// [`CaretBlink`] (R56.1.j) on recognized keys so the caret stays
    /// solid while the user is interacting.
    ///
    /// R56.1.e §5.22 — also intercepts Ctrl+C / Ctrl+X / Ctrl+V
    /// when a [`Clipboard`] is attached (mirror of the R56.1.f.2
    /// Ctrl/Cmd+A select-all binding's modifier gate: Ctrl OR Meta,
    /// not Alt). Without an attached clipboard the keystrokes fall
    /// through to the printable-char branch in `apply_key` and are
    /// recognised as plain `c` / `x` / `v` inserts when no modifier
    /// is held; the modifier gate routes only the Ctrl/Cmd-prefixed
    /// chord to the clipboard path so plain typing stays unchanged.
    ///
    /// Returns the `apply_key` recognition result verbatim — the RPC
    /// `Bool(true)` / `Bool(false)` payload AI clients gate
    /// `defaultPrevented`-style branching on.
    fn dispatch_key(&mut self, key_str: &str, modifiers: crate::input::Modifiers) -> bool {
        // R56.1.e §5.22 — clipboard keystroke pre-empt. The Ctrl/Meta
        // gate (without Alt — same AltGr safety as R56.1.f.2
        // select-all) fires only when a clipboard is attached AND a
        // state sidecar exists (no state ⇒ no text to copy / paste
        // into). Returns Bool(true) on consumed clipboard chord, even
        // when the visible mutation is a no-op (Ctrl+C with no
        // selection produces an empty copy — the key was *handled*,
        // matching the W3C `defaultPrevented` discipline).
        if modifiers.command_key() && !modifiers.alt_key() {
            if let (Some(state), Some(cb)) = (self.em.inner.text_state(), self.em.inner.clipboard())
            {
                match key_str {
                    "c" => {
                        if let Some(text) = state.selection_text() {
                            cb.copy(text);
                        }
                        if let Some(blink) = self.em.inner.blink() {
                            blink.reset();
                        }
                        return true;
                    }
                    "x" => {
                        if let Some(text) = state.selection_text() {
                            cb.copy(text);
                            state.backspace();
                        }
                        if let Some(blink) = self.em.inner.blink() {
                            blink.reset();
                        }
                        return true;
                    }
                    "v" => {
                        if let Some(paste) = cb.paste() {
                            if !paste.is_empty() {
                                state.insert(&paste);
                            }
                        }
                        if let Some(blink) = self.em.inner.blink() {
                            blink.reset();
                        }
                        return true;
                    }
                    _ => {}
                }
            }
        }
        let handled = match self.text_state() {
            Some(state) => apply_key(state.as_ref(), key_str, modifiers),
            None => false,
        };
        if handled {
            if let Some(blink) = self.em.inner.blink() {
                blink.reset();
            }
            // R56.2.e §5.22 — auto-publish the active selection to the
            // PRIMARY clipboard after any handled key. X11 / Wayland
            // desktop convention: the *act of selection* implicitly
            // writes to PRIMARY (independent of any Ctrl+C). This
            // hook fires on Ctrl+A select-all, Shift+Arrow extend,
            // and the print-char "drain selection" path — only the
            // first two land non-empty selection_text so the hook is
            // a no-op for plain typing. On non-Linux platforms the
            // `Clipboard::copy_to(Primary, ...)` default impl is a
            // no-op, so this stays free for macOS / Windows
            // applications.
            self.publish_primary_selection_if_any();
        }
        handled
    }

    /// R56.2.e §5.22 — publish the active selection text to the
    /// PRIMARY clipboard if both a [`TextEditState`] and a
    /// [`Clipboard`] are attached AND the selection is non-empty.
    /// No-op otherwise (collapsed caret keeps PRIMARY untouched, per
    /// X11 convention — PRIMARY retains the previous selection until
    /// a new one is published).
    ///
    /// Used by [`Self::dispatch_key`] after any handled key and by
    /// the RPC `intervene("selection", ...)` arm so AI-client-driven
    /// selection writes also reach PRIMARY (the canonical Linux
    /// desktop "select text + middle-click in another app" UX path
    /// remains observable from out-of-process introspection).
    fn publish_primary_selection_if_any(&self) {
        if let (Some(state), Some(cb)) = (self.em.inner.text_state(), self.em.inner.clipboard()) {
            if let Some(text) = state.selection_text() {
                // Empty-string selection_text is impossible (anchor ==
                // focus collapses to `None`), but guard defensively
                // so a future TextEditState change cannot regress
                // into spurious PRIMARY writes.
                if !text.is_empty() {
                    cb.copy_to(ClipboardSelection::Primary, text);
                }
            }
        }
    }

    /// R56.2.e §5.22 — paste the current PRIMARY clipboard payload
    /// at the caret, draining any active selection (matches the
    /// W3C / X11 / GTK middle-click paste UX). Returns `true` when
    /// a non-empty payload was inserted, `false` when no
    /// [`Clipboard`] is attached, no [`TextEditState`] is attached,
    /// PRIMARY is empty / unavailable (default macOS / Windows
    /// behaviour), or the read returned an empty string.
    ///
    /// Called from the shell's middle-mouse-click handler (R56.2.e.3)
    /// after focus routing — the caller is responsible for ensuring
    /// this widget is the focused one. The blink reset matches the
    /// Ctrl+V handler's recognised-keystroke discipline so the
    /// caret stays solid through the paste.
    pub fn paste_from_primary(&mut self) -> bool {
        let (Some(state), Some(cb)) = (self.em.inner.text_state(), self.em.inner.clipboard())
        else {
            return false;
        };
        let Some(text) = cb.paste_from(ClipboardSelection::Primary) else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        state.insert(&text);
        if let Some(blink) = self.em.inner.blink() {
            blink.reset();
        }
        true
    }
}

/// R56.1.f.0 §5.13 — extract the `(key, modifiers)` payload from a
/// JSON object argument to `invoke("key", ...)`. The wire shape
/// mirrors the W3C `KeyboardEvent` modifier surface so RPC clients
/// constructed as direct W3C event serialisers route through one
/// call site:
///
/// ```json
/// {
///   "key":   "ArrowLeft",   // required, W3C KeyboardEvent.key
///   "shift": true,          // optional, defaults to false
///   "ctrl":  false,         // optional, defaults to false
///   "alt":   false,         // optional, defaults to false
///   "meta":  false          // optional, defaults to false
/// }
/// ```
///
/// Returns `None` if the JSON is not an object, if `"key"` is missing
/// or not a string, or if any modifier field is present but not a
/// boolean (defensive against silently coercing `0`/`1`/`"true"` —
/// the W3C shape is strictly boolean and the RPC discipline mirrors
/// that strictly to surface client-side encoder bugs early).
/// R56.1.f.3 §5.38 §5.22 — extract the `(start, end)` payload from
/// the Json argument to `intervene("selection", ...)`. The wire
/// shape mirrors the W3C `HTMLInputElement.selectionStart` /
/// `selectionEnd` pair:
///
/// ```json
/// {
///   "start": 0,    // required, non-negative integer
///   "end":   5     // required, non-negative integer
/// }
/// ```
///
/// Returns `None` if the JSON is not an object, if either slot is
/// missing / non-integer / negative, or if the integer falls outside
/// `usize` range (the unreachable 2^64-overflow guard).
fn parse_selection_intervene_json(value: &serde_json::Value) -> Option<(usize, usize)> {
    parse_byte_range_json(value.as_object()?)
}

/// R903 §5.22 — encode an optional `(start, end)` selection range as the
/// canonical `{"start", "end"}` Json (the same shape the `selection` query
/// emits) or `Null` when there is no range. The read-outcome return of
/// `invoke("find-next" / "find-prev", ...)`.
fn selection_range_to_value(range: Option<(usize, usize)>) -> IntrospectValue {
    match range {
        Some((start, end)) => IntrospectValue::Json(serde_json::json!({
            "start": start,
            "end":   end,
        })),
        None => IntrospectValue::Null,
    }
}

/// R768 §5.36 §5.22 — shared `{"start", "end"}` non-negative-integer
/// byte-range extraction. The `selection` intervene, `apply-style`, and
/// `clear-style` invoke wire shapes all carry the same integer pair
/// (mirror of W3C `selectionStart` / `selectionEnd`), so the decode
/// lives in one place — a slot rename can't silently desync the three
/// call sites ([[snapshot-vs-drain-api-duality]] decode-SSOT). Returns
/// `None` if either slot is missing / non-integer / negative / outside
/// `usize` (the unreachable 2^64-overflow guard).
fn parse_byte_range_json(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Option<(usize, usize)> {
    let start_i64 = obj.get("start")?.as_i64()?;
    let end_i64 = obj.get("end")?.as_i64()?;
    if start_i64 < 0 || end_i64 < 0 {
        return None;
    }
    Some((
        usize::try_from(start_i64).ok()?,
        usize::try_from(end_i64).ok()?,
    ))
}

/// R768 §5.36 §5.22 — extract the `(start, end, style)` payload from the
/// `apply-style` invoke arg. Wire shape:
///
/// ```json
/// {
///   "start": 0,          // required, non-negative integer
///   "end":   5,          // required, non-negative integer
///   "fg":    "#d02828",  // required, CSS colour (Color::from_hex)
///   "size":  16          // optional, run font size in px (default 16)
/// }
/// ```
///
/// Two style forms are accepted:
///
/// - **Canonical `"style"` object** — the *exact* shape a run's read
///   emits (`style_run_to_json` / `text_style_to_json` in pinion-rpc), so
///   an AI round-trips a run: read its full style via `scene/snapshot`,
///   mutate any character-format field (e.g. set `font_weight` to bold),
///   write it back. This is what makes bold / italic / size / decoration
///   RPC-settable, and keeps the read/write a decode-mirrors-encode pair
///   ([`json_to_text_style`]). Wholesale (`setCharFormat`) — fields the
///   object omits fall back to the `TextStyle::new()` default.
/// - **`fg` (+ optional `size`) shorthand** — the common recolour case
///   (R768), kept for ergonomics. `fg` is a CSS colour ([`Color::from_hex`]).
///
/// Returns `None` if the range is malformed or (shorthand path) `"fg"` is
/// missing / unparseable / `"size"` is not a `u32`-range integer.
fn parse_apply_style_json(value: &serde_json::Value) -> Option<(usize, usize, TextStyle)> {
    let obj = value.as_object()?;
    let (start, end) = parse_byte_range_json(obj)?;
    if let Some(style_obj) = obj.get("style").and_then(serde_json::Value::as_object) {
        return Some((start, end, json_to_text_style(style_obj)));
    }
    let fg = Color::from_hex(obj.get("fg")?.as_str()?)?;
    let mut style = TextStyle::new().with_fg(fg);
    if let Some(size_v) = obj.get("size") {
        style = style.with_size_px(u32::try_from(size_v.as_u64()?).ok()?);
    }
    Some((start, end, style))
}

/// R951 §5.36 §5.22 — decode the `mark` invoke arg into the [`TextStyle`] to
/// arm at the collapsed caret. The wire shape is the `style_at_caret` read
/// shape: either a bare full-style object (the round-trip — read, mutate a
/// field, write it back) or `{"style": {...}}` (the same wrapped form
/// [`parse_apply_style_json`] accepts). Unlike `apply-style` it carries no
/// byte range (the mark applies to the *next* insert, not an existing span),
/// so there is nothing to clamp. Any object decodes (missing fields default
/// via [`json_to_text_style`], a wholesale set-with-defaults like
/// [`TextEditState::set_pending_style`](crate::widgets::text_edit::TextEditState::set_pending_style)
/// expects); a non-object arg is `None` (→ `TypeMismatch`).
fn parse_mark_style_json(value: &serde_json::Value) -> Option<TextStyle> {
    let obj = value.as_object()?;
    let style_obj = obj
        .get("style")
        .and_then(serde_json::Value::as_object)
        .unwrap_or(obj);
    Some(json_to_text_style(style_obj))
}

/// R951 §5.36 §5.22 — encode an optional [`TextStyle`] for a query read: the
/// full [`TextStyle`] serde object (the `apply-style` `style` shape, round-trippable
/// via [`json_to_text_style`]) or `Null` for the field base. The shared encode
/// of the `style_at_caret` + `pending_style` reads, so the two never drift.
fn style_to_value(style: Option<TextStyle>) -> IntrospectValue {
    match style {
        Some(style) => {
            IntrospectValue::Json(serde_json::to_value(style).unwrap_or(serde_json::Value::Null))
        }
        None => IntrospectValue::Null,
    }
}

/// R770.1 §5.36 §5.49 — decode a wire `style` object back into a
/// [`TextStyle`]. The exact inverse of `text_style_to_json` (pinion-rpc),
/// so a run round-trips: snapshot → mutate → `apply-style`. Each field is
/// applied onto a `TextStyle::new()` base; a missing / unparseable field
/// keeps the default (a partial object is a wholesale set-with-defaults,
/// matching [`TextEditState::apply_style_run`]'s `setCharFormat`
/// semantics). The encode lives in pinion-rpc; a round-trip test there
/// pins the pair in sync (the R615 `from_hex`/`to_hex` precedent —
/// decode is the inverse of encode, drift-guarded by a test).
#[must_use]
pub fn json_to_text_style(obj: &serde_json::Map<String, serde_json::Value>) -> TextStyle {
    let mut s = TextStyle::new();
    if let Some(v) = obj.get("font_family") {
        // Untyped wire string → typed family (R1002): a CSS generic keyword
        // classifies to `Generic`, anything else to `Named`.
        s.font_family = v
            .as_str()
            .map(|f| crate::style::FontFamily::parse_css(f.to_string()));
    }
    if let Some(px) = obj.get("font_size_px").and_then(serde_json::Value::as_u64) {
        if let Ok(px) = u32::try_from(px) {
            s.font_size_px = px;
        }
    }
    if let Some(c) = obj.get("fg_color").and_then(json_to_color) {
        s.fg_color = c;
    }
    if let Some(w) = obj.get("font_weight").and_then(serde_json::Value::as_u64) {
        if let Ok(w) = u16::try_from(w) {
            s.font_weight = FontWeight(w);
        }
    }
    if let Some(fs) = obj.get("font_style").and_then(json_to_font_style) {
        s.font_style = fs;
    }
    if let Some(lh) = obj.get("line_height").and_then(json_to_line_height) {
        s.line_height = lh;
    }
    if let Some(ls) = obj
        .get("letter_spacing")
        .and_then(serde_json::Value::as_i64)
    {
        if let Ok(ls) = i32::try_from(ls) {
            s.letter_spacing = ls;
        }
    }
    if let Some(ta) = obj.get("text_align").and_then(serde_json::Value::as_str) {
        s.text_align = match ta {
            "Center" => TextAlign::Center,
            "End" => TextAlign::End,
            "Justify" => TextAlign::Justify,
            _ => TextAlign::Start,
        };
    }
    if let Some(d) = obj.get("decoration").and_then(serde_json::Value::as_object) {
        s.decoration = TextDecoration {
            underline: d
                .get("underline")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            strikethrough: d
                .get("strikethrough")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        };
    }
    if let Some(o) = obj.get("overflow").and_then(serde_json::Value::as_str) {
        s.overflow = match o {
            "Clip" => TextOverflow::Clip,
            "Ellipsis" => TextOverflow::Ellipsis,
            _ => TextOverflow::Visible,
        };
    }
    s
}

/// Decode the `{r, g, b, a}` colour object `color_to_json` emits.
fn json_to_color(v: &serde_json::Value) -> Option<Color> {
    let o = v.as_object()?;
    let ch = |k: &str| {
        o.get(k)
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| u8::try_from(n).ok())
    };
    Some(Color::rgba(
        ch("r")?,
        ch("g")?,
        ch("b")?,
        ch("a").unwrap_or(0xff),
    ))
}

/// Decode `font_style_to_json`: `"Normal"` / `"Italic"` /
/// `{kind:"Oblique", angle: int|null}`.
fn json_to_font_style(v: &serde_json::Value) -> Option<FontStyle> {
    match v {
        serde_json::Value::String(s) => match s.as_str() {
            "Normal" => Some(FontStyle::Normal),
            "Italic" => Some(FontStyle::Italic),
            _ => None,
        },
        serde_json::Value::Object(o)
            if o.get("kind").and_then(serde_json::Value::as_str) == Some("Oblique") =>
        {
            let angle = match o.get("angle") {
                Some(serde_json::Value::Null) | None => None,
                Some(a) => Some(i16::try_from(a.as_i64()?).ok()?),
            };
            Some(FontStyle::Oblique(angle))
        }
        _ => None,
    }
}

/// Decode `line_height_to_json`: `"Normal"` / `{kind:"Px", value}` /
/// `{kind:"MultiplierX100", value}`.
fn json_to_line_height(v: &serde_json::Value) -> Option<LineHeight> {
    match v {
        serde_json::Value::String(s) if s == "Normal" => Some(LineHeight::Normal),
        serde_json::Value::Object(o) => {
            let value = o.get("value").and_then(serde_json::Value::as_u64)?;
            match o.get("kind").and_then(serde_json::Value::as_str)? {
                "Px" => Some(LineHeight::Px(u32::try_from(value).ok()?)),
                "MultiplierX100" => Some(LineHeight::MultiplierX100(u16::try_from(value).ok()?)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// R768 §5.36 §5.22 — extract the `(start, end)` range from the
/// `clear-style` invoke arg (the `{"start", "end"}` shape shared via
/// [`parse_byte_range_json`]).
fn parse_clear_style_json(value: &serde_json::Value) -> Option<(usize, usize)> {
    parse_byte_range_json(value.as_object()?)
}

fn parse_key_invoke_json(value: &serde_json::Value) -> Option<(String, crate::input::Modifiers)> {
    let obj = value.as_object()?;
    let key_str = obj.get("key")?.as_str()?.to_string();
    let modifier_bit = |name: &str| -> Option<bool> {
        match obj.get(name) {
            None => Some(false),
            Some(v) => v.as_bool(),
        }
    };
    let modifiers = crate::input::Modifiers {
        shift: modifier_bit("shift")?,
        ctrl: modifier_bit("ctrl")?,
        alt: modifier_bit("alt")?,
        meta: modifier_bit("meta")?,
    };
    Some((key_str, modifiers))
}

/// R56.1.g.2 §5.38 §5.22 — parsed action surface for the
/// `composition` invoke arm. Mirrors the W3C `CompositionEvent`
/// vocabulary: `start` / `update` / `end` carry a corresponding
/// platform IME event; `cancel` is the platform-driven discard
/// (Escape during composition, Wayland IME cancel). The variant
/// carries the `data` payload inline so the dispatch site reads it
/// without a second JSON lookup.
enum CompositionAction {
    Start,
    Update(String),
    End(String),
    Cancel,
}

/// R56.1.g.2 §5.38 §5.22 — extract the composition action surface
/// from a JSON object argument to `invoke("composition", ...)`. The
/// wire shape mirrors the W3C `CompositionEvent` vocabulary:
///
/// ```json
/// {"action": "start"}                              // compositionstart
/// {"action": "update", "data": "preedit_string"}   // compositionupdate
/// {"action": "end",    "data": "committed_string"} // compositionend
/// {"action": "cancel"}                             // cancel
/// ```
///
/// Returns `None` if the JSON is not an object, if `action` is
/// missing / not a string / not one of the four canonical values,
/// or if `data` is required (`update` / `end`) but missing / not a
/// string. The `data` field is silently ignored for `start` /
/// `cancel` (defensive against AI clients that send a uniform
/// envelope).
fn parse_composition_invoke_json(value: &serde_json::Value) -> Option<CompositionAction> {
    let obj = value.as_object()?;
    let action = obj.get("action")?.as_str()?;
    match action {
        "start" => Some(CompositionAction::Start),
        "update" => {
            let data = obj.get("data")?.as_str()?.to_string();
            Some(CompositionAction::Update(data))
        }
        "end" => {
            let data = obj.get("data")?.as_str()?.to_string();
            Some(CompositionAction::End(data))
        }
        "cancel" => Some(CompositionAction::Cancel),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! R56.1.a §5.38 — `TextField` widget binding regression battery.
    //! Mirror of the R55.D.2 `ScrollBar` test layout: initial state,
    //! four-state transition graph, commit/cancel detection, ARIA
    //! commit-on-blur path, introspect surface.

    use super::{TextField, TextFieldEvent, TextFieldExternal, TextFieldState};
    use crate::external::SchemaField;
    use crate::external::{
        Backend, External, ExternalIntrospect, InterveneError, IntrospectValue, InvokeError,
        RepaintOwner, ThreadOwnership,
    };
    use crate::{WidgetEventName, WidgetStateName};

    // ─────────────────────────────────────────────────────────────
    // SCXML state machine — transition graph
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn initial_state_is_idle() {
        assert_eq!(TextField::new().state(), TextFieldState::Idle);
    }

    #[test]
    fn focus_transitions_idle_to_focused() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        assert_eq!(tf.state(), TextFieldState::Focused);
    }

    #[test]
    fn blur_returns_focused_to_idle() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::Blur);
        assert_eq!(tf.state(), TextFieldState::Idle);
    }

    #[test]
    fn begin_edit_from_focused_enters_editing() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        assert_eq!(tf.state(), TextFieldState::Editing);
    }

    #[test]
    fn commit_edit_returns_editing_to_focused() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::CommitEdit);
        assert_eq!(tf.state(), TextFieldState::Focused);
    }

    #[test]
    fn cancel_edit_returns_editing_to_focused() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::CancelEdit);
        assert_eq!(tf.state(), TextFieldState::Focused);
    }

    #[test]
    fn blur_from_editing_drops_to_idle() {
        // IME canonical: focus loss during composition exits all the
        // way to Idle (and commits — see the *External* test below).
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::Blur);
        assert_eq!(tf.state(), TextFieldState::Idle);
    }

    #[test]
    fn disable_from_any_state_enters_disabled() {
        // SCXML lands the `disable` transition on idle / focused /
        // editing — every non-disabled state is reachable.
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Disable);
        assert_eq!(tf.state(), TextFieldState::Disabled);

        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::Disable);
        assert_eq!(tf.state(), TextFieldState::Disabled);

        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::Disable);
        assert_eq!(tf.state(), TextFieldState::Disabled);
    }

    #[test]
    fn enable_from_disabled_returns_to_idle() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Disable);
        tf.send(TextFieldEvent::Enable);
        assert_eq!(tf.state(), TextFieldState::Idle);
    }

    // ─────────────────────────────────────────────────────────────
    // Intent emission — commit / cancel / disable
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn commit_edit_emits_text_committed_intent() {
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::BeginEdit);
        tfx.send(TextFieldEvent::CommitEdit);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1, "exactly one commit per CommitEdit");
        assert_eq!(harvested[0].tag_str(), "text_committed");
        assert!(matches!(harvested[0].payload, IntrospectValue::Null));
    }

    #[test]
    fn blur_during_editing_emits_text_committed_intent() {
        // IME canonical commit-on-blur — Wayland text-input-v3, GTK
        // IBus, macOS NSTextInputContext, Windows TSF.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::BeginEdit);
        tfx.send(TextFieldEvent::Blur);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1, "commit-on-blur is canonical IME");
        assert_eq!(harvested[0].tag_str(), "text_committed");
    }

    #[test]
    fn cancel_edit_does_not_emit_commit() {
        // Escape-during-composition discards preedit silently.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::BeginEdit);
        tfx.send(TextFieldEvent::CancelEdit);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "CancelEdit must discard preedit silently",
        );
    }

    #[test]
    fn disable_during_editing_does_not_emit_commit() {
        // Wayland IME disable contract — drop preedit on the floor.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::BeginEdit);
        tfx.send(TextFieldEvent::Disable);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "Disable during editing must drop preedit silently",
        );
    }

    #[test]
    fn blur_from_focused_does_not_emit_commit() {
        // Focus loss without composition is not a commit event —
        // only `Editing → *` raises `textfield.commit` in the SCXML.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::Blur);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "blur without composition must not commit",
        );
    }

    // ─────────────────────────────────────────────────────────────
    // External adapter — introspect surface
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn external_schema_declares_thirty_five_slots() {
        // R56.1.b grew the surface: state + text + caret + send.
        // R56.1.d grew the surface: + key (W3C UI Events keystroke
        // dispatch).
        // R56.1.f.3 grew the surface: + selection (W3C
        // selectionStart/End Json mirror).
        // R56.1.g.2 grew the surface: + preedit (W3C CompositionEvent
        // .data mirror) + composition (W3C CompositionEvent action
        // dispatch surface).
        // R56.2.e grew the surface: + paste-primary (PRIMARY paste).
        // R768 grew the surface: + apply-style + clear-style (rich-text
        // setCharFormat / clearFormat over a byte range).
        // R769.1 grew the surface: + style_runs (applied formatting read
        // peer of apply-style / clear-style, a JSON run array).
        // R903 grew the surface: + find_query / find_case_sensitive /
        // find_whole_word / find_matches (find session read+write +
        // derived match read) + find-next / find-prev / replace /
        // replace-all (navigation + mutation actions).
        // R926 grew the surface: + bracket_match (derived matching-bracket
        // read — `{open, close}` byte pair or Null).
        // R933 grew the surface: + fold_regions (derived foldable-block read,
        // a JSON array) + toggle-fold / fold-all / unfold-all (code-folding
        // actions, the AI-first peers of the gutter chevron).
        // R938 / R939 / R941 grew the surface: + indent / dedent /
        // toggle-comment / line_count / go-to-line.
        // R945 grew the surface: + move-line-up / move-line-down /
        // duplicate-line-up / duplicate-line-down (line manipulation actions,
        // the AI-first peers of Alt+Up / Alt+Down + Shift+Alt copy).
        // R951 grew the surface: + style_at_caret / pending_style (active
        // typing-mark reads) + mark / clear-mark (arm / drop the mark, the
        // AI-first peer of pressing Bold with nothing selected, then typing).
        // The schema shape is stable across bare and wired-up
        // TextFields — text/caret/selection/preedit queries return
        // None / intervene returns ReadOnly when no TextEditState is
        // attached; the key / apply-style / clear-style invokes return
        // `Bool(false)` for bare TextFields; the composition invoke
        // still drives SCXML for bare TextFields.
        let tfx = TextFieldExternal::new();
        let schema = tfx.schema();
        assert_eq!(
            schema.fields,
            &[
                SchemaField::new("state", "string"),
                SchemaField::new("text", "string"),
                SchemaField::new("caret", "number"),
                SchemaField::new("selection", "object"),
                SchemaField::new("style_runs", "json"),
                SchemaField::new("preedit", "string"),
                SchemaField::new("send", "string"),
                SchemaField::new("key", "string"),
                SchemaField::new("composition", "string"),
                SchemaField::new("paste-primary", "boolean"),
                SchemaField::new("apply-style", "boolean"),
                SchemaField::new("clear-style", "boolean"),
                SchemaField::new("toggle-format", "boolean"),
                SchemaField::new("find_query", "string"),
                SchemaField::new("find_case_sensitive", "boolean"),
                SchemaField::new("find_whole_word", "boolean"),
                SchemaField::new("find_matches", "json"),
                SchemaField::new("find-next", "object"),
                SchemaField::new("find-prev", "object"),
                SchemaField::new("replace", "boolean"),
                SchemaField::new("replace-all", "number"),
                SchemaField::new("bracket_match", "object"),
                SchemaField::new("fold_regions", "json"),
                SchemaField::new("toggle-fold", "boolean"),
                SchemaField::new("fold-all", "number"),
                SchemaField::new("unfold-all", "number"),
                SchemaField::new("indent", "boolean"),
                SchemaField::new("dedent", "boolean"),
                SchemaField::new("toggle-comment", "boolean"),
                SchemaField::new("line_count", "number"),
                SchemaField::new("go-to-line", "number"),
                SchemaField::new("move-line-up", "boolean"),
                SchemaField::new("move-line-down", "boolean"),
                SchemaField::new("duplicate-line-up", "boolean"),
                SchemaField::new("duplicate-line-down", "boolean"),
                SchemaField::new("style_at_caret", "json"),
                SchemaField::new("pending_style", "json"),
                SchemaField::new("mark", "boolean"),
                SchemaField::new("clear-mark", "boolean"),
            ],
        );
    }

    #[test]
    fn external_query_state_returns_pascal_case() {
        let tfx = TextFieldExternal::new();
        assert_eq!(
            tfx.query("state").unwrap(),
            IntrospectValue::Text("Idle".to_string()),
        );
        assert_eq!(tfx.query("value"), None);
    }

    #[test]
    fn external_query_state_reflects_transitions() {
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        assert_eq!(
            tfx.query("state").unwrap(),
            IntrospectValue::Text("Focused".to_string()),
        );
        tfx.send(TextFieldEvent::BeginEdit);
        assert_eq!(
            tfx.query("state").unwrap(),
            IntrospectValue::Text("Editing".to_string()),
        );
    }

    #[test]
    fn external_intervene_state_read_only() {
        let mut tfx = TextFieldExternal::new();
        assert_eq!(
            tfx.intervene("state", IntrospectValue::Text("Focused".to_string()),),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(tfx.state(), TextFieldState::Idle);
    }

    #[test]
    fn external_intervene_unknown_path_rejects() {
        // R56.1.f.3 §5.38 §5.22 — `selection` is a known path since
        // R56.1.f.3 land; a truly unknown path (e.g. `placeholder`,
        // which would be a future R56.x slot for the watermark
        // text) still returns `UnknownPath` so the wildcard arm
        // stays observable.
        let mut tfx = TextFieldExternal::new();
        assert_eq!(
            tfx.intervene("placeholder", IntrospectValue::Text(String::new())),
            Err(InterveneError::UnknownPath),
        );
    }

    #[test]
    fn external_invoke_send_drives_transition() {
        let mut tfx = TextFieldExternal::new();
        let out = tfx
            .invoke("send", IntrospectValue::Text("Focus".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Text("Focused".to_string()));
        assert_eq!(tfx.state(), TextFieldState::Focused);
    }

    #[test]
    fn external_invoke_send_rejects_unknown_event() {
        let mut tfx = TextFieldExternal::new();
        let r = tfx.invoke("send", IntrospectValue::Text("Click".to_string()));
        assert_eq!(r, Err(InvokeError::Rejected));
    }

    #[test]
    fn external_invoke_send_rejects_non_text_args() {
        let mut tfx = TextFieldExternal::new();
        let r = tfx.invoke("send", IntrospectValue::Int(0));
        assert_eq!(r, Err(InvokeError::TypeMismatch));
    }

    #[test]
    fn external_invoke_unknown_path_rejects() {
        let mut tfx = TextFieldExternal::new();
        let r = tfx.invoke("set_text", IntrospectValue::Text(String::new()));
        assert_eq!(r, Err(InvokeError::UnknownPath));
    }

    #[test]
    fn external_invoke_full_commit_cycle_returns_focused_state_name() {
        // End-to-end: Focus → BeginEdit → CommitEdit. The final
        // `send` returns the post-transition state name as proof of
        // the dispatch round-trip through introspect.
        let mut tfx = TextFieldExternal::new();
        tfx.invoke("send", IntrospectValue::Text("Focus".to_string()))
            .unwrap();
        tfx.invoke("send", IntrospectValue::Text("BeginEdit".to_string()))
            .unwrap();
        let out = tfx
            .invoke("send", IntrospectValue::Text("CommitEdit".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Text("Focused".to_string()));
        // And a `text_committed` intent was queued.
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "text_committed");
    }

    // ─────────────────────────────────────────────────────────────
    // External adapter — backend + ownership contract
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn external_backend_support_declares_gui_and_rpc() {
        // R56.1.a — TextField is GUI + RPC visible; the TUI backend
        // gates on R49+ key dispatch + R47.5 text shaping carry, so
        // it is not declared on this slice. `Skip` fallback matches
        // the ScrollBar R55.D.2 convention.
        let tfx = TextFieldExternal::new();
        let bs = tfx.backends();
        assert!(bs.supported.contains(&Backend::Gui));
        assert!(bs.supported.contains(&Backend::Rpc));
    }

    #[test]
    fn external_repaint_ownership_is_framework() {
        // Framework drives repaint — caret blink (R56.1.c) will be
        // an animation-tick driver under framework ownership, not a
        // self-repainting widget. Matches Slider / ScrollBar.
        let tfx = TextFieldExternal::new();
        assert!(matches!(tfx.repaint_ownership(), RepaintOwner::Framework));
    }

    #[test]
    fn external_thread_ownership_is_ui_thread_sync() {
        // No background work on R56.1.a — IME composition (R56.1.g)
        // may need an OS-side preedit channel later, but the
        // statechart itself stays UI-thread synchronous.
        let tfx = TextFieldExternal::new();
        assert!(matches!(
            tfx.thread_ownership(),
            ThreadOwnership::UiThreadSync,
        ));
    }

    #[test]
    fn external_does_not_want_pointer_capture() {
        // No drag interaction on R56.1.a — the default `External`
        // returns `false` for `wants_pointer_capture`. R56.1.f
        // selection (mouse-drag selection) may revisit this, but
        // the canonical text-input contract is *cursor* + *keyboard*,
        // not drag-style pointer capture.
        let tfx = TextFieldExternal::new();
        assert!(!tfx.wants_pointer_capture());
    }

    // ─────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn state_name_helper_covers_every_variant() {
        // Guard the WidgetStateName `as_name` mapping (R698 §5.16) —
        // every state's name must be stable so RPC consumers can build
        // assertions against it.
        assert_eq!(TextFieldState::Idle.as_name(), "Idle");
        assert_eq!(TextFieldState::Focused.as_name(), "Focused");
        assert_eq!(TextFieldState::Editing.as_name(), "Editing");
        assert_eq!(TextFieldState::Disabled.as_name(), "Disabled");
    }

    #[test]
    fn event_parser_covers_every_input_variant() {
        // Every externally-dispatchable event resolves. The internal
        // `TextfieldCommit` raise event is NOT in `from_name`'s
        // external set (consumers do not drive raised events directly).
        assert!(matches!(
            TextFieldEvent::from_name("Focus"),
            Some(TextFieldEvent::Focus),
        ));
        assert!(matches!(
            TextFieldEvent::from_name("Blur"),
            Some(TextFieldEvent::Blur),
        ));
        assert!(matches!(
            TextFieldEvent::from_name("BeginEdit"),
            Some(TextFieldEvent::BeginEdit),
        ));
        assert!(matches!(
            TextFieldEvent::from_name("CommitEdit"),
            Some(TextFieldEvent::CommitEdit),
        ));
        assert!(matches!(
            TextFieldEvent::from_name("CancelEdit"),
            Some(TextFieldEvent::CancelEdit),
        ));
        assert!(matches!(
            TextFieldEvent::from_name("Disable"),
            Some(TextFieldEvent::Disable),
        ));
        assert!(matches!(
            TextFieldEvent::from_name("Enable"),
            Some(TextFieldEvent::Enable),
        ));
        assert_eq!(TextFieldEvent::from_name("textfield_commit"), None);
        assert_eq!(TextFieldEvent::from_name(""), None);
        // R699 §5.16 — internal raise + Null reject; `as_name` total.
        assert_eq!(TextFieldEvent::from_name("TextfieldCommit"), None);
        assert_eq!(TextFieldEvent::from_name("Null"), None);
        assert_eq!(TextFieldEvent::TextfieldCommit.as_name(), "TextfieldCommit");
    }
}

#[cfg(test)]
mod r56_1_b_tests {
    //! R56.1.b §5.38 §5.21 — `caret_rect` closed-form helper +
    //! [`TextField`] composition with [`TextEditState`] +
    //! introspect text/caret slots.

    use super::{TextField, TextFieldEvent, TextFieldExternal, caret_rect};
    use crate::external::{ExternalIntrospect, InterveneError, IntrospectValue};
    use crate::scene::Rect;
    use crate::widgets::text_edit::TextEditState;
    use std::rc::Rc;

    // ─────────────────────────────────────────────────────────────
    // caret_rect closed-form
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_caret_rect_at_line_origin() {
        // Caret at x offset 0 → rect anchored at line origin.
        let r = caret_rect((10, 20), 0, 1, 16);
        assert_eq!(r, Rect::new(10, 20, 1, 16));
    }

    #[test]
    fn r56_1_b_caret_rect_offset_along_text() {
        // Caret 50 px into the line → rect anchored at origin.x +
        // 50, same y, same width / line height.
        let r = caret_rect((10, 20), 50, 1, 16);
        assert_eq!(r, Rect::new(60, 20, 1, 16));
    }

    #[test]
    fn r56_1_b_caret_rect_width_2_is_lo_dpi_canonical() {
        // 2 px caret for Lo-DPI integer-scaled displays — paint
        // helper passes the width verbatim.
        let r = caret_rect((0, 0), 0, 2, 16);
        assert_eq!(r, Rect::new(0, 0, 2, 16));
    }

    #[test]
    fn r56_1_b_caret_rect_line_height_drives_full_box_extent() {
        // Caret spans the full line box height (textbook full-height
        // blink), independent of the actual glyph ascent.
        let r = caret_rect((0, 0), 0, 1, 24);
        assert_eq!(r.h, 24);
    }

    #[test]
    fn r56_1_b_caret_rect_saturates_on_u32_overflow() {
        // u32::MAX + 1 saturates instead of wrapping — the bounded
        // arithmetic contract from R55.D.1.
        let r = caret_rect((u32::MAX, 0), 1, 1, 16);
        assert_eq!(r.x, u32::MAX, "x saturates at u32::MAX");
    }

    // ─────────────────────────────────────────────────────────────
    // TextField composition with TextEditState
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_bare_text_field_has_no_text_state() {
        let tf = TextField::new();
        assert!(tf.text_state().is_none());
    }

    #[test]
    fn r56_1_b_attach_state_records_handle() {
        let state = Rc::new(TextEditState::with_initial("hello".to_string()));
        let tf = TextField::new().attach_state(Rc::clone(&state));
        assert!(tf.text_state().is_some());
        // Same Rc — composition, not copy.
        assert!(Rc::ptr_eq(tf.text_state().unwrap(), &state));
    }

    #[test]
    fn r56_1_b_external_bare_has_no_text_state() {
        let tfx = TextFieldExternal::new();
        assert!(tfx.text_state().is_none());
    }

    #[test]
    fn r56_1_b_external_attach_state_records_handle() {
        let state = Rc::new(TextEditState::with_initial("hi".to_string()));
        let tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        assert!(Rc::ptr_eq(tfx.text_state().unwrap(), &state));
    }

    #[test]
    fn r56_1_b_external_send_after_attach_preserves_state() {
        // Driving an SCXML event after attaching a state must not
        // detach the handle (the builder pattern moves once at
        // construction, transitions stay through the same instance).
        let state = Rc::new(TextEditState::with_initial("hi".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        tfx.send(TextFieldEvent::Focus);
        assert!(tfx.text_state().is_some());
        assert!(Rc::ptr_eq(tfx.text_state().unwrap(), &state));
    }

    // ─────────────────────────────────────────────────────────────
    // Introspect — text / caret query
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_query_text_returns_none_without_attached_state() {
        let tfx = TextFieldExternal::new();
        assert!(tfx.query("text").is_none());
        assert!(tfx.query("caret").is_none());
    }

    #[test]
    fn r56_1_b_query_text_returns_attached_buffer() {
        let state = Rc::new(TextEditState::with_initial("hello".to_string()));
        let tfx = TextFieldExternal::new().attach_state(state);
        assert_eq!(
            tfx.query("text").unwrap(),
            IntrospectValue::Text("hello".to_string()),
        );
    }

    #[test]
    fn r56_1_b_query_caret_returns_attached_offset() {
        let state = Rc::new(TextEditState::with_initial("hi".to_string()));
        let tfx = TextFieldExternal::new().attach_state(state);
        assert_eq!(tfx.query("caret").unwrap(), IntrospectValue::Int(2));
    }

    #[test]
    fn r56_1_b_query_text_reflects_mutations() {
        // The attached Rc is shared with the application code that
        // mutates the state directly — the introspect query reads
        // through the same handle on every call.
        let state = Rc::new(TextEditState::new());
        let tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        state.insert("abc");
        assert_eq!(
            tfx.query("text").unwrap(),
            IntrospectValue::Text("abc".to_string()),
        );
        assert_eq!(tfx.query("caret").unwrap(), IntrospectValue::Int(3));
    }

    // ─────────────────────────────────────────────────────────────
    // Introspect — text / caret intervene
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_intervene_text_without_attached_state_is_read_only() {
        // The slot exists in the schema, but the widget has no
        // backing reactive store → ReadOnly (not UnknownPath; the
        // slot is *visible* but uneditable).
        let mut tfx = TextFieldExternal::new();
        assert_eq!(
            tfx.intervene("text", IntrospectValue::Text("x".to_string())),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            tfx.intervene("caret", IntrospectValue::Int(0)),
            Err(InterveneError::ReadOnly),
        );
    }

    #[test]
    fn r56_1_b_intervene_text_writes_attached_buffer() {
        let state = Rc::new(TextEditState::with_initial("old".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        tfx.intervene("text", IntrospectValue::Text("new".to_string()))
            .unwrap();
        assert_eq!(state.text(), "new");
    }

    #[test]
    fn r56_1_b_intervene_text_rejects_wrong_type() {
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state);
        assert_eq!(
            tfx.intervene("text", IntrospectValue::Int(0)),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn r56_1_b_intervene_caret_writes_attached_offset() {
        let state = Rc::new(TextEditState::with_initial("abc".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        tfx.intervene("caret", IntrospectValue::Int(1)).unwrap();
        assert_eq!(state.caret(), 1);
    }

    #[test]
    fn r56_1_b_intervene_caret_rejects_negative_as_out_of_range() {
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state);
        assert_eq!(
            tfx.intervene("caret", IntrospectValue::Int(-1)),
            Err(InterveneError::OutOfRange),
        );
    }

    #[test]
    fn r56_1_b_intervene_caret_rejects_wrong_type() {
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state);
        assert_eq!(
            tfx.intervene("caret", IntrospectValue::Text("0".to_string())),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn r56_1_b_intervene_caret_clamps_past_end() {
        // TextEditState::set_caret clamps to text.len() — the
        // intervene path inherits that contract (no error for
        // past-end positive offsets, just clamping).
        let state = Rc::new(TextEditState::with_initial("ab".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        tfx.intervene("caret", IntrospectValue::Int(999)).unwrap();
        assert_eq!(state.caret(), 2, "past-end clamps to text.len()");
    }

    #[test]
    fn r56_1_b_state_slot_still_read_only_after_attach() {
        // R56.1.b additions must not regress the R56.1.a invariants —
        // the SCXML state stays read-only-from-RPC regardless of
        // whether TextEditState is attached.
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state);
        assert_eq!(
            tfx.intervene("state", IntrospectValue::Text("Focused".to_string()),),
            Err(InterveneError::ReadOnly),
        );
    }
}

#[cfg(test)]
mod r56_1_d_tests {
    //! R56.1.d §5.38 §5.22 — [`apply_key`] W3C UI Events keystroke
    //! dispatch helper + [`TextFieldExternal`] `invoke("key", ...)`
    //! RPC path. Mirrors the R56.1.b test layout (closed-form helper
    //! battery + External RPC battery).

    use super::{TextField, TextFieldExternal, apply_key};
    use crate::external::{External, ExternalIntrospect, IntrospectValue, InvokeError};
    use crate::scene::StyleRun;
    use crate::style::{Color, FontStyle, FontWeight, TextStyle};
    use crate::widgets::text_edit::TextEditState;
    use std::rc::Rc;

    // ─────────────────────────────────────────────────────────────
    // apply_key — recognized named keys (caret-relative edit ops)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_d_backspace_deletes_char_left_of_caret() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(3);
        assert!(apply_key(
            &state,
            "Backspace",
            crate::input::Modifiers::empty()
        ));
        assert_eq!(state.text(), "ab");
        assert_eq!(state.caret(), 2);
    }

    #[test]
    fn r56_1_d_backspace_at_caret_zero_no_ops_but_returns_handled() {
        // W3C `defaultPrevented` semantics: the key was *recognized*
        // (consumed) even when the visible mutation is a no-op.
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(0);
        assert!(apply_key(
            &state,
            "Backspace",
            crate::input::Modifiers::empty()
        ));
        assert_eq!(state.text(), "abc");
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn r56_1_d_delete_removes_char_at_caret() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(0);
        assert!(apply_key(
            &state,
            "Delete",
            crate::input::Modifiers::empty()
        ));
        assert_eq!(state.text(), "bc");
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn r56_1_d_delete_at_end_no_ops_but_returns_handled() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(3);
        assert!(apply_key(
            &state,
            "Delete",
            crate::input::Modifiers::empty()
        ));
        assert_eq!(state.text(), "abc");
    }

    #[test]
    fn r56_1_d_arrow_left_moves_caret_back_one() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(2);
        assert!(apply_key(
            &state,
            "ArrowLeft",
            crate::input::Modifiers::empty()
        ));
        assert_eq!(state.caret(), 1);
    }

    #[test]
    fn r56_1_d_arrow_left_at_zero_no_ops_but_returns_handled() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(0);
        assert!(apply_key(
            &state,
            "ArrowLeft",
            crate::input::Modifiers::empty()
        ));
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn r56_1_d_arrow_right_moves_caret_forward_one() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(1);
        assert!(apply_key(
            &state,
            "ArrowRight",
            crate::input::Modifiers::empty()
        ));
        assert_eq!(state.caret(), 2);
    }

    #[test]
    fn r56_1_d_arrow_right_at_end_no_ops_but_returns_handled() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(3);
        assert!(apply_key(
            &state,
            "ArrowRight",
            crate::input::Modifiers::empty()
        ));
        assert_eq!(state.caret(), 3);
    }

    #[test]
    fn r56_1_d_home_moves_caret_to_zero() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_caret(4);
        assert!(apply_key(&state, "Home", crate::input::Modifiers::empty()));
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn r56_1_d_end_moves_caret_to_text_len() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_caret(2);
        assert!(apply_key(&state, "End", crate::input::Modifiers::empty()));
        assert_eq!(state.caret(), 6);
    }

    #[test]
    fn r56_1_d_space_inserts_single_space() {
        let state = TextEditState::with_initial("ab".to_string());
        state.set_caret(2);
        assert!(apply_key(&state, "Space", crate::input::Modifiers::empty()));
        assert_eq!(state.text(), "ab ");
        assert_eq!(state.caret(), 3);
    }

    // ─────────────────────────────────────────────────────────────
    // apply_key — printable single-char insertion
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_d_lowercase_letter_inserts_at_caret() {
        let state = TextEditState::new();
        assert!(apply_key(&state, "a", crate::input::Modifiers::empty()));
        assert_eq!(state.text(), "a");
        assert_eq!(state.caret(), 1);
    }

    #[test]
    fn r56_1_d_uppercase_letter_inserts_at_caret() {
        let state = TextEditState::new();
        assert!(apply_key(&state, "A", crate::input::Modifiers::empty()));
        assert_eq!(state.text(), "A");
    }

    #[test]
    fn r939_apply_key_ctrl_slash_toggles_comment_when_opted_in() {
        let ctrl = crate::input::Modifiers {
            shift: false,
            ctrl: true,
            alt: false,
            meta: false,
        };
        let state = TextEditState::with_initial("ab".to_string());
        state.set_caret(0);
        // Not opted in → Ctrl+/ is unhandled (falls through to the application)
        // and never inserts a literal "/" under Ctrl.
        assert!(
            !apply_key(&state, "/", ctrl),
            "no marker configured → falls through"
        );
        assert_eq!(state.text(), "ab");
        // Opt in → Ctrl+/ toggles the line comment, the keymap-SSOT twin of the
        // `toggle-comment` RPC verb.
        state.set_line_comment("//");
        assert!(apply_key(&state, "/", ctrl), "opted-in Ctrl+/ is handled");
        assert_eq!(state.text(), "// ab");
        // A plain "/" (no modifier) is still a literal insert.
        state.set_caret(state.text().len());
        assert!(apply_key(&state, "/", crate::input::Modifiers::empty()));
        assert_eq!(state.text(), "// ab/");
    }

    #[test]
    fn r1268_apply_key_enter_auto_indents_when_opted_in() {
        let state = TextEditState::with_initial("    foo".to_string());
        // Not opted in → Enter is unhandled (the field keeps its submit / focus
        // policy — a single-line field is byte-unchanged) and inserts nothing.
        assert!(
            !apply_key(&state, "Enter", crate::input::Modifiers::empty()),
            "no auto-indent opt-in → Enter falls through"
        );
        assert_eq!(
            state.text(),
            "    foo",
            "unhandled Enter never mutates the buffer"
        );
        // Opt in → Enter inserts an indent-copying newline (the keymap-SSOT twin
        // of TextEditState::insert_newline).
        state.set_auto_indent(true);
        assert!(
            apply_key(&state, "Enter", crate::input::Modifiers::empty()),
            "opted-in Enter is handled"
        );
        assert_eq!(
            state.text(),
            "    foo\n    ",
            "the new line copies the 4-space indent"
        );
        // Shift+Enter is the same indent-aware newline (the modifier is ignored).
        let shift = crate::input::Modifiers {
            shift: true,
            ctrl: false,
            alt: false,
            meta: false,
        };
        assert!(
            apply_key(&state, "Enter", shift),
            "Shift+Enter is handled too"
        );
        assert_eq!(
            state.text(),
            "    foo\n    \n    ",
            "Shift+Enter also inserts an auto-indented newline"
        );
    }

    #[test]
    fn r56_1_d_digit_inserts_at_caret() {
        let state = TextEditState::new();
        assert!(apply_key(&state, "7", crate::input::Modifiers::empty()));
        assert_eq!(state.text(), "7");
    }

    #[test]
    fn r56_1_d_punctuation_inserts_at_caret() {
        // Listbox typeahead rejects non-alphanumeric; text input
        // accepts every non-control codepoint.
        let state = TextEditState::new();
        assert!(apply_key(&state, "!", crate::input::Modifiers::empty()));
        assert!(apply_key(&state, ",", crate::input::Modifiers::empty()));
        assert!(apply_key(&state, "$", crate::input::Modifiers::empty()));
        assert_eq!(state.text(), "!,$");
    }

    #[test]
    fn r56_1_d_cjk_ideograph_inserts_at_caret() {
        // Pre-composed CJK glyph (already-resolved by IME) flows
        // through the printable-char branch as a single codepoint.
        // Multi-char IME composition results are R56.1.g territory.
        let state = TextEditState::new();
        assert!(apply_key(&state, "漢", crate::input::Modifiers::empty()));
        assert_eq!(state.text(), "漢");
        // Caret advances by `s.len()` bytes per R56.1.b
        // TextEditState contract (caret is a UTF-8 byte offset, not
        // a char index — `text[..caret]` stays slice-safe). "漢"
        // (U+6F22) encodes to 3 UTF-8 bytes.
        assert_eq!(state.caret(), 3);
    }

    #[test]
    fn r56_1_d_korean_syllable_inserts_at_caret() {
        // U+C548 ("안") is a single pre-composed Unicode codepoint —
        // accepts through printable-char branch. Pre-decomposed jamo
        // (multi-codepoint) is R56.1.g IME path. 3-byte UTF-8.
        let state = TextEditState::new();
        assert!(apply_key(&state, "안", crate::input::Modifiers::empty()));
        assert_eq!(state.text(), "안");
        assert_eq!(state.caret(), 3, "안 = 3 UTF-8 bytes (U+C548)");
    }

    #[test]
    fn r56_1_d_insert_at_mid_position_splices() {
        let state = TextEditState::with_initial("ac".to_string());
        state.set_caret(1);
        assert!(apply_key(&state, "b", crate::input::Modifiers::empty()));
        assert_eq!(state.text(), "abc");
        assert_eq!(state.caret(), 2);
    }

    // ─────────────────────────────────────────────────────────────
    // apply_key — unrecognized keys (return false, no mutation)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_d_arrow_up_returns_false_pending_multiline() {
        // Vertical navigation pends R56.1.h multi-line shaping.
        // Returning false lets the application apply_key chain fall
        // through to the focus manager Tab traversal.
        let state = TextEditState::with_initial("abc".to_string());
        let before = (state.text(), state.caret());
        assert!(!apply_key(
            &state,
            "ArrowUp",
            crate::input::Modifiers::empty()
        ));
        assert_eq!((state.text(), state.caret()), before);
    }

    #[test]
    fn r56_1_d_arrow_down_returns_false_pending_multiline() {
        let state = TextEditState::new();
        assert!(!apply_key(
            &state,
            "ArrowDown",
            crate::input::Modifiers::empty()
        ));
    }

    #[test]
    fn r56_1_d_page_up_down_return_false() {
        let state = TextEditState::new();
        assert!(!apply_key(
            &state,
            "PageUp",
            crate::input::Modifiers::empty()
        ));
        assert!(!apply_key(
            &state,
            "PageDown",
            crate::input::Modifiers::empty()
        ));
    }

    #[test]
    fn r56_1_d_enter_returns_false_pending_submit_event() {
        // R56.1.h plans the submit-class statechart event; on
        // R56.1.d Enter falls through (Enter is shell-reserved
        // upstream anyway — it never reaches apply_key in
        // practice, but the rejection is defensive).
        let state = TextEditState::with_initial("abc".to_string());
        assert!(!apply_key(
            &state,
            "Enter",
            crate::input::Modifiers::empty()
        ));
        assert_eq!(state.text(), "abc");
    }

    #[test]
    fn r56_1_d_function_keys_return_false() {
        let state = TextEditState::new();
        assert!(!apply_key(&state, "F1", crate::input::Modifiers::empty()));
        assert!(!apply_key(&state, "F12", crate::input::Modifiers::empty()));
        assert_eq!(state.text(), "");
    }

    #[test]
    fn r56_1_d_empty_key_returns_false() {
        let state = TextEditState::new();
        assert!(!apply_key(&state, "", crate::input::Modifiers::empty()));
        assert_eq!(state.text(), "");
    }

    #[test]
    fn r56_1_d_multi_char_string_returns_false() {
        // IME composition multi-char output (R56.1.g territory)
        // flows through the preedit-buffer substrate, not this hook.
        let state = TextEditState::new();
        assert!(!apply_key(&state, "ab", crate::input::Modifiers::empty()));
        assert!(!apply_key(
            &state,
            "hello",
            crate::input::Modifiers::empty()
        ));
        assert_eq!(state.text(), "");
    }

    #[test]
    fn r56_1_d_control_char_returns_false() {
        // Raw tab / newline / null are bug-fixture paths — the
        // framework converts these to named keys at the input
        // boundary. Defensive rejection.
        let state = TextEditState::new();
        assert!(!apply_key(&state, "\t", crate::input::Modifiers::empty()));
        assert!(!apply_key(&state, "\n", crate::input::Modifiers::empty()));
        assert!(!apply_key(
            &state,
            "\u{0000}",
            crate::input::Modifiers::empty()
        ));
        assert_eq!(state.text(), "");
    }

    // ─────────────────────────────────────────────────────────────
    // ExternalIntrospect::invoke("key", ...) — RPC dispatch path
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_d_invoke_key_with_printable_inserts_via_attached_state() {
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        let result = tfx
            .invoke("key", IntrospectValue::Text("h".to_string()))
            .unwrap();
        assert_eq!(result, IntrospectValue::Bool(true));
        assert_eq!(state.text(), "h");
    }

    #[test]
    fn r56_1_d_invoke_key_with_backspace_deletes_via_attached_state() {
        let state = Rc::new(TextEditState::with_initial("abc".to_string()));
        state.set_caret(3);
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        let result = tfx
            .invoke("key", IntrospectValue::Text("Backspace".to_string()))
            .unwrap();
        assert_eq!(result, IntrospectValue::Bool(true));
        assert_eq!(state.text(), "ab");
    }

    #[test]
    fn r56_1_d_invoke_key_with_arrow_moves_caret_via_attached_state() {
        let state = Rc::new(TextEditState::with_initial("abc".to_string()));
        state.set_caret(0);
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        tfx.invoke("key", IntrospectValue::Text("ArrowRight".to_string()))
            .unwrap();
        assert_eq!(state.caret(), 1);
    }

    #[test]
    fn r56_1_d_invoke_key_with_space_inserts_via_attached_state() {
        let state = Rc::new(TextEditState::with_initial("a".to_string()));
        state.set_caret(1);
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        tfx.invoke("key", IntrospectValue::Text("Space".to_string()))
            .unwrap();
        assert_eq!(state.text(), "a ");
    }

    #[test]
    fn r56_1_d_invoke_key_on_bare_text_field_returns_bool_false() {
        // No TextEditState attached → key is recognized at the
        // path level but no edit occurs. `Bool(false)` distinguishes
        // "unbound widget" from `Err(UnknownPath)`.
        let mut tfx = TextFieldExternal::new();
        let result = tfx
            .invoke("key", IntrospectValue::Text("a".to_string()))
            .unwrap();
        assert_eq!(result, IntrospectValue::Bool(false));
    }

    // ─────────────────────────────────────────────────────────────
    // R768 §5.36 §5.22 — invoke("apply-style" / "clear-style", ...)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r768_invoke_apply_style_overlays_a_colour_run() {
        let state = Rc::new(TextEditState::with_initial("hello world".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        let result = tfx
            .invoke(
                "apply-style",
                IntrospectValue::Json(serde_json::json!({
                    "start": 6, "end": 11, "fg": "#d02828",
                })),
            )
            .unwrap();
        assert_eq!(result, IntrospectValue::Bool(true));
        let runs = state.style_runs();
        assert_eq!(runs.len(), 1, "one run applied");
        assert_eq!((runs[0].start, runs[0].end), (6, 11));
        assert_eq!(runs[0].style.fg_color, Color::rgb(0xD0, 0x28, 0x28));
    }

    #[test]
    fn r967_invoke_toggle_format_flips_one_field_preserving_colour() {
        // R967 — the AI-first `toggle-format` verb: toggle ONE field over the
        // selection, preserving the run's colour (the toolbar B / I peer).
        let state = Rc::new(TextEditState::with_initial("hello".to_string()));
        state.set_style_runs(vec![StyleRun::new(
            0,
            5,
            TextStyle::new().with_fg(Color::rgb(0xD0, 0x28, 0x28)),
        )]);
        state.set_selection(0, 5);
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        // First toggle bolds -> Bool(true); the run's colour is untouched.
        let r = tfx
            .invoke("toggle-format", IntrospectValue::Text("bold".to_owned()))
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true), "the new on-state is bold");
        let runs = state.style_runs();
        assert_eq!(
            runs[0].style.font_weight,
            crate::style::FontWeight::BOLD,
            "bold via RPC"
        );
        assert_eq!(
            runs[0].style.fg_color,
            Color::rgb(0xD0, 0x28, 0x28),
            "RPC toggle keeps colour"
        );
        // A second toggle returns Bool(false) and un-bolds (round-trip).
        let r2 = tfx
            .invoke("toggle-format", IntrospectValue::Text("bold".to_owned()))
            .unwrap();
        assert_eq!(r2, IntrospectValue::Bool(false));
        assert_eq!(
            state.style_runs()[0].style.font_weight,
            crate::style::FontWeight::NORMAL,
            "un-bolded via RPC",
        );
        // An unknown field token is rejected (not a silent no-op).
        assert!(
            tfx.invoke("toggle-format", IntrospectValue::Text("rainbow".to_owned()))
                .is_err(),
            "an unknown field token is a TypeMismatch",
        );
    }

    #[test]
    fn r768_invoke_apply_style_honours_optional_size() {
        let state = Rc::new(TextEditState::with_initial("hello".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        tfx.invoke(
            "apply-style",
            IntrospectValue::Json(serde_json::json!({
                "start": 0, "end": 5, "fg": "#264cd8", "size": 24,
            })),
        )
        .unwrap();
        assert_eq!(
            state.style_runs()[0].style.font_size_px,
            24,
            "size override threads through"
        );
    }

    #[test]
    fn r768_invoke_clear_style_strips_a_range() {
        let state = Rc::new(TextEditState::with_initial("hello world".to_string()));
        state.set_style_runs(vec![StyleRun::new(
            0,
            11,
            TextStyle::new().with_fg(Color::rgb(0xD0, 0x28, 0x28)),
        )]);
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        let result = tfx
            .invoke(
                "clear-style",
                IntrospectValue::Json(serde_json::json!({ "start": 3, "end": 8 })),
            )
            .unwrap();
        assert_eq!(result, IntrospectValue::Bool(true));
        let spans: Vec<(u32, u32)> = state
            .style_runs()
            .iter()
            .map(|r| (r.start, r.end))
            .collect();
        assert_eq!(
            spans,
            vec![(0, 3), (8, 11)],
            "clear splits the run without shifting"
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R951 §5.36 — mark / clear-mark + style_at_caret / pending_style
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r951_invoke_mark_then_key_types_styled_text() {
        let state = Rc::new(TextEditState::with_initial(String::new()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        let armed = tfx
            .invoke(
                "mark",
                IntrospectValue::Json(serde_json::json!({ "font_weight": 700 })),
            )
            .unwrap();
        assert_eq!(armed, IntrospectValue::Bool(true));
        // Typing through the same code path the shell drives picks up the mark.
        tfx.invoke("key", IntrospectValue::Text("X".to_string()))
            .unwrap();
        let runs = state.style_runs();
        assert_eq!(runs.len(), 1, "the armed mark styles the typed char");
        assert_eq!((runs[0].start, runs[0].end), (0, 1));
        assert_eq!(runs[0].style.font_weight, FontWeight::BOLD);
    }

    #[test]
    fn r951_query_style_at_caret_returns_armed_mark() {
        let state = Rc::new(TextEditState::with_initial("hi".to_string()));
        state.set_caret(1);
        let tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        let mut s = TextStyle::new();
        s.font_size_px = 30;
        state.set_pending_style(Some(s));
        match tfx.query("style_at_caret").unwrap() {
            IntrospectValue::Json(v) => assert_eq!(
                v.get("font_size_px").and_then(serde_json::Value::as_u64),
                Some(30),
                "the armed mark round-trips through the read"
            ),
            other => panic!("expected Json, got {other:?}"),
        }
        assert!(
            matches!(
                tfx.query("pending_style").unwrap(),
                IntrospectValue::Json(_)
            ),
            "pending_style reports the armed mark"
        );
    }

    #[test]
    fn r951_query_pending_style_null_when_inherited() {
        let state = Rc::new(TextEditState::with_initial("abcd".to_string()));
        state.set_style_runs(vec![StyleRun::new(
            0,
            2,
            TextStyle::new().with_fg(Color::rgb(0xD0, 0x28, 0x28)),
        )]);
        state.set_caret(2); // inheriting the red run, but nothing armed
        let tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        assert_eq!(
            tfx.query("pending_style").unwrap(),
            IntrospectValue::Null,
            "inherited style is not an armed mark"
        );
        assert!(
            matches!(
                tfx.query("style_at_caret").unwrap(),
                IntrospectValue::Json(_)
            ),
            "style_at_caret still reflects the inherited run"
        );
    }

    #[test]
    fn r951_invoke_clear_mark_drops_the_mark() {
        let state = Rc::new(TextEditState::with_initial(String::new()));
        let mut armed = TextStyle::new();
        armed.font_size_px = 30;
        state.set_pending_style(Some(armed));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        assert!(matches!(
            tfx.query("pending_style").unwrap(),
            IntrospectValue::Json(_)
        ));
        let r = tfx.invoke("clear-mark", IntrospectValue::Null).unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(
            tfx.query("pending_style").unwrap(),
            IntrospectValue::Null,
            "clear-mark drops the mark"
        );
    }

    #[test]
    fn r951_invoke_mark_on_bare_field_returns_false() {
        // No TextEditState attached → recognized at the path level, no effect.
        let mut tfx = TextFieldExternal::new();
        let r = tfx
            .invoke(
                "mark",
                IntrospectValue::Json(serde_json::json!({ "font_weight": 700 })),
            )
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(false));
    }

    #[test]
    fn r951_invoke_mark_rejects_non_json_arg() {
        let state = Rc::new(TextEditState::with_initial(String::new()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        assert!(matches!(
            tfx.invoke("mark", IntrospectValue::Text("nope".to_string())),
            Err(InvokeError::TypeMismatch)
        ));
    }

    #[test]
    fn r768_invoke_apply_style_on_bare_field_returns_bool_false() {
        let mut tfx = TextFieldExternal::new();
        let result = tfx
            .invoke(
                "apply-style",
                IntrospectValue::Json(serde_json::json!({
                    "start": 0, "end": 1, "fg": "#000000",
                })),
            )
            .unwrap();
        assert_eq!(
            result,
            IntrospectValue::Bool(false),
            "unbound widget reports no-op"
        );
    }

    #[test]
    fn r768_invoke_apply_style_rejects_bad_payload() {
        let state = Rc::new(TextEditState::with_initial("hi".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(state);
        // Missing "fg".
        assert!(matches!(
            tfx.invoke(
                "apply-style",
                IntrospectValue::Json(serde_json::json!({ "start": 0, "end": 2 })),
            ),
            Err(InvokeError::TypeMismatch)
        ));
        // Non-Json arg.
        assert!(matches!(
            tfx.invoke("apply-style", IntrospectValue::Text("nope".to_string())),
            Err(InvokeError::TypeMismatch)
        ));
        // Unparseable colour.
        assert!(matches!(
            tfx.invoke(
                "apply-style",
                IntrospectValue::Json(serde_json::json!({
                    "start": 0, "end": 2, "fg": "octarine",
                })),
            ),
            Err(InvokeError::TypeMismatch)
        ));
    }

    #[test]
    fn r770_1_invoke_apply_style_full_object_sets_weight_and_style() {
        // R770.1 — the full `style` object form makes bold / italic
        // RPC-settable (the colour-only shorthand could not). The AI
        // reads a run's style, sets font_weight / font_style, writes back.
        let state = Rc::new(TextEditState::with_initial("hello".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        let result = tfx
            .invoke(
                "apply-style",
                IntrospectValue::Json(serde_json::json!({
                    "start": 0, "end": 5,
                    "style": {
                        "font_weight": 700,
                        "font_style": "Italic",
                        "fg_color": {"r": 0xD0, "g": 0x28, "b": 0x28, "a": 255},
                    }
                })),
            )
            .unwrap();
        assert_eq!(result, IntrospectValue::Bool(true));
        let runs = state.style_runs();
        assert_eq!(
            runs[0].style.font_weight,
            FontWeight::BOLD,
            "weight set via full style"
        );
        assert_eq!(
            runs[0].style.font_style,
            FontStyle::Italic,
            "italic set via full style"
        );
        assert_eq!(
            runs[0].style.fg_color,
            Color::rgb(0xD0, 0x28, 0x28),
            "colour set via full style"
        );
    }

    #[test]
    fn r56_1_d_invoke_key_rejects_unrecognized_returns_bool_false() {
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state);
        let result = tfx
            .invoke("key", IntrospectValue::Text("F7".to_string()))
            .unwrap();
        assert_eq!(result, IntrospectValue::Bool(false));
    }

    #[test]
    fn r56_1_d_invoke_key_rejects_non_text_args_with_type_mismatch() {
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state);
        assert!(matches!(
            tfx.invoke("key", IntrospectValue::Int(65)),
            Err(InvokeError::TypeMismatch),
        ));
        assert!(matches!(
            tfx.invoke("key", IntrospectValue::Bool(true)),
            Err(InvokeError::TypeMismatch),
        ));
    }

    #[test]
    fn r56_1_d_invoke_unknown_path_still_unknown() {
        // R56.1.d only added the `key` path — `send` stays the only
        // other invoke surface, and unrecognized paths must still
        // surface as `UnknownPath` (not silently absorbed by the
        // new `key` branch).
        let mut tfx = TextFieldExternal::new();
        assert!(matches!(
            tfx.invoke("press", IntrospectValue::Text("a".to_string())),
            Err(InvokeError::UnknownPath),
        ));
    }

    // ─────────────────────────────────────────────────────────────
    // Schema + send invoke regression
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_d_schema_contains_key_slot() {
        let tfx = TextFieldExternal::new();
        let schema = tfx.schema();
        assert!(
            schema
                .fields
                .iter()
                .any(|f| f.path == "key" && f.ty == "string"),
            "key slot must be in schema",
        );
    }

    #[test]
    fn r56_1_d_invoke_send_still_works_after_key_path_added() {
        // R56.1.d additive — the existing `send` invoke path (R56.1.a)
        // must keep its contract after `key` is added.
        let mut tfx = TextFieldExternal::new();
        let result = tfx
            .invoke("send", IntrospectValue::Text("Focus".to_string()))
            .unwrap();
        assert_eq!(result, IntrospectValue::Text("Focused".to_string()));
    }

    #[test]
    fn r56_1_d_apply_key_does_not_drive_statechart_transitions() {
        // R56.1.d is the *keystroke* slice — focus/blur/begin_edit
        // statechart drive is R56.1.h. Verify apply_key on a bare
        // TextField does not surface text_committed (no statechart
        // transitions fire on edit-class keys).
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state);
        // Drive every recognized edit-class key.
        for key in [
            "a",
            "b",
            "c",
            "Space",
            "Backspace",
            "ArrowLeft",
            "ArrowRight",
            "Home",
            "End",
            "Delete",
        ] {
            tfx.invoke("key", IntrospectValue::Text(key.to_string()))
                .unwrap();
        }
        // The statechart did not transition; no intent was raised
        // (text_committed only fires on Editing exit per the R56.1.a
        // commit semantics).
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "R56.1.d keystrokes must not raise text_committed",
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R56.1.f.0 §5.13 — RPC invoke("key", Json {...}) modifier path
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_f_0_invoke_key_json_no_modifier_handles_recognized_key() {
        // The JSON shape with default-false modifiers is observably
        // equivalent to the IntrospectValue::Text(key) shape — both
        // forward Modifiers::empty() to apply_key. Confirms the Json
        // arg shape parses correctly when no modifier bit is set.
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state.clone());
        let r = tfx
            .invoke(
                "key",
                IntrospectValue::Json(serde_json::json!({ "key": "a" })),
            )
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(state.text(), "a");
    }

    #[test]
    fn r56_1_f_0_invoke_key_json_carries_shift_modifier_through_dispatch() {
        // R56.1.f.0 substrate verification — the parsed `shift: true`
        // bit reaches apply_key. R56.1.d's apply_key still ignores
        // the modifier (no selection semantics yet), so the recognised
        // result matches the no-modifier path; the visible mutation
        // also matches (Shift+a is still a single 'a' insert because
        // the W3C `KeyboardEvent.key` value for shifted-A on US is
        // "A", not "a" — the RPC client passes the already-shifted
        // character verbatim). What this test asserts is that the
        // dispatch round-trip *accepts* the modifier-carrying shape
        // without parse rejection.
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state.clone());
        let r = tfx
            .invoke(
                "key",
                IntrospectValue::Json(serde_json::json!({
                    "key": "ArrowLeft",
                    "shift": true,
                })),
            )
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
    }

    #[test]
    fn r56_1_f_0_invoke_key_json_carries_all_four_modifier_bits() {
        // Every W3C `KeyboardEvent` modifier (shift / ctrl / alt /
        // meta) parses through the Json shape. The dispatch result
        // gates on key recognition (not modifier presence), so a
        // recognized key like ArrowLeft round-trips Bool(true).
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state);
        let r = tfx
            .invoke(
                "key",
                IntrospectValue::Json(serde_json::json!({
                    "key":   "ArrowLeft",
                    "shift": true,
                    "ctrl":  true,
                    "alt":   true,
                    "meta":  true,
                })),
            )
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
    }

    #[test]
    fn r56_1_f_0_invoke_key_json_missing_key_field_returns_type_mismatch() {
        // The JSON object MUST carry a "key" string. Defensive
        // rejection so a buggy RPC client (e.g. one that mis-spells
        // "key" as "Key") fails loudly rather than silently treating
        // every keystroke as unrecognized.
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state);
        let r = tfx.invoke(
            "key",
            IntrospectValue::Json(serde_json::json!({ "shift": true })),
        );
        assert_eq!(r, Err(InvokeError::TypeMismatch));
    }

    #[test]
    fn r56_1_f_0_invoke_key_json_non_bool_modifier_returns_type_mismatch() {
        // Modifier fields are strictly boolean — a number, string,
        // or null in the modifier slot triggers TypeMismatch. The
        // W3C `KeyboardEvent` modifier surface is strictly boolean,
        // and the RPC discipline mirrors that strictly (no silent
        // 0/1/"true" coercion).
        let state = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(state);
        let r = tfx.invoke(
            "key",
            IntrospectValue::Json(serde_json::json!({
                "key":   "ArrowLeft",
                "shift": 1,
            })),
        );
        assert_eq!(r, Err(InvokeError::TypeMismatch));
    }

    #[test]
    fn r56_1_f_0_invoke_key_rejects_non_text_non_json_args() {
        // The two accepted arg shapes are Text(key) and Json({...}).
        // Other variants (Int / Bool / Null) trigger TypeMismatch.
        let mut tfx = TextFieldExternal::new();
        assert_eq!(
            tfx.invoke("key", IntrospectValue::Int(0)),
            Err(InvokeError::TypeMismatch),
        );
        assert_eq!(
            tfx.invoke("key", IntrospectValue::Bool(true)),
            Err(InvokeError::TypeMismatch),
        );
        assert_eq!(
            tfx.invoke("key", IntrospectValue::Null),
            Err(InvokeError::TypeMismatch),
        );
    }

    #[test]
    fn r56_1_d_apply_key_works_on_bare_text_field_via_direct_helper() {
        // Direct helper API parity — `apply_key(state, key, modifiers)`
        // works on any TextEditState handle, independent of whether
        // the state is attached to a TextField/TextFieldExternal.
        let state = TextEditState::new();
        let mods = crate::input::Modifiers::empty();
        assert!(apply_key(&state, "h", mods));
        assert!(apply_key(&state, "i", mods));
        assert_eq!(state.text(), "hi");
    }

    #[test]
    fn r56_1_d_text_field_without_text_state_ignores_key_invoke_no_panic() {
        // Bare TextField (no attached TextEditState) — invoke("key")
        // must not panic; returns Bool(false). Regression guard for
        // the unattached state path.
        let mut tfx = TextFieldExternal::new();
        for key in ["a", "Space", "Backspace", "ArrowLeft", "Home", "Delete"] {
            let r = tfx
                .invoke("key", IntrospectValue::Text(key.to_string()))
                .unwrap();
            assert_eq!(r, IntrospectValue::Bool(false));
        }
        // Also: TextField itself remains in Idle (no statechart drive).
        let state_query = tfx.query("state").unwrap();
        assert_eq!(state_query, IntrospectValue::Text("Idle".to_string()));
        // Construction-time fresh TextField equivalence.
        let fresh = TextField::new();
        assert_eq!(fresh.state(), tfx.state());
    }
}

#[cfg(test)]
mod r56_1_f_tests {
    //! R56.1.f.2 §5.22 §5.38 — `apply_key` Shift-prefix selection
    //! extension + Ctrl+A select-all + selection-replace dispatch
    //! through the `TextField` keystroke surface.

    use super::{TextFieldExternal, apply_key};
    use crate::external::{ExternalIntrospect, IntrospectValue};
    use crate::input::Modifiers;
    use crate::widgets::text_edit::TextEditState;
    use std::rc::Rc;

    // ─────────────────────────────────────────────────────────────
    // Shift-prefix → select_* extension (W3C ARIA single-line text)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_f_shift_arrow_left_extends_selection() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_caret(3);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::empty()
        };
        assert!(apply_key(&state, "ArrowLeft", shift));
        assert_eq!(state.caret(), 2);
        assert_eq!(state.selection_anchor(), Some(3));
        assert_eq!(state.selection_range(), Some((2, 3)));
    }

    #[test]
    fn r56_1_f_shift_arrow_right_extends_selection() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_caret(2);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::empty()
        };
        assert!(apply_key(&state, "ArrowRight", shift));
        assert_eq!(state.caret(), 3);
        assert_eq!(state.selection_anchor(), Some(2));
    }

    #[test]
    fn r56_1_f_shift_home_extends_selection_to_zero() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_caret(4);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::empty()
        };
        assert!(apply_key(&state, "Home", shift));
        assert_eq!(state.caret(), 0);
        assert_eq!(state.selection_range(), Some((0, 4)));
    }

    #[test]
    fn r56_1_f_shift_end_extends_selection_to_text_len() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_caret(2);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::empty()
        };
        assert!(apply_key(&state, "End", shift));
        assert_eq!(state.caret(), 6);
        assert_eq!(state.selection_range(), Some((2, 6)));
    }

    #[test]
    fn r56_1_f_plain_arrow_left_with_selection_collapses_to_start() {
        // R56.1.f.1 / R56.1.f.2 — ArrowLeft (no Shift) with active
        // selection lands at selection-start (W3C "ArrowLeft on a
        // selection collapses to leading edge").
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_selection(1, 4);
        assert!(apply_key(&state, "ArrowLeft", Modifiers::empty()));
        assert_eq!(state.caret(), 1);
        assert_eq!(state.selection_anchor(), None);
    }

    #[test]
    fn r56_1_f_plain_arrow_right_with_selection_collapses_to_end() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_selection(1, 4);
        assert!(apply_key(&state, "ArrowRight", Modifiers::empty()));
        assert_eq!(state.caret(), 4);
        assert_eq!(state.selection_anchor(), None);
    }

    // ─────────────────────────────────────────────────────────────
    // Ctrl+A / Cmd+A select-all
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_f_ctrl_a_selects_entire_text() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_caret(2);
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::empty()
        };
        assert!(apply_key(&state, "a", ctrl));
        assert_eq!(state.selection_range(), Some((0, 6)));
        assert_eq!(state.caret(), 6);
    }

    #[test]
    fn r56_1_f_cmd_a_selects_entire_text_on_mac() {
        // macOS canonical: Cmd (= meta) instead of Ctrl. The same
        // binding fires on both modifier bits so apps do not need
        // platform-specific keymaps.
        let state = TextEditState::with_initial("abcdef".to_string());
        let meta = Modifiers {
            meta: true,
            ..Modifiers::empty()
        };
        assert!(apply_key(&state, "a", meta));
        assert_eq!(state.selection_range(), Some((0, 6)));
    }

    #[test]
    fn r56_1_f_plain_a_inserts_literal_does_not_select_all() {
        // Without a modifier, "a" is a plain insertion (R56.1.d
        // printable-char branch). The Ctrl/Meta gate is essential —
        // a missing gate would make typing "a" select-all.
        let state = TextEditState::with_initial("xy".to_string());
        state.set_caret(1);
        assert!(apply_key(&state, "a", Modifiers::empty()));
        assert_eq!(state.text(), "xay");
        assert_eq!(state.caret(), 2);
        assert_eq!(state.selection_anchor(), None);
    }

    #[test]
    fn r56_1_f_ctrl_alt_a_is_not_select_all() {
        // Ctrl+Alt is a separate chord (AltGr on European layouts);
        // refusing the select-all binding here keeps non-US keymap
        // typing safe.
        let state = TextEditState::with_initial("xy".to_string());
        state.set_caret(0);
        let ctrl_alt = Modifiers {
            ctrl: true,
            alt: true,
            ..Modifiers::empty()
        };
        // The key still recognises (printable-char branch fires);
        // selection-all branch passes through because alt is set.
        let _ = apply_key(&state, "a", ctrl_alt);
        // No selection set by the apply_key path.
        assert_eq!(state.selection_anchor(), None);
    }

    #[test]
    fn r932_1_ctrl_shift_z_redo_handles_uppercase_z() {
        // R932.1 — the undo chord routes through the shared `undo_redo_verb`
        // SSOT, which case-folds the key. The platform delivers uppercase "Z"
        // when Shift is held (TUI `Char(c)`, winit `Key::Character` both keep
        // case), so the prior hand-rolled `other == "z"` match silently dropped
        // Ctrl+Shift+Z redo. This guards the fix + the lower-case chords.
        let state = TextEditState::with_initial(String::new());
        state.attach_undo(std::rc::Rc::new(crate::undo::UndoStack::new()));
        assert!(apply_key(&state, "x", Modifiers::empty()), "type 'x'");
        assert_eq!(state.text(), "x");
        assert!(state.undo(), "undo the insert");
        assert_eq!(state.text(), "", "undone");
        // Ctrl+Shift+Z arrives as uppercase "Z" — must still redo.
        let ctrl_shift = Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::empty()
        };
        assert!(
            apply_key(&state, "Z", ctrl_shift),
            "Ctrl+Shift+Z is consumed"
        );
        assert_eq!(
            state.text(),
            "x",
            "Ctrl+Shift+Z redid the insert (uppercase Z case-folded)"
        );
        // Lower-case chords still work: Ctrl+Z undoes, Ctrl+Y redoes.
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::empty()
        };
        assert!(apply_key(&state, "z", ctrl), "Ctrl+Z consumed");
        assert_eq!(state.text(), "", "Ctrl+Z undid");
        assert!(apply_key(&state, "y", ctrl), "Ctrl+Y consumed");
        assert_eq!(state.text(), "x", "Ctrl+Y redid");
    }

    // ─────────────────────────────────────────────────────────────
    // Type-to-replace (printable / Space / Backspace / Delete on
    // active selection)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_f_printable_with_selection_replaces_range() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_selection(1, 4);
        assert!(apply_key(&state, "X", Modifiers::empty()));
        assert_eq!(state.text(), "aXef");
        assert_eq!(state.caret(), 2);
        assert_eq!(state.selection_anchor(), None);
    }

    #[test]
    fn r56_1_f_space_with_selection_replaces_range() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_selection(1, 4);
        assert!(apply_key(&state, "Space", Modifiers::empty()));
        assert_eq!(state.text(), "a ef");
        assert_eq!(state.caret(), 2);
    }

    #[test]
    fn r56_1_f_backspace_with_selection_drains_range() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_selection(1, 4);
        assert!(apply_key(&state, "Backspace", Modifiers::empty()));
        assert_eq!(state.text(), "aef");
        assert_eq!(state.caret(), 1);
    }

    #[test]
    fn r56_1_f_delete_with_selection_drains_range() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_selection(1, 4);
        assert!(apply_key(&state, "Delete", Modifiers::empty()));
        assert_eq!(state.text(), "aef");
        assert_eq!(state.caret(), 1);
    }

    // ─────────────────────────────────────────────────────────────
    // RPC invoke(key, Json) carries the Shift bit end-to-end
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_f_invoke_key_json_shift_arrow_right_extends_selection_via_rpc() {
        let state = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        state.set_caret(2);
        let mut tfx = TextFieldExternal::new().attach_state(state.clone());
        let r = tfx
            .invoke(
                "key",
                IntrospectValue::Json(serde_json::json!({
                    "key":   "ArrowRight",
                    "shift": true,
                })),
            )
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(state.caret(), 3);
        assert_eq!(state.selection_anchor(), Some(2));
    }

    #[test]
    fn r56_1_f_invoke_key_json_ctrl_a_selects_entire_text_via_rpc() {
        let state = Rc::new(TextEditState::with_initial("hello world".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(state.clone());
        let r = tfx
            .invoke(
                "key",
                IntrospectValue::Json(serde_json::json!({
                    "key":  "a",
                    "ctrl": true,
                })),
            )
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(state.selection_range(), Some((0, 11)));
    }

    // ─────────────────────────────────────────────────────────────
    // R56.1.f.3 §5.38 §5.22 — RPC selection introspect path
    // ─────────────────────────────────────────────────────────────

    use crate::external::InterveneError;

    #[test]
    fn r56_1_f_3_query_selection_returns_null_for_collapsed_caret() {
        let state = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        let tfx = TextFieldExternal::new().attach_state(state);
        assert_eq!(tfx.query("selection").unwrap(), IntrospectValue::Null);
    }

    #[test]
    fn r56_1_f_3_query_selection_returns_json_for_active_selection() {
        let state = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        state.set_selection(1, 4);
        let tfx = TextFieldExternal::new().attach_state(state);
        let v = tfx.query("selection").unwrap();
        let IntrospectValue::Json(obj) = v else {
            panic!("expected Json selection payload, got {v:?}");
        };
        assert_eq!(obj["start"], 1);
        assert_eq!(obj["end"], 4);
    }

    #[test]
    fn r56_1_f_3_query_selection_normalises_when_focus_before_anchor() {
        // anchor=4, focus=1 (mouse drag right-to-left); selection
        // surfaces as {start: 1, end: 4} regardless.
        let state = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        state.set_selection(4, 1);
        let tfx = TextFieldExternal::new().attach_state(state);
        let IntrospectValue::Json(obj) = tfx.query("selection").unwrap() else {
            panic!("expected Json payload");
        };
        assert_eq!(obj["start"], 1);
        assert_eq!(obj["end"], 4);
    }

    #[test]
    fn r56_1_f_3_query_selection_returns_none_for_bare_text_field() {
        // Bare TextField (no attached TextEditState) returns None
        // (path "selection" is unbound), distinguishing "no state"
        // from "no selection" for the RPC client.
        let tfx = TextFieldExternal::new();
        assert_eq!(tfx.query("selection"), None);
    }

    #[test]
    fn r56_1_f_3_intervene_selection_null_clears_anchor() {
        let state = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        state.set_selection(1, 4);
        let mut tfx = TextFieldExternal::new().attach_state(state.clone());
        tfx.intervene("selection", IntrospectValue::Null).unwrap();
        assert_eq!(state.selection_anchor(), None);
        assert_eq!(state.caret(), 4, "Null clears anchor, keeps caret");
    }

    #[test]
    fn r56_1_f_3_intervene_selection_json_sets_anchor_and_focus() {
        let state = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(state.clone());
        tfx.intervene(
            "selection",
            IntrospectValue::Json(serde_json::json!({ "start": 1, "end": 4 })),
        )
        .unwrap();
        assert_eq!(state.selection_range(), Some((1, 4)));
        assert_eq!(state.caret(), 4);
    }

    #[test]
    fn r56_1_f_3_intervene_selection_rejects_missing_start_field() {
        let state = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(state);
        let r = tfx.intervene(
            "selection",
            IntrospectValue::Json(serde_json::json!({ "end": 4 })),
        );
        assert_eq!(r, Err(InterveneError::TypeMismatch));
    }

    #[test]
    fn r56_1_f_3_intervene_selection_rejects_negative_offset() {
        let state = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(state);
        let r = tfx.intervene(
            "selection",
            IntrospectValue::Json(serde_json::json!({ "start": -1, "end": 4 })),
        );
        assert_eq!(r, Err(InterveneError::TypeMismatch));
    }

    #[test]
    fn r56_1_f_3_intervene_selection_text_args_rejects_with_type_mismatch() {
        let state = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(state);
        let r = tfx.intervene("selection", IntrospectValue::Text("1,4".to_string()));
        assert_eq!(r, Err(InterveneError::TypeMismatch));
    }

    #[test]
    fn r56_1_f_3_intervene_selection_read_only_on_bare_text_field() {
        let mut tfx = TextFieldExternal::new();
        let r = tfx.intervene(
            "selection",
            IntrospectValue::Json(serde_json::json!({ "start": 0, "end": 0 })),
        );
        assert_eq!(r, Err(InterveneError::ReadOnly));
    }
}

#[cfg(test)]
mod r56_1_e_tests {
    //! R56.1.e §5.22 §5.38 — Clipboard substrate + Ctrl/Cmd+C/X/V
    //! keystroke dispatch through the `TextField` `invoke("key", ...)`
    //! channel.

    use super::TextFieldExternal;
    use crate::clipboard::{Clipboard, InMemoryClipboard};
    use crate::external::{ExternalIntrospect, IntrospectValue, InvokeError};
    use crate::input::Modifiers;
    use crate::widgets::text_edit::TextEditState;
    use std::rc::Rc;

    fn make_tfx_with_clipboard(
        text: &str,
    ) -> (TextFieldExternal, Rc<TextEditState>, Rc<dyn Clipboard>) {
        let state = Rc::new(TextEditState::with_initial(text.to_string()));
        let cb: Rc<dyn Clipboard> = Rc::new(InMemoryClipboard::new());
        let tfx = TextFieldExternal::new()
            .attach_state(state.clone())
            .attach_clipboard(cb.clone());
        (tfx, state, cb)
    }

    fn json_key(key: &str, ctrl: bool) -> IntrospectValue {
        IntrospectValue::Json(serde_json::json!({
            "key": key,
            "ctrl": ctrl,
        }))
    }

    // ─────────────────────────────────────────────────────────────
    // Ctrl+C — copy selection to clipboard
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_e_ctrl_c_copies_selection_to_clipboard() {
        let (mut tfx, state, cb) = make_tfx_with_clipboard("hello world");
        state.set_selection(0, 5);
        let r = tfx.invoke("key", json_key("c", true)).unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(cb.paste(), Some("hello".to_string()));
        // Selection survives copy (the canonical UX — copy does not
        // collapse the selection).
        assert_eq!(state.selection_text(), Some("hello".to_string()));
        // Text unchanged.
        assert_eq!(state.text(), "hello world");
    }

    #[test]
    fn r56_1_e_ctrl_c_with_no_selection_is_handled_but_no_copy() {
        let (mut tfx, _state, cb) = make_tfx_with_clipboard("hello");
        // No selection — Ctrl+C still consumes the key but the
        // clipboard payload stays at None.
        let r = tfx.invoke("key", json_key("c", true)).unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(cb.paste(), None);
    }

    // ─────────────────────────────────────────────────────────────
    // Ctrl+X — cut (copy + delete selection)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_e_ctrl_x_cuts_selection_into_clipboard() {
        let (mut tfx, state, cb) = make_tfx_with_clipboard("hello world");
        state.set_selection(6, 11);
        let r = tfx.invoke("key", json_key("x", true)).unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(cb.paste(), Some("world".to_string()));
        // Selection drained from text.
        assert_eq!(state.text(), "hello ");
        assert_eq!(state.caret(), 6);
        assert_eq!(state.selection_anchor(), None);
    }

    #[test]
    fn r56_1_e_ctrl_x_with_no_selection_is_handled_but_no_change() {
        let (mut tfx, state, cb) = make_tfx_with_clipboard("hello");
        let r = tfx.invoke("key", json_key("x", true)).unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(cb.paste(), None);
        assert_eq!(state.text(), "hello");
    }

    // ─────────────────────────────────────────────────────────────
    // Ctrl+V — paste (insert clipboard at caret, replace selection)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_e_ctrl_v_inserts_clipboard_payload_at_caret() {
        let (mut tfx, state, cb) = make_tfx_with_clipboard("abc");
        state.set_caret(3);
        cb.copy("XY".to_string());
        let r = tfx.invoke("key", json_key("v", true)).unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(state.text(), "abcXY");
        assert_eq!(state.caret(), 5);
    }

    #[test]
    fn r56_1_e_ctrl_v_with_active_selection_replaces_range() {
        let (mut tfx, state, cb) = make_tfx_with_clipboard("hello world");
        state.set_selection(0, 5);
        cb.copy("HI".to_string());
        let r = tfx.invoke("key", json_key("v", true)).unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        // Selection drained + clipboard payload inserted.
        assert_eq!(state.text(), "HI world");
        assert_eq!(state.caret(), 2);
        assert_eq!(state.selection_anchor(), None);
    }

    #[test]
    fn r56_1_e_ctrl_v_with_empty_clipboard_is_handled_but_no_change() {
        let (mut tfx, state, _cb) = make_tfx_with_clipboard("abc");
        // Clipboard never written — paste is a no-op (recognised).
        let r = tfx.invoke("key", json_key("v", true)).unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(state.text(), "abc");
    }

    // ─────────────────────────────────────────────────────────────
    // Meta+C/X/V — macOS Cmd+C canonical
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_e_meta_c_copies_on_macos() {
        // Cmd+C on macOS is the same binding as Ctrl+C elsewhere; the
        // same Ctrl-OR-Meta gate fires for both.
        let (mut tfx, state, cb) = make_tfx_with_clipboard("hello");
        state.set_selection(0, 5);
        let r = tfx
            .invoke(
                "key",
                IntrospectValue::Json(serde_json::json!({
                    "key": "c",
                    "meta": true,
                })),
            )
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(cb.paste(), Some("hello".to_string()));
    }

    // ─────────────────────────────────────────────────────────────
    // No clipboard attached — plain 'c' / 'x' / 'v' insert literally
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_e_bare_textfield_clipboard_keys_fall_through_to_plain_insert() {
        // No clipboard attached but a state is — Ctrl+C is rejected
        // by the clipboard branch (no Rc) so the dispatcher falls
        // through to the printable-char `apply_key` path. apply_key
        // does NOT consume Ctrl-modified keys (no select-all gate
        // for "c"), so the result is Bool(false).
        let state = Rc::new(TextEditState::with_initial("abc".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(state.clone());
        let r = tfx.invoke("key", json_key("c", true)).unwrap();
        assert_eq!(
            r,
            IntrospectValue::Bool(false),
            "no clipboard attached ⇒ Ctrl+C unhandled",
        );
        assert_eq!(state.text(), "abc", "no mutation");
    }

    #[test]
    fn r56_1_e_plain_c_x_v_without_modifier_inserts_literal() {
        // No modifier — plain printable insert path (R56.1.d) fires.
        let (mut tfx, state, cb) = make_tfx_with_clipboard("");
        for ch in ['c', 'x', 'v'] {
            let r = tfx
                .invoke("key", IntrospectValue::Text(ch.to_string()))
                .unwrap();
            assert_eq!(r, IntrospectValue::Bool(true));
        }
        assert_eq!(state.text(), "cxv");
        // Clipboard untouched.
        assert_eq!(cb.paste(), None);
    }

    // ─────────────────────────────────────────────────────────────
    // Ctrl+Alt+c — refused (AltGr safety)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_e_ctrl_alt_c_does_not_trigger_copy() {
        // Ctrl+Alt is a separate AltGr-style chord on European
        // layouts; refusing the clipboard binding keeps non-US
        // typing safe (mirror of R56.1.f.2 select-all gate).
        let (mut tfx, state, cb) = make_tfx_with_clipboard("hello");
        state.set_selection(0, 5);
        let mods_ctrl_alt = Modifiers {
            ctrl: true,
            alt: true,
            ..Modifiers::empty()
        };
        let _ = mods_ctrl_alt; // documented gate; the modifier shape
        // routes through the Json arg path.
        let r = tfx
            .invoke(
                "key",
                IntrospectValue::Json(serde_json::json!({
                    "key":  "c",
                    "ctrl": true,
                    "alt":  true,
                })),
            )
            .unwrap();
        // The plain-printable arm rejects (Ctrl+Alt+c is not a
        // recognised select-all / clipboard binding) and the
        // clipboard branch's modifier gate refuses (alt set).
        assert_eq!(r, IntrospectValue::Bool(false));
        assert_eq!(cb.paste(), None);
    }

    // ─────────────────────────────────────────────────────────────
    // Builder round-trip
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_e_attach_clipboard_is_readable_back() {
        let state = Rc::new(TextEditState::with_initial(String::new()));
        let cb: Rc<dyn Clipboard> = Rc::new(InMemoryClipboard::new());
        let tfx = TextFieldExternal::new()
            .attach_state(state)
            .attach_clipboard(cb.clone());
        assert!(tfx.clipboard().is_some());
        // Round-trip the Rc pointer to confirm the handle is the
        // exact instance the builder accepted (not a clone-of-Rc).
        let attached = tfx.clipboard().unwrap();
        assert!(Rc::ptr_eq(attached, &cb));
    }

    #[test]
    fn r56_1_e_fresh_textfield_has_no_clipboard() {
        let tfx = TextFieldExternal::new();
        assert!(tfx.clipboard().is_none());
    }

    // ─────────────────────────────────────────────────────────────
    // R56.2.e §5.22 — auto-publish active selection to PRIMARY
    // ─────────────────────────────────────────────────────────────

    use crate::clipboard::ClipboardSelection;

    #[test]
    fn r56_2_e_ctrl_a_select_all_publishes_primary() {
        // Ctrl+A select-all is the canonical "selection mutation"
        // that the auto-publish hook covers; the entire text lands
        // on PRIMARY so an out-of-process middle-click in another
        // app pastes the whole field.
        let (mut tfx, state, cb) = make_tfx_with_clipboard("hello world");
        // PRIMARY starts empty.
        assert_eq!(cb.paste_from(ClipboardSelection::Primary), None);
        let r = tfx.invoke("key", json_key("a", true)).unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(state.selection_text(), Some("hello world".to_string()),);
        assert_eq!(
            cb.paste_from(ClipboardSelection::Primary),
            Some("hello world".to_string()),
        );
        // CLIPBOARD untouched — auto-publish targets PRIMARY only.
        assert_eq!(cb.paste(), None);
    }

    #[test]
    fn r56_2_e_shift_arrow_right_publishes_primary() {
        // Shift+ArrowRight extends the selection one char; the
        // auto-publish hook updates PRIMARY on every extension so
        // a Linux desktop user observes the latest selection in
        // PRIMARY without an explicit Ctrl+C.
        let (mut tfx, state, cb) = make_tfx_with_clipboard("hello");
        state.set_caret(0);
        let r = tfx
            .invoke(
                "key",
                IntrospectValue::Json(serde_json::json!({
                    "key": "ArrowRight",
                    "shift": true,
                })),
            )
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(state.selection_text(), Some("h".to_string()));
        assert_eq!(
            cb.paste_from(ClipboardSelection::Primary),
            Some("h".to_string()),
        );
    }

    #[test]
    fn r56_2_e_plain_typing_does_not_publish_primary() {
        // Plain 'x' insertion does not create a selection — the
        // hook fires but finds selection_text == None and skips
        // the write. PRIMARY stays whatever it was (None when
        // never published).
        let (mut tfx, state, cb) = make_tfx_with_clipboard("");
        let r = tfx
            .invoke(
                "key",
                IntrospectValue::Json(serde_json::json!({
                    "key": "x",
                })),
            )
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(state.text(), "x");
        assert_eq!(cb.paste_from(ClipboardSelection::Primary), None);
    }

    #[test]
    fn r56_2_e_plain_arrow_collapse_preserves_prior_primary() {
        // X11 convention: PRIMARY retains the last selection until a
        // new selection is published. Plain ArrowRight collapses any
        // active selection but must NOT clear PRIMARY.
        let (mut tfx, state, cb) = make_tfx_with_clipboard("hello");
        state.set_selection(0, 5);
        // Seed PRIMARY through Ctrl+A first (canonical publish).
        let _ = tfx.invoke("key", json_key("a", true)).unwrap();
        assert_eq!(
            cb.paste_from(ClipboardSelection::Primary),
            Some("hello".to_string()),
        );
        // Plain ArrowRight collapses to the trailing edge.
        let r = tfx
            .invoke(
                "key",
                IntrospectValue::Json(serde_json::json!({
                    "key": "ArrowRight",
                })),
            )
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(state.selection_text(), None);
        // PRIMARY still holds the prior selection.
        assert_eq!(
            cb.paste_from(ClipboardSelection::Primary),
            Some("hello".to_string()),
        );
    }

    #[test]
    fn r56_2_e_rpc_intervene_selection_publishes_primary() {
        // AI client driving selection via the RPC intervene path
        // gets the same PRIMARY auto-publish behaviour the
        // keyboard-driven user sees.
        let (mut tfx, _state, cb) = make_tfx_with_clipboard("hello world");
        tfx.intervene(
            "selection",
            IntrospectValue::Json(serde_json::json!({
                "start": 6,
                "end": 11,
            })),
        )
        .unwrap();
        assert_eq!(
            cb.paste_from(ClipboardSelection::Primary),
            Some("world".to_string()),
        );
    }

    #[test]
    fn r56_2_e_rpc_intervene_null_does_not_clear_primary() {
        // intervene("selection", Null) collapses the selection but
        // PRIMARY stays at the previous value (X11 convention).
        let (mut tfx, state, cb) = make_tfx_with_clipboard("hello");
        state.set_selection(0, 5);
        let _ = tfx.invoke("key", json_key("a", true)).unwrap();
        assert_eq!(
            cb.paste_from(ClipboardSelection::Primary),
            Some("hello".to_string()),
        );
        tfx.intervene("selection", IntrospectValue::Null).unwrap();
        assert_eq!(
            cb.paste_from(ClipboardSelection::Primary),
            Some("hello".to_string()),
        );
    }

    #[test]
    fn r56_2_e_paste_from_primary_inserts_at_caret() {
        // The shell's middle-click handler calls paste_from_primary;
        // this test pins the substrate behaviour end-to-end without
        // a shell.
        let (mut tfx, state, cb) = make_tfx_with_clipboard("ab");
        state.set_caret(2);
        cb.copy_to(ClipboardSelection::Primary, "XY".to_string());
        let inserted = tfx.paste_from_primary();
        assert!(inserted, "non-empty PRIMARY must produce true");
        assert_eq!(state.text(), "abXY");
        assert_eq!(state.caret(), 4);
    }

    #[test]
    fn r56_2_e_paste_from_primary_with_empty_primary_is_noop() {
        // PRIMARY never published → returns false, text unchanged.
        let (mut tfx, state, _cb) = make_tfx_with_clipboard("ab");
        let inserted = tfx.paste_from_primary();
        assert!(!inserted);
        assert_eq!(state.text(), "ab");
    }

    #[test]
    fn r56_2_e_paste_from_primary_with_no_clipboard_returns_false() {
        // No clipboard attached — paste_from_primary must short-
        // circuit to false even when a state is present.
        let state = Rc::new(TextEditState::with_initial("hello".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(state.clone());
        assert!(!tfx.paste_from_primary());
        assert_eq!(state.text(), "hello");
    }

    #[test]
    fn r56_2_e_paste_from_primary_with_no_state_returns_false() {
        // No state attached — paste_from_primary short-circuits to
        // false even with a populated clipboard.
        let cb: Rc<dyn Clipboard> = Rc::new(InMemoryClipboard::new());
        cb.copy_to(ClipboardSelection::Primary, "noop".to_string());
        let mut tfx = TextFieldExternal::new().attach_clipboard(cb);
        assert!(!tfx.paste_from_primary());
    }

    #[test]
    fn r56_2_e_paste_from_primary_replaces_active_selection() {
        // X11 / GTK middle-click paste replaces the active selection
        // before inserting (TextEditState::insert is selection-aware).
        let (mut tfx, state, cb) = make_tfx_with_clipboard("hello world");
        state.set_selection(0, 5);
        cb.copy_to(ClipboardSelection::Primary, "HI".to_string());
        let inserted = tfx.paste_from_primary();
        assert!(inserted);
        assert_eq!(state.text(), "HI world");
        assert_eq!(state.caret(), 2);
        assert_eq!(state.selection_anchor(), None);
    }

    #[test]
    fn r56_2_e_paste_from_primary_independent_of_clipboard() {
        // PRIMARY and CLIPBOARD are independent. paste_from_primary
        // reads PRIMARY only; a populated CLIPBOARD with empty
        // PRIMARY must produce a no-op.
        let (mut tfx, state, cb) = make_tfx_with_clipboard("ab");
        state.set_caret(2);
        cb.copy("from-clipboard".to_string());
        // PRIMARY untouched.
        let inserted = tfx.paste_from_primary();
        assert!(!inserted);
        assert_eq!(state.text(), "ab");
    }

    #[test]
    fn r56_2_e_invoke_paste_primary_routes_through_substrate() {
        // R56.2.e.3 §5.22 — the shell's middle-click handler funnels
        // through `invoke("paste-primary", Null)`. Verify the wire
        // round-trip: PRIMARY publish + invoke read-back lands the
        // same payload `paste_from_primary` would.
        let (mut tfx, state, cb) = make_tfx_with_clipboard("ab");
        state.set_caret(2);
        cb.copy_to(ClipboardSelection::Primary, "XY".to_string());
        let r = tfx.invoke("paste-primary", IntrospectValue::Null).unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert_eq!(state.text(), "abXY");
        assert_eq!(state.caret(), 4);
    }

    #[test]
    fn r56_2_e_invoke_paste_primary_empty_primary_returns_false() {
        // The substrate's `paste_from_primary` returns false for
        // empty / unavailable PRIMARY; the invoke wire propagates the
        // bool verbatim.
        let (mut tfx, state, _cb) = make_tfx_with_clipboard("ab");
        let r = tfx.invoke("paste-primary", IntrospectValue::Null).unwrap();
        assert_eq!(r, IntrospectValue::Bool(false));
        assert_eq!(state.text(), "ab");
    }

    #[test]
    fn r56_2_e_invoke_paste_primary_rejects_non_null_arg() {
        // R56.2.e.3 — defensive against wire-shape drift. The
        // `paste-primary` slot takes `Null` only; any other variant
        // returns `TypeMismatch` so AI clients sending a stray
        // Text("paste") payload land an immediate error.
        let (mut tfx, _state, _cb) = make_tfx_with_clipboard("ab");
        let err = tfx
            .invoke("paste-primary", IntrospectValue::Text("paste".to_string()))
            .unwrap_err();
        assert!(matches!(err, InvokeError::TypeMismatch));
    }

    #[test]
    fn r56_2_e_primary_publish_is_independent_of_ctrl_c() {
        // Ctrl+A publishes the new selection to PRIMARY immediately,
        // before any Ctrl+C — this is the "select to publish" Linux
        // convention. A follow-on Ctrl+C copies to CLIPBOARD without
        // disturbing PRIMARY.
        let (mut tfx, _state, cb) = make_tfx_with_clipboard("hello");
        let _ = tfx.invoke("key", json_key("a", true)).unwrap();
        assert_eq!(
            cb.paste_from(ClipboardSelection::Primary),
            Some("hello".to_string()),
        );
        assert_eq!(cb.paste(), None);

        let _ = tfx.invoke("key", json_key("c", true)).unwrap();
        // CLIPBOARD now matches PRIMARY (same payload).
        assert_eq!(cb.paste(), Some("hello".to_string()));
        assert_eq!(
            cb.paste_from(ClipboardSelection::Primary),
            Some("hello".to_string()),
        );
    }
}

#[cfg(test)]
mod r56_1_h_tests {
    //! R56.1.h §5.38 §5.39 §5.28 — focus/blur lifecycle wire:
    //! [`TextField::attach_blink`] composition + `sync_blink` statechart
    //! sync + [`TextFieldExternal::on_focus_change`] external focus
    //! drive. The shell-substrate side (focus mgr ↔
    //! [`External::on_focus_change`] wire) is tested in
    //! `pinion-shell/tests/focus_lifecycle_wire.rs`.

    use super::{TextField, TextFieldEvent, TextFieldExternal, TextFieldState};
    use crate::external::{External, IntrospectValue};
    use crate::widgets::caret_blink::CaretBlink;
    use std::rc::Rc;

    // ─────────────────────────────────────────────────────────────
    // TextField::attach_blink composition
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_h_bare_text_field_has_no_blink() {
        let tf = TextField::new();
        assert!(tf.blink().is_none());
    }

    #[test]
    fn r56_1_h_attach_blink_records_handle() {
        let blink = Rc::new(CaretBlink::new());
        let tf = TextField::new().attach_blink(Rc::clone(&blink));
        assert!(tf.blink().is_some());
        assert!(Rc::ptr_eq(tf.blink().unwrap(), &blink));
    }

    #[test]
    fn r56_1_h_external_bare_has_no_blink() {
        let tfx = TextFieldExternal::new();
        assert!(tfx.blink().is_none());
    }

    #[test]
    fn r56_1_h_external_attach_blink_records_handle() {
        let blink = Rc::new(CaretBlink::new());
        let tfx = TextFieldExternal::new().attach_blink(Rc::clone(&blink));
        assert!(tfx.blink().is_some());
        assert!(Rc::ptr_eq(tfx.blink().unwrap(), &blink));
    }

    // ─────────────────────────────────────────────────────────────
    // sync_blink — statechart state ↔ blink enabled gate
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_h_attach_blink_on_idle_disables_initial_sync() {
        let blink = Rc::new(CaretBlink::new());
        // Caret blink defaults to disabled (R56.1.c contract); the
        // attach-time sync should preserve that (Idle → disabled).
        assert!(!blink.enabled(), "default disabled");
        let _tf = TextField::new().attach_blink(Rc::clone(&blink));
        assert!(!blink.enabled(), "Idle attach keeps blink disabled");
    }

    #[test]
    fn r56_1_h_attach_blink_on_focused_enables_initial_sync() {
        // Pre-driven Focused state — attach must reconcile to enabled.
        let blink = Rc::new(CaretBlink::new());
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        assert_eq!(tf.state(), TextFieldState::Focused);
        let _tf = tf.attach_blink(Rc::clone(&blink));
        assert!(blink.enabled(), "Focused attach enables blink at once");
    }

    #[test]
    fn r56_1_h_focus_event_enables_blink() {
        let blink = Rc::new(CaretBlink::new());
        let mut tf = TextField::new().attach_blink(Rc::clone(&blink));
        assert!(!blink.enabled());
        tf.send(TextFieldEvent::Focus);
        assert!(blink.enabled(), "Idle→Focused enables blink");
    }

    #[test]
    fn r56_1_h_blur_event_disables_blink() {
        let blink = Rc::new(CaretBlink::new());
        let mut tf = TextField::new().attach_blink(Rc::clone(&blink));
        tf.send(TextFieldEvent::Focus);
        assert!(blink.enabled());
        tf.send(TextFieldEvent::Blur);
        assert!(!blink.enabled(), "Focused→Idle disables blink");
    }

    #[test]
    fn r56_1_h_begin_edit_keeps_blink_enabled() {
        // Editing inherits the enabled gate — IME composition still
        // shows the caret (Wayland text-input-v3 / GTK IBus / macOS).
        let blink = Rc::new(CaretBlink::new());
        let mut tf = TextField::new().attach_blink(Rc::clone(&blink));
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        assert_eq!(tf.state(), TextFieldState::Editing);
        assert!(blink.enabled(), "Editing keeps caret blinking");
    }

    #[test]
    fn r56_1_h_commit_edit_keeps_blink_enabled() {
        let blink = Rc::new(CaretBlink::new());
        let mut tf = TextField::new().attach_blink(Rc::clone(&blink));
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::CommitEdit);
        assert_eq!(tf.state(), TextFieldState::Focused);
        assert!(blink.enabled(), "Editing→Focused keeps blink enabled");
    }

    #[test]
    fn r56_1_h_cancel_edit_keeps_blink_enabled() {
        let blink = Rc::new(CaretBlink::new());
        let mut tf = TextField::new().attach_blink(Rc::clone(&blink));
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::CancelEdit);
        assert_eq!(tf.state(), TextFieldState::Focused);
        assert!(blink.enabled(), "Editing→Focused (cancel) keeps blink");
    }

    #[test]
    fn r56_1_h_blur_from_editing_disables_blink() {
        // Editing→Idle via Blur — caret disappears (focus loss commits
        // preedit then hides the widget per R56.1.a contract).
        let blink = Rc::new(CaretBlink::new());
        let mut tf = TextField::new().attach_blink(Rc::clone(&blink));
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::Blur);
        assert_eq!(tf.state(), TextFieldState::Idle);
        assert!(!blink.enabled(), "Editing→Idle (blur) disables blink");
    }

    #[test]
    fn r56_1_h_disable_from_focused_disables_blink() {
        let blink = Rc::new(CaretBlink::new());
        let mut tf = TextField::new().attach_blink(Rc::clone(&blink));
        tf.send(TextFieldEvent::Focus);
        assert!(blink.enabled());
        tf.send(TextFieldEvent::Disable);
        assert_eq!(tf.state(), TextFieldState::Disabled);
        assert!(!blink.enabled(), "Focused→Disabled disables blink");
    }

    #[test]
    fn r56_1_h_enable_from_disabled_keeps_blink_disabled() {
        // Disabled→Idle (via Enable) returns to Idle, not Focused —
        // the blink stays disabled. Application must re-focus to
        // re-enable.
        let blink = Rc::new(CaretBlink::new());
        let mut tf = TextField::new().attach_blink(Rc::clone(&blink));
        tf.send(TextFieldEvent::Disable);
        tf.send(TextFieldEvent::Enable);
        assert_eq!(tf.state(), TextFieldState::Idle);
        assert!(!blink.enabled(), "Disabled→Idle (enable) keeps blink off");
    }

    #[test]
    fn r56_1_h_bare_text_field_send_does_not_panic_without_blink() {
        // No-op sync_blink on bare TextField — regression guard.
        let mut tf = TextField::new();
        for ev in [
            TextFieldEvent::Focus,
            TextFieldEvent::BeginEdit,
            TextFieldEvent::CommitEdit,
            TextFieldEvent::Blur,
            TextFieldEvent::Disable,
            TextFieldEvent::Enable,
        ] {
            tf.send(ev);
        }
        // Final state is Idle (Disable→Enable round trip ends in Idle).
        assert_eq!(tf.state(), TextFieldState::Idle);
    }

    // ─────────────────────────────────────────────────────────────
    // TextFieldExternal::on_focus_change — External trait override
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_h_on_focus_change_true_drives_focus_event() {
        let mut tfx = TextFieldExternal::new();
        assert_eq!(tfx.state(), TextFieldState::Idle);
        tfx.on_focus_change(true);
        assert_eq!(tfx.state(), TextFieldState::Focused);
    }

    #[test]
    fn r56_1_h_on_focus_change_false_drives_blur_event() {
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        assert_eq!(tfx.state(), TextFieldState::Focused);
        tfx.on_focus_change(false);
        assert_eq!(tfx.state(), TextFieldState::Idle);
    }

    #[test]
    fn r56_1_h_on_focus_change_false_during_editing_commits() {
        // IME canonical: focus loss during composition commits the
        // preedit (raises textfield.commit → text_committed intent).
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::BeginEdit);
        assert_eq!(tfx.state(), TextFieldState::Editing);
        // Clear any prior queued intents.
        let mut prior = Vec::new();
        tfx.drain_intents(&mut |i| prior.push(i));
        // Now drive blur via on_focus_change — should both transition
        // and emit text_committed.
        tfx.on_focus_change(false);
        assert_eq!(tfx.state(), TextFieldState::Idle);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "text_committed");
    }

    #[test]
    fn r56_1_h_on_focus_change_on_disabled_is_no_op() {
        // SCXML rejects focus / blur from Disabled (no transition
        // declared). The dispatch consumes the event but the state
        // stays put.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Disable);
        assert_eq!(tfx.state(), TextFieldState::Disabled);
        tfx.on_focus_change(true);
        assert_eq!(
            tfx.state(),
            TextFieldState::Disabled,
            "Disabled absorbs focus",
        );
        tfx.on_focus_change(false);
        assert_eq!(
            tfx.state(),
            TextFieldState::Disabled,
            "Disabled absorbs blur",
        );
    }

    #[test]
    fn r56_1_h_on_focus_change_syncs_blink_through_external() {
        // End-to-end: External::on_focus_change → IntentEmitter::dispatch
        // → TextField::send → sync_blink → CaretBlink::set_enabled.
        let blink = Rc::new(CaretBlink::new());
        let mut tfx = TextFieldExternal::new().attach_blink(Rc::clone(&blink));
        assert!(!blink.enabled(), "Idle bare blink stays disabled");
        tfx.on_focus_change(true);
        assert!(blink.enabled(), "focus=true enables blink");
        tfx.on_focus_change(false);
        assert!(!blink.enabled(), "focus=false disables blink");
    }

    #[test]
    fn r56_1_h_on_focus_change_true_emits_no_intent() {
        // Idle→Focused is silent (no text_committed) — sanity guard
        // that the focus-drive doesn't spuriously raise commits.
        let mut tfx = TextFieldExternal::new();
        tfx.on_focus_change(true);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "Idle→Focused must not raise text_committed",
        );
    }

    #[test]
    fn r56_1_h_on_focus_change_blur_from_focused_emits_no_intent() {
        // Focused→Idle (without Editing) is silent.
        let mut tfx = TextFieldExternal::new();
        tfx.on_focus_change(true);
        // drain Idle→Focused (no intents).
        let mut prior = Vec::new();
        tfx.drain_intents(&mut |i| prior.push(i));
        tfx.on_focus_change(false);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "Focused→Idle without Editing must not raise text_committed",
        );
    }

    #[test]
    fn r793_blur_intent_opt_in_emits_on_focus_loss() {
        // R793 — an opted-in editor field emits a "blur" intent on every
        // focus loss (the DOM focusout mirror), even from plain Focused
        // (no IME composition) where text_committed stays silent.
        let mut tfx = TextFieldExternal::new().with_blur_intent();
        tfx.on_focus_change(true);
        let mut prior = Vec::new();
        tfx.drain_intents(&mut |i| prior.push(i));
        assert!(prior.is_empty(), "focus-in emits no blur");
        tfx.on_focus_change(false);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1, "one blur intent on focus loss");
        assert_eq!(harvested[0].tag_str(), "blur");
    }

    #[test]
    fn r793_blur_intent_is_off_by_default() {
        // Without opting in, a plain field stays silent on blur (zero blast
        // radius on every existing TextField consumer).
        let mut tfx = TextFieldExternal::new();
        tfx.on_focus_change(true);
        let mut prior = Vec::new();
        tfx.drain_intents(&mut |i| prior.push(i));
        tfx.on_focus_change(false);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty(), "default field emits no blur intent");
    }

    // ─────────────────────────────────────────────────────────────
    // Composition — attach_state + attach_blink interplay
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_h_attach_blink_after_attach_state_preserves_text_state() {
        use crate::widgets::text_edit::TextEditState;
        let text = Rc::new(TextEditState::with_initial("abc".to_string()));
        let blink = Rc::new(CaretBlink::new());
        let tf = TextField::new()
            .attach_state(Rc::clone(&text))
            .attach_blink(Rc::clone(&blink));
        // Both handles present; state and blink independent.
        assert!(tf.text_state().is_some());
        assert!(tf.blink().is_some());
        assert!(Rc::ptr_eq(tf.text_state().unwrap(), &text));
        assert!(Rc::ptr_eq(tf.blink().unwrap(), &blink));
    }

    #[test]
    fn r56_1_h_external_focus_drive_does_not_disturb_text_state() {
        // R56.1.h focus drive must not perturb attached text content
        // (R56.1.b orthogonal sidecar contract).
        use crate::widgets::text_edit::TextEditState;
        let text = Rc::new(TextEditState::with_initial("hello".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&text));
        let initial = text.text();
        tfx.on_focus_change(true);
        tfx.on_focus_change(false);
        tfx.on_focus_change(true);
        assert_eq!(text.text(), initial, "focus drive leaves text alone");
    }

    #[test]
    fn r56_1_h_introspect_state_reflects_external_focus_drive() {
        // Sanity: the introspect `state` query reflects the post-
        // on_focus_change state, exactly as it would for a manual
        // `invoke("send", Text("Focus"))`.
        use crate::external::ExternalIntrospect;
        let mut tfx = TextFieldExternal::new();
        tfx.on_focus_change(true);
        assert_eq!(
            tfx.query("state").unwrap(),
            IntrospectValue::Text("Focused".to_string()),
        );
        tfx.on_focus_change(false);
        assert_eq!(
            tfx.query("state").unwrap(),
            IntrospectValue::Text("Idle".to_string()),
        );
    }
}

#[cfg(test)]
mod r56_1_j_tests {
    //! R56.1.j §5.38 §5.28 — caret blink reset on recognized keystroke.
    //!
    //! Pins the macOS / iOS / GTK / Web canonical UX contract: the
    //! caret stays solid while the user is typing or navigating, then
    //! resumes blinking once they pause. Implementation lives in
    //! [`TextFieldExternal::invoke`]'s `"key"` arm — handled keys call
    //! [`CaretBlink::reset`] on the attached blink (no-op when no
    //! blink is attached or when the key was unrecognized).
    //!
    //! The substrate-level reset contract (timer back to 0.0, visible
    //! snaps to true while the blink is enabled, no-op while disabled)
    //! is owned by [`CaretBlink::reset`] tests in
    //! `widgets/caret_blink.rs`; this module verifies only the
    //! wiring from the keystroke surface into that contract.

    use super::{TextFieldEvent, TextFieldExternal};
    use crate::animation::Tickable;
    use crate::external::{ExternalIntrospect, IntrospectValue};
    use crate::widgets::caret_blink::CaretBlink;
    use crate::widgets::text_edit::TextEditState;
    use std::rc::Rc;

    /// Build a focused `TextFieldExternal` with both reactive sidecars
    /// attached and the blink driven into the hidden phase, so a
    /// successful `reset()` is observable as a visibility flip back to
    /// `true`.
    fn focused_external_with_hidden_blink() -> (TextFieldExternal, Rc<CaretBlink>) {
        let text = Rc::new(TextEditState::new());
        let blink = Rc::new(CaretBlink::new());
        let mut tfx = TextFieldExternal::new()
            .attach_state(Rc::clone(&text))
            .attach_blink(Rc::clone(&blink));
        tfx.send(TextFieldEvent::Focus);
        // Drive the blink past one full period — `tick(0.6)` exceeds
        // the 0.530 s `PERIOD_SECS`, so the visible phase flips from
        // the post-Focus `true` to `false`. `reset()` must then snap
        // it back to `true`.
        blink.tick(0.6);
        assert!(!blink.visible(), "fixture: blink driven into hidden phase");
        (tfx, blink)
    }

    // ─────────────────────────────────────────────────────────────
    // Recognized key resets the blink
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_j_printable_key_resets_blink_to_visible() {
        let (mut tfx, blink) = focused_external_with_hidden_blink();
        let r = tfx
            .invoke("key", IntrospectValue::Text("h".to_string()))
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true), "printable key recognized");
        assert!(
            blink.visible(),
            "recognized key snaps blink back to visible"
        );
    }

    #[test]
    fn r56_1_j_backspace_resets_blink() {
        // Backspace at caret 0 is a recognized no-op (returns true) —
        // still resets the blink because the user interacted with the
        // field. Mirrors the W3C `defaultPrevented` semantic.
        let (mut tfx, blink) = focused_external_with_hidden_blink();
        let r = tfx
            .invoke("key", IntrospectValue::Text("Backspace".to_string()))
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert!(
            blink.visible(),
            "Backspace resets even when it's a caret-0 no-op"
        );
    }

    #[test]
    fn r56_1_j_arrow_left_resets_blink() {
        // Navigation keys reset too — the user is "moving the cursor",
        // a visible interaction with the field. macOS Cocoa /
        // `NSTextView` / GTK `GtkEntry` all reset on arrow-key navigation.
        let (mut tfx, blink) = focused_external_with_hidden_blink();
        let r = tfx
            .invoke("key", IntrospectValue::Text("ArrowLeft".to_string()))
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert!(blink.visible(), "ArrowLeft resets the blink phase");
    }

    #[test]
    fn r56_1_j_home_resets_blink() {
        let (mut tfx, blink) = focused_external_with_hidden_blink();
        let r = tfx
            .invoke("key", IntrospectValue::Text("Home".to_string()))
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert!(blink.visible());
    }

    #[test]
    fn r56_1_j_space_resets_blink() {
        let (mut tfx, blink) = focused_external_with_hidden_blink();
        let r = tfx
            .invoke("key", IntrospectValue::Text("Space".to_string()))
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
        assert!(blink.visible());
    }

    // ─────────────────────────────────────────────────────────────
    // Unrecognized key does NOT reset
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_j_unrecognized_key_does_not_reset_blink() {
        // F1 / ArrowUp / Enter — apply_key returns false (R56.1.d
        // rejection list). The user did not interact with the field's
        // content, so the blink phase stays where it was (hidden, in
        // this fixture).
        let (mut tfx, blink) = focused_external_with_hidden_blink();
        for unrecognized in ["F1", "ArrowUp", "Enter", "Escape", "Tab"] {
            // Reset the blink to known hidden state between iterations
            // by ticking past the period again (Focus's initial reset
            // happens only once; subsequent ticks keep cycling).
            assert_eq!(
                tfx.invoke("key", IntrospectValue::Text(unrecognized.to_string()))
                    .unwrap(),
                IntrospectValue::Bool(false),
                "{unrecognized:?} must be unrecognized",
            );
            assert!(
                !blink.visible(),
                "unrecognized key {unrecognized:?} must not reset the blink",
            );
        }
    }

    // ─────────────────────────────────────────────────────────────
    // Bare TextField (no blink attached) safely no-ops
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_j_bare_text_field_no_panic_on_recognized_key() {
        // No attached blink — the reset chain `blink().map(...)` must
        // not panic. Returns the same Bool(false) the R56.1.d bare
        // path returned (the bare TextField has no TextEditState
        // either, so apply_key fails).
        let mut tfx = TextFieldExternal::new();
        let r = tfx
            .invoke("key", IntrospectValue::Text("h".to_string()))
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(false));
    }

    #[test]
    fn r56_1_j_text_state_attached_blink_unattached_no_panic() {
        // TextEditState attached but no blink — recognized key handled
        // normally, reset chain silently no-ops because blink() is None.
        let text = Rc::new(TextEditState::new());
        let mut tfx = TextFieldExternal::new().attach_state(text);
        let r = tfx
            .invoke("key", IntrospectValue::Text("h".to_string()))
            .unwrap();
        assert_eq!(r, IntrospectValue::Bool(true));
    }
}

#[cfg(test)]
mod r56_1_g_tests {
    //! R56.1.g §5.38 §5.22 — `TextField` IME composition dispatch path.
    //!
    //! Covers the `apply_composition_start` / `_update` / `_commit` /
    //! `_cancel` lifecycle on `TextFieldExternal`, the SCXML transitions
    //! they drive, the `Intent("text_committed", Text(_))` payload
    //! upgrade contract, the commit-on-blur extension to
    //! `External::on_focus_change`, and the blink-reset side effects.
    //! Plain `send(CommitEdit | Blur)` paths (no composition layer) are
    //! covered in the legacy `tests` module and continue to emit the
    //! `IntrospectValue::Null` payload for backward compat.

    use super::{TextField, TextFieldEvent, TextFieldExternal, TextFieldState};
    use crate::animation::Tickable;
    use crate::external::{External, IntrospectValue};
    use crate::intent::Intent;
    use crate::widgets::caret_blink::CaretBlink;
    use crate::widgets::text_edit::TextEditState;
    use std::rc::Rc;

    fn focused_external_with_state() -> (TextFieldExternal, Rc<TextEditState>) {
        let state = Rc::new(TextEditState::with_initial(String::new()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        tfx.send(TextFieldEvent::Focus);
        (tfx, state)
    }

    fn drain(tfx: &mut TextFieldExternal) -> Vec<Intent> {
        let mut out = Vec::new();
        tfx.drain_intents(&mut |i| out.push(i));
        out
    }

    // ─────────────────────────────────────────────────────────────
    // apply_composition_start
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_apply_composition_start_drives_begin_edit() {
        let (mut tfx, _state) = focused_external_with_state();
        tfx.apply_composition_start();
        assert_eq!(tfx.state(), TextFieldState::Editing);
    }

    #[test]
    fn r56_1_g_apply_composition_start_seeds_preedit_empty_some() {
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        assert_eq!(state.preedit(), Some(String::new()));
        assert!(state.is_composing());
    }

    #[test]
    fn r56_1_g_apply_composition_start_on_bare_external_still_drives_scxml() {
        // No attached TextEditState — SCXML transition still fires so the
        // AI client can observe state via the introspect `state` slot.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.apply_composition_start();
        assert_eq!(tfx.state(), TextFieldState::Editing);
    }

    #[test]
    fn r56_1_g_apply_composition_start_drains_active_selection() {
        // Compose-over-selection: selection (1,4) drained, then compose
        // begins. Identical to TextEditState::preedit_start contract.
        let state = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        tfx.send(TextFieldEvent::Focus);
        state.set_selection(1, 4);
        tfx.apply_composition_start();
        assert_eq!(state.text(), "aef");
        assert_eq!(state.caret(), 1);
        assert_eq!(state.selection_anchor(), None);
        assert_eq!(state.preedit(), Some(String::new()));
    }

    // ─────────────────────────────────────────────────────────────
    // apply_composition_update
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_apply_composition_update_sets_preedit_content() {
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("hello");
        assert_eq!(state.preedit(), Some("hello".to_string()));
    }

    #[test]
    fn r56_1_g_apply_composition_update_keeps_scxml_in_editing() {
        let (mut tfx, _state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("hi");
        tfx.apply_composition_update("hi!");
        assert_eq!(tfx.state(), TextFieldState::Editing);
    }

    #[test]
    fn r56_1_g_apply_composition_update_no_op_when_not_composing() {
        // Defensive against out-of-order: update without start stays None.
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_update("hi");
        assert_eq!(state.preedit(), None);
        assert_eq!(state.text(), "");
    }

    // ─────────────────────────────────────────────────────────────
    // apply_composition_commit
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_apply_composition_commit_inserts_text_at_caret() {
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("xyz");
        tfx.apply_composition_commit("XYZ");
        assert_eq!(state.text(), "XYZ");
        assert_eq!(state.caret(), 3);
        assert_eq!(state.preedit(), None);
    }

    #[test]
    fn r56_1_g_apply_composition_commit_drives_commit_edit() {
        let (mut tfx, _state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_commit("hi");
        assert_eq!(tfx.state(), TextFieldState::Focused);
    }

    #[test]
    fn r56_1_g_apply_composition_commit_emits_text_intent_with_payload() {
        // The payload upgrade contract: plain CommitEdit emits Null,
        // apply_composition_commit emits Text(committed).
        let (mut tfx, _state) = focused_external_with_state();
        tfx.apply_composition_start();
        let _ = drain(&mut tfx); // discard any prior intents
        tfx.apply_composition_commit("hello");
        let intents = drain(&mut tfx);
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].tag_str(), "text_committed");
        assert!(matches!(
            intents[0].payload,
            IntrospectValue::Text(ref s) if s == "hello"
        ));
    }

    #[test]
    fn r56_1_g_apply_composition_commit_emits_exactly_one_intent() {
        // The em.inner.send bypass must NOT additionally fire detect's
        // Null intent on top of the manual Text intent.
        let (mut tfx, _state) = focused_external_with_state();
        tfx.apply_composition_start();
        let _ = drain(&mut tfx);
        tfx.apply_composition_commit("hi");
        let intents = drain(&mut tfx);
        assert_eq!(
            intents.len(),
            1,
            "exactly one intent per commit (no detect duplicate)",
        );
    }

    #[test]
    fn r56_1_g_apply_composition_commit_empty_clears_without_intent() {
        // Empty commit is the cancel-shape compositionend: clears the
        // preedit, drives SCXML, but emits NO intent (no text committed).
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("h");
        let _ = drain(&mut tfx);
        tfx.apply_composition_commit("");
        assert_eq!(state.preedit(), None);
        assert_eq!(state.text(), "");
        assert_eq!(tfx.state(), TextFieldState::Focused);
        assert!(drain(&mut tfx).is_empty(), "empty commit emits no intent");
    }

    #[test]
    fn r56_1_g_apply_composition_commit_no_intent_on_bare_external() {
        // No attached state means no composition could have been active,
        // so no commit intent fires (the AI client gates on `was_composing`).
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.apply_composition_start();
        tfx.apply_composition_commit("hi");
        assert!(
            drain(&mut tfx).is_empty(),
            "bare external commit emits no intent (no was_composing)",
        );
    }

    #[test]
    fn r56_1_g_apply_composition_commit_no_intent_when_not_composing() {
        // Defensive: commit without start is not a valid lifecycle —
        // no intent fires (the substrate preedit_commit also no-ops).
        let (mut tfx, _state) = focused_external_with_state();
        let _ = drain(&mut tfx);
        tfx.apply_composition_commit("hi");
        assert!(
            drain(&mut tfx).is_empty(),
            "commit without start emits no intent",
        );
    }

    #[test]
    fn r56_1_g_apply_composition_commit_into_middle_of_text() {
        let state = Rc::new(TextEditState::with_initial("ad".to_string()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        tfx.send(TextFieldEvent::Focus);
        state.set_caret(1);
        tfx.apply_composition_start();
        tfx.apply_composition_update("bc");
        tfx.apply_composition_commit("bc");
        assert_eq!(state.text(), "abcd");
        assert_eq!(state.caret(), 3);
    }

    // ─────────────────────────────────────────────────────────────
    // apply_composition_cancel
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_apply_composition_cancel_clears_preedit_without_insert() {
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("xyz");
        tfx.apply_composition_cancel();
        assert_eq!(state.preedit(), None);
        assert_eq!(state.text(), "");
    }

    #[test]
    fn r56_1_g_apply_composition_cancel_drives_cancel_edit() {
        let (mut tfx, _state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_cancel();
        assert_eq!(tfx.state(), TextFieldState::Focused);
    }

    #[test]
    fn r56_1_g_apply_composition_cancel_emits_no_intent() {
        // CancelEdit is silent in detect — the IME canonical
        // cancel-discards-preedit contract.
        let (mut tfx, _state) = focused_external_with_state();
        tfx.apply_composition_start();
        let _ = drain(&mut tfx);
        tfx.apply_composition_cancel();
        assert!(drain(&mut tfx).is_empty(), "cancel must be silent");
    }

    // ─────────────────────────────────────────────────────────────
    // Backward compat — plain send paths still emit Null payload
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_plain_commit_edit_still_emits_null_intent() {
        // Plain ext.send(CommitEdit) without going through the
        // composition layer continues to emit Intent(Null) per the
        // R56.1.a legacy contract.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::BeginEdit);
        tfx.send(TextFieldEvent::CommitEdit);
        let intents = drain(&mut tfx);
        assert_eq!(intents.len(), 1);
        assert!(matches!(intents[0].payload, IntrospectValue::Null));
    }

    #[test]
    fn r56_1_g_plain_blur_during_editing_still_emits_null_intent() {
        // Plain ext.send(Blur) bypasses on_focus_change, so the legacy
        // commit-on-blur fires Intent(Null) via the detect rule.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::BeginEdit);
        tfx.send(TextFieldEvent::Blur);
        let intents = drain(&mut tfx);
        assert_eq!(intents.len(), 1);
        assert!(matches!(intents[0].payload, IntrospectValue::Null));
    }

    // ─────────────────────────────────────────────────────────────
    // on_focus_change — commit-on-blur with preedit
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_blur_during_composition_with_preedit_emits_text_intent() {
        // The W3C IME canonical commit-on-blur: focus loss with an
        // active preedit commits the preedit as if compositionend(data)
        // fired with the preedit string as data.
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("hi");
        let _ = drain(&mut tfx);
        tfx.on_focus_change(false);
        let intents = drain(&mut tfx);
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].tag_str(), "text_committed");
        assert!(matches!(
            intents[0].payload,
            IntrospectValue::Text(ref s) if s == "hi"
        ));
        assert_eq!(state.text(), "hi", "preedit committed into buffer");
        assert_eq!(tfx.state(), TextFieldState::Idle, "blur reached Idle");
    }

    #[test]
    fn r56_1_g_blur_during_composition_with_empty_preedit_cancels() {
        // compositionstart-without-update-then-blur: substrate cancels
        // instead of committing (no-data compositionend is a cancel).
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        let _ = drain(&mut tfx);
        tfx.on_focus_change(false);
        assert_eq!(state.preedit(), None, "cancel cleared preedit");
        assert_eq!(state.text(), "", "no insertion on cancel");
        assert!(
            drain(&mut tfx).is_empty(),
            "empty-preedit blur emits no intent",
        );
        assert_eq!(tfx.state(), TextFieldState::Idle);
    }

    #[test]
    fn r56_1_g_blur_from_focused_without_compose_is_silent() {
        // Plain Focused → Idle blur (no composition active) emits no
        // intent. Same shape as the legacy backward-compat test, here
        // verified through on_focus_change to cover the routing path.
        let (mut tfx, _state) = focused_external_with_state();
        tfx.on_focus_change(false);
        assert!(drain(&mut tfx).is_empty());
        assert_eq!(tfx.state(), TextFieldState::Idle);
    }

    // ─────────────────────────────────────────────────────────────
    // Blink reset on composition lifecycle methods
    // ─────────────────────────────────────────────────────────────

    fn focused_external_with_hidden_blink() -> (TextFieldExternal, Rc<CaretBlink>) {
        let state = Rc::new(TextEditState::with_initial(String::new()));
        let blink = Rc::new(CaretBlink::new());
        let mut tfx = TextFieldExternal::new()
            .attach_state(state)
            .attach_blink(Rc::clone(&blink));
        tfx.send(TextFieldEvent::Focus);
        // Tick into the hidden half of the 530ms period so a `reset()`
        // flips visibility back to `true` observably.
        blink.tick(0.6);
        assert!(!blink.visible(), "fixture: blink starts the test hidden");
        (tfx, blink)
    }

    #[test]
    fn r56_1_g_apply_composition_start_resets_blink() {
        let (mut tfx, blink) = focused_external_with_hidden_blink();
        tfx.apply_composition_start();
        assert!(blink.visible(), "compose-start resets blink to visible");
    }

    #[test]
    fn r56_1_g_apply_composition_update_resets_blink() {
        let (mut tfx, blink) = focused_external_with_hidden_blink();
        tfx.apply_composition_start();
        blink.tick(0.6);
        assert!(!blink.visible());
        tfx.apply_composition_update("h");
        assert!(blink.visible(), "compose-update resets blink to visible");
    }

    #[test]
    fn r56_1_g_apply_composition_commit_resets_blink() {
        let (mut tfx, blink) = focused_external_with_hidden_blink();
        tfx.apply_composition_start();
        blink.tick(0.6);
        assert!(!blink.visible());
        tfx.apply_composition_commit("h");
        assert!(blink.visible(), "compose-commit resets blink to visible");
    }

    #[test]
    fn r56_1_g_apply_composition_cancel_resets_blink() {
        let (mut tfx, blink) = focused_external_with_hidden_blink();
        tfx.apply_composition_start();
        blink.tick(0.6);
        assert!(!blink.visible());
        tfx.apply_composition_cancel();
        assert!(blink.visible(), "compose-cancel resets blink to visible");
    }

    // ─────────────────────────────────────────────────────────────
    // Multi-byte UTF-8 (Korean composition end-to-end)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_korean_composition_commits_three_byte_syllable() {
        // Canonical Korean IME flow: 'ㅎ' + 'ㅏ' + 'ㄴ' jamo → "한"
        // (3-byte UTF-8). The substrate doesn't know jamo composition —
        // the platform IME composes; substrate just inserts the
        // committed string verbatim at the caret.
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("ㅎ");
        tfx.apply_composition_update("하");
        tfx.apply_composition_commit("한");
        assert_eq!(state.text(), "한");
        assert_eq!(state.caret(), 3, "caret advanced 3 bytes");
        // Buffer is still valid UTF-8.
        let _: &str = state.text().as_str();
    }

    // ─────────────────────────────────────────────────────────────
    // TextField as bare widget (sanity check the SCXML path)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_text_field_send_unchanged() {
        // Sanity: TextField (not Ext) still drives SCXML normally
        // through plain send. R56.1.g doesn't touch the inner widget.
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::CommitEdit);
        assert_eq!(tf.state(), TextFieldState::Focused);
    }
}

#[cfg(test)]
mod r56_1_g_2_tests {
    //! R56.1.g.2 §5.38 §5.22 — RPC `preedit` query/intervene slot +
    //! `composition` invoke slot. Pins the AI-first introspection
    //! surface for the W3C `CompositionEvent` lifecycle ([[w3c-
    //! composition-event-shape]]) and the
    //! [[rpc-introspect-pair-complete]] contract that every state
    //! axis has both read and write wires.

    use super::{TextFieldEvent, TextFieldExternal, TextFieldState};
    use crate::external::{ExternalIntrospect, InterveneError, IntrospectValue, InvokeError};
    use crate::widgets::text_edit::TextEditState;
    use std::rc::Rc;

    fn focused_external_with_state() -> (TextFieldExternal, Rc<TextEditState>) {
        let state = Rc::new(TextEditState::with_initial(String::new()));
        let mut tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        tfx.send(TextFieldEvent::Focus);
        (tfx, state)
    }

    // ─────────────────────────────────────────────────────────────
    // Schema
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_2_schema_contains_preedit_and_composition_slots() {
        let tfx = TextFieldExternal::new();
        let schema = tfx.schema();
        let names: Vec<&str> = schema.fields.iter().map(|f| f.path).collect();
        assert!(names.contains(&"preedit"));
        assert!(names.contains(&"composition"));
    }

    // ─────────────────────────────────────────────────────────────
    // query("preedit")
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_2_query_preedit_returns_null_when_not_composing() {
        let (tfx, _state) = focused_external_with_state();
        assert_eq!(tfx.query("preedit").unwrap(), IntrospectValue::Null);
    }

    #[test]
    fn r56_1_g_2_query_preedit_returns_text_while_composing() {
        let (mut tfx, _state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("hello");
        assert_eq!(
            tfx.query("preedit").unwrap(),
            IntrospectValue::Text("hello".to_string()),
        );
    }

    #[test]
    fn r56_1_g_2_query_preedit_returns_empty_text_after_start_before_update() {
        // compositionstart-before-compositionupdate: preedit is
        // Some(String::new()) — empty Text.
        let (mut tfx, _state) = focused_external_with_state();
        tfx.apply_composition_start();
        assert_eq!(
            tfx.query("preedit").unwrap(),
            IntrospectValue::Text(String::new()),
        );
    }

    #[test]
    fn r56_1_g_2_query_preedit_returns_none_on_bare_external() {
        let tfx = TextFieldExternal::new();
        assert_eq!(tfx.query("preedit"), None);
    }

    // R769.1 §5.36 §5.22 — `style_runs` read slot: applied rich-text
    // formatting (the read peer of `apply-style` / `clear-style`), so an
    // AI verifies bold / italic / colour without walking `scene/snapshot`.

    #[test]
    fn r769_1_query_style_runs_reports_applied_formatting() {
        use crate::style::{FontWeight, TextStyle};
        let (tfx, state) = focused_external_with_state();
        // Unstyled buffer -> empty JSON array.
        assert_eq!(
            tfx.query("style_runs").unwrap(),
            IntrospectValue::Json(serde_json::Value::Array(Vec::new())),
            "an unstyled field reports no runs",
        );
        // Apply one bold run; it surfaces as a {start, end, style} object.
        state.set_text("hello".to_string());
        state.apply_style_run(0, 3, TextStyle::new().with_weight(FontWeight::BOLD));
        let IntrospectValue::Json(serde_json::Value::Array(runs)) =
            tfx.query("style_runs").unwrap()
        else {
            panic!("style_runs is a JSON array");
        };
        assert_eq!(runs.len(), 1, "one applied run");
        assert_eq!(runs[0]["start"], serde_json::json!(0));
        assert_eq!(runs[0]["end"], serde_json::json!(3));
        assert_eq!(
            runs[0]["style"]["font_weight"],
            serde_json::json!(700),
            "the run carries the applied bold weight",
        );
    }

    #[test]
    fn r769_1_query_style_runs_none_on_bare_external() {
        let tfx = TextFieldExternal::new();
        assert_eq!(tfx.query("style_runs"), None);
    }

    // ─────────────────────────────────────────────────────────────
    // intervene("preedit", ...)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_2_intervene_preedit_text_auto_starts_composition() {
        // Substrate idempotence: preedit_start no-ops if already
        // composing; preedit_update sets the buffer. Net effect:
        // intervene with Text auto-starts then sets.
        let (mut tfx, state) = focused_external_with_state();
        tfx.intervene("preedit", IntrospectValue::Text("hi".to_string()))
            .unwrap();
        assert_eq!(state.preedit(), Some("hi".to_string()));
        assert!(state.is_composing());
    }

    #[test]
    fn r56_1_g_2_intervene_preedit_text_updates_when_already_composing() {
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.intervene("preedit", IntrospectValue::Text("a".to_string()))
            .unwrap();
        tfx.intervene("preedit", IntrospectValue::Text("ab".to_string()))
            .unwrap();
        assert_eq!(state.preedit(), Some("ab".to_string()));
    }

    #[test]
    fn r56_1_g_2_intervene_preedit_null_cancels_composition() {
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("xyz");
        tfx.intervene("preedit", IntrospectValue::Null).unwrap();
        assert_eq!(state.preedit(), None);
        assert_eq!(state.text(), "", "no insertion on Null intervene");
    }

    #[test]
    fn r56_1_g_2_intervene_preedit_keeps_scxml_state_stable() {
        // intervene does NOT drive SCXML (no BeginEdit / CancelEdit
        // — applications that need the transition use the
        // `composition` invoke surface). State stays Focused after
        // a Text intervene from Focused.
        let (mut tfx, _state) = focused_external_with_state();
        tfx.intervene("preedit", IntrospectValue::Text("hi".to_string()))
            .unwrap();
        assert_eq!(tfx.state(), TextFieldState::Focused);
    }

    #[test]
    fn r56_1_g_2_intervene_preedit_json_returns_type_mismatch() {
        let (mut tfx, _state) = focused_external_with_state();
        let err = tfx
            .intervene("preedit", IntrospectValue::Json(serde_json::json!({})))
            .unwrap_err();
        assert!(matches!(err, InterveneError::TypeMismatch));
    }

    #[test]
    fn r56_1_g_2_intervene_preedit_int_returns_type_mismatch() {
        let (mut tfx, _state) = focused_external_with_state();
        let err = tfx
            .intervene("preedit", IntrospectValue::Int(42))
            .unwrap_err();
        assert!(matches!(err, InterveneError::TypeMismatch));
    }

    #[test]
    fn r56_1_g_2_intervene_preedit_on_bare_external_returns_read_only() {
        let mut tfx = TextFieldExternal::new();
        let err = tfx
            .intervene("preedit", IntrospectValue::Text("hi".to_string()))
            .unwrap_err();
        assert!(matches!(err, InterveneError::ReadOnly));
    }

    // ─────────────────────────────────────────────────────────────
    // invoke("composition", ...)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_2_invoke_composition_start_drives_begin_edit() {
        let (mut tfx, state) = focused_external_with_state();
        let result = tfx
            .invoke(
                "composition",
                IntrospectValue::Json(serde_json::json!({"action": "start"})),
            )
            .unwrap();
        assert_eq!(result, IntrospectValue::Text("Editing".to_string()));
        assert!(state.is_composing());
    }

    #[test]
    fn r56_1_g_2_invoke_composition_update_sets_preedit() {
        let (mut tfx, state) = focused_external_with_state();
        tfx.invoke(
            "composition",
            IntrospectValue::Json(serde_json::json!({"action": "start"})),
        )
        .unwrap();
        tfx.invoke(
            "composition",
            IntrospectValue::Json(serde_json::json!({"action": "update", "data": "hi"})),
        )
        .unwrap();
        assert_eq!(state.preedit(), Some("hi".to_string()));
    }

    #[test]
    fn r56_1_g_2_invoke_composition_end_commits_and_returns_focused() {
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("ㅎ");
        let result = tfx
            .invoke(
                "composition",
                IntrospectValue::Json(serde_json::json!({"action": "end", "data": "한"})),
            )
            .unwrap();
        assert_eq!(result, IntrospectValue::Text("Focused".to_string()));
        assert_eq!(state.text(), "한");
        assert_eq!(state.preedit(), None);
    }

    #[test]
    fn r56_1_g_2_invoke_composition_end_empty_data_clears_without_insert() {
        // cancel-shape compositionend: data: "" clears preedit, no
        // text inserted, SCXML transitions to Focused, no intent.
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("hi");
        let result = tfx
            .invoke(
                "composition",
                IntrospectValue::Json(serde_json::json!({"action": "end", "data": ""})),
            )
            .unwrap();
        assert_eq!(result, IntrospectValue::Text("Focused".to_string()));
        assert_eq!(state.text(), "");
        assert_eq!(state.preedit(), None);
    }

    #[test]
    fn r56_1_g_2_invoke_composition_cancel_clears_preedit() {
        let (mut tfx, state) = focused_external_with_state();
        tfx.apply_composition_start();
        tfx.apply_composition_update("hi");
        let result = tfx
            .invoke(
                "composition",
                IntrospectValue::Json(serde_json::json!({"action": "cancel"})),
            )
            .unwrap();
        assert_eq!(result, IntrospectValue::Text("Focused".to_string()));
        assert_eq!(state.preedit(), None);
        assert_eq!(state.text(), "", "no insertion on cancel");
    }

    #[test]
    fn r56_1_g_2_invoke_composition_bogus_action_returns_type_mismatch() {
        let (mut tfx, _state) = focused_external_with_state();
        let err = tfx
            .invoke(
                "composition",
                IntrospectValue::Json(serde_json::json!({"action": "bogus"})),
            )
            .unwrap_err();
        assert!(matches!(err, InvokeError::TypeMismatch));
    }

    #[test]
    fn r56_1_g_2_invoke_composition_update_missing_data_returns_type_mismatch() {
        let (mut tfx, _state) = focused_external_with_state();
        let err = tfx
            .invoke(
                "composition",
                IntrospectValue::Json(serde_json::json!({"action": "update"})),
            )
            .unwrap_err();
        assert!(matches!(err, InvokeError::TypeMismatch));
    }

    #[test]
    fn r56_1_g_2_invoke_composition_end_missing_data_returns_type_mismatch() {
        let (mut tfx, _state) = focused_external_with_state();
        let err = tfx
            .invoke(
                "composition",
                IntrospectValue::Json(serde_json::json!({"action": "end"})),
            )
            .unwrap_err();
        assert!(matches!(err, InvokeError::TypeMismatch));
    }

    #[test]
    fn r56_1_g_2_invoke_composition_text_args_returns_type_mismatch() {
        // Action surface is Json-only; Text args rejected so the
        // wire shape stays strict.
        let (mut tfx, _state) = focused_external_with_state();
        let err = tfx
            .invoke("composition", IntrospectValue::Text("start".to_string()))
            .unwrap_err();
        assert!(matches!(err, InvokeError::TypeMismatch));
    }

    #[test]
    fn r56_1_g_2_invoke_composition_on_bare_external_drives_scxml() {
        // No attached TextEditState: composition invoke still drives
        // SCXML so AI client can observe state transitions.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        let result = tfx
            .invoke(
                "composition",
                IntrospectValue::Json(serde_json::json!({"action": "start"})),
            )
            .unwrap();
        assert_eq!(result, IntrospectValue::Text("Editing".to_string()));
    }

    #[test]
    fn r56_1_g_2_invoke_unknown_path_returns_unknown_path() {
        let (mut tfx, _state) = focused_external_with_state();
        let err = tfx
            .invoke("bogus_method", IntrospectValue::Null)
            .unwrap_err();
        assert!(matches!(err, InvokeError::UnknownPath));
    }
}

#[cfg(test)]
mod r903_find_replace_tests {
    //! R903 §5.22 §5.52 — find &amp; replace RPC surface on
    //! [`TextFieldExternal`]: query / intervene read-write symmetry, the
    //! `find-next` / `find-prev` navigation, and `replace` / `replace-all`
    //! mutation (the AI-first peer of the find bar's keyboard).

    use super::TextFieldExternal;
    use crate::external::{ExternalIntrospect, InterveneError, IntrospectValue, InvokeError};
    use crate::widgets::text_edit::TextEditState;
    use std::rc::Rc;

    fn wired(text: &str) -> (Rc<TextEditState>, TextFieldExternal) {
        let state = Rc::new(TextEditState::with_initial(text.to_string()));
        let tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        (state, tfx)
    }

    #[test]
    fn r903_find_query_read_write_round_trips() {
        let (_state, mut tfx) = wired("at bat");
        tfx.intervene("find_query", IntrospectValue::Text("at".to_string()))
            .unwrap();
        assert_eq!(
            tfx.query("find_query").unwrap(),
            IntrospectValue::Text("at".to_string()),
        );
    }

    #[test]
    fn r903_case_and_whole_word_flags_round_trip() {
        let (_state, mut tfx) = wired("Cat cat");
        tfx.intervene("find_case_sensitive", IntrospectValue::Bool(true))
            .unwrap();
        tfx.intervene("find_whole_word", IntrospectValue::Bool(true))
            .unwrap();
        assert_eq!(
            tfx.query("find_case_sensitive").unwrap(),
            IntrospectValue::Bool(true),
        );
        assert_eq!(
            tfx.query("find_whole_word").unwrap(),
            IntrospectValue::Bool(true),
        );
    }

    #[test]
    fn r903_find_matches_reports_count_ranges_and_current() {
        let (state, mut tfx) = wired("at bat hat");
        state.set_caret(0);
        tfx.intervene("find_query", IntrospectValue::Text("at".to_string()))
            .unwrap();
        // Before navigating, current is null (selection not on a match).
        let IntrospectValue::Json(j) = tfx.query("find_matches").unwrap() else {
            panic!("find_matches must be Json");
        };
        assert_eq!(j["count"], serde_json::json!(3));
        assert_eq!(j["ranges"], serde_json::json!([[0, 2], [4, 6], [8, 10]]));
        assert_eq!(j["current"], serde_json::Value::Null);
        // find-next selects the first match → current becomes 0.
        let sel = tfx.invoke("find-next", IntrospectValue::Null).unwrap();
        assert_eq!(sel, super::selection_range_to_value(Some((0, 2))));
        let IntrospectValue::Json(j2) = tfx.query("find_matches").unwrap() else {
            panic!("json");
        };
        assert_eq!(j2["current"], serde_json::json!(0));
    }

    #[test]
    fn r926_query_bracket_match_reports_pair_or_null() {
        let (state, tfx) = wired("f(x)");
        // Caret not next to a bracket → Null (the path is known but no
        // pair, distinct from a bare field returning None).
        state.set_caret(0);
        assert_eq!(tfx.query("bracket_match").unwrap(), IntrospectValue::Null);
        // Caret after ')' → {open: 1, close: 3}.
        state.set_caret(4);
        let IntrospectValue::Json(j) = tfx.query("bracket_match").unwrap() else {
            panic!("bracket_match must be Json when matched");
        };
        assert_eq!(j["open"], serde_json::json!(1));
        assert_eq!(j["close"], serde_json::json!(3));
        // Caret just after '(' → same pair from the other side.
        state.set_caret(2);
        let IntrospectValue::Json(j2) = tfx.query("bracket_match").unwrap() else {
            panic!("json");
        };
        assert_eq!(j2["open"], serde_json::json!(1));
        assert_eq!(j2["close"], serde_json::json!(3));
    }

    #[test]
    fn r926_query_bracket_match_none_on_bare_text_field() {
        // A bare TextField (no attached TextEditState) does not know the
        // path — the AI client reads None ("not bound"), not Null ("no
        // pair"), the same bare/wired distinction text / caret draw.
        let tfx = TextFieldExternal::new();
        assert!(tfx.query("bracket_match").is_none());
    }

    #[test]
    fn r903_find_next_prev_navigate_and_wrap() {
        let (state, mut tfx) = wired("at bat hat");
        state.set_caret(0);
        tfx.intervene("find_query", IntrospectValue::Text("at".to_string()))
            .unwrap();
        assert_eq!(
            tfx.invoke("find-next", IntrospectValue::Null).unwrap(),
            super::selection_range_to_value(Some((0, 2))),
        );
        assert_eq!(
            tfx.invoke("find-next", IntrospectValue::Null).unwrap(),
            super::selection_range_to_value(Some((4, 6))),
        );
        assert_eq!(
            tfx.invoke("find-prev", IntrospectValue::Null).unwrap(),
            super::selection_range_to_value(Some((0, 2))),
        );
    }

    #[test]
    fn r903_find_next_on_no_matches_returns_null() {
        let (_state, mut tfx) = wired("hello");
        tfx.intervene("find_query", IntrospectValue::Text("zzz".to_string()))
            .unwrap();
        assert_eq!(
            tfx.invoke("find-next", IntrospectValue::Null).unwrap(),
            IntrospectValue::Null,
        );
    }

    #[test]
    fn r903_replace_current_mutates_and_returns_bool() {
        let (state, mut tfx) = wired("at bat");
        state.set_caret(0);
        tfx.intervene("find_query", IntrospectValue::Text("at".to_string()))
            .unwrap();
        // First press selects (false), second replaces (true).
        assert_eq!(
            tfx.invoke("replace", IntrospectValue::Text("X".to_string()))
                .unwrap(),
            IntrospectValue::Bool(false),
        );
        assert_eq!(
            tfx.invoke("replace", IntrospectValue::Text("X".to_string()))
                .unwrap(),
            IntrospectValue::Bool(true),
        );
        assert_eq!(state.text(), "X bat");
    }

    #[test]
    fn r903_replace_all_returns_count_and_rewrites() {
        let (state, mut tfx) = wired("red red red");
        tfx.intervene("find_query", IntrospectValue::Text("red".to_string()))
            .unwrap();
        assert_eq!(
            tfx.invoke("replace-all", IntrospectValue::Text("blue".to_string()))
                .unwrap(),
            IntrospectValue::Int(3),
        );
        assert_eq!(state.text(), "blue blue blue");
    }

    #[test]
    fn r903_find_surface_on_bare_field_is_inert_not_unknown() {
        // No state attached → reads return None (path known, unbound), the
        // setter returns ReadOnly, the actions return their empty outcome —
        // never UnknownPath (the slots exist in the schema unconditionally).
        let mut tfx = TextFieldExternal::new();
        assert_eq!(tfx.query("find_query"), None);
        assert!(matches!(
            tfx.intervene("find_query", IntrospectValue::Text("a".to_string())),
            Err(InterveneError::ReadOnly),
        ));
        assert_eq!(
            tfx.invoke("find-next", IntrospectValue::Null).unwrap(),
            IntrospectValue::Null,
        );
        assert_eq!(
            tfx.invoke("replace-all", IntrospectValue::Text("x".to_string()))
                .unwrap(),
            IntrospectValue::Int(0),
        );
    }

    #[test]
    fn r903_find_actions_reject_wrong_arg_shapes() {
        let (_state, mut tfx) = wired("at");
        assert!(matches!(
            tfx.invoke("find-next", IntrospectValue::Text("oops".to_string())),
            Err(InvokeError::TypeMismatch),
        ));
        assert!(matches!(
            tfx.invoke("replace", IntrospectValue::Int(1)),
            Err(InvokeError::TypeMismatch),
        ));
        assert!(matches!(
            tfx.intervene(
                "find_case_sensitive",
                IntrospectValue::Text("yes".to_string())
            ),
            Err(InterveneError::TypeMismatch),
        ));
    }
}

#[cfg(test)]
mod r933_fold_tests {
    //! R933 §5.36 — code-folding RPC surface on [`TextFieldExternal`]:
    //! the `fold_regions` derived read and the `toggle-fold` / `fold-all`
    //! / `unfold-all` actions (the AI-first peer of the gutter chevron).

    use super::{TextFieldExternal, TextFieldSendKey};
    use crate::external::{ExternalIntrospect, IntrospectValue, InvokeError};
    use crate::widgets::text_edit::TextEditState;
    use std::rc::Rc;

    fn wired(text: &str) -> (Rc<TextEditState>, TextFieldExternal) {
        let state = Rc::new(TextEditState::with_initial(text.to_string()));
        let tfx = TextFieldExternal::new().attach_state(Rc::clone(&state));
        (state, tfx)
    }

    fn regions(tfx: &TextFieldExternal) -> Vec<serde_json::Value> {
        let IntrospectValue::Json(serde_json::Value::Array(rs)) =
            tfx.query("fold_regions").unwrap()
        else {
            panic!("fold_regions must be a Json array");
        };
        rs
    }

    #[test]
    fn r933_query_fold_regions_lists_blocks_and_collapse_flag() {
        let (state, mut tfx) = wired("a {\n b\n}\n");
        let rs = regions(&tfx);
        assert_eq!(rs.len(), 1, "one foldable brace block");
        assert_eq!(rs[0]["start_line"], serde_json::json!(0));
        assert_eq!(rs[0]["end_line"], serde_json::json!(2));
        assert_eq!(rs[0]["collapsed"], serde_json::json!(false));
        // Collapse via the action; the read reflects it (one derivation,
        // wire and gutter never disagree).
        assert_eq!(
            tfx.invoke("toggle-fold", IntrospectValue::Int(0)).unwrap(),
            IntrospectValue::Bool(true),
        );
        assert_eq!(regions(&tfx)[0]["collapsed"], serde_json::json!(true));
        assert!(state.is_line_hidden(1), "substrate agrees the interior hid");
    }

    #[test]
    fn r933_invoke_toggle_fold_round_trips_and_no_opener_is_false() {
        let (_state, mut tfx) = wired("x {\n y\n}\n");
        assert_eq!(
            tfx.invoke("toggle-fold", IntrospectValue::Int(0)).unwrap(),
            IntrospectValue::Bool(true),
        );
        assert_eq!(regions(&tfx)[0]["collapsed"], serde_json::json!(true));
        assert_eq!(
            tfx.invoke("toggle-fold", IntrospectValue::Int(0)).unwrap(),
            IntrospectValue::Bool(true),
            "toggling again still reports a region toggled (now expanded)",
        );
        assert_eq!(regions(&tfx)[0]["collapsed"], serde_json::json!(false));
        // A line with no opening bracket toggles nothing.
        assert_eq!(
            tfx.invoke("toggle-fold", IntrospectValue::Int(1)).unwrap(),
            IntrospectValue::Bool(false),
        );
    }

    #[test]
    fn r933_invoke_fold_all_unfold_all_return_collapsed_counts() {
        let (_state, mut tfx) = wired("a {\n b {\n  c\n }\n}\n");
        assert_eq!(
            tfx.invoke("fold-all", IntrospectValue::Null).unwrap(),
            IntrospectValue::Int(2),
            "both nested blocks collapse",
        );
        assert_eq!(
            tfx.invoke("unfold-all", IntrospectValue::Null).unwrap(),
            IntrospectValue::Int(0),
            "all expanded",
        );
    }

    #[test]
    fn r933_query_fold_regions_none_on_bare_field() {
        // A bare TextField (no attached state) does not know the path —
        // None ("not bound"), the same bare/wired distinction text draws.
        let tfx = TextFieldExternal::new();
        assert!(tfx.query("fold_regions").is_none());
    }

    #[test]
    fn r933_invoke_fold_rejects_bad_args() {
        let (_state, mut tfx) = wired("x {\n y\n}\n");
        assert!(
            matches!(
                tfx.invoke("toggle-fold", IntrospectValue::Int(-1)),
                Err(InvokeError::Rejected),
            ),
            "a negative line cannot name a row",
        );
        assert!(matches!(
            tfx.invoke("toggle-fold", IntrospectValue::Null),
            Err(InvokeError::TypeMismatch),
        ));
        assert!(matches!(
            tfx.invoke("fold-all", IntrospectValue::Int(3)),
            Err(InvokeError::TypeMismatch),
        ));
    }

    #[test]
    fn r938_invoke_indent_then_dedent_round_trips() {
        // The AI-first verb twin of Tab / Shift+Tab: indent a multi-line
        // selection, then dedent it back, each reporting whether it changed
        // the buffer (the setter-returns-read-outcome contract).
        let (state, mut tfx) = wired("a\nb");
        state.set_selection(0, 3);
        assert_eq!(
            tfx.invoke("indent", IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(true),
        );
        assert_eq!(state.text(), "    a\n    b");
        state.set_selection(0, state.text().len());
        assert_eq!(
            tfx.invoke("dedent", IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(true),
        );
        assert_eq!(state.text(), "a\nb");
        // A dedent with nothing to strip is a Bool(false) no-op.
        state.set_selection(0, state.text().len());
        assert_eq!(
            tfx.invoke("dedent", IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(false),
        );
    }

    #[test]
    fn r938_invoke_indent_rejects_non_null_and_bare_field_is_inert() {
        let (_state, mut tfx) = wired("a\nb");
        assert!(matches!(
            tfx.invoke("indent", IntrospectValue::Int(2)),
            Err(InvokeError::TypeMismatch),
        ));
        // A bare field (no state) is inert — Bool(false), never UnknownPath.
        let mut bare = TextFieldExternal::new();
        assert_eq!(
            bare.invoke("indent", IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(false),
        );
    }

    #[test]
    fn r939_invoke_toggle_comment_round_trips() {
        // The AI-first verb twin of Ctrl+/: comment a multi-line selection,
        // then toggle it back, each reporting whether it changed the buffer.
        let (state, mut tfx) = wired("a\nb");
        state.set_line_comment("//");
        state.set_selection(0, state.text().len());
        assert_eq!(
            tfx.invoke("toggle-comment", IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(true),
        );
        assert_eq!(state.text(), "// a\n// b");
        state.set_selection(0, state.text().len());
        assert_eq!(
            tfx.invoke("toggle-comment", IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(true),
        );
        assert_eq!(state.text(), "a\nb");
    }

    #[test]
    fn r939_invoke_toggle_comment_inert_without_marker_or_state() {
        // A field that never called `set_line_comment` is inert — Bool(false),
        // never UnknownPath — even though it has state to toggle.
        let (state, mut tfx) = wired("a\nb");
        state.set_selection(0, state.text().len());
        assert_eq!(
            tfx.invoke("toggle-comment", IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(false),
        );
        assert_eq!(state.text(), "a\nb", "no marker → no edit");
        // A non-Null arg is a TypeMismatch.
        assert!(matches!(
            tfx.invoke("toggle-comment", IntrospectValue::Int(1)),
            Err(InvokeError::TypeMismatch),
        ));
        // A bare field (no state) is inert too.
        let mut bare = TextFieldExternal::new();
        assert_eq!(
            bare.invoke("toggle-comment", IntrospectValue::Null)
                .unwrap(),
            IntrospectValue::Bool(false),
        );
    }

    // ─── R941 go-to-line ──────────────────────────────────────────

    #[test]
    fn r941_invoke_go_to_line_jumps_and_returns_resolved() {
        // The AI-first peer of Ctrl+G: jump the caret to a line, echoing the
        // resolved line (the setter-returns-read-outcome contract).
        let (state, mut tfx) = wired("zero\none\ntwo\nthree"); // starts [0, 5, 9, 13]
        assert_eq!(
            tfx.invoke("go-to-line", IntrospectValue::Int(3)).unwrap(),
            IntrospectValue::Int(3),
        );
        assert_eq!(state.caret(), 9, "caret at line 3 (\"two\") start");
        assert_eq!(tfx.query("caret").unwrap(), IntrospectValue::Int(9));
    }

    #[test]
    fn r941_invoke_go_to_line_clamps_out_of_range() {
        let (state, mut tfx) = wired("a\nb\nc"); // 3 lines, starts [0, 2, 4]
        assert_eq!(
            tfx.invoke("go-to-line", IntrospectValue::Int(99)).unwrap(),
            IntrospectValue::Int(3),
            "past the end clamps to the last line",
        );
        assert_eq!(state.caret(), 4);
        assert_eq!(
            tfx.invoke("go-to-line", IntrospectValue::Int(0)).unwrap(),
            IntrospectValue::Int(1),
            "0 clamps up to the first line",
        );
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn r941_invoke_go_to_line_rejects_negative_and_bad_type() {
        let (_state, mut tfx) = wired("a\nb");
        assert!(
            matches!(
                tfx.invoke("go-to-line", IntrospectValue::Int(-1)),
                Err(InvokeError::Rejected)
            ),
            "a negative line cannot name a row (the toggle-fold guard mirror)",
        );
        assert!(matches!(
            tfx.invoke("go-to-line", IntrospectValue::Null),
            Err(InvokeError::TypeMismatch),
        ));
    }

    #[test]
    fn r941_query_line_count_tracks_the_buffer() {
        let (state, tfx) = wired("a\nb\nc");
        assert_eq!(tfx.query("line_count").unwrap(), IntrospectValue::Int(3));
        state.set_text("solo".to_string());
        assert_eq!(tfx.query("line_count").unwrap(), IntrospectValue::Int(1));
    }

    #[test]
    fn r941_bare_field_go_to_line_inert_and_line_count_none() {
        // A bare field (no attached state): go-to-line returns Int(0) — nothing
        // to navigate, distinct from a real line-1 landing — and line_count is
        // None ("not bound"), the same bare/wired distinction caret draws.
        let mut bare = TextFieldExternal::new();
        assert_eq!(
            bare.invoke("go-to-line", IntrospectValue::Int(1)).unwrap(),
            IntrospectValue::Int(0),
        );
        assert!(bare.query("line_count").is_none());
    }

    // ─── R959 / R961 §5.36 §5.22 — gutter send-route (TextFieldSendKey) ──────

    fn send_text(
        tfx: &mut TextFieldExternal,
        payload: &str,
    ) -> Result<IntrospectValue, InvokeError> {
        tfx.invoke("send", IntrospectValue::Text(payload.into()))
    }

    #[test]
    fn r961_send_key_encode_decode_roundtrip() {
        // The paint producer and the `send` decoder share one SSOT, so the
        // wire grammar cannot drift — across BOTH kinds.
        assert_eq!(
            TextFieldSendKey::gutter_line_tag("main_textarea", 3),
            "main_textarea#gl3"
        );
        assert_eq!(
            TextFieldSendKey::fold_toggle_tag("code_editor", 2),
            "code_editor#fold2"
        );
        let (primary, sub) = crate::composite_tag::split_subindex("main_textarea#gl3");
        assert_eq!(primary, "main_textarea");
        assert_eq!(
            TextFieldSendKey::parse(sub.unwrap()),
            Some(TextFieldSendKey::GutterLine { line: 3 })
        );
        assert_eq!(
            TextFieldSendKey::parse("fold2"),
            Some(TextFieldSendKey::FoldToggle { line: 2 }),
        );
        // The `gl` / `fold` prefixes are disjoint and a non-key sub does not decode.
        assert_eq!(
            TextFieldSendKey::parse("3"),
            None,
            "a bare numeric is not a key"
        );
        assert_eq!(
            TextFieldSendKey::parse("glx"),
            None,
            "a non-numeric gl tail is rejected"
        );
        assert_eq!(
            TextFieldSendKey::parse("foldx"),
            None,
            "a non-numeric fold tail is rejected"
        );
    }

    #[test]
    fn r959_send_gutter_line_jumps_caret() {
        // A gutter-number click reaches the field as `"gl<n>:PointerUp"`; the
        // caret jumps to that 1-based line's start — focus-independent, no
        // caret-drag arm (the press hook never sees it).
        let (state, mut tfx) = wired("zero\none\ntwo\nthree\nfour"); // starts [0,5,9,13,19]
        assert!(send_text(&mut tfx, "gl3:PointerUp").is_ok());
        assert_eq!(
            state.caret(),
            9,
            "click gutter 3 -> caret at line 3 (\"two\") start"
        );
        assert_eq!(
            state.selection_range(),
            None,
            "a plain gutter click collapses any selection"
        );
    }

    #[test]
    fn r959_send_gutter_line_shift_extends_selection() {
        // `Shift`+click extends the selection from the live anchor to the
        // clicked line's start (the `line_start_byte` peer, not `go_to_line`
        // which would collapse). Modifiers ride the third wire segment.
        let (state, mut tfx) = wired("zero\none\ntwo\nthree\nfour"); // starts [0,5,9,13,19]
        send_text(&mut tfx, "gl2:PointerUp").unwrap(); // caret -> line 2 start (5), anchor
        assert_eq!(state.caret(), 5);
        send_text(&mut tfx, "gl5:PointerUp:s").unwrap(); // Shift+click -> extend to line 5 start (19)
        assert_eq!(
            state.selection_range(),
            Some((5, 19)),
            "Shift+click extends from line 2 to line 5 start",
        );
    }

    #[test]
    fn r959_send_gutter_line_non_activation_edge_is_noop() {
        // The router fires the whole pointer cycle; only the `PointerUp`
        // activation edge acts. `PointerDown` / `PointerEnter` / `PointerLeave`
        // are recognized no-ops (Ok, caret unmoved) — not `Rejected`.
        let (state, mut tfx) = wired("zero\none\ntwo\nthree\nfour");
        state.set_caret(2);
        for edge in ["gl4:PointerDown", "gl4:PointerEnter", "gl4:PointerLeave"] {
            assert!(
                send_text(&mut tfx, edge).is_ok(),
                "{edge} is a recognized send"
            );
            assert_eq!(state.caret(), 2, "{edge} does not move the caret");
        }
    }

    #[test]
    fn r959_send_bare_event_and_non_gutter_composite_unaffected() {
        // The gutter branch is decoded before the bare-event path, so the
        // existing SCXML-event send is unchanged, and a non-gutter composite
        // falls through to `from_name` (which rejects it).
        let (_state, mut tfx) = wired("a\nb");
        assert!(
            send_text(&mut tfx, "Focus").is_ok(),
            "a bare SCXML event still dispatches"
        );
        assert!(
            matches!(
                send_text(&mut tfx, "foo:PointerUp"),
                Err(InvokeError::Rejected)
            ),
            "a non-gutter composite is not a recognized send",
        );
    }

    #[test]
    fn r959_send_gutter_line_bare_field_inert() {
        // A bare field (no attached state) is inert — Ok, never a panic.
        let mut bare = TextFieldExternal::new();
        assert!(send_text(&mut bare, "gl3:PointerUp").is_ok());
    }

    #[test]
    fn r961_send_fold_toggle_collapses_and_expands_region() {
        // A fold-chevron click reaches the field as `"fold<n>:PointerUp"` (0-based
        // opener line); it toggles the region opening there — the same
        // `toggle_fold` SSOT the keyboard `Enter` + `toggle-fold` RPC drive.
        let (_state, mut tfx) = wired("x {\n y\n}\n"); // a region opens on line 0
        assert!(send_text(&mut tfx, "fold0:PointerUp").is_ok());
        assert_eq!(
            regions(&tfx)[0]["collapsed"],
            serde_json::json!(true),
            "click collapsed line 0"
        );
        assert!(send_text(&mut tfx, "fold0:PointerUp").is_ok());
        assert_eq!(
            regions(&tfx)[0]["collapsed"],
            serde_json::json!(false),
            "click again expanded it"
        );
    }

    #[test]
    fn r961_send_fold_toggle_non_activation_and_no_opener_are_noops() {
        // Only the `PointerUp` activation edge acts; the `PointerDown` /
        // `PointerEnter` cycle around the click is a recognized no-op. A fold
        // sub on a non-opener line toggles nothing (toggle_fold returns false).
        let (_state, mut tfx) = wired("x {\n y\n}\n");
        assert!(send_text(&mut tfx, "fold0:PointerDown").is_ok());
        assert_eq!(
            regions(&tfx)[0]["collapsed"],
            serde_json::json!(false),
            "PointerDown did not fold"
        );
        assert!(
            send_text(&mut tfx, "fold1:PointerUp").is_ok(),
            "no opener on line 1 -> recognized no-op"
        );
        assert_eq!(
            regions(&tfx)[0]["collapsed"],
            serde_json::json!(false),
            "line 1 has no region"
        );
    }

    // ─── R945 §5.22 — move-line / duplicate-line invoke verbs ───────────────

    #[test]
    fn r945_invoke_move_line_down_and_up() {
        let (state, mut tfx) = wired("a\nb\nc");
        state.set_caret(0); // line "a"
        assert_eq!(
            tfx.invoke("move-line-down", IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(true),
            "move-line-down reports the buffer changed",
        );
        assert_eq!(state.text(), "b\na\nc");
        assert_eq!(
            tfx.invoke("move-line-up", IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(true),
        );
        assert_eq!(
            state.text(),
            "a\nb\nc",
            "move up restores the original order"
        );
    }

    #[test]
    fn r945_invoke_move_line_boundary_is_a_false_noop() {
        let (state, mut tfx) = wired("a\nb");
        state.set_caret(0); // first line
        assert_eq!(
            tfx.invoke("move-line-up", IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(false),
            "the first line cannot move up (no-op reports false)",
        );
        assert_eq!(state.text(), "a\nb", "no change");
    }

    #[test]
    fn r945_invoke_duplicate_line_down_and_up() {
        let (state, mut tfx) = wired("a\nb");
        state.set_caret(0);
        assert_eq!(
            tfx.invoke("duplicate-line-down", IntrospectValue::Null)
                .unwrap(),
            IntrospectValue::Bool(true),
        );
        assert_eq!(state.text(), "a\na\nb");
        assert_eq!(state.caret(), 2, "down lands the caret on the lower copy");

        let (state, mut tfx) = wired("a\nb");
        state.set_caret(0);
        tfx.invoke("duplicate-line-up", IntrospectValue::Null)
            .unwrap();
        assert_eq!(state.text(), "a\na\nb");
        assert_eq!(state.caret(), 0, "up keeps the caret on the upper instance");
    }

    #[test]
    fn r945_invoke_line_move_bare_inert_and_type_checked() {
        let mut bare = TextFieldExternal::new();
        for verb in [
            "move-line-up",
            "move-line-down",
            "duplicate-line-up",
            "duplicate-line-down",
        ] {
            assert_eq!(
                bare.invoke(verb, IntrospectValue::Null).unwrap(),
                IntrospectValue::Bool(false),
                "a bare field is inert ({verb}), never UnknownPath",
            );
        }
        let (_state, mut tfx) = wired("a\nb");
        assert!(
            matches!(
                tfx.invoke("move-line-down", IntrospectValue::Int(1)),
                Err(InvokeError::TypeMismatch),
            ),
            "a non-Null arg is a type mismatch"
        );
    }
}
