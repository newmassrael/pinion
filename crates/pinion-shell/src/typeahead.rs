//! R51.106 §5.38 — WAI-ARIA Listbox / Menu / Tree / Grid optional
//! type-ahead navigation substrate.
//!
//! Lifted from hello-listbox / hello-listbox-multi after the
//! [[substrate-incompleteness-signal]] trigger: two consumers
//! (R51.99 hello-listbox + R51.104 hello-listbox-multi) carry
//! identical `TypeaheadState` / `typeahead_step` /
//! `find_next_cyclic_match` / `find_first_prefix_match` /
//! `chars_eq_ci` / `single_printable_char` /
//! `TYPEAHEAD_TIMEOUT` (~150 LOC duplication). Future Menu /
//! `TreeView` / `Grid` composites will be the third+ consumer; lifting
//! now is the canonical R51.4 → R51.12 substrate-evolution cycle.
//!
//! ## Design boundary
//!
//! The cursor is **application-side state, not framework
//! primitive**. The user's keyboard session (buffer + last-typed
//! instant) belongs to the application's input loop, not the widget's
//! value sidecar. This module provides the pure-logic primitives
//! (`step`, `find_*`, `char_match_ci`, `is_typeahead_char`) and a
//! lightweight `TypeaheadCursor` container; the application owns the
//! storage (thread-local, struct field, ECS resource — caller's
//! choice).
//!
//! The pure-function shape makes the §2 #3 `dry_run` invariant safe:
//! `Instant` reaches the substrate only via the `now: Instant` step
//! parameter (caller-injected), never through an internal clock read.
//! Tests pass synthetic instants; production calls `Instant::now()`.
//!
//! ## Behavior (WAI-ARIA APG Listbox text-search algorithm)
//!
//! * Within [`TYPEAHEAD_TIMEOUT`] (500 ms default) of the previous
//!   keystroke, the new char appends to the buffer (prefix mode).
//! * Past the timeout, the buffer resets and the new char starts a
//!   fresh single-char cyclic search.
//! * Single-char buffer → cyclic search from `current + 1` so
//!   repeated taps of the same letter cycle through every matching
//!   option.
//! * Multi-char buffer → first-prefix-match starting at index 0
//!   (no cycling; the user's typed prefix is selecting one specific
//!   row).
//! * All comparisons use Unicode case folding via
//!   [`char::to_lowercase`] — i18n labels (CJK, accented Latin,
//!   Hangul, etc.) all participate.

use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::tree_nav::{
    apply_tree_key, flat_visible, reveal_row_cursor, TreeNode, VisibleRow,
};
use pinion_core::{Owner, Signal};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// W3C ARIA Authoring Practices common default for the typeahead
/// reset window. Most browser implementations of `<select>` /
/// `<datalist>` use 500 ms; some accessibility tools tune it
/// shorter (300 ms) or longer (1 s). Re-exported so applications
/// can read the default but not configurable here — call-site
/// configurability is a future axis when the first non-default
/// consumer appears.
pub const TYPEAHEAD_TIMEOUT: Duration = Duration::from_millis(500);

/// R51.106 §5.38 — type-ahead cursor: the keystroke buffer and the
/// last-typed `Instant`. Default constructed empty; the application
/// owns the storage (thread-local, struct field, etc.) and feeds it
/// to [`Self::step`] on each printable keypress.
#[derive(Default, Debug, Clone)]
pub struct TypeaheadCursor {
    buffer: String,
    last_typed: Option<Instant>,
}

impl TypeaheadCursor {
    /// Fresh empty cursor (no prior keystroke; next `step` starts a
    /// new search).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current search buffer. Empty before the first `step` call;
    /// holds the accumulated prefix while subsequent steps fall
    /// within [`TYPEAHEAD_TIMEOUT`].
    #[must_use]
    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    /// Reset the cursor (clear buffer + `last_typed`). Applications
    /// typically call this on focus-loss so a refocus starts a fresh
    /// search.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.last_typed = None;
    }

    /// Apply `key` at `now`, returning the option index to focus
    /// (or `None` when no label matches the current buffer).
    ///
    /// See module docs for the full WAI-ARIA APG algorithm.
    /// `current` is the currently-focused option index (caller reads
    /// from the widget's `focused_index` and passes here).
    pub fn step(
        &mut self,
        key: char,
        current: Option<usize>,
        now: Instant,
        labels: &[&str],
    ) -> Option<usize> {
        let fresh = self
            .last_typed
            .is_none_or(|t| now.duration_since(t) >= TYPEAHEAD_TIMEOUT);
        if fresh {
            self.buffer.clear();
        }
        self.buffer.push(key);
        self.last_typed = Some(now);
        if self.buffer.chars().count() == 1 {
            find_next_cyclic_match(current, key, labels)
        } else {
            find_first_prefix_match(&self.buffer, labels)
        }
    }
}

/// R51.106 §5.38 — `true` if `key` is a printable Unicode codepoint
/// suitable for type-ahead. Exactly one alphanumeric char (Unicode-
/// aware via [`char::is_alphanumeric`]); named keys (`ArrowDown`,
/// `Tab`, `F1`), control chars, whitespace, punctuation all return
/// `false`. Applications call this to gate type-ahead from their
/// `apply_key` fallback path.
#[must_use]
pub fn is_typeahead_char(key: &str) -> Option<char> {
    let mut iter = key.chars();
    let first = iter.next()?;
    if iter.next().is_some() {
        return None;
    }
    first.is_alphanumeric().then_some(first)
}

/// R51.106 §5.38 — Unicode-aware cyclic search for the next option
/// whose label begins with `key` (case-insensitive). Search starts
/// at `current + 1` so successive taps of the same letter cycle
/// through every matching option; falls back to `0` when `current`
/// is `None` or `>= labels.len()`.
#[must_use]
pub fn find_next_cyclic_match(
    current: Option<usize>,
    key: char,
    labels: &[&str],
) -> Option<usize> {
    let n = labels.len();
    if n == 0 {
        return None;
    }
    let start = match current {
        Some(c) if c < n => (c + 1) % n,
        _ => 0,
    };
    for offset in 0..n {
        let i = (start + offset) % n;
        if let Some(first) = labels[i].chars().next() {
            if char_match_ci(first, key) {
                return Some(i);
            }
        }
    }
    None
}

/// R51.106 §5.38 — first-prefix-match (no cycling) for multi-char
/// type-ahead. Compares the lowercased buffer against each label's
/// lowercased start; returns the first index that matches.
#[must_use]
pub fn find_first_prefix_match(buffer: &str, labels: &[&str]) -> Option<usize> {
    let buf_lower: String = buffer.chars().flat_map(char::to_lowercase).collect();
    for (i, label) in labels.iter().enumerate() {
        let label_lower: String =
            label.chars().flat_map(char::to_lowercase).collect();
        if label_lower.starts_with(&buf_lower) {
            return Some(i);
        }
    }
    None
}

/// R51.106 §5.38 — Unicode case-folded character equality.
/// [`char::to_lowercase`] returns an iterator (single-char may map
/// to multi-char, e.g. German `ß` → `ss`); we compare iterators
/// element-wise.
#[must_use]
pub fn char_match_ci(a: char, b: char) -> bool {
    a.to_lowercase().eq(b.to_lowercase())
}

/// R820.1 §5.27 §5.38 — type-ahead jump over a tree's flat visible rows:
/// the shared glue funnelling [`TypeaheadCursor`] for every tree consumer
/// (`hello-tree-view`, `hello-virtual-tree`, the `hello-dock-panels`
/// inspector). Gate on a printable char, locate the cursor's flat index by
/// id, collect the visible labels, step the owner-cached cursor, and move
/// `focused` to the matched row. Returns `false` on a non-printable key or
/// no match (the caller reports the key unhandled).
///
/// `cache_key` is the only caller-side parameter: the [`TypeaheadCursor`]
/// lives on the shell's root [`Owner`] under a per-binding slot (R811 —
/// the *cursor storage* is caller-choice; the surrounding flatten → index →
/// step → set glue is not, so it is the SSOT here). `rows` is the already
/// flattened visible sequence
/// ([`flat_visible`](pinion_core::widgets::tree_nav::flat_visible)), which
/// keeps this crate-direction clean: [`VisibleRow`] / [`Signal`] are
/// `pinion-core`, [`TypeaheadCursor`] is local to `pinion-shell`.
///
/// # Panics
///
/// Panics if called outside a `CoreShell::apply_key` reactive scope (no
/// [`Owner::current`]) — every shell key-dispatch path wraps the call in
/// the root Owner, so a panic here means the binding bypassed that wrap.
#[must_use]
pub fn tree_typeahead_jump(
    focused: &Signal<Option<String>>,
    rows: &[VisibleRow],
    cache_key: &'static str,
    key: &str,
) -> bool {
    let Some(ch) = is_typeahead_char(key) else {
        return false;
    };
    if rows.is_empty() {
        return false;
    }
    let current = focused
        .get()
        .and_then(|id| rows.iter().position(|row| row.id == id));
    let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
    let owner =
        Owner::current().expect("tree_typeahead_jump must run inside the apply_key Owner wrap");
    let cursor: Rc<RefCell<TypeaheadCursor>> =
        owner.cache(cache_key, || RefCell::new(TypeaheadCursor::new()));
    let target = cursor.borrow_mut().step(ch, current, Instant::now(), &labels);
    let Some(idx) = target else {
        return false;
    };
    focused.set(Some(rows[idx].id.clone()));
    true
}

/// R866 §5.27 §5.50 — apply one key to a **windowed** keyboard-roving tree: the
/// full keyboard step a virtualized tree binding's
/// [`apply_key`](pinion_core::WidgetCore::apply_key) delegates to. Resolves the
/// key through the lifted [`apply_tree_key`] (WAI-ARIA APG navigation →
/// expand/collapse flag store), falls through to [`tree_typeahead_jump`]
/// (type-ahead), then scrolls the resulting cursor into the rendered window via
/// [`reveal_row_cursor`] (keyboard ⊥ virtualization). Returns the handled
/// verdict so the shell swallow contract stays exact (an unhandled key like Tab
/// falls through).
///
/// Lifted at R866 from the byte-identical `apply_key_impl` copies in
/// `hello-virtual-tree` (R820) and `hello-tree-grid` (R864) — the windowed
/// counterpart of the un-windowed `apply_tree_key`, completing the
/// windowed-tree-keyboard substrate so the second consumer composes one call
/// instead of re-deriving the resolve→typeahead→reveal pipeline. The caller
/// supplies only its per-binding state (`nodes` / `focused`), scroll, and the
/// `typeahead_key` cache slot (R811 cursor-storage-is-caller-choice); the
/// pipeline glue is the SSOT here.
#[must_use]
pub fn apply_windowed_tree_key<N>(
    nodes: &Signal<Vec<N>>,
    focused: &Signal<Option<String>>,
    scroll: &ScrollState,
    typeahead_key: &'static str,
    page: usize,
    pitch: u32,
    key: &str,
) -> bool
where
    N: TreeNode + Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    let handled = apply_tree_key(nodes, focused, key, page)
        || tree_typeahead_jump(focused, &flat_visible(&nodes.get()), typeahead_key, key);
    if handled {
        let rows = flat_visible(&nodes.get());
        reveal_row_cursor(&rows, focused.get().as_deref(), scroll, pitch);
    }
    handled
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    /// Minimal flat visible row for the `tree_typeahead_jump` test (only
    /// `id` + `label` matter to type-ahead).
    fn row(id: &str, label: &str) -> VisibleRow {
        VisibleRow {
            id: id.to_owned(),
            label: label.to_owned(),
            depth: 0,
            position_in_set: 1,
            size_of_set: 1,
            has_children: false,
            expanded: false,
        }
    }

    #[test]
    fn tree_typeahead_jump_moves_focus_to_matching_label_and_reports_unhandled() {
        Owner::new().run(|| {
            let rows = vec![row("a", "Apple"), row("b", "Banana"), row("c", "Cherry")];
            let focused = Signal::new(Some(String::from("a")));
            // 'b' jumps the cursor to the next label starting with B.
            assert!(tree_typeahead_jump(&focused, &rows, "tree_ta_test", "b"), "printable handled");
            assert_eq!(focused.get().as_deref(), Some("b"), "cursor moved to Banana");
            // A non-printable named key is unhandled (caller falls through).
            assert!(!tree_typeahead_jump(&focused, &rows, "tree_ta_test", "ArrowDown"), "named key unhandled");
            // No match leaves the cursor and reports unhandled.
            assert!(!tree_typeahead_jump(&focused, &rows, "tree_ta_test", "z"), "no-match unhandled");
            assert_eq!(focused.get().as_deref(), Some("b"), "cursor unchanged on no match");
        });
    }

    fn labels_abc() -> Vec<&'static str> {
        vec!["Apple", "Banana", "Cherry", "Date"]
    }

    fn labels_overlapping() -> Vec<&'static str> {
        vec!["Apple", "Apricot", "Banana", "Blueberry", "Cherry", "Date"]
    }

    #[test]
    fn r51_106_is_typeahead_char_accepts_alphanumeric() {
        assert_eq!(is_typeahead_char("A"), Some('A'));
        assert_eq!(is_typeahead_char("0"), Some('0'));
        // Unicode-aware: Hangul, CJK ideographs, accented Latin.
        assert_eq!(is_typeahead_char("á"), Some('á'));
        assert_eq!(is_typeahead_char("한"), Some('한'));
        assert_eq!(is_typeahead_char("漢"), Some('漢'));
    }

    #[test]
    fn r51_106_is_typeahead_char_rejects_named_and_multi() {
        assert_eq!(is_typeahead_char("ArrowDown"), None);
        assert_eq!(is_typeahead_char("Tab"), None);
        assert_eq!(is_typeahead_char(""), None);
        assert_eq!(is_typeahead_char(" "), None);
        assert_eq!(is_typeahead_char("-"), None);
    }

    #[test]
    fn r51_106_char_match_ci_unicode() {
        assert!(char_match_ci('A', 'a'));
        assert!(char_match_ci('Á', 'á'));
        assert!(!char_match_ci('a', 'b'));
    }

    #[test]
    fn r51_106_find_next_cyclic_from_unfocused() {
        let labels = labels_abc();
        assert_eq!(find_next_cyclic_match(None, 'A', &labels), Some(0));
        assert_eq!(find_next_cyclic_match(None, 'a', &labels), Some(0));
        assert_eq!(find_next_cyclic_match(None, 'D', &labels), Some(3));
    }

    #[test]
    fn r51_106_find_next_cyclic_starts_after_current() {
        let labels = labels_overlapping();
        // From Apple (0), 'A' wraps and finds Apricot (1).
        assert_eq!(find_next_cyclic_match(Some(0), 'A', &labels), Some(1));
        // From Apricot (1), 'A' wraps back to Apple (0).
        assert_eq!(find_next_cyclic_match(Some(1), 'A', &labels), Some(0));
    }

    #[test]
    fn r51_106_find_next_cyclic_no_match() {
        let labels = labels_abc();
        assert_eq!(find_next_cyclic_match(None, 'Z', &labels), None);
    }

    #[test]
    fn r51_106_find_next_cyclic_out_of_range_current() {
        let labels = labels_abc();
        assert_eq!(find_next_cyclic_match(Some(99), 'A', &labels), Some(0));
    }

    #[test]
    fn r51_106_find_prefix_case_insensitive() {
        let labels = vec!["Apple", "BANANA", "blueberry"];
        assert_eq!(find_first_prefix_match("ban", &labels), Some(1));
        assert_eq!(find_first_prefix_match("BL", &labels), Some(2));
        assert_eq!(find_first_prefix_match("xyz", &labels), None);
    }

    #[test]
    fn r51_106_cursor_default_is_empty() {
        let cur = TypeaheadCursor::new();
        assert!(cur.buffer().is_empty());
    }

    #[test]
    fn r51_106_cursor_single_char_uses_cyclic() {
        let labels = labels_abc();
        let mut cur = TypeaheadCursor::new();
        let now = Instant::now();
        assert_eq!(cur.step('a', None, now, &labels), Some(0));
        assert_eq!(cur.buffer(), "a");
    }

    #[test]
    fn r51_106_cursor_multi_char_uses_prefix() {
        let labels = labels_overlapping();
        let mut cur = TypeaheadCursor::new();
        let t0 = Instant::now();
        assert_eq!(cur.step('a', None, t0, &labels), Some(0));
        let t1 = t0 + Duration::from_millis(100);
        assert_eq!(cur.step('p', Some(0), t1, &labels), Some(0));
        let t2 = t1 + Duration::from_millis(100);
        // "apr" matches only Apricot.
        assert_eq!(cur.step('r', Some(0), t2, &labels), Some(1));
        assert_eq!(cur.buffer(), "apr");
    }

    #[test]
    fn r51_106_cursor_timeout_resets() {
        let labels = vec!["Apple", "Pear"];
        let mut cur = TypeaheadCursor::new();
        let t0 = Instant::now();
        let _ = cur.step('a', None, t0, &labels);
        // Wait past timeout.
        let t1 = t0 + TYPEAHEAD_TIMEOUT + Duration::from_millis(1);
        assert_eq!(cur.step('p', Some(0), t1, &labels), Some(1));
        assert_eq!(cur.buffer(), "p", "fresh single-char buffer");
    }

    #[test]
    fn r51_106_cursor_reset_clears_buffer() {
        let labels = labels_abc();
        let mut cur = TypeaheadCursor::new();
        let _ = cur.step('a', None, Instant::now(), &labels);
        assert_eq!(cur.buffer(), "a");
        cur.reset();
        assert!(cur.buffer().is_empty());
    }

    #[test]
    fn r51_106_cursor_unicode_fold() {
        let labels = vec!["Älpine", "Mango"];
        let mut cur = TypeaheadCursor::new();
        let now = Instant::now();
        // 'ä' (U+00E4) matches 'Ä' (U+00C4) via Unicode case fold.
        assert_eq!(cur.step('ä', None, now, &labels), Some(0));
    }

    #[test]
    fn r51_106_cursor_empty_labels_returns_none() {
        let mut cur = TypeaheadCursor::new();
        let r = cur.step('a', None, Instant::now(), &[]);
        assert_eq!(r, None);
    }
}
