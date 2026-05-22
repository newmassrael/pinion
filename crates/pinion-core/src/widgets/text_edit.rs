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
//! - Clipboard / selection / IME: R56.1.e / R56.1.f / R56.1.g.
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
            tag: None,
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
            tag: None,
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

    /// Replace the buffer with `new_text`. The caret is clamped
    /// to the nearest `char` boundary at-or-below the new text
    /// length (so a `set_text` shorter than the current text moves
    /// the caret in instead of leaving it past-end).
    ///
    /// Wrapped in [`batch`] so the `text` + (optional) `caret_pos`
    /// writes collapse into one notification cascade — the
    /// R55.G.24 atomic-multi-axis contract.
    pub fn set_text(&self, new_text: String) {
        let new_len = new_text.len();
        let cur_caret = self.caret_pos.get();
        let clamped_caret = clamp_to_char_boundary(&new_text, cur_caret.min(new_len));
        batch(|| {
            self.text.set(new_text);
            self.caret_pos.set(clamped_caret);
        });
    }

    /// Move the caret to `pos`, clamped to `[0, text.len()]` and
    /// snapped to the nearest preceding `char` boundary if `pos`
    /// would land mid-codepoint. Signal equality-skip suppresses
    /// the re-run when the clamped target matches the current
    /// caret.
    pub fn set_caret(&self, pos: usize) {
        let text = self.text.get();
        let clamped = clamp_to_char_boundary(&text, pos.min(text.len()));
        self.caret_pos.set(clamped);
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
    pub fn insert(&self, s: &str) {
        if s.is_empty() {
            return;
        }
        let mut buf = self.text.get();
        let pos = self.caret_pos.get().min(buf.len());
        let snapped = clamp_to_char_boundary(&buf, pos);
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
    pub fn backspace(&self) {
        let mut buf = self.text.get();
        let caret = self.caret_pos.get().min(buf.len());
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
    pub fn delete_forward(&self) {
        let mut buf = self.text.get();
        let caret = self.caret_pos.get().min(buf.len());
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
    pub fn move_left(&self) {
        let text = self.text.get();
        let caret = self.caret_pos.get().min(text.len());
        if caret == 0 {
            return;
        }
        let prev = prev_char_boundary(&text, caret);
        self.caret_pos.set(prev);
    }

    /// Move the caret one `char` to the right (towards `text.len()`).
    /// No-op when caret is at `text.len()`.
    pub fn move_right(&self) {
        let text = self.text.get();
        let caret = self.caret_pos.get().min(text.len());
        if caret >= text.len() {
            return;
        }
        let next = next_char_boundary(&text, caret);
        self.caret_pos.set(next);
    }

    /// Move the caret to the start of the buffer (Home / Ctrl-A
    /// canonical on single-line fields).
    pub fn move_home(&self) {
        self.caret_pos.set(0);
    }

    /// Move the caret to the end of the buffer (End / Ctrl-E
    /// canonical on single-line fields).
    pub fn move_end(&self) {
        let len = self.text.get().len();
        self.caret_pos.set(len);
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
}
