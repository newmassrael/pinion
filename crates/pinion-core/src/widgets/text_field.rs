//! R56.1 §5.38 — `TextField` widget catalogue entry.
//!
//! First slice of the R56 axis: §5.38 SCXML statechart + Rust binding
//! ([`TextField`] / [`TextFieldEvent`] / [`TextFieldState`] /
//! [`TextFieldExternal`]). Subsequent sub-rounds layer on top:
//! caret rendering (R56.1.b), blink animation (R56.1.c), key input
//! (R56.1.d — see [`apply_key`]), clipboard (R56.1.e), selection
//! (R56.1.f), and IME composition (R56.1.g). The R56.1.h
//! focus-lifecycle wire — shell focus mgr ↔ `External::on_focus_change`
//! ↔ statechart focus/blur drive ↔ [`CaretBlink`](crate::widgets::caret_blink::CaretBlink)
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
    clippy::all,
)]
mod sm {
    include!(concat!(env!("OUT_DIR"), "/text_field_sm.rs"));
}

pub use sm::{TextFieldEvent, TextFieldState};
use sm::TextFieldPolicy;

use std::rc::Rc;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner,
    ThreadOwnership,
};
use crate::intent::Intent;
use crate::scene::Rect;
use crate::widgets::caret_blink::CaretBlink;
use crate::widgets::text_edit::TextEditState;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};

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
///   `"Enter"`, `"Escape"`, `"Tab"`). The latter three are
///   shell-reserved upstream (`"Tab"` advances focus, `"Escape"`
///   quits the window, `"Enter"` will arrive on R56.1.h with the
///   submit-class statechart event) and never reach this hook in
///   practice; `apply_key` rejects defensively so a misrouted
///   delivery does not silently insert a literal letter.
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
pub fn apply_key(state: &TextEditState, key: &str) -> bool {
    match key {
        "Backspace" => {
            state.backspace();
            true
        }
        "Delete" => {
            state.delete_forward();
            true
        }
        "ArrowLeft" => {
            state.move_left();
            true
        }
        "ArrowRight" => {
            state.move_right();
            true
        }
        "Home" => {
            state.move_home();
            true
        }
        "End" => {
            state.move_end();
            true
        }
        // U+0020 SPACE arrives through the W3C named-key channel
        // (`NamedKey::Space → "Space"` on winit, `KeyCode::Char(' ')
        // → "Space"` on crossterm — see R51.111 pinion-tui/input.rs
        // bridge). Explicit handler avoids depending on the
        // printable-char branch interpreting `" "` (which would
        // require shell change to bypass the named-key conversion).
        "Space" => {
            state.insert(" ");
            true
        }
        other => match is_printable_key(other) {
            Some(c) => {
                // 4-byte UTF-8 buffer covers every Unicode code
                // point through U+10FFFF; `encode_utf8` returns the
                // populated subslice that `TextEditState::insert`
                // splices into the reactive text.
                let mut buf = [0u8; 4];
                state.insert(c.encode_utf8(&mut buf));
                true
            }
            None => false,
        },
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
    /// `Idle` / `Disabled` → disabled (the [`CaretBlink::tick`] no-op
    /// holds the off frame so the caret is hidden whenever the
    /// widget is unfocused).
    blink: Option<Rc<CaretBlink>>,
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
    /// [`use_text_edit_state`](crate::widgets::text_edit::use_text_edit_state)
    /// hook returns the canonical shared handle).
    #[must_use]
    pub fn text_state(&self) -> Option<&Rc<TextEditState>> {
        self.text_state.as_ref()
    }

    /// R56.1.h §5.38 §5.28 — attach a [`CaretBlink`] animation handle.
    /// After attachment, every statechart transition
    /// ([`Self::send`]) syncs the blink's enabled gate via
    /// [`Self::sync_blink`]: `Focused` / `Editing` → enabled,
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

    fn detect(
        before: Self::Snapshot,
        event: Self::Event,
        after: Self::Snapshot,
    ) -> Vec<Intent> {
        let commit_via_event = matches!(
            event,
            TextFieldEvent::CommitEdit | TextFieldEvent::Blur,
        );
        let exited_editing = matches!(before, TextFieldState::Editing)
            && !matches!(after, TextFieldState::Editing);
        if commit_via_event && exited_editing {
            vec![Intent::new_static("text_committed", IntrospectValue::Null)]
        } else {
            Vec::new()
        }
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
}

impl TextFieldExternal {
    /// Construct a `TextFieldExternal` wrapping a fresh
    /// [`TextField`] in [`TextFieldState::Idle`].
    #[must_use]
    pub fn new() -> Self {
        Self { em: IntentEmitter::default() }
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
}

impl Default for TextFieldExternal {
    fn default() -> Self {
        Self::new()
    }
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
    /// [`notify_focus_change`](`pinion_shell::ShellSubstrate::notify_focus_change`)
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
    /// The blink lifecycle syncs automatically through
    /// [`TextField::sync_blink`] (called from [`TextField::send`]),
    /// so attaching a [`CaretBlink`] makes the gate flip in lockstep
    /// with the statechart transition.
    fn on_focus_change(&mut self, focused: bool) {
        self.send(if focused {
            TextFieldEvent::Focus
        } else {
            TextFieldEvent::Blur
        });
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
        IntrospectSchema::new(&[
            ("state", "string"),
            ("text", "string"),
            ("caret", "number"),
            ("send", "string"),
            ("key", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(
                text_field_state_name(self.state()).to_string(),
            )),
            // R56.1.b §5.38 — text + caret read through the attached
            // [`TextEditState`]. `None` when no handle is attached;
            // the AI client treats that as "widget not bound to
            // reactive state" and gates intervene/invoke accordingly.
            "text" => self
                .text_state()
                .map(|s| IntrospectValue::Text(s.text())),
            "caret" => self.text_state().map(|s| {
                // usize → i64 — caret is bounded by `text.len() <=
                // isize::MAX` on every platform pinion targets, so
                // the cast is lossless. The `try_from` defends
                // against the unreachable 2^63-byte text case.
                IntrospectValue::Int(i64::try_from(s.caret()).unwrap_or(i64::MAX))
            }),
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        value: IntrospectValue,
    ) -> Result<(), InterveneError> {
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
                IntrospectValue::Text(ref name) => {
                    let ev =
                        parse_text_field_event(name).ok_or(InvokeError::Rejected)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(
                        text_field_state_name(self.state()).to_string(),
                    ))
                }
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
            "key" => match args {
                IntrospectValue::Text(ref key_str) => {
                    let handled = match self.text_state() {
                        Some(state) => apply_key(state.as_ref(), key_str),
                        None => false,
                    };
                    if handled {
                        if let Some(blink) = self.em.inner.blink() {
                            blink.reset();
                        }
                    }
                    Ok(IntrospectValue::Bool(handled))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

fn text_field_state_name(state: TextFieldState) -> &'static str {
    match state {
        TextFieldState::Idle => "Idle",
        TextFieldState::Focused => "Focused",
        TextFieldState::Editing => "Editing",
        TextFieldState::Disabled => "Disabled",
    }
}

/// External-introspect [`InvokeError::Rejected`] guard. Lowercase /
/// snake-case event aliases are not accepted — the AI client passes
/// the `PascalCase` variant name verbatim, mirroring the
/// `parse_scroll_bar_event` / `parse_slider_event` convention.
fn parse_text_field_event(name: &str) -> Option<TextFieldEvent> {
    match name {
        "Focus" => Some(TextFieldEvent::Focus),
        "Blur" => Some(TextFieldEvent::Blur),
        "BeginEdit" => Some(TextFieldEvent::BeginEdit),
        "CommitEdit" => Some(TextFieldEvent::CommitEdit),
        "CancelEdit" => Some(TextFieldEvent::CancelEdit),
        "Disable" => Some(TextFieldEvent::Disable),
        "Enable" => Some(TextFieldEvent::Enable),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! R56.1.a §5.38 — `TextField` widget binding regression battery.
    //! Mirror of the R55.D.2 `ScrollBar` test layout: initial state,
    //! four-state transition graph, commit/cancel detection, ARIA
    //! commit-on-blur path, introspect surface.

    use super::{
        parse_text_field_event, text_field_state_name, TextField, TextFieldEvent,
        TextFieldExternal, TextFieldState,
    };
    use crate::external::{
        Backend, External, ExternalIntrospect, InterveneError, IntrospectValue,
        InvokeError, RepaintOwner, ThreadOwnership,
    };

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
    fn external_schema_declares_five_slots() {
        // R56.1.b grew the surface: state + text + caret + send.
        // R56.1.d grew the surface: + key (W3C UI Events keystroke
        // dispatch).
        // The schema shape is stable across bare and wired-up
        // TextFields — text/caret queries return None / intervene
        // returns ReadOnly when no TextEditState is attached; the
        // key invoke returns `Bool(false)` for bare TextFields.
        let tfx = TextFieldExternal::new();
        let schema = tfx.schema();
        assert_eq!(
            schema.fields,
            &[
                ("state", "string"),
                ("text", "string"),
                ("caret", "number"),
                ("send", "string"),
                ("key", "string"),
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
            tfx.intervene(
                "state",
                IntrospectValue::Text("Focused".to_string()),
            ),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(tfx.state(), TextFieldState::Idle);
    }

    #[test]
    fn external_intervene_unknown_path_rejects() {
        let mut tfx = TextFieldExternal::new();
        assert_eq!(
            tfx.intervene("selection", IntrospectValue::Text(String::new())),
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
        tfx.invoke("send", IntrospectValue::Text("Focus".to_string())).unwrap();
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
        // Guard the parse_text_field_event ↔ text_field_state_name
        // string round-trip — every state's name must be stable so
        // RPC consumers can build assertions against it.
        assert_eq!(text_field_state_name(TextFieldState::Idle), "Idle");
        assert_eq!(text_field_state_name(TextFieldState::Focused), "Focused");
        assert_eq!(text_field_state_name(TextFieldState::Editing), "Editing");
        assert_eq!(text_field_state_name(TextFieldState::Disabled), "Disabled");
    }

    #[test]
    fn event_parser_covers_every_input_variant() {
        // Every externally-dispatchable event resolves. The internal
        // `TextfieldCommit` raise event is NOT in the parser table
        // (consumers do not drive raised events directly).
        assert!(matches!(
            parse_text_field_event("Focus"),
            Some(TextFieldEvent::Focus),
        ));
        assert!(matches!(
            parse_text_field_event("Blur"),
            Some(TextFieldEvent::Blur),
        ));
        assert!(matches!(
            parse_text_field_event("BeginEdit"),
            Some(TextFieldEvent::BeginEdit),
        ));
        assert!(matches!(
            parse_text_field_event("CommitEdit"),
            Some(TextFieldEvent::CommitEdit),
        ));
        assert!(matches!(
            parse_text_field_event("CancelEdit"),
            Some(TextFieldEvent::CancelEdit),
        ));
        assert!(matches!(
            parse_text_field_event("Disable"),
            Some(TextFieldEvent::Disable),
        ));
        assert!(matches!(
            parse_text_field_event("Enable"),
            Some(TextFieldEvent::Enable),
        ));
        assert_eq!(parse_text_field_event("textfield_commit"), None);
        assert_eq!(parse_text_field_event(""), None);
    }
}

#[cfg(test)]
mod r56_1_b_tests {
    //! R56.1.b §5.38 §5.21 — `caret_rect` closed-form helper +
    //! [`TextField`] composition with [`TextEditState`] +
    //! introspect text/caret slots.

    use super::{caret_rect, TextField, TextFieldEvent, TextFieldExternal};
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
            tfx.intervene(
                "state",
                IntrospectValue::Text("Focused".to_string()),
            ),
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

    use super::{apply_key, TextField, TextFieldExternal};
    use crate::external::{External, ExternalIntrospect, IntrospectValue, InvokeError};
    use crate::widgets::text_edit::TextEditState;
    use std::rc::Rc;

    // ─────────────────────────────────────────────────────────────
    // apply_key — recognized named keys (caret-relative edit ops)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_d_backspace_deletes_char_left_of_caret() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(3);
        assert!(apply_key(&state, "Backspace"));
        assert_eq!(state.text(), "ab");
        assert_eq!(state.caret(), 2);
    }

    #[test]
    fn r56_1_d_backspace_at_caret_zero_no_ops_but_returns_handled() {
        // W3C `defaultPrevented` semantics: the key was *recognized*
        // (consumed) even when the visible mutation is a no-op.
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(0);
        assert!(apply_key(&state, "Backspace"));
        assert_eq!(state.text(), "abc");
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn r56_1_d_delete_removes_char_at_caret() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(0);
        assert!(apply_key(&state, "Delete"));
        assert_eq!(state.text(), "bc");
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn r56_1_d_delete_at_end_no_ops_but_returns_handled() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(3);
        assert!(apply_key(&state, "Delete"));
        assert_eq!(state.text(), "abc");
    }

    #[test]
    fn r56_1_d_arrow_left_moves_caret_back_one() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(2);
        assert!(apply_key(&state, "ArrowLeft"));
        assert_eq!(state.caret(), 1);
    }

    #[test]
    fn r56_1_d_arrow_left_at_zero_no_ops_but_returns_handled() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(0);
        assert!(apply_key(&state, "ArrowLeft"));
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn r56_1_d_arrow_right_moves_caret_forward_one() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(1);
        assert!(apply_key(&state, "ArrowRight"));
        assert_eq!(state.caret(), 2);
    }

    #[test]
    fn r56_1_d_arrow_right_at_end_no_ops_but_returns_handled() {
        let state = TextEditState::with_initial("abc".to_string());
        state.set_caret(3);
        assert!(apply_key(&state, "ArrowRight"));
        assert_eq!(state.caret(), 3);
    }

    #[test]
    fn r56_1_d_home_moves_caret_to_zero() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_caret(4);
        assert!(apply_key(&state, "Home"));
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn r56_1_d_end_moves_caret_to_text_len() {
        let state = TextEditState::with_initial("abcdef".to_string());
        state.set_caret(2);
        assert!(apply_key(&state, "End"));
        assert_eq!(state.caret(), 6);
    }

    #[test]
    fn r56_1_d_space_inserts_single_space() {
        let state = TextEditState::with_initial("ab".to_string());
        state.set_caret(2);
        assert!(apply_key(&state, "Space"));
        assert_eq!(state.text(), "ab ");
        assert_eq!(state.caret(), 3);
    }

    // ─────────────────────────────────────────────────────────────
    // apply_key — printable single-char insertion
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_d_lowercase_letter_inserts_at_caret() {
        let state = TextEditState::new();
        assert!(apply_key(&state, "a"));
        assert_eq!(state.text(), "a");
        assert_eq!(state.caret(), 1);
    }

    #[test]
    fn r56_1_d_uppercase_letter_inserts_at_caret() {
        let state = TextEditState::new();
        assert!(apply_key(&state, "A"));
        assert_eq!(state.text(), "A");
    }

    #[test]
    fn r56_1_d_digit_inserts_at_caret() {
        let state = TextEditState::new();
        assert!(apply_key(&state, "7"));
        assert_eq!(state.text(), "7");
    }

    #[test]
    fn r56_1_d_punctuation_inserts_at_caret() {
        // Listbox typeahead rejects non-alphanumeric; text input
        // accepts every non-control codepoint.
        let state = TextEditState::new();
        assert!(apply_key(&state, "!"));
        assert!(apply_key(&state, ","));
        assert!(apply_key(&state, "$"));
        assert_eq!(state.text(), "!,$");
    }

    #[test]
    fn r56_1_d_cjk_ideograph_inserts_at_caret() {
        // Pre-composed CJK glyph (already-resolved by IME) flows
        // through the printable-char branch as a single codepoint.
        // Multi-char IME composition results are R56.1.g territory.
        let state = TextEditState::new();
        assert!(apply_key(&state, "漢"));
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
        assert!(apply_key(&state, "안"));
        assert_eq!(state.text(), "안");
        assert_eq!(state.caret(), 3, "안 = 3 UTF-8 bytes (U+C548)");
    }

    #[test]
    fn r56_1_d_insert_at_mid_position_splices() {
        let state = TextEditState::with_initial("ac".to_string());
        state.set_caret(1);
        assert!(apply_key(&state, "b"));
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
        assert!(!apply_key(&state, "ArrowUp"));
        assert_eq!((state.text(), state.caret()), before);
    }

    #[test]
    fn r56_1_d_arrow_down_returns_false_pending_multiline() {
        let state = TextEditState::new();
        assert!(!apply_key(&state, "ArrowDown"));
    }

    #[test]
    fn r56_1_d_page_up_down_return_false() {
        let state = TextEditState::new();
        assert!(!apply_key(&state, "PageUp"));
        assert!(!apply_key(&state, "PageDown"));
    }

    #[test]
    fn r56_1_d_enter_returns_false_pending_submit_event() {
        // R56.1.h plans the submit-class statechart event; on
        // R56.1.d Enter falls through (Enter is shell-reserved
        // upstream anyway — it never reaches apply_key in
        // practice, but the rejection is defensive).
        let state = TextEditState::with_initial("abc".to_string());
        assert!(!apply_key(&state, "Enter"));
        assert_eq!(state.text(), "abc");
    }

    #[test]
    fn r56_1_d_function_keys_return_false() {
        let state = TextEditState::new();
        assert!(!apply_key(&state, "F1"));
        assert!(!apply_key(&state, "F12"));
        assert_eq!(state.text(), "");
    }

    #[test]
    fn r56_1_d_empty_key_returns_false() {
        let state = TextEditState::new();
        assert!(!apply_key(&state, ""));
        assert_eq!(state.text(), "");
    }

    #[test]
    fn r56_1_d_multi_char_string_returns_false() {
        // IME composition multi-char output (R56.1.g territory)
        // flows through the preedit-buffer substrate, not this hook.
        let state = TextEditState::new();
        assert!(!apply_key(&state, "ab"));
        assert!(!apply_key(&state, "hello"));
        assert_eq!(state.text(), "");
    }

    #[test]
    fn r56_1_d_control_char_returns_false() {
        // Raw tab / newline / null are bug-fixture paths — the
        // framework converts these to named keys at the input
        // boundary. Defensive rejection.
        let state = TextEditState::new();
        assert!(!apply_key(&state, "\t"));
        assert!(!apply_key(&state, "\n"));
        assert!(!apply_key(&state, "\u{0000}"));
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
            schema.fields.iter().any(|(name, ty)| *name == "key" && *ty == "string"),
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
            "a", "b", "c", "Space", "Backspace", "ArrowLeft", "ArrowRight",
            "Home", "End", "Delete",
        ] {
            tfx.invoke("key", IntrospectValue::Text(key.to_string())).unwrap();
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

    #[test]
    fn r56_1_d_apply_key_works_on_bare_text_field_via_direct_helper() {
        // Direct helper API parity — `apply_key(state, key)` works
        // on any TextEditState handle, independent of whether the
        // state is attached to a TextField/TextFieldExternal.
        let state = TextEditState::new();
        assert!(apply_key(&state, "h"));
        assert!(apply_key(&state, "i"));
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

    use super::{TextFieldExternal, TextFieldEvent};
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
        assert!(blink.visible(), "recognized key snaps blink back to visible");
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
        assert!(blink.visible(), "Backspace resets even when it's a caret-0 no-op");
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

