//! R56.1.b §5.38 §5.22 — Reactive state companion for the
//! [`TextField`](crate::widgets::text_field::TextField) widget.
//!
//! Mirror of the R55.B [`ScrollState`](crate::widgets::scroll::ScrollState)
//! pattern: a §5.22 reactive sidecar that holds the **content**
//! (text + caret position) of an editable text widget, kept
//! orthogonal to the widget's SCXML interaction state. The SCXML
//! statechart (R56.1.a) owns Idle / Focused / Editing / Disabled;
//! this primitive owns the byte buffer + caret index.
//!
//! ## Scope on this slice
//!
//! - [`TextEditState`]: byte-string text + UTF-8 byte-offset caret +
//!   the §5.22 `Signal` wrapper around each so view-fn subscribers
//!   re-run on edits, with [`crate::reactive::batch`] wrapping each
//!   multi-axis mutation (text + caret) for the textbook
//!   atomic-update reactive contract.
//! - [`use_text_edit_state`]: `Owner::cache`-keyed hook (mirror of
//!   [`use_scroll_state`](crate::widgets::scroll::use_scroll_state)).
//! - Caret motion (`move_left` / `move_right` / `move_home` /
//!   `move_end`) honours **char boundaries** via
//!   [`str::is_char_boundary`] — never lands the caret in the middle
//!   of a UTF-8 multi-byte sequence. Extended-grapheme-cluster
//!   motion (the canonical "one user-perceived character") is
//!   R56.1.f carry — requires the `unicode-segmentation` crate and
//!   only matters once the selection axis exposes shift+arrow
//!   semantics.
//!
//! ## Deferred to later sub-rounds
//!
//! - Caret geometry derivation against shaped glyph runs lives in
//!   the [`super::text_field`] R56.1.b caret-rect helper (the
//!   geometry side); this primitive is the value side only.
//! - Caret blink animation: R56.1.c (`Owner::cache` + `Tickable`).
//! - Key input dispatch: R56.1.d (`apply_key` route).
//! - Clipboard / IME: R56.1.e / R56.1.g.
//!
//! ## R56.1.f §5.22 — selection sidecar
//!
//! `selection_anchor: Signal<Option<usize>>` carries the byte offset
//! where the user-driven selection extension started. The pair
//! `(anchor, caret)` follows the W3C DOM Selection API shape: caret
//! is the **focus** (the moving end), anchor is the **anchor** (the
//! pinned end). `None` means "no active selection" (caret-only); the
//! canonical W3C "collapsed" state is also surfaced here as `None`
//! after a same-position write so `has_selection` is `false` whenever
//! anchor and caret coincide. Every text mutator (`insert` /
//! `backspace` / `delete_forward`) drains the selected range before
//! applying its edit, then clears the anchor — the canonical macOS /
//! GTK / Web replace-on-keystroke contract.
//!
//! ## R56.1.g §5.22 — IME preedit buffer
//!
//! `preedit_buffer: Signal<Option<String>>` mirrors the W3C
//! `CompositionEvent` `data` payload across the composition lifecycle.
//! `None` is "not composing"; `Some(s)` is "active composition with
//! `s` as the current preedit string" (`s` may be empty during the
//! transient compositionstart → compositionupdate window). The buffer
//! lives orthogonal to [`text`](Self::text): the canonical platform
//! IME contract (Wayland text-input-v3, macOS `NSTextInputContext`,
//! Windows TSF, GTK `IBus`) keeps preedit display in a separate
//! channel from the committed text so the application paint code
//! stitches the two together (`text[..caret] + preedit + text[caret..]`).
//! Four mutators drive the lifecycle: [`preedit_start`](Self::preedit_start),
//! [`preedit_update`](Self::preedit_update),
//! [`preedit_commit`](Self::preedit_commit),
//! [`preedit_cancel`](Self::preedit_cancel). The mutators batch the
//! 2- or 3-axis writes (`text` + `caret` + `preedit_buffer`) under
//! the R55.G.24 atomic-multi-axis reactive contract.
//!
//! ## Why a separate reactive primitive
//!
//! Same R55.B rationale: the SCXML interaction state and the value
//! sidecar evolve on different cadences. The interaction state
//! changes ~once per user gesture (focus, begin compose, commit);
//! the value sidecar changes every keystroke. Separating them lets
//! every keystroke flow through one `Signal::set` cascade without
//! re-driving the SCXML, and lets the AI introspection layer
//! observe the text content without going through the `send` invoke
//! channel.

use std::cell::Cell;
use std::rc::Rc;

use crate::reactive::{batch, Owner, Signal};

/// R56.1.b §5.38 §5.22 — Reactive text + caret pair for one
/// [`TextField`](crate::widgets::text_field::TextField).
///
/// Lifecycle: created lazily via [`use_text_edit_state`] (which
/// delegates to [`Owner::cache`](crate::reactive::Owner::cache)).
/// The cache contract guarantees the same key resolves to the same
/// `Rc<TextEditState>` across view re-runs, so the buffer + caret
/// persist across paints — the standard `use_*` hook contract.
///
/// Caret encoding: **UTF-8 byte offset** into [`Self::text`]. The
/// mutators ensure every stored offset lands on a `char` boundary
/// (so `text[..caret]` and `text[caret..]` are always valid
/// `&str`s). Multi-byte char movement: callers use [`Self::move_left`]
/// / [`Self::move_right`] which respect `is_char_boundary`. Extended
/// grapheme cluster movement (e.g. a flag emoji is one user-
/// perceived "character" but several code-points) is R56.1.f carry.
///
/// Subscription: [`Self::text`] / [`Self::caret`] /
/// [`Self::snapshot`] trigger `Signal` auto-subscription when called
/// inside a view-fn (`root_owner.run(...)` wrap, per R51.146 /
/// R51.152 / R51.171 callback-root-owner-wrap discipline). The view
/// re-runs on the next value-changing `set` — the framework's
/// standard reactive shape.
///
/// Atomicity: every mutator that touches both text and caret
/// ([`Self::set_text`], [`Self::insert`], [`Self::backspace`],
/// [`Self::delete_forward`]) wraps the two `Signal::set` calls in
/// [`batch`] so a subscribed `Effect` / `Owner` re-runs **once** per
/// logical edit. Mirror of the R55.G.24 [`ScrollState`] batched
/// multi-axis contract — atomic-update is the canonical reactive
/// shape, equality-skip alone does not suffice.
#[derive(Debug)]
pub struct TextEditState {
    /// Byte buffer of the editable text. Always a valid `String`
    /// (UTF-8 invariant guaranteed by Rust's `String` type).
    text: Signal<String>,
    /// UTF-8 byte offset of the caret into [`Self::text`]. Always
    /// `<= text.len()` and always a `char` boundary. The `usize`
    /// type matches `String` indexing conventions; a future R56.x
    /// round may layer grapheme-cluster indexing on top.
    caret_pos: Signal<usize>,
    /// R56.1.f §5.22 — byte offset of the **selection anchor** into
    /// [`Self::text`]. The pair `(anchor, caret)` carries a W3C DOM
    /// Selection: caret is the focus (the moving end), anchor is the
    /// pinned end. `None` is "no selection" (caret-only). On any
    /// `select_*` extension that lands the anchor at the current
    /// caret position the field collapses back to `None` so
    /// [`Self::has_selection`] is a pure boolean predicate (no
    /// distinction between "no anchor" and "anchor coincides with
    /// caret"). Same `char`-boundary invariant as `caret_pos`.
    selection_anchor: Signal<Option<usize>>,
    /// R56.1.g §5.22 — IME preedit buffer. `None` is "not composing"
    /// (post-`preedit_cancel` / post-`preedit_commit` / default).
    /// `Some(s)` is "composition active with `s` as the current
    /// preedit string"; `s` may be empty during the transient window
    /// between `compositionstart` and the first `compositionupdate`.
    /// Stays orthogonal to [`Self::text`]: the canonical platform IME
    /// contract (Wayland text-input-v3, macOS `NSTextInputContext`,
    /// Windows TSF, GTK `IBus`) keeps preedit display in a separate
    /// channel from the committed buffer so the application paint code
    /// stitches the two together for visual rendering. See the
    /// [`Self::preedit`] / [`Self::is_composing`] accessors and the
    /// four canonical mutators ([`Self::preedit_start`],
    /// [`Self::preedit_update`], [`Self::preedit_commit`],
    /// [`Self::preedit_cancel`]).
    preedit_buffer: Signal<Option<String>>,
    /// Canonical input-router / introspection tag for this text
    /// container. Set by [`use_text_edit_state`] from the
    /// `Owner::cache` key so the matching [`TextField`] /
    /// [`TextFieldExternal`] can route ARIA accessible-name + RPC
    /// `scene/<tag>/...` calls without the caller repeating the
    /// string literal. `None` for states constructed via
    /// [`Self::new`] directly (test fixtures, manual wiring).
    ///
    /// Mirrors the R51.190 [`ScrollState::tag`] convention.
    ///
    /// [`TextField`]: crate::widgets::text_field::TextField
    /// [`TextFieldExternal`]: crate::widgets::text_field::TextFieldExternal
    /// [`ScrollState::tag`]: crate::widgets::scroll::ScrollState::tag
    tag: Option<&'static str>,
    /// R766 §5.22 — **goal column** for multi-line vertical caret
    /// navigation (`ArrowUp` / `ArrowDown`). Holds the layout-space
    /// `x` the caret should aim for as it crosses visual lines, so a
    /// run of vertical moves through a short line and back to a long
    /// one returns to the original column instead of drifting to the
    /// short line's end. Mirror of the `h_pos` parley's own
    /// [`Selection`](parley::Selection) maintains across `move_lines`
    /// calls — but pinion's caret is a geometry-free byte offset
    /// reshaped each frame, so the goal cannot ride along inside a
    /// persisted `Selection`; it lives here instead.
    ///
    /// `None` means "no active vertical run" — the next `ArrowUp` /
    /// `ArrowDown` seeds the goal from the caret's current column. The
    /// geometry-aware multi-line binding re-arms it (via
    /// [`Self::set_goal_column`]) after each vertical move; **every**
    /// caret-repositioning or text-editing mutator below clears it so
    /// any horizontal move / click / edit / `Home` / `End` resets the
    /// goal — the canonical "goal column survives only an unbroken
    /// vertical sequence" contract shared by macOS / Windows / GTK /
    /// every code editor.
    ///
    /// A non-reactive [`Cell`] (not a [`Signal`]): no view-fn renders
    /// the goal column, it is pure caret-navigation scratch state, so
    /// arming / clearing it must never trigger a re-paint cascade.
    goal_column: Cell<Option<f32>>,
}

impl TextEditState {
    /// Construct a fresh `TextEditState` with empty text and caret
    /// at `0`. Most application code reaches `TextEditState`
    /// through [`use_text_edit_state`] (which calls
    /// [`Self::with_tag`] under the hood); direct callers are
    /// typically tests + manual fixtures.
    #[must_use]
    pub fn new() -> Self {
        Self {
            text: Signal::new(String::new()),
            caret_pos: Signal::new(0),
            selection_anchor: Signal::new(None),
            preedit_buffer: Signal::new(None),
            tag: None,
            goal_column: Cell::new(None),
        }
    }

    /// Construct a `TextEditState` tagged with `key`. Used by
    /// [`use_text_edit_state`] as the [`Owner::cache`] factory so a
    /// downstream consumer (e.g. an introspection layer) can read
    /// the tag back without repeating the string. Mirrors
    /// [`ScrollState::with_tag`](crate::widgets::scroll::ScrollState::with_tag).
    #[must_use]
    pub fn with_tag(key: &'static str) -> Self {
        Self {
            tag: Some(key),
            ..Self::new()
        }
    }

    /// Construct a `TextEditState` pre-populated with `initial_text`
    /// and the caret at the end of that text (mirror of the
    /// canonical text-input behaviour: open a field with prefilled
    /// content, caret ready to append). Provided for test fixtures
    /// and application code that wants to surface a non-empty
    /// initial value without a two-step `new` + `set_text` dance.
    ///
    /// The caret is placed at `initial_text.len()` (the textbook end
    /// position) and is guaranteed to land on a `char` boundary
    /// because `String::len()` is always one.
    #[must_use]
    pub fn with_initial(initial_text: String) -> Self {
        let caret = initial_text.len();
        Self {
            text: Signal::new(initial_text),
            caret_pos: Signal::new(caret),
            selection_anchor: Signal::new(None),
            preedit_buffer: Signal::new(None),
            tag: None,
            goal_column: Cell::new(None),
        }
    }

    /// Canonical tag for this text container. Returns the `key`
    /// passed to [`use_text_edit_state`] (or [`Self::with_tag`]);
    /// `None` for states constructed via [`Self::new`] /
    /// [`Self::with_initial`] directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// Current text buffer. Triggers a `Signal` subscription when
    /// called inside a view-fn — the view re-runs when any
    /// mutator that touches the text fires.
    #[must_use]
    pub fn text(&self) -> String {
        self.text.get()
    }

    /// Current caret byte offset. Subscription semantics symmetric
    /// with [`Self::text`].
    #[must_use]
    pub fn caret(&self) -> usize {
        self.caret_pos.get()
    }

    /// Current `(text, caret)` snapshot. Both signals subscribe;
    /// use this when the view-fn renders both pieces together (the
    /// canonical caret-rendering path needs the substring up to the
    /// caret + the caret offset itself).
    #[must_use]
    pub fn snapshot(&self) -> (String, usize) {
        (self.text(), self.caret())
    }

    /// R56.1.f §5.22 — current selection anchor as a raw `Option`.
    /// `Some(idx)` carries the byte offset where a user-driven
    /// selection extension started (Shift+Arrow / Shift+Home /
    /// Shift+End / mouse drag). `None` means "no active selection";
    /// the caret stands alone. Subscription semantics symmetric with
    /// [`Self::caret`].
    #[must_use]
    pub fn selection_anchor(&self) -> Option<usize> {
        self.selection_anchor.get()
    }

    /// R56.1.f §5.22 — current selection range as `(start, end)`
    /// byte offsets with `start <= end`. Returns `None` when the
    /// selection is collapsed (no anchor, or anchor coincides with
    /// the caret). Subscribes to both `caret` and `selection_anchor`
    /// signals so a view-fn rendering the selection rect re-runs on
    /// every selection-affecting mutation.
    ///
    /// Both offsets are guaranteed `char` boundaries — the mutators
    /// snap any anchor input through [`clamp_to_char_boundary`].
    #[must_use]
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor.get()?;
        let caret = self.caret_pos.get();
        if anchor == caret {
            return None;
        }
        Some((anchor.min(caret), anchor.max(caret)))
    }

    /// R56.1.e §5.22 — substring of the active selection, ready for
    /// the [`Clipboard`](crate::clipboard::Clipboard) `copy(text)`
    /// path. Returns `None` when the selection is collapsed
    /// (caret-only). Subscribes to both `text` and `caret_pos` /
    /// `selection_anchor` so the canonical clipboard-keystroke
    /// branch (Ctrl+C / Ctrl+X) inside a reactive scope picks up
    /// every selection-affecting mutation.
    #[must_use]
    pub fn selection_text(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        let text = self.text.get();
        Some(text[start..end].to_string())
    }

    /// R56.1.f §5.22 — `true` when [`Self::selection_range`] would
    /// return `Some(_)`. Convenience predicate so view-fn /
    /// `apply_key` branches that gate on "selection present" do not
    /// have to destructure the tuple.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    /// R56.1.f §5.22 — set both ends of the selection in one batched
    /// write. `anchor` becomes the pinned end (the user's drag start
    /// / Shift-modifier latch); `focus` becomes the caret. Both
    /// offsets are clamped to `[0, text.len()]` and snapped to the
    /// nearest preceding `char` boundary; the call collapses back to
    /// the caret-only shape (`selection_anchor = None`) when the two
    /// snapped offsets coincide.
    ///
    /// Wrapped in [`batch`] so subscribed effects re-run **once** per
    /// `set_selection` call (the R55.G.24 atomic-multi-axis contract;
    /// `selection_anchor` + `caret_pos` collapse into one cascade).
    pub fn set_selection(&self, anchor: usize, focus: usize) {
        self.goal_column.set(None);
        let text = self.text.get();
        let len = text.len();
        let snapped_anchor = clamp_to_char_boundary(&text, anchor.min(len));
        let snapped_focus = clamp_to_char_boundary(&text, focus.min(len));
        batch(|| {
            self.caret_pos.set(snapped_focus);
            self.selection_anchor.set(if snapped_anchor == snapped_focus {
                None
            } else {
                Some(snapped_anchor)
            });
        });
    }

    /// R56.1.f §5.22 — collapse any active selection to the caret-
    /// only shape (`selection_anchor = None`). The caret stays put.
    /// Called by every selection-clearing keystroke
    /// (`ArrowLeft` / `ArrowRight` / `Home` / `End` without Shift —
    /// see [`Self::move_left`] et al.) and by [`Self::set_text`] /
    /// [`Self::set_caret`] (any explicit caret repositioning drops
    /// the selection per the W3C `selectionchange` canonical
    /// behaviour).
    pub fn clear_selection(&self) {
        self.selection_anchor.set(None);
    }

    /// R766 §5.22 — current vertical-navigation **goal column** (the
    /// layout-space `x` an `ArrowUp` / `ArrowDown` run aims for).
    /// `None` when no vertical run is active. See the
    /// [`goal_column`](Self::goal_column) field doc for the full
    /// contract. Non-reactive: reading it never subscribes a view-fn.
    #[must_use]
    pub fn goal_column(&self) -> Option<f32> {
        self.goal_column.get()
    }

    /// R766 §5.22 — arm (or clear, with `None`) the vertical-navigation
    /// goal column. The geometry-aware multi-line binding calls this
    /// with `Some(x)` **after** writing the new caret for an `ArrowUp` /
    /// `ArrowDown` move (the caret write itself cleared the goal), so
    /// the next vertical move in the run reuses the same target column.
    /// Non-reactive: arming it never triggers a re-paint.
    pub fn set_goal_column(&self, x: Option<f32>) {
        self.goal_column.set(x);
    }

    /// Replace the buffer with `new_text`. The caret is clamped
    /// to the nearest `char` boundary at-or-below the new text
    /// length (so a `set_text` shorter than the current text moves
    /// the caret in instead of leaving it past-end).
    ///
    /// R56.1.f §5.22 — also clears any active selection (an
    /// explicit `set_text` invalidates the prior anchor since the
    /// underlying byte string changed wholesale).
    ///
    /// R56.1.g §5.22 — also clears any active preedit (an explicit
    /// `set_text` invalidates the composition the same way it
    /// invalidates the selection — the underlying text changed
    /// wholesale, the preedit byte offset references no longer make
    /// sense). The four writes (`text`, `caret_pos`,
    /// `selection_anchor`, `preedit_buffer`) collapse into one
    /// notification cascade via [`batch`].
    pub fn set_text(&self, new_text: String) {
        self.goal_column.set(None);
        let new_len = new_text.len();
        let cur_caret = self.caret_pos.get();
        let clamped_caret = clamp_to_char_boundary(&new_text, cur_caret.min(new_len));
        batch(|| {
            self.text.set(new_text);
            self.caret_pos.set(clamped_caret);
            self.selection_anchor.set(None);
            self.preedit_buffer.set(None);
        });
    }

    /// Move the caret to `pos`, clamped to `[0, text.len()]` and
    /// snapped to the nearest preceding `char` boundary if `pos`
    /// would land mid-codepoint. Signal equality-skip suppresses
    /// the re-run when the clamped target matches the current
    /// caret.
    ///
    /// R56.1.f §5.22 — an explicit caret-set drops any active
    /// selection (W3C `selectionchange` canonical: every
    /// caret-affecting operation that is not a Shift-modified
    /// extension collapses to caret-only).
    pub fn set_caret(&self, pos: usize) {
        self.goal_column.set(None);
        let text = self.text.get();
        let clamped = clamp_to_char_boundary(&text, pos.min(text.len()));
        batch(|| {
            self.caret_pos.set(clamped);
            self.selection_anchor.set(None);
        });
    }

    /// Insert `s` at the current caret position and advance the
    /// caret by `s.len()` bytes (canonical insert-after-cursor
    /// behaviour shared by every text widget on every platform).
    /// No-op if `s.is_empty()` — equality-skip on both signals.
    ///
    /// The caret advances by **bytes**, not chars / graphemes. The
    /// returned position is always a `char` boundary because `s` is
    /// a valid UTF-8 `&str` (Rust invariant) and the insertion
    /// happens at a pre-existing boundary.
    ///
    /// R56.1.f §5.22 — when a selection is active, the selected
    /// range is drained first and `s` is inserted at the range
    /// start (macOS / iOS / GTK / Web canonical "type to replace"
    /// behaviour). The selection collapses to `None` post-write.
    pub fn insert(&self, s: &str) {
        self.goal_column.set(None);
        if s.is_empty() {
            return;
        }
        let mut buf = self.text.get();
        let caret = self.caret_pos.get().min(buf.len());
        if let Some((start, end)) = self.selection_range_against(&buf, caret) {
            buf.drain(start..end);
            buf.insert_str(start, s);
            let new_caret = start + s.len();
            batch(|| {
                self.text.set(buf);
                self.caret_pos.set(new_caret);
                self.selection_anchor.set(None);
            });
            return;
        }
        let snapped = clamp_to_char_boundary(&buf, caret);
        buf.insert_str(snapped, s);
        let new_caret = snapped + s.len();
        batch(|| {
            self.text.set(buf);
            self.caret_pos.set(new_caret);
        });
    }

    /// Delete the `char` immediately preceding the caret and move
    /// the caret back by the deleted span (Backspace canonical).
    /// No-op when caret is at `0`. Handles multi-byte chars
    /// correctly via [`str::is_char_boundary`].
    ///
    /// R56.1.f §5.22 — when a selection is active, the selected
    /// range is drained and the caret lands at the range start
    /// (Backspace-as-selection-delete is the W3C canonical behaviour
    /// for `inputType: "deleteContentBackward"` with non-collapsed
    /// selection).
    pub fn backspace(&self) {
        self.goal_column.set(None);
        let mut buf = self.text.get();
        let caret = self.caret_pos.get().min(buf.len());
        if let Some((start, end)) = self.selection_range_against(&buf, caret) {
            buf.drain(start..end);
            batch(|| {
                self.text.set(buf);
                self.caret_pos.set(start);
                self.selection_anchor.set(None);
            });
            return;
        }
        if caret == 0 {
            return;
        }
        let prev = prev_char_boundary(&buf, caret);
        buf.drain(prev..caret);
        batch(|| {
            self.text.set(buf);
            self.caret_pos.set(prev);
        });
    }

    /// Delete the `char` immediately following the caret. The caret
    /// stays in place (Delete-key / `Ctrl-D` canonical). No-op when
    /// caret is at `text.len()`.
    ///
    /// R56.1.f §5.22 — when a selection is active, the selected
    /// range is drained and the caret lands at the range start
    /// (Delete-as-selection-delete is the W3C canonical behaviour
    /// for `inputType: "deleteContentForward"` with non-collapsed
    /// selection).
    pub fn delete_forward(&self) {
        self.goal_column.set(None);
        let mut buf = self.text.get();
        let caret = self.caret_pos.get().min(buf.len());
        if let Some((start, end)) = self.selection_range_against(&buf, caret) {
            buf.drain(start..end);
            batch(|| {
                self.text.set(buf);
                self.caret_pos.set(start);
                self.selection_anchor.set(None);
            });
            return;
        }
        if caret >= buf.len() {
            return;
        }
        let next = next_char_boundary(&buf, caret);
        buf.drain(caret..next);
        // Caret stays — only the text changes. Single `Signal::set`
        // means no `batch` needed (the contract is "atomic multi-
        // axis", a single-axis write is already atomic).
        self.text.set(buf);
    }

    /// Move the caret one `char` to the left (towards `0`).
    /// No-op when caret is at `0`.
    ///
    /// R56.1.f §5.22 — collapses any active selection: plain
    /// `ArrowLeft` (no Shift) drops the anchor; if the selection was
    /// active, the caret lands at the **selection start** (the W3C
    /// canonical "collapse to leading edge" behaviour shared by
    /// macOS / iOS / GTK / Chrome), not one char to the left of the
    /// caret. See [`Self::select_left`] for the Shift+ArrowLeft
    /// extension variant.
    pub fn move_left(&self) {
        self.goal_column.set(None);
        let text = self.text.get();
        let caret = self.caret_pos.get().min(text.len());
        if let Some((start, _)) = self.selection_range_against(&text, caret) {
            batch(|| {
                self.caret_pos.set(start);
                self.selection_anchor.set(None);
            });
            return;
        }
        if caret == 0 {
            return;
        }
        let prev = prev_char_boundary(&text, caret);
        self.caret_pos.set(prev);
    }

    /// Move the caret one `char` to the right (towards `text.len()`).
    /// No-op when caret is at `text.len()`.
    ///
    /// R56.1.f §5.22 — collapses any active selection to the
    /// **selection end** (W3C "collapse to trailing edge"). See
    /// [`Self::select_right`] for the Shift+ArrowRight extension.
    pub fn move_right(&self) {
        self.goal_column.set(None);
        let text = self.text.get();
        let caret = self.caret_pos.get().min(text.len());
        if let Some((_, end)) = self.selection_range_against(&text, caret) {
            batch(|| {
                self.caret_pos.set(end);
                self.selection_anchor.set(None);
            });
            return;
        }
        if caret >= text.len() {
            return;
        }
        let next = next_char_boundary(&text, caret);
        self.caret_pos.set(next);
    }

    /// Move the caret to the start of the buffer (Home / Ctrl-A
    /// canonical on single-line fields). Clears any active
    /// selection (R56.1.f).
    pub fn move_home(&self) {
        self.goal_column.set(None);
        batch(|| {
            self.caret_pos.set(0);
            self.selection_anchor.set(None);
        });
    }

    /// Move the caret to the end of the buffer (End / Ctrl-E
    /// canonical on single-line fields). Clears any active
    /// selection (R56.1.f).
    pub fn move_end(&self) {
        self.goal_column.set(None);
        let len = self.text.get().len();
        batch(|| {
            self.caret_pos.set(len);
            self.selection_anchor.set(None);
        });
    }

    // ───────────────────────────────────────────────────────────────
    // R56.1.f §5.22 — selection-extending caret motion (Shift+Arrow)
    // ───────────────────────────────────────────────────────────────

    /// R56.1.f §5.22 — extend the selection one `char` to the left.
    /// If no selection is active, the current caret position becomes
    /// the anchor; the caret then moves one char left. If the
    /// extension lands the caret on the anchor (collapsing the
    /// selection back), the anchor clears to `None` so the
    /// caret-only invariant holds.
    ///
    /// Shift+ArrowLeft / Shift+Backspace canonical on every desktop
    /// platform. No-op when the caret is already at byte 0 and no
    /// selection is active.
    pub fn select_left(&self) {
        self.goal_column.set(None);
        let text = self.text.get();
        let caret = self.caret_pos.get().min(text.len());
        if caret == 0 {
            // Caret already at the left edge — the only useful
            // mutation here would be to set the anchor without
            // moving the caret, but that contradicts the W3C
            // "extend = move + remember-old-position" contract.
            return;
        }
        let new_caret = prev_char_boundary(&text, caret);
        let anchor = self.selection_anchor.get().unwrap_or(caret);
        batch(|| {
            self.caret_pos.set(new_caret);
            self.selection_anchor.set(if anchor == new_caret {
                None
            } else {
                Some(anchor)
            });
        });
    }

    /// R56.1.f §5.22 — extend the selection one `char` to the right
    /// (mirror of [`Self::select_left`]). Shift+ArrowRight canonical.
    pub fn select_right(&self) {
        self.goal_column.set(None);
        let text = self.text.get();
        let caret = self.caret_pos.get().min(text.len());
        if caret >= text.len() {
            return;
        }
        let new_caret = next_char_boundary(&text, caret);
        let anchor = self.selection_anchor.get().unwrap_or(caret);
        batch(|| {
            self.caret_pos.set(new_caret);
            self.selection_anchor.set(if anchor == new_caret {
                None
            } else {
                Some(anchor)
            });
        });
    }

    /// R56.1.f §5.22 — extend the selection to the start of the
    /// buffer. Shift+Home canonical (single-line fields). If no
    /// selection was active, the current caret position becomes the
    /// anchor; the caret then jumps to byte 0.
    pub fn select_home(&self) {
        let caret = self.caret_pos.get();
        let anchor = self.selection_anchor.get().unwrap_or(caret);
        batch(|| {
            self.caret_pos.set(0);
            self.selection_anchor.set(if anchor == 0 {
                None
            } else {
                Some(anchor)
            });
        });
    }

    /// R56.1.f §5.22 — extend the selection to the end of the
    /// buffer. Shift+End canonical (single-line fields).
    pub fn select_end(&self) {
        let caret = self.caret_pos.get();
        let len = self.text.get().len();
        let anchor = self.selection_anchor.get().unwrap_or(caret);
        batch(|| {
            self.caret_pos.set(len);
            self.selection_anchor.set(if anchor == len {
                None
            } else {
                Some(anchor)
            });
        });
    }

    // ───────────────────────────────────────────────────────────────
    // R56.1.g §5.22 — IME preedit buffer
    // ───────────────────────────────────────────────────────────────

    /// R56.1.g §5.22 — current preedit string. `None` when no
    /// composition is active; `Some(s)` when a composition is in
    /// flight with `s` as the current preedit text. Mirror of the
    /// W3C `CompositionEvent.data` payload observed during
    /// `compositionupdate`. Subscribes to the `preedit_buffer`
    /// signal so a view-fn rendering the preedit underline re-runs
    /// on every composition-affecting mutation.
    #[must_use]
    pub fn preedit(&self) -> Option<String> {
        self.preedit_buffer.get()
    }

    /// R56.1.g §5.22 — `true` when a composition is active (preedit
    /// buffer is `Some(_)`, even if the preedit string itself is
    /// empty during the transient compositionstart-before-update
    /// window). Mirror of the W3C `KeyboardEvent.isComposing`
    /// predicate that gates IME-aware key handling.
    #[must_use]
    pub fn is_composing(&self) -> bool {
        self.preedit_buffer.get().is_some()
    }

    /// R56.2.f §5.38 §5.22 — splice the active preedit string into the
    /// committed text at `caret` to produce the "effective text" the
    /// user actually sees. Returns a 3-tuple:
    ///
    /// - `effective_text`: committed text with the preedit spliced
    ///   in at `caret` (or the committed text alone when no preedit
    ///   is active).
    /// - `visual_caret_byte`: the byte offset where the visual caret
    ///   renders. During composition this is the *end* of the
    ///   spliced preedit (W3C `compositionupdate` canonical caret
    ///   position); otherwise it equals the `caret` argument.
    /// - `preedit_byte_range`: `Some((start, end))` when composing
    ///   with a non-empty preedit, marking the splice window inside
    ///   `effective_text`. `None` otherwise. Drives the underline +
    ///   tinted-background paint geometry that signals the preedit
    ///   run to the user (W3C IME affordance — Wayland / macOS /
    ///   Windows / GTK clients all paint a similar overlay).
    ///
    /// The `caret` argument is explicit (rather than read from
    /// `self.caret()`) so view-fns can thread the state-arg's caret
    /// byte through verbatim. Pinion's R51.173 by-value snapshot
    /// contract makes the state arg's caret the authoritative
    /// "paint at this position" signal — using `self.caret()`
    /// internally would couple the helper to whichever reactive
    /// value is current *now* rather than the snapshot the view-fn
    /// was invoked with. Callers outside a view-fn pass
    /// `self.caret()` explicitly when they want the current
    /// substrate caret.
    ///
    /// Subscribes to `text` + `preedit` signals (`caret` is supplied
    /// by the caller and contributes no extra subscription). A view-
    /// fn calling this helper re-runs on every text or composition
    /// mutation, plus any caret mutation that touches the state arg
    /// (via R56.1.b's reactive `read_state` snapshot).
    ///
    /// Used by the view fn and `WidgetView::ime_caret_rect` paths in
    /// `hello-textfield` (Vello GUI) and `hello-textfield-tui` (TUI
    /// mirror) so the caret-area, glyph-run, selection-overlay, and
    /// IME candidate-popup geometries all derive from the same
    /// splice. The shared call ensures the
    /// [`pinion_text::LayoutCache`] key (`(text, style, max_width)`)
    /// is identical across the paths so the layout lookup is a
    /// cache hit, not a re-shape.
    ///
    /// The collapse path (`preedit == None || preedit == ""`) is
    /// O(1) — no allocation. The splice path allocates a fresh
    /// `String` of capacity `text.len() + p.len()` so the
    /// `push_str` runs do not re-grow. `caret` is clamped to
    /// `text.len()` defensively so a stale arg cannot panic the
    /// internal `&text[..caret]` slice.
    #[must_use]
    pub fn splice_preedit(&self, caret: usize) -> (String, usize, Option<(usize, usize)>) {
        let text = self.text();
        let preedit = self.preedit_buffer.get();
        match preedit.as_ref() {
            Some(p) if !p.is_empty() => {
                let caret_clamped = caret.min(text.len());
                let mut composed = String::with_capacity(text.len() + p.len());
                composed.push_str(&text[..caret_clamped]);
                composed.push_str(p);
                composed.push_str(&text[caret_clamped..]);
                let preedit_end = caret_clamped + p.len();
                (composed, preedit_end, Some((caret_clamped, preedit_end)))
            }
            _ => (text, caret, None),
        }
    }

    /// R56.1.g §5.22 — begin a new IME composition. Sets the preedit
    /// buffer to `Some(String::new())` (composition active, no
    /// preedit text yet — mirror of the W3C `compositionstart` event
    /// where `data` is empty until the first `compositionupdate`).
    ///
    /// If a selection is active at composition start, the selected
    /// range is drained first and the caret lands at the range start
    /// (canonical macOS / iOS / GTK / Web "compose-over-selection
    /// replaces the selection"). The 4-axis write
    /// (`text` + `caret` + `selection_anchor` + `preedit_buffer`)
    /// collapses into one notification cascade via [`batch`].
    ///
    /// No-op when a composition is already active (defensive: the
    /// platform IME contract is `compositionstart` exactly once per
    /// composition; the framework wire layer enforces the protocol,
    /// the substrate just stays idempotent).
    pub fn preedit_start(&self) {
        if self.preedit_buffer.get().is_some() {
            return;
        }
        let buf = self.text.get();
        let caret = self.caret_pos.get().min(buf.len());
        if let Some((start, end)) = self.selection_range_against(&buf, caret) {
            let mut drained = buf;
            drained.drain(start..end);
            batch(|| {
                self.text.set(drained);
                self.caret_pos.set(start);
                self.selection_anchor.set(None);
                self.preedit_buffer.set(Some(String::new()));
            });
        } else {
            self.preedit_buffer.set(Some(String::new()));
        }
    }

    /// R56.1.g §5.22 — update the active preedit string. Mirror of
    /// W3C `compositionupdate` where `data` carries the current
    /// preedit text. No-op when no composition is active (defensive:
    /// the framework wire layer enforces the protocol order
    /// compositionstart → compositionupdate*, but the substrate
    /// stays idempotent against out-of-order delivery — the AI
    /// client / RPC path could otherwise drive an `update` before
    /// `start`).
    pub fn preedit_update(&self, preedit: &str) {
        if self.preedit_buffer.get().is_none() {
            return;
        }
        self.preedit_buffer.set(Some(preedit.to_string()));
    }

    /// R56.1.g §5.22 — commit the active composition. Inserts
    /// `committed` at the caret position (advancing the caret by
    /// `committed.len()` bytes), then clears the preedit buffer.
    /// Mirror of W3C `compositionend` where `data` carries the final
    /// committed string. The three writes
    /// (`text` + `caret` + `preedit_buffer`) collapse into one
    /// notification cascade via [`batch`].
    ///
    /// Empty `committed` is the cancel-shaped commit (composition
    /// ended with no text — e.g. Escape during composition discards
    /// the preedit without inserting anything); the preedit buffer
    /// clears but the text + caret stay untouched.
    ///
    /// No-op when no composition is active (defensive against
    /// out-of-order delivery). The caret advances by **bytes**, not
    /// chars / graphemes — the post-commit caret offset is always a
    /// `char` boundary because `committed` is a valid UTF-8 `&str`
    /// (Rust invariant) and the insertion happens at a pre-existing
    /// boundary.
    pub fn preedit_commit(&self, committed: &str) {
        if self.preedit_buffer.get().is_none() {
            return;
        }
        if committed.is_empty() {
            self.preedit_buffer.set(None);
            return;
        }
        let mut buf = self.text.get();
        let caret = self.caret_pos.get().min(buf.len());
        let snapped = clamp_to_char_boundary(&buf, caret);
        buf.insert_str(snapped, committed);
        let new_caret = snapped + committed.len();
        batch(|| {
            self.text.set(buf);
            self.caret_pos.set(new_caret);
            self.preedit_buffer.set(None);
        });
    }

    /// R56.1.g §5.22 — cancel the active composition. Clears the
    /// preedit buffer without inserting anything. Mirror of the IME
    /// cancel path (Escape during composition, or the platform
    /// `compositionend` with empty `data` after a cancel).
    /// No-op when no composition is active.
    pub fn preedit_cancel(&self) {
        if self.preedit_buffer.get().is_none() {
            return;
        }
        self.preedit_buffer.set(None);
    }

    // ───────────────────────────────────────────────────────────────
    // Internal selection helpers
    // ───────────────────────────────────────────────────────────────

    /// R56.1.f §5.22 — `selection_range` against an already-fetched
    /// text + caret. Used by mutators that have already pulled the
    /// text buffer through `self.text.get()` (cloning the buffer is
    /// non-trivial; reusing the fetched copy avoids a second clone
    /// on the selection branch). The clamping logic stays consistent
    /// with [`Self::selection_range`].
    fn selection_range_against(&self, text: &str, caret: usize) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor.get()?;
        let snapped_anchor = clamp_to_char_boundary(text, anchor.min(text.len()));
        if snapped_anchor == caret {
            return None;
        }
        Some((snapped_anchor.min(caret), snapped_anchor.max(caret)))
    }
}

impl Default for TextEditState {
    fn default() -> Self {
        Self::new()
    }
}

/// R56.1.b §5.22 — Resolve (or lazily initialize) the
/// [`TextEditState`] for the current view scope.
///
/// Delegates to [`Owner::cache`](crate::reactive::Owner::cache); the
/// `key` MUST be a `&'static str`. The canonical pattern is to pass
/// the matching [`TextField`]'s tag verbatim — (R56.1.b.1 §5.22) the
/// underlying `Owner::cache` is keyed by `(TypeId, &'static str)`, so
/// the same widget tag composes cleanly across typed hooks:
/// `use_text_edit_state(tag)` and
/// [`use_caret_blink`](crate::widgets::caret_blink::use_caret_blink)`(tag)`
/// resolve to distinct slots without collision. Mirrors
/// [`use_scroll_state`](crate::widgets::scroll::use_scroll_state).
///
/// # Panics
///
/// Panics if no current [`Owner`] is set — i.e. when invoked outside
/// a `root_owner.run(...)` wrap. Per the callback-root-owner-wrap
/// discipline (R51.146 / R51.152 / R51.171), framework-internal
/// dispatch sites supply this wrap; application code reaches
/// `use_text_edit_state` only from within `V::view` / `V::update` /
/// `V::apply_key` / similar hooks.
///
/// Panics if the cache key was previously bound to a value of a
/// different concrete type within the same owner — see
/// [`Owner::cache`](crate::reactive::Owner::cache) for the
/// underlying contract.
///
/// [`TextField`]: crate::widgets::text_field::TextField
#[must_use]
pub fn use_text_edit_state(key: &'static str) -> Rc<TextEditState> {
    Owner::current()
        .expect("use_text_edit_state requires an active Owner scope")
        .cache(key, || TextEditState::with_tag(key))
}

// ─────────────────────────────────────────────────────────────────
// Internal helpers — UTF-8 boundary navigation
// ─────────────────────────────────────────────────────────────────

/// Largest `char`-boundary offset that is `<= pos`. Used by every
/// `set_*` / `insert` / `move_*` path so the caret never lands in
/// the middle of a UTF-8 multi-byte sequence. Mirrors the standard
/// `str::ceil_char_boundary` / `floor_char_boundary` semantics
/// (stable-Rust does not expose them yet, so the implementation is
/// inline).
fn clamp_to_char_boundary(text: &str, pos: usize) -> usize {
    let mut i = pos.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Byte offset of the `char` boundary strictly preceding `pos`. The
/// caller must guarantee `pos > 0`; passing `pos == 0` returns `0`
/// (defensive guard against caret-at-start callers, even though
/// every public mutator above checks that condition itself).
fn prev_char_boundary(text: &str, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut i = pos - 1;
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Byte offset of the `char` boundary strictly following `pos`. The
/// caller must guarantee `pos < text.len()`; passing `pos >= len`
/// returns `len` (defensive guard).
fn next_char_boundary(text: &str, pos: usize) -> usize {
    let len = text.len();
    if pos >= len {
        return len;
    }
    let mut i = pos + 1;
    while i < len && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    //! R56.1.b §5.38 §5.22 — `TextEditState` regression battery.
    //! Covers ASCII edits, multi-byte UTF-8 caret navigation, atomic
    //! batched-multi-axis subscriber semantics, and the `Owner::cache`
    //! hook integration.

    use super::{
        clamp_to_char_boundary, next_char_boundary, prev_char_boundary,
        use_text_edit_state, TextEditState,
    };
    use crate::reactive::{Effect, Owner};
    use std::cell::Cell;
    use std::rc::Rc;

    // ─────────────────────────────────────────────────────────────
    // Initial state + construction
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_initial_state_is_empty_caret_zero() {
        let s = TextEditState::new();
        assert_eq!(s.text(), "");
        assert_eq!(s.caret(), 0);
        assert_eq!(s.tag(), None);
    }

    #[test]
    fn r56_1_b_with_tag_records_key() {
        let s = TextEditState::with_tag("primary_field");
        assert_eq!(s.tag(), Some("primary_field"));
        assert_eq!(s.text(), "");
        assert_eq!(s.caret(), 0);
    }

    #[test]
    fn r56_1_b_with_initial_places_caret_at_end() {
        let s = TextEditState::with_initial("hello".to_string());
        assert_eq!(s.text(), "hello");
        assert_eq!(s.caret(), 5);
    }

    #[test]
    fn r56_1_b_snapshot_returns_both_axes() {
        let s = TextEditState::with_initial("ab".to_string());
        assert_eq!(s.snapshot(), ("ab".to_string(), 2));
    }

    // ─────────────────────────────────────────────────────────────
    // ASCII edit operations
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_insert_at_end_extends_text_and_advances_caret() {
        let s = TextEditState::with_initial("ab".to_string());
        s.insert("cd");
        assert_eq!(s.text(), "abcd");
        assert_eq!(s.caret(), 4);
    }

    #[test]
    fn r56_1_b_insert_at_start_shifts_existing_text() {
        let s = TextEditState::with_initial("cd".to_string());
        s.set_caret(0);
        s.insert("ab");
        assert_eq!(s.text(), "abcd");
        assert_eq!(s.caret(), 2);
    }

    #[test]
    fn r56_1_b_insert_in_middle_splices() {
        let s = TextEditState::with_initial("ad".to_string());
        s.set_caret(1);
        s.insert("bc");
        assert_eq!(s.text(), "abcd");
        assert_eq!(s.caret(), 3);
    }

    #[test]
    fn r56_1_b_insert_empty_is_noop() {
        let s = TextEditState::with_initial("ab".to_string());
        s.insert("");
        assert_eq!(s.text(), "ab");
        assert_eq!(s.caret(), 2);
    }

    #[test]
    fn r56_1_b_backspace_from_end_drops_last_char_and_retreats_caret() {
        let s = TextEditState::with_initial("abc".to_string());
        s.backspace();
        assert_eq!(s.text(), "ab");
        assert_eq!(s.caret(), 2);
    }

    #[test]
    fn r56_1_b_backspace_in_middle_drops_preceding_char() {
        let s = TextEditState::with_initial("abc".to_string());
        s.set_caret(2);
        s.backspace();
        assert_eq!(s.text(), "ac");
        assert_eq!(s.caret(), 1);
    }

    #[test]
    fn r56_1_b_backspace_at_start_is_noop() {
        let s = TextEditState::with_initial("abc".to_string());
        s.set_caret(0);
        s.backspace();
        assert_eq!(s.text(), "abc");
        assert_eq!(s.caret(), 0);
    }

    #[test]
    fn r56_1_b_delete_forward_drops_following_char_caret_stays() {
        let s = TextEditState::with_initial("abc".to_string());
        s.set_caret(1);
        s.delete_forward();
        assert_eq!(s.text(), "ac");
        assert_eq!(s.caret(), 1);
    }

    #[test]
    fn r56_1_b_delete_forward_at_end_is_noop() {
        let s = TextEditState::with_initial("abc".to_string());
        s.delete_forward();
        assert_eq!(s.text(), "abc");
        assert_eq!(s.caret(), 3);
    }

    // ─────────────────────────────────────────────────────────────
    // Caret movement
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_move_left_decrements_caret() {
        let s = TextEditState::with_initial("abc".to_string());
        s.move_left();
        assert_eq!(s.caret(), 2);
    }

    #[test]
    fn r56_1_b_move_left_at_zero_is_noop() {
        let s = TextEditState::with_initial("abc".to_string());
        s.set_caret(0);
        s.move_left();
        assert_eq!(s.caret(), 0);
    }

    #[test]
    fn r56_1_b_move_right_increments_caret() {
        let s = TextEditState::with_initial("abc".to_string());
        s.set_caret(0);
        s.move_right();
        assert_eq!(s.caret(), 1);
    }

    #[test]
    fn r56_1_b_move_right_at_end_is_noop() {
        let s = TextEditState::with_initial("abc".to_string());
        s.move_right();
        assert_eq!(s.caret(), 3);
    }

    #[test]
    fn r56_1_b_move_home_and_end() {
        let s = TextEditState::with_initial("abc".to_string());
        s.set_caret(1);
        s.move_home();
        assert_eq!(s.caret(), 0);
        s.move_end();
        assert_eq!(s.caret(), 3);
    }

    // ─────────────────────────────────────────────────────────────
    // Multi-byte UTF-8 — caret stays on char boundaries
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_korean_text_caret_lands_on_char_boundary() {
        // "한글" — two Korean syllables, each 3 bytes in UTF-8.
        // Total len() = 6 bytes; valid boundaries are 0, 3, 6.
        let s = TextEditState::with_initial("한글".to_string());
        assert_eq!(s.text().len(), 6);
        assert_eq!(s.caret(), 6);
        s.move_left();
        assert_eq!(s.caret(), 3, "move_left must skip mid-codepoint bytes");
        s.move_left();
        assert_eq!(s.caret(), 0);
        s.move_right();
        assert_eq!(s.caret(), 3);
    }

    #[test]
    fn r56_1_b_set_caret_clamps_mid_codepoint_to_preceding_boundary() {
        // "한" (U+D55C, 3 bytes). Valid boundaries: 0, 3.
        // set_caret(1) lands mid-codepoint — must snap to 0.
        let s = TextEditState::with_initial("한".to_string());
        s.set_caret(1);
        assert_eq!(s.caret(), 0, "mid-codepoint offset must snap down");
        s.set_caret(2);
        assert_eq!(s.caret(), 0, "still mid-codepoint");
        s.set_caret(3);
        assert_eq!(s.caret(), 3, "valid boundary, unchanged");
    }

    #[test]
    fn r56_1_b_backspace_removes_entire_korean_syllable() {
        // 3-byte char must be removed wholesale, never byte-by-byte.
        let s = TextEditState::with_initial("a한b".to_string());
        // len = 1 + 3 + 1 = 5; caret at end.
        assert_eq!(s.caret(), 5);
        s.backspace();
        assert_eq!(s.text(), "a한", "drop trailing 'b'");
        assert_eq!(s.caret(), 4);
        s.backspace();
        assert_eq!(s.text(), "a", "drop the 한 syllable in one backspace");
        assert_eq!(s.caret(), 1);
    }

    #[test]
    fn r56_1_b_delete_forward_removes_entire_korean_syllable() {
        let s = TextEditState::with_initial("a한b".to_string());
        s.set_caret(1);
        s.delete_forward();
        assert_eq!(s.text(), "ab", "drop the 한 syllable forward");
        assert_eq!(s.caret(), 1, "caret stays");
    }

    #[test]
    fn r56_1_b_insert_preserves_utf8_invariant() {
        let s = TextEditState::with_initial("a".to_string());
        s.insert("한글");
        assert_eq!(s.text(), "a한글");
        assert_eq!(s.caret(), 1 + 6);
        // Round-trip — the buffer is still valid UTF-8.
        let _: &str = s.text().as_str();
    }

    // ─────────────────────────────────────────────────────────────
    // set_text — clamp caret on shrink
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_set_text_clamps_caret_to_new_length() {
        let s = TextEditState::with_initial("abcdef".to_string());
        // caret = 6; set shorter text, caret must clamp into bounds.
        s.set_text("xy".to_string());
        assert_eq!(s.text(), "xy");
        assert_eq!(s.caret(), 2);
    }

    #[test]
    fn r56_1_b_set_text_to_empty_collapses_caret_to_zero() {
        let s = TextEditState::with_initial("abc".to_string());
        s.set_text(String::new());
        assert_eq!(s.text(), "");
        assert_eq!(s.caret(), 0);
    }

    #[test]
    fn r56_1_b_set_text_longer_preserves_caret() {
        let s = TextEditState::with_initial("ab".to_string());
        s.set_caret(1);
        s.set_text("hello world".to_string());
        assert_eq!(s.caret(), 1, "caret stays at byte 1 inside longer text");
    }

    // ─────────────────────────────────────────────────────────────
    // Atomic batched-multi-axis subscriber semantics
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_insert_fires_subscriber_exactly_once() {
        // R55.G.24 [[signal-batch-atomic-multi-axis-update]] mirror —
        // an `insert` touches text + caret; a single `Effect` must
        // see exactly one fire, not two.
        let owner = Owner::new();
        let s = Rc::new(TextEditState::with_initial("a".to_string()));
        let count = Rc::new(Cell::new(0));
        let s_ref = Rc::clone(&s);
        let count_ref = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            // Subscribe to both axes; if either fires separately the
            // count goes to 2 per write.
            let _ = s_ref.snapshot();
            count_ref.set(count_ref.get() + 1);
        });
        // Initial fire from Effect::new.
        assert_eq!(count.get(), 1);
        s.insert("b");
        assert_eq!(
            count.get(),
            2,
            "insert must batch text+caret into one Effect re-run",
        );
    }

    #[test]
    fn r56_1_b_backspace_fires_subscriber_exactly_once() {
        let owner = Owner::new();
        let s = Rc::new(TextEditState::with_initial("ab".to_string()));
        let count = Rc::new(Cell::new(0));
        let s_ref = Rc::clone(&s);
        let count_ref = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            let _ = s_ref.snapshot();
            count_ref.set(count_ref.get() + 1);
        });
        assert_eq!(count.get(), 1);
        s.backspace();
        assert_eq!(count.get(), 2, "backspace must batch text+caret");
    }

    #[test]
    fn r56_1_b_set_text_fires_subscriber_exactly_once() {
        let owner = Owner::new();
        let s = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        let count = Rc::new(Cell::new(0));
        let s_ref = Rc::clone(&s);
        let count_ref = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            let _ = s_ref.snapshot();
            count_ref.set(count_ref.get() + 1);
        });
        assert_eq!(count.get(), 1);
        s.set_text("xy".to_string());
        assert_eq!(count.get(), 2, "set_text must batch text+caret clamp");
    }

    #[test]
    fn r56_1_b_caret_only_move_fires_subscriber_exactly_once() {
        // Single-axis writes (move_left / set_caret) are already
        // atomic — verify they fire exactly once, no double-fire.
        let owner = Owner::new();
        let s = Rc::new(TextEditState::with_initial("abc".to_string()));
        let count = Rc::new(Cell::new(0));
        let s_ref = Rc::clone(&s);
        let count_ref = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            let _ = s_ref.caret();
            count_ref.set(count_ref.get() + 1);
        });
        assert_eq!(count.get(), 1);
        s.move_left();
        assert_eq!(count.get(), 2);
    }

    // ─────────────────────────────────────────────────────────────
    // use_text_edit_state hook — Owner::cache integration
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_use_hook_returns_same_state_across_runs() {
        let owner = Owner::new();
        let (a, b) = owner.run(|| {
            let a = use_text_edit_state("k1");
            let b = use_text_edit_state("k1");
            (a, b)
        });
        assert!(Rc::ptr_eq(&a, &b), "same key resolves to same Rc");
    }

    #[test]
    fn r56_1_b_use_hook_distinct_keys_distinct_states() {
        let owner = Owner::new();
        let (a, b) = owner.run(|| {
            let a = use_text_edit_state("k1");
            let b = use_text_edit_state("k2");
            (a, b)
        });
        assert!(!Rc::ptr_eq(&a, &b), "distinct keys resolve to distinct Rc");
    }

    #[test]
    fn r56_1_b_use_hook_records_tag() {
        let owner = Owner::new();
        let s = owner.run(|| use_text_edit_state("primary_field"));
        assert_eq!(s.tag(), Some("primary_field"));
    }

    #[test]
    #[should_panic(expected = "use_text_edit_state requires an active Owner scope")]
    fn r56_1_b_use_hook_panics_outside_owner_scope() {
        // Defensive: without an active Owner, the hook cannot
        // anchor its cache slot. The panic message points the
        // caller at the canonical fix (wrap in `Owner::run` or rely
        // on a framework dispatch site that already wraps).
        let _ = use_text_edit_state("k");
    }

    // ─────────────────────────────────────────────────────────────
    // Internal char-boundary helpers
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_clamp_to_char_boundary_handles_ascii() {
        assert_eq!(clamp_to_char_boundary("abc", 0), 0);
        assert_eq!(clamp_to_char_boundary("abc", 1), 1);
        assert_eq!(clamp_to_char_boundary("abc", 3), 3);
        assert_eq!(clamp_to_char_boundary("abc", 99), 3, "clamps to len");
    }

    #[test]
    fn r56_1_b_clamp_to_char_boundary_snaps_mid_codepoint() {
        let s = "한"; // 3 bytes; boundaries {0, 3}
        assert_eq!(clamp_to_char_boundary(s, 0), 0);
        assert_eq!(clamp_to_char_boundary(s, 1), 0, "mid → 0");
        assert_eq!(clamp_to_char_boundary(s, 2), 0, "mid → 0");
        assert_eq!(clamp_to_char_boundary(s, 3), 3);
    }

    // ─────────────────────────────────────────────────────────────
    // R56.1.f §5.22 — Selection sidecar
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_f_initial_selection_anchor_is_none() {
        let s = TextEditState::new();
        assert_eq!(s.selection_anchor(), None);
        assert!(!s.has_selection());
        assert_eq!(s.selection_range(), None);
    }

    #[test]
    fn r56_1_f_set_selection_stores_anchor_and_focus() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(2, 5);
        assert_eq!(s.selection_anchor(), Some(2));
        assert_eq!(s.caret(), 5);
        assert!(s.has_selection());
        assert_eq!(s.selection_range(), Some((2, 5)));
    }

    #[test]
    fn r56_1_f_set_selection_normalises_range_when_focus_before_anchor() {
        // anchor=5, focus=2 → range (2, 5); selection_range always
        // returns (min, max).
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(5, 2);
        assert_eq!(s.selection_anchor(), Some(5));
        assert_eq!(s.caret(), 2);
        assert_eq!(s.selection_range(), Some((2, 5)));
    }

    #[test]
    fn r56_1_f_set_selection_collapses_to_none_when_anchor_equals_focus() {
        // The W3C canonical "collapsed selection" state surfaces as
        // anchor = None here so has_selection is a pure predicate.
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(3, 3);
        assert_eq!(s.selection_anchor(), None);
        assert_eq!(s.caret(), 3);
        assert!(!s.has_selection());
    }

    #[test]
    fn r56_1_f_set_selection_clamps_to_text_len() {
        let s = TextEditState::with_initial("abc".to_string());
        s.set_selection(0, 99);
        assert_eq!(s.selection_range(), Some((0, 3)));
    }

    #[test]
    fn r56_1_f_clear_selection_drops_anchor_but_keeps_caret() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(1, 4);
        s.clear_selection();
        assert_eq!(s.selection_anchor(), None);
        assert_eq!(s.caret(), 4, "clear_selection must not move caret");
    }

    #[test]
    fn r56_1_f_set_caret_clears_active_selection() {
        // Explicit caret-set is a "click-to-reposition" — drops the
        // selection per W3C selectionchange canonical.
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(1, 4);
        s.set_caret(2);
        assert_eq!(s.caret(), 2);
        assert_eq!(s.selection_anchor(), None);
    }

    #[test]
    fn r56_1_f_set_text_clears_active_selection() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(0, 3);
        s.set_text("xy".to_string());
        assert_eq!(s.text(), "xy");
        assert_eq!(s.selection_anchor(), None);
    }

    // ─────────────────────────────────────────────────────────────
    // Selection-extending caret motion (Shift+Arrow / Shift+Home/End)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_f_select_left_seeds_anchor_from_caret_then_moves() {
        // From caret=3, no selection: select_left puts anchor at 3,
        // moves caret to 2; range = (2, 3).
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_caret(3);
        s.select_left();
        assert_eq!(s.caret(), 2);
        assert_eq!(s.selection_anchor(), Some(3));
        assert_eq!(s.selection_range(), Some((2, 3)));
    }

    #[test]
    fn r56_1_f_select_right_seeds_anchor_from_caret_then_moves() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_caret(2);
        s.select_right();
        assert_eq!(s.caret(), 3);
        assert_eq!(s.selection_anchor(), Some(2));
        assert_eq!(s.selection_range(), Some((2, 3)));
    }

    #[test]
    fn r56_1_f_select_left_preserves_existing_anchor() {
        // Selection already (2, 5); extending left moves the *caret*
        // (the focus) but leaves the anchor at 2.
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(2, 5);
        s.select_left();
        assert_eq!(s.caret(), 4);
        assert_eq!(s.selection_anchor(), Some(2));
    }

    #[test]
    fn r56_1_f_select_right_can_collapse_back_to_anchor() {
        // Anchor=2, caret=3 → range (2,3). select_left brings caret
        // to 2; caret == anchor so the selection collapses to None.
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(2, 3);
        s.select_left();
        assert_eq!(s.caret(), 2);
        assert_eq!(s.selection_anchor(), None);
        assert!(!s.has_selection());
    }

    #[test]
    fn r56_1_f_select_home_extends_to_zero() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_caret(4);
        s.select_home();
        assert_eq!(s.caret(), 0);
        assert_eq!(s.selection_anchor(), Some(4));
        assert_eq!(s.selection_range(), Some((0, 4)));
    }

    #[test]
    fn r56_1_f_select_end_extends_to_text_len() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_caret(2);
        s.select_end();
        assert_eq!(s.caret(), 6);
        assert_eq!(s.selection_anchor(), Some(2));
        assert_eq!(s.selection_range(), Some((2, 6)));
    }

    #[test]
    fn r56_1_f_select_left_at_caret_zero_is_noop() {
        let s = TextEditState::with_initial("abc".to_string());
        s.set_caret(0);
        s.select_left();
        assert_eq!(s.caret(), 0);
        assert_eq!(s.selection_anchor(), None);
    }

    #[test]
    fn r56_1_f_select_right_at_caret_end_is_noop() {
        let s = TextEditState::with_initial("abc".to_string());
        s.set_caret(3);
        s.select_right();
        assert_eq!(s.caret(), 3);
        assert_eq!(s.selection_anchor(), None);
    }

    // ─────────────────────────────────────────────────────────────
    // Plain caret motion collapses an active selection
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_f_move_left_collapses_selection_to_start() {
        // W3C "ArrowLeft on a selection collapses to the leading
        // edge" — not "one char to the left of the caret".
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(1, 4);
        s.move_left();
        assert_eq!(s.caret(), 1, "lands at selection start");
        assert_eq!(s.selection_anchor(), None);
    }

    #[test]
    fn r56_1_f_move_right_collapses_selection_to_end() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(1, 4);
        s.move_right();
        assert_eq!(s.caret(), 4, "lands at selection end");
        assert_eq!(s.selection_anchor(), None);
    }

    #[test]
    fn r56_1_f_move_home_clears_selection() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(2, 5);
        s.move_home();
        assert_eq!(s.caret(), 0);
        assert_eq!(s.selection_anchor(), None);
    }

    #[test]
    fn r56_1_f_move_end_clears_selection() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(0, 3);
        s.move_end();
        assert_eq!(s.caret(), 6);
        assert_eq!(s.selection_anchor(), None);
    }

    // ─────────────────────────────────────────────────────────────
    // Selection-aware insert / backspace / delete_forward
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_f_insert_with_selection_replaces_range() {
        // Type-to-replace canonical: selection (1, 4) replaced by
        // "XY" → "aXYef", caret at end of inserted text (3 = 1+2).
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(1, 4);
        s.insert("XY");
        assert_eq!(s.text(), "aXYef");
        assert_eq!(s.caret(), 3);
        assert_eq!(s.selection_anchor(), None);
    }

    #[test]
    fn r56_1_f_insert_with_selection_empty_string_is_a_delete() {
        // R56.1.b documented insert("") as a no-op; that still holds
        // (the early-return short-circuits before the selection
        // branch fires). Explicit deletion uses backspace / delete.
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(1, 4);
        s.insert("");
        assert_eq!(s.text(), "abcdef", "insert(\"\") stays a no-op");
        assert_eq!(s.selection_anchor(), Some(1), "selection unchanged");
    }

    #[test]
    fn r56_1_f_backspace_with_selection_drains_range() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(1, 4);
        s.backspace();
        assert_eq!(s.text(), "aef");
        assert_eq!(s.caret(), 1);
        assert_eq!(s.selection_anchor(), None);
    }

    #[test]
    fn r56_1_f_delete_forward_with_selection_drains_range() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(1, 4);
        s.delete_forward();
        assert_eq!(s.text(), "aef");
        assert_eq!(s.caret(), 1);
        assert_eq!(s.selection_anchor(), None);
    }

    #[test]
    fn r56_1_f_insert_at_selection_focus_before_anchor_still_replaces_range() {
        // Selection range is normalised (min, max) so an
        // anchor-after-focus selection (e.g. mouse-drag-back-to-left)
        // still drains the same byte range.
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(4, 1); // anchor=4, focus=1, range=(1,4)
        s.insert("Z");
        assert_eq!(s.text(), "aZef");
        assert_eq!(s.caret(), 2);
        assert_eq!(s.selection_anchor(), None);
    }

    // ─────────────────────────────────────────────────────────────
    // Multi-byte UTF-8 selection
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_f_select_right_lands_on_char_boundary_korean() {
        // "한글" — two 3-byte CJK syllables; valid boundaries {0,3,6}.
        let s = TextEditState::with_initial("한글".to_string());
        s.set_caret(0);
        s.select_right();
        assert_eq!(s.caret(), 3, "skip mid-codepoint bytes");
        assert_eq!(s.selection_range(), Some((0, 3)));
    }

    #[test]
    fn r56_1_f_select_left_lands_on_char_boundary_korean() {
        let s = TextEditState::with_initial("한글".to_string());
        s.set_caret(6);
        s.select_left();
        assert_eq!(s.caret(), 3);
        assert_eq!(s.selection_range(), Some((3, 6)));
    }

    #[test]
    fn r56_1_f_backspace_drains_korean_selection_wholesale() {
        let s = TextEditState::with_initial("a한b".to_string());
        s.set_selection(1, 4); // range covers the entire 한 syllable
        s.backspace();
        assert_eq!(s.text(), "ab");
        assert_eq!(s.caret(), 1);
    }

    // ─────────────────────────────────────────────────────────────
    // R56.1.e §5.22 — selection_text accessor (Clipboard hook)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_e_selection_text_returns_none_for_collapsed_caret() {
        let s = TextEditState::with_initial("abcdef".to_string());
        assert_eq!(s.selection_text(), None);
    }

    #[test]
    fn r56_1_e_selection_text_returns_substring_for_active_selection() {
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(1, 4);
        assert_eq!(s.selection_text(), Some("bcd".to_string()));
    }

    #[test]
    fn r56_1_e_selection_text_normalises_reverse_selection() {
        // anchor=4, focus=1 → range (1,4) → same substring.
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(4, 1);
        assert_eq!(s.selection_text(), Some("bcd".to_string()));
    }

    #[test]
    fn r56_1_e_selection_text_korean_multi_byte() {
        // "한글" — 6 bytes; full selection round-trips both syllables.
        let s = TextEditState::with_initial("한글".to_string());
        s.set_selection(0, 6);
        assert_eq!(s.selection_text(), Some("한글".to_string()));
    }

    #[test]
    fn r56_1_e_selection_text_korean_partial() {
        let s = TextEditState::with_initial("a한b".to_string());
        // selection covers just the 한 syllable (bytes 1..4).
        s.set_selection(1, 4);
        assert_eq!(s.selection_text(), Some("한".to_string()));
    }

    // ─────────────────────────────────────────────────────────────
    // 3-axis atomic batched-multi-axis subscriber semantics
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_f_set_selection_fires_subscriber_exactly_once() {
        let owner = Owner::new();
        let s = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        let count = Rc::new(Cell::new(0));
        let s_ref = Rc::clone(&s);
        let count_ref = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            let _ = s_ref.selection_range();
            count_ref.set(count_ref.get() + 1);
        });
        // Initial fire.
        assert_eq!(count.get(), 1);
        s.set_selection(1, 4);
        assert_eq!(
            count.get(),
            2,
            "set_selection must batch caret + anchor into one re-run",
        );
    }

    #[test]
    fn r56_1_f_insert_with_selection_fires_subscriber_exactly_once() {
        // Three-axis batched write (text + caret + selection_anchor)
        // collapses to a single Effect re-run.
        let owner = Owner::new();
        let s = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        let count = Rc::new(Cell::new(0));
        let s_ref = Rc::clone(&s);
        let count_ref = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            let _ = s_ref.snapshot();
            let _ = s_ref.selection_range();
            count_ref.set(count_ref.get() + 1);
        });
        assert_eq!(count.get(), 1);
        s.set_selection(1, 4);
        assert_eq!(count.get(), 2, "set_selection: one re-run");
        s.insert("Z");
        assert_eq!(
            count.get(),
            3,
            "insert with selection batches text+caret+anchor into one re-run",
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R56.1.g §5.22 — IME preedit buffer
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_initial_preedit_is_none_not_composing() {
        let s = TextEditState::new();
        assert_eq!(s.preedit(), None);
        assert!(!s.is_composing());
    }

    #[test]
    fn r56_1_g_preedit_start_sets_buffer_to_empty_some() {
        // W3C compositionstart canonical: data is empty until the
        // first compositionupdate. Substrate mirrors that exactly.
        let s = TextEditState::with_initial("ab".to_string());
        s.preedit_start();
        assert_eq!(s.preedit(), Some(String::new()));
        assert!(s.is_composing());
    }

    #[test]
    fn r56_1_g_preedit_start_is_idempotent_when_already_composing() {
        // Substrate stays defensive against duplicate compositionstart
        // (the platform contract is once-per-composition, but the AI
        // client / RPC path could drive it twice — the second call
        // must not stomp an already-buffered preedit string).
        let s = TextEditState::with_initial("ab".to_string());
        s.preedit_start();
        s.preedit_update("hi");
        s.preedit_start();
        assert_eq!(
            s.preedit(),
            Some("hi".to_string()),
            "second preedit_start must not wipe the buffer",
        );
    }

    #[test]
    fn r56_1_g_preedit_start_with_active_selection_drains_range() {
        // Compose-over-selection: the selected text is removed first,
        // composition begins at the selection start. Canonical macOS /
        // iOS / GTK / Web "compose replaces selection" behaviour.
        let s = TextEditState::with_initial("abcdef".to_string());
        s.set_selection(1, 4);
        s.preedit_start();
        assert_eq!(s.text(), "aef", "selection drained on compose-start");
        assert_eq!(s.caret(), 1, "caret at drained-selection start");
        assert_eq!(s.selection_anchor(), None, "selection cleared");
        assert_eq!(s.preedit(), Some(String::new()), "composition began");
    }

    #[test]
    fn r56_1_g_preedit_update_sets_buffer_to_provided_text() {
        let s = TextEditState::with_initial("ab".to_string());
        s.preedit_start();
        s.preedit_update("hello");
        assert_eq!(s.preedit(), Some("hello".to_string()));
    }

    #[test]
    fn r56_1_g_preedit_update_no_op_when_not_composing() {
        // Defensive against out-of-order delivery: an update without
        // a prior start is a no-op.
        let s = TextEditState::with_initial("ab".to_string());
        s.preedit_update("hello");
        assert_eq!(s.preedit(), None, "update without start stays None");
    }

    #[test]
    fn r56_1_g_preedit_update_can_replace_existing_preedit_string() {
        // Successive compositionupdates send the *current full* preedit
        // (not deltas) — the buffer replaces, not appends.
        let s = TextEditState::with_initial("ab".to_string());
        s.preedit_start();
        s.preedit_update("h");
        s.preedit_update("hi");
        s.preedit_update("hi!");
        assert_eq!(s.preedit(), Some("hi!".to_string()));
    }

    #[test]
    fn r56_1_g_preedit_commit_inserts_at_caret_and_clears_buffer() {
        let s = TextEditState::with_initial("ab".to_string());
        s.set_caret(1);
        s.preedit_start();
        s.preedit_update("xyz");
        s.preedit_commit("XYZ");
        assert_eq!(s.text(), "aXYZb");
        assert_eq!(s.caret(), 1 + 3, "caret advanced by committed bytes");
        assert_eq!(s.preedit(), None, "preedit cleared on commit");
        assert!(!s.is_composing());
    }

    #[test]
    fn r56_1_g_preedit_commit_with_empty_string_clears_without_insert() {
        // compositionend with empty `data` (cancel-shape) clears the
        // preedit but leaves the text untouched.
        let s = TextEditState::with_initial("ab".to_string());
        s.set_caret(1);
        s.preedit_start();
        s.preedit_update("xyz");
        s.preedit_commit("");
        assert_eq!(s.text(), "ab", "text unchanged on empty commit");
        assert_eq!(s.caret(), 1, "caret unchanged on empty commit");
        assert_eq!(s.preedit(), None, "preedit cleared");
    }

    #[test]
    fn r56_1_g_preedit_commit_no_op_when_not_composing() {
        // Defensive against out-of-order: a commit without prior
        // start is a no-op (text untouched).
        let s = TextEditState::with_initial("ab".to_string());
        s.preedit_commit("hi");
        assert_eq!(s.text(), "ab", "no-composition commit is silent");
    }

    #[test]
    fn r56_1_g_preedit_cancel_clears_buffer_without_insert() {
        let s = TextEditState::with_initial("ab".to_string());
        s.preedit_start();
        s.preedit_update("xyz");
        s.preedit_cancel();
        assert_eq!(s.text(), "ab", "text untouched on cancel");
        assert_eq!(s.preedit(), None, "preedit cleared");
        assert!(!s.is_composing());
    }

    #[test]
    fn r56_1_g_preedit_cancel_no_op_when_not_composing() {
        let s = TextEditState::with_initial("ab".to_string());
        s.preedit_cancel();
        assert_eq!(s.preedit(), None);
        assert!(!s.is_composing());
    }

    #[test]
    fn r56_1_g_set_text_clears_active_preedit() {
        // An explicit set_text invalidates the composition (same way
        // it invalidates the selection — the underlying text changed
        // wholesale, the preedit byte offset references no longer
        // make sense).
        let s = TextEditState::with_initial("abcdef".to_string());
        s.preedit_start();
        s.preedit_update("xyz");
        s.set_text("hello".to_string());
        assert_eq!(s.preedit(), None, "preedit cleared by set_text");
        assert_eq!(s.text(), "hello");
    }

    #[test]
    fn r56_1_g_preedit_commit_korean_multi_byte() {
        // Korean composition canonical: 'ㅎ' + 'ㅏ' + 'ㄴ' jamo →
        // syllable "한" (3 bytes UTF-8). The substrate doesn't know
        // about jamo composition — it just inserts the final
        // committed string verbatim.
        let s = TextEditState::with_initial(String::new());
        s.preedit_start();
        s.preedit_update("ㅎ");
        s.preedit_update("하");
        s.preedit_commit("한");
        assert_eq!(s.text(), "한");
        assert_eq!(s.caret(), 3, "caret advanced by 3 bytes");
        let _: &str = s.text().as_str();
    }

    #[test]
    fn r56_1_g_preedit_commit_into_middle_of_text() {
        // Compose at caret position inside existing text.
        let s = TextEditState::with_initial("ad".to_string());
        s.set_caret(1);
        s.preedit_start();
        s.preedit_update("bc");
        s.preedit_commit("bc");
        assert_eq!(s.text(), "abcd");
        assert_eq!(s.caret(), 3);
    }

    // ─────────────────────────────────────────────────────────────
    // R56.1.g §5.22 — atomic batched-multi-axis subscriber semantics
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_g_preedit_start_with_selection_fires_subscriber_exactly_once() {
        // 4-axis batched write (text + caret + selection_anchor +
        // preedit_buffer) collapses to a single Effect re-run.
        let owner = Owner::new();
        let s = Rc::new(TextEditState::with_initial("abcdef".to_string()));
        let count = Rc::new(Cell::new(0));
        let s_ref = Rc::clone(&s);
        let count_ref = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            let _ = s_ref.snapshot();
            let _ = s_ref.selection_range();
            let _ = s_ref.preedit();
            count_ref.set(count_ref.get() + 1);
        });
        assert_eq!(count.get(), 1);
        s.set_selection(1, 4);
        assert_eq!(count.get(), 2);
        s.preedit_start();
        assert_eq!(
            count.get(),
            3,
            "preedit_start with selection batches 4 axes into one re-run",
        );
    }

    #[test]
    fn r56_1_g_preedit_commit_fires_subscriber_exactly_once() {
        // 3-axis batched write (text + caret + preedit_buffer).
        let owner = Owner::new();
        let s = Rc::new(TextEditState::with_initial("ab".to_string()));
        let count = Rc::new(Cell::new(0));
        let s_ref = Rc::clone(&s);
        let count_ref = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            let _ = s_ref.snapshot();
            let _ = s_ref.preedit();
            count_ref.set(count_ref.get() + 1);
        });
        assert_eq!(count.get(), 1);
        s.preedit_start();
        assert_eq!(count.get(), 2);
        s.preedit_commit("xyz");
        assert_eq!(
            count.get(),
            3,
            "preedit_commit batches text+caret+preedit_buffer into one re-run",
        );
    }

    #[test]
    fn r56_1_b_prev_next_char_boundary_walks_grapheme() {
        let s = "a한b"; // bytes: 'a'(1) + 한(3) + 'b'(1) = 5
                            // boundaries: {0, 1, 4, 5}
        assert_eq!(prev_char_boundary(s, 5), 4);
        assert_eq!(prev_char_boundary(s, 4), 1, "skip mid-codepoint");
        assert_eq!(prev_char_boundary(s, 1), 0);
        assert_eq!(prev_char_boundary(s, 0), 0);
        assert_eq!(next_char_boundary(s, 0), 1);
        assert_eq!(next_char_boundary(s, 1), 4, "skip mid-codepoint");
        assert_eq!(next_char_boundary(s, 4), 5);
        assert_eq!(next_char_boundary(s, 5), 5);
    }

    // ─────────────────────────────────────────────────────────────
    // R56.2.f §5.38 §5.22 — splice_preedit substrate helper
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_2_f_splice_preedit_collapsed_no_composition() {
        // No preedit → effective_text == committed text, visual caret
        // == caret arg, range == None. O(1) path.
        let s = TextEditState::with_initial("hello".to_owned());
        let (eff, vc, range) = s.splice_preedit(2);
        assert_eq!(eff, "hello");
        assert_eq!(vc, 2);
        assert_eq!(range, None);
    }

    #[test]
    fn r56_2_f_splice_preedit_empty_string_treated_as_no_composition() {
        // preedit_start sets Some("") — the helper treats the empty
        // string the same as no composition (no visible splice, no
        // range to paint). Matches the W3C contract: an empty
        // preedit conveys "composition active but data is empty"
        // which has no visible affordance to render.
        let s = TextEditState::with_initial("hello".to_owned());
        s.preedit_start();
        assert!(s.is_composing(), "preedit_start sets Some(\"\")");
        let (eff, vc, range) = s.splice_preedit(2);
        assert_eq!(eff, "hello");
        assert_eq!(vc, 2);
        assert_eq!(range, None);
    }

    #[test]
    fn r56_2_f_splice_preedit_non_empty_composition_splices_at_caret() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.preedit_start();
        s.preedit_update("XY");
        let (eff, vc, range) = s.splice_preedit(2);
        assert_eq!(eff, "heXYllo", "preedit spliced at caret arg = 2");
        assert_eq!(vc, 4, "visual caret at preedit end (caret + preedit.len())");
        assert_eq!(range, Some((2, 4)));
    }

    #[test]
    fn r56_2_f_splice_preedit_at_caret_zero_prepends() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.preedit_start();
        s.preedit_update("AB");
        let (eff, vc, range) = s.splice_preedit(0);
        assert_eq!(eff, "ABhello");
        assert_eq!(vc, 2);
        assert_eq!(range, Some((0, 2)));
    }

    #[test]
    fn r56_2_f_splice_preedit_at_caret_end_appends() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.preedit_start();
        s.preedit_update("AB");
        let (eff, vc, range) = s.splice_preedit(5);
        assert_eq!(eff, "helloAB");
        assert_eq!(vc, 7);
        assert_eq!(range, Some((5, 7)));
    }

    #[test]
    fn r56_2_f_splice_preedit_multi_byte_preedit() {
        // Korean syllable splice — preedit "한" (3 bytes UTF-8). The
        // helper preserves byte offsets so the LayoutCache key
        // (effective_text) matches the view fn's shaped run.
        let s = TextEditState::with_initial("ab".to_owned());
        s.preedit_start();
        s.preedit_update("한"); // 3 bytes
        let (eff, vc, range) = s.splice_preedit(1);
        assert_eq!(eff, "a한b");
        assert_eq!(vc, 4, "preedit_end = caret(1) + len(3) = 4");
        assert_eq!(range, Some((1, 4)));
    }

    #[test]
    fn r56_2_f_splice_preedit_clamps_caret_past_text_length() {
        // Defensive: even if the caller-supplied caret arg drifted past
        // `text.len()` (a future caret-state desync), the splice
        // helper clamps so the slice does not panic.
        let s = TextEditState::with_initial("ab".to_owned());
        s.preedit_start();
        s.preedit_update("X");
        let (eff, vc, range) = s.splice_preedit(99);
        assert_eq!(eff, "abX");
        assert_eq!(vc, 3, "splice clamps to text.len(), preedit_end = 2 + 1 = 3");
        assert_eq!(range, Some((2, 3)));
    }
}
