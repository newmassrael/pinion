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

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::reactive::{batch, Owner, Signal};
use crate::scene::StyleRun;
use crate::style::TextStyle;
use crate::undo::{UndoCommand, UndoStack};

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
    /// R767 §5.36 §5.22 — **styled runs** for rich-text editing: an
    /// ordered, non-overlapping list of [`StyleRun`] spans over the
    /// current [`text`](Self::text) byte buffer (the Qt `FormatRange`
    /// model — each run is a fully-resolved [`TextStyle`] over a UTF-8
    /// byte range). Empty (the default) is the single-style fast path;
    /// the field's paint threads the runs into the
    /// [`layout_with_runs`](pinion_text) shaping the visible caret /
    /// hit-test share, so paint and geometry stay one layout.
    ///
    /// Maintained across edits: [`Self::insert`] shifts runs at/after
    /// the caret, [`Self::backspace`] / [`Self::delete_forward`] clip
    /// runs against the removed range — the canonical editor contract
    /// that a styled span tracks *its text*, not a fixed byte offset.
    /// Applying / clearing formatting over a range is a later slice
    /// ([`Self::set_style_runs`] is the substrate seam in the meantime).
    ///
    /// A reactive [`Signal`] — the same content-state shape as
    /// [`text`](Self::text) / [`caret_pos`](Self::caret_pos) /
    /// [`selection_anchor`](Self::selection_anchor), **not** the
    /// non-reactive scratch shape [`goal_column`](Self::goal_column)
    /// uses. Two reasons, both from being a content peer of `text`:
    ///
    /// 1. **Consistency** — every other content field of this struct is
    ///    a `Signal`; the runs are observable content (the field paints
    ///    them, `scene/snapshot` exposes them), so they share that shape
    ///    rather than diverging to interior mutability.
    /// 2. **Reactivity** — a view-fn that reads the runs (the field
    ///    paint via `field_shaping`) subscribes, so a formatting change
    ///    re-runs it. R767 edits also write [`text`](Self::text), so a
    ///    `text` subscriber repaints anyway; but a *runs-only* mutation
    ///    — applying bold / colour over a selection without a text edit
    ///    (the next slice) — repaints **only** through this direct
    ///    subscription, so modelling runs reactively now means that
    ///    slice needs no state-shape change.
    ///
    /// Scope note: like its `text` / `caret` peers, the runs live in the
    /// §5.22 reactive **sidecar**, which is **outside** the `dry_run`
    /// determinism guarantee (§5.8 bounds that to scene + SCE state).
    /// `scene/simulate` restores the scene's External introspect values
    /// it mutates, not this sidecar — so no snapshot/restore is claimed
    /// here. `Signal<T>` requires `Serialize + DeserializeOwned`, which
    /// is why R767.1 derived `serde` on the
    /// [`TextStyle`](crate::style::TextStyle) family (resolving the
    /// `Color`-is-serde-but-`TextStyle`-is-not inconsistency it surfaced).
    style_runs: Signal<Vec<StyleRun>>,
    /// R796 §5.52 — optional attached undo / redo history. `None`
    /// (the default) means edits are not journalled — every existing
    /// caller is byte-unchanged. When a binding calls
    /// [`Self::attach_undo`], each content-changing mutator records a
    /// whole-content [`TextEditCommand`] (coalescing consecutive typing
    /// into one step), and [`Self::undo`] / [`Self::redo`] replay it.
    /// A `RefCell` (not a `Signal`): the attachment is wiring, not
    /// observable content — no view-fn renders "is undo attached".
    undo: RefCell<Option<Rc<UndoStack>>>,
    /// R903 §5.22 — **find &amp; replace** session: the active search needle.
    /// Empty (the default) is "no search active" — [`find_matches`](Self::find_matches)
    /// yields nothing and the highlight paint draws no bands. A reactive
    /// `Signal` (content peer of [`text`](Self::text)) so the find-highlight
    /// paint and the match-count status subscribe and re-derive the moment the
    /// needle changes. The session lives on the editable buffer itself (not a
    /// separate widget) so the field is **self-describing** to AI introspection:
    /// `scene/<tag>/external/find_matches` reports the editor's own match state
    /// without the agent reconstructing it from a find-bar's text. The "current
    /// match" is **not** a field — it is implicit in the selection (the
    /// browser / VS Code model: find-next searches forward from the selection
    /// and lands the selection on the hit), so there is no current-index state
    /// to keep clamped against an edit-shifted match list.
    find_query: Signal<String>,
    /// R903 §5.22 — case-sensitivity of the [`find_query`](Self::find_query)
    /// match. `false` (the default) folds **ASCII** case only (`A`..=`Z` ↔
    /// `a`..=`z`); non-ASCII code points compare exactly, which keeps every
    /// match range on the source's own byte boundaries (Unicode case folding
    /// changes byte lengths and is a deferred axis). Reactive for the same
    /// reason as [`find_query`](Self::find_query).
    find_case_sensitive: Signal<bool>,
    /// R903 §5.22 — whole-word constraint: when `true`, a match counts only
    /// when neither neighbour char is a *word* char (alphanumeric or `_`), the
    /// canonical editor "Match Whole Word" toggle. `false` (the default)
    /// matches any substring. Reactive peer of the other find axes.
    find_whole_word: Signal<bool>,
    /// R904 §5.36 — optional **syntax highlighter**: a pure
    /// `Fn(&str) -> Vec<StyleRun>` deriving the displayed styled runs from the
    /// buffer content (a tokeniser; see [`crate::syntax::highlight_code`]).
    /// `None` (the default) leaves [`style_runs`](Self::style_runs) returning
    /// the manually-applied runs (the rich-text path). When a binding calls
    /// [`attach_highlighter`](Self::attach_highlighter), `style_runs` instead
    /// re-derives from the live text on every read (the
    /// [`find_matches`](Self::find_matches) re-derive shape) so paint, caret
    /// geometry, and the `scene/style_runs` RPC all see the syntax coloring —
    /// a highlighter and manual styling are the editor's two mutually-exclusive
    /// modes (code editor vs rich-text). A `RefCell` (not a `Signal`): the
    /// attachment is wiring, not observable content — the reactive dependency
    /// is the *text* the closure reads through [`style_runs`](Self::style_runs).
    highlighter: RefCell<Option<Highlighter>>,
}

/// R904 §5.36 — a syntax-highlighter closure: derives the displayed
/// [`StyleRun`]s from buffer text. See
/// [`TextEditState::attach_highlighter`].
pub type HighlighterFn = Rc<dyn Fn(&str) -> Vec<StyleRun>>;

/// R904 §5.36 — boxed syntax-highlighter closure. A newtype solely so
/// [`TextEditState`] keeps its `#[derive(Debug)]` (a bare `dyn Fn` is not
/// `Debug`); the manual impl prints a placeholder.
#[derive(Clone)]
struct Highlighter(HighlighterFn);

impl core::fmt::Debug for Highlighter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Highlighter(..)")
    }
}

/// R796 §5.52 — which edits may coalesce. Two commands fold into one undo
/// step only when they share a non-[`Boundary`](CoalesceGroup::Boundary)
/// group and are contiguous, so an insertion run, a Backspace run, and a
/// Delete-forward run each collapse to one step while a wholesale replace or
/// a selection-delete stands alone (`QTextDocument` typing-coalesce model).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CoalesceGroup {
    /// Never coalesces (`set_text`, or any selection-replacing edit).
    Boundary,
    /// Consecutive character insertions.
    Insert,
    /// Consecutive Backspace deletions.
    DeleteBack,
    /// Consecutive Delete-forward deletions.
    DeleteForward,
}

/// R796.1 §5.52 — a reversible **granular** text edit (the text field's
/// [`UndoCommand`]). It stores only the single contiguous splice it made —
/// `removed` bytes at `offset` replaced by `inserted` — plus the style-run
/// coverage that splice destroyed, so it reverses **without snapshotting the
/// whole document**. Memory is `O(edit size + runs in the edited span)`, not
/// `O(document)`: a keystroke in plain text costs one byte, not a full copy
/// of the buffer (the `QTextDocument` / xi-editor delta-undo model). `redo`
/// re-derives the run shift deterministically (the same clip+shift the
/// mutators apply); `undo` reverses the splice and restores the destroyed
/// run coverage from `removed_runs`, re-normalising. Consecutive same-group
/// contiguous edits coalesce via [`merge`](UndoCommand::merge) so a typing
/// run is one Ctrl+Z.
#[derive(Debug)]
pub(crate) struct TextEditCommand {
    text: Signal<String>,
    caret: Signal<usize>,
    anchor: Signal<Option<usize>>,
    runs: Signal<Vec<StyleRun>>,
    /// Byte offset of the splice.
    offset: usize,
    /// Bytes removed by the edit (empty for a pure insert).
    removed: String,
    /// Bytes inserted by the edit (empty for a pure delete).
    inserted: String,
    /// Style-run fragments covering `[offset, offset + removed.len())` before
    /// the edit (absolute byte positions), restored on undo.
    removed_runs: Vec<StyleRun>,
    caret_before: usize,
    caret_after: usize,
    anchor_before: Option<usize>,
    anchor_after: Option<usize>,
    group: CoalesceGroup,
    /// Whether a following same-group edit may fold into this one. Cleared
    /// once a word boundary (whitespace) lands, so the next keystroke opens a
    /// fresh undo step (word-level granularity).
    coalescable: bool,
    label: Cow<'static, str>,
}

impl UndoCommand for TextEditCommand {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        let mut buf = self.text.get();
        buf.replace_range(self.offset..self.offset + self.removed.len(), &self.inserted);
        let mut runs = self.runs.get();
        // The same maintenance the forward mutators apply: drop the deleted
        // span's coverage, then shift trailing runs right for the insert.
        clip_runs_for_delete(&mut runs, self.offset, self.offset + self.removed.len());
        shift_runs_for_insert(&mut runs, self.offset, self.inserted.len());
        batch(|| {
            self.text.set(buf);
            self.caret.set(self.caret_after);
            self.anchor.set(self.anchor_after);
            self.runs.set(runs);
        });
    }

    fn undo(&self) {
        let mut buf = self.text.get();
        buf.replace_range(self.offset..self.offset + self.inserted.len(), &self.removed);
        let mut runs = self.runs.get();
        // Reverse the forward clip+shift, restore the coverage the delete
        // destroyed, and re-fuse any survivor that was extended back over it.
        clip_runs_for_delete(&mut runs, self.offset, self.offset + self.inserted.len());
        shift_runs_for_insert(&mut runs, self.offset, self.removed.len());
        runs.extend(self.removed_runs.iter().cloned());
        normalize_runs(&mut runs);
        batch(|| {
            self.text.set(buf);
            self.caret.set(self.caret_before);
            self.anchor.set(self.anchor_before);
            self.runs.set(runs);
        });
    }

    fn as_any(&self) -> Option<&dyn core::any::Any> {
        Some(self)
    }

    fn merge(&mut self, next: &dyn UndoCommand) -> bool {
        let Some(next) = next
            .as_any()
            .and_then(|a| a.downcast_ref::<TextEditCommand>())
        else {
            return false;
        };
        if !self.coalescable || self.group != next.group {
            return false;
        }
        match self.group {
            // Pure inserts extend rightward: the next insert begins exactly
            // where this one ended.
            CoalesceGroup::Insert => {
                if !self.removed.is_empty()
                    || !next.removed.is_empty()
                    || next.offset != self.offset + self.inserted.len()
                {
                    return false;
                }
                self.inserted.push_str(&next.inserted);
            }
            // Backspace runs grow leftward: the next delete ends where this
            // one begins. Prepend its bytes + run coverage (already absolute).
            CoalesceGroup::DeleteBack => {
                if !self.inserted.is_empty()
                    || !next.inserted.is_empty()
                    || next.offset + next.removed.len() != self.offset
                {
                    return false;
                }
                let mut bytes = next.removed.clone();
                bytes.push_str(&self.removed);
                self.removed = bytes;
                let mut cov = next.removed_runs.clone();
                cov.extend(self.removed_runs.iter().cloned());
                self.removed_runs = cov;
                self.offset = next.offset;
            }
            // Delete-forward runs grow rightward at a fixed offset; the next
            // delete removes what the previous one exposed, so its coverage
            // re-bases past the already-removed bytes.
            CoalesceGroup::DeleteForward => {
                if !self.inserted.is_empty()
                    || !next.inserted.is_empty()
                    || next.offset != self.offset
                {
                    return false;
                }
                let shift = u32::try_from(self.removed.len()).unwrap_or(0);
                self.removed.push_str(&next.removed);
                self.removed_runs.extend(
                    next.removed_runs
                        .iter()
                        .map(|r| StyleRun::new(r.start + shift, r.end + shift, r.style.clone())),
                );
            }
            CoalesceGroup::Boundary => return false,
        }
        self.caret_after = next.caret_after;
        self.anchor_after = next.anchor_after;
        // Inherit the continuation's coalescability: a whitespace insert
        // (coalescable = false) ends the word, so the *next* keystroke opens
        // a new step.
        self.coalescable = next.coalescable;
        true
    }
}

/// R796.1 §5.52 — the style-run fragments overlapping `[start, end)`, clamped
/// to that range (absolute byte positions). Captures the formatting a delete
/// destroys so [`TextEditCommand::undo`] can restore it.
fn runs_over_range(runs: &[StyleRun], start: usize, end: usize) -> Vec<StyleRun> {
    let (a, b) = (
        u32::try_from(start).unwrap_or(u32::MAX),
        u32::try_from(end).unwrap_or(u32::MAX),
    );
    runs.iter()
        .filter(|r| r.start < b && r.end > a)
        .map(|r| StyleRun::new(r.start.max(a), r.end.min(b), r.style.clone()))
        .collect()
}

/// R796.1 §5.52 — canonicalise a run list after an undo restore: sort by
/// start, then fuse runs that overlap or abut and share a style (so a
/// survivor extended back over the restored span re-fuses with its restored
/// fragment). Drops empty runs.
fn normalize_runs(runs: &mut Vec<StyleRun>) {
    runs.retain(|r| r.start < r.end);
    runs.sort_by_key(|r| r.start);
    let mut merged: Vec<StyleRun> = Vec::with_capacity(runs.len());
    for r in runs.drain(..) {
        if let Some(last) = merged.last_mut() {
            if last.end >= r.start && last.style == r.style {
                last.end = last.end.max(r.end);
                continue;
            }
        }
        merged.push(r);
    }
    *runs = merged;
}

/// R796.1 §5.52 — the minimal single contiguous splice between `before` and
/// `after`: strip the common prefix + suffix (clamped to `char` boundaries)
/// and return `(offset, removed, inserted)`. Each text mutator performs one
/// contiguous edit, so this recovers exactly that edit for granular undo.
fn text_diff(before: &str, after: &str) -> (usize, String, String) {
    let (bb, ab) = (before.as_bytes(), after.as_bytes());
    let max_p = bb.len().min(ab.len());
    let mut p = 0;
    while p < max_p && bb[p] == ab[p] {
        p += 1;
    }
    while p > 0 && !before.is_char_boundary(p) {
        p -= 1;
    }
    let mut s = 0;
    let smax = (bb.len() - p).min(ab.len() - p);
    while s < smax && bb[bb.len() - 1 - s] == ab[ab.len() - 1 - s] {
        s += 1;
    }
    while s > 0
        && (!before.is_char_boundary(bb.len() - s) || !after.is_char_boundary(ab.len() - s))
    {
        s -= 1;
    }
    (
        p,
        before[p..bb.len() - s].to_string(),
        after[p..ab.len() - s].to_string(),
    )
}

/// R903 §5.22 — a *word* character for whole-word find matching: alphanumeric
/// (Unicode-aware) or `_`, the canonical editor word class (`\w`).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// R903 §5.22 — do two chars match under the find case policy? Case-sensitive
/// is exact; case-insensitive folds **ASCII** letters only ([`char::eq_ignore_ascii_case`]),
/// leaving non-ASCII code points to compare exactly so a match never straddles
/// a byte boundary the source does not have.
fn chars_match(a: char, b: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        a == b
    } else {
        a.eq_ignore_ascii_case(&b)
    }
}

/// R903 §5.22 — if `needle` matches `haystack` starting at byte `at` (under the
/// case policy), the end byte offset of that match; else `None`. `at` must be a
/// `char` boundary. The end is a `char` boundary because it sums whole
/// haystack-char byte lengths.
fn match_at(haystack: &str, at: usize, needle: &str, case_sensitive: bool) -> Option<usize> {
    let mut hchars = haystack[at..].chars();
    let mut consumed = 0usize;
    for nc in needle.chars() {
        match hchars.next() {
            Some(hc) if chars_match(hc, nc, case_sensitive) => consumed += hc.len_utf8(),
            _ => return None,
        }
    }
    Some(at + consumed)
}

/// R903 §5.22 — whole-word guard: the match `[start, end)` sits on word
/// boundaries (each neighbour is a non-word char or the buffer edge).
fn is_word_boundary(haystack: &str, start: usize, end: usize) -> bool {
    let before_ok = start == 0
        || !haystack[..start]
            .chars()
            .next_back()
            .is_some_and(is_word_char);
    let after_ok = end == haystack.len()
        || !haystack[end..].chars().next().is_some_and(is_word_char);
    before_ok && after_ok
}

/// R903 §5.22 — every non-overlapping match of `needle` in `haystack` as
/// `(start, end)` byte ranges, left to right (a match resumes at the previous
/// match's end — textbook find-all). An empty `needle` yields none.
/// `case_sensitive == false` folds ASCII case only; `whole_word` additionally
/// requires both neighbours to be non-word chars (or the buffer edge). The
/// pure search core shared by every [`TextEditState`] find / replace method
/// and unit-testable without a reactive scope (the `text_diff` sibling shape).
fn find_matches_in(
    haystack: &str,
    needle: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < haystack.len() {
        if let Some(end) = match_at(haystack, idx, needle, case_sensitive) {
            if !whole_word || is_word_boundary(haystack, idx, end) {
                out.push((idx, end));
                idx = end; // non-overlapping: resume past the match
                continue;
            }
        }
        // No match (or whole-word reject) here — step to the next char start.
        idx += haystack[idx..].chars().next().map_or(1, char::len_utf8);
    }
    out
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
            style_runs: Signal::new(Vec::new()),
            undo: RefCell::new(None),
            find_query: Signal::new(String::new()),
            find_case_sensitive: Signal::new(false),
            find_whole_word: Signal::new(false),
            highlighter: RefCell::new(None),
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
            style_runs: Signal::new(Vec::new()),
            undo: RefCell::new(None),
            find_query: Signal::new(String::new()),
            find_case_sensitive: Signal::new(false),
            find_whole_word: Signal::new(false),
            highlighter: RefCell::new(None),
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

    /// R796 §5.52 — attach an [`UndoStack`] so subsequent content edits are
    /// journalled and [`undo`](Self::undo) / [`redo`](Self::redo) replay
    /// them. Attaching on the **state** (not the external) is deliberate: the
    /// content mutators here are the single write path, so this is where the
    /// before/after delta is captured. Call once at wiring time with a
    /// `use_undo_stack` handle; the default (unattached) leaves every
    /// existing caller byte-unchanged. Re-attaching replaces the stack.
    pub fn attach_undo(&self, stack: Rc<UndoStack>) {
        *self.undo.borrow_mut() = Some(stack);
    }

    /// The attached [`UndoStack`], or `None` when no history is wired.
    #[must_use]
    pub fn undo_stack(&self) -> Option<Rc<UndoStack>> {
        self.undo.borrow().clone()
    }

    /// Run a content mutation `f`, journalling its **granular** delta onto
    /// the attached [`UndoStack`]. A no-op passthrough when no stack is
    /// attached (the default path). The edit is pushed *already applied*
    /// (`f` wrote the signals eagerly); the resulting before/after text is
    /// diffed to the single contiguous splice (`offset`, `removed`,
    /// `inserted`) so the command stores `O(edit)` bytes, not the whole
    /// buffer. A no-change `f` (empty insert, Backspace at offset 0) records
    /// nothing.
    fn record_edit(
        &self,
        group: CoalesceGroup,
        coalescable: bool,
        label: &'static str,
        f: impl FnOnce(),
    ) {
        let Some(stack) = self.undo.borrow().clone() else {
            f();
            return;
        };
        let before_text = self.text.get();
        let caret_before = self.caret_pos.get();
        let anchor_before = self.selection_anchor.get();
        let before_runs = self.style_runs.get();
        f();
        let after_text = self.text.get();
        if before_text == after_text {
            return;
        }
        let (offset, removed, inserted) = text_diff(&before_text, &after_text);
        let removed_runs = runs_over_range(&before_runs, offset, offset + removed.len());
        stack.push_applied(TextEditCommand {
            text: self.text.clone(),
            caret: self.caret_pos.clone(),
            anchor: self.selection_anchor.clone(),
            runs: self.style_runs.clone(),
            offset,
            removed,
            inserted,
            removed_runs,
            caret_before,
            caret_after: self.caret_pos.get(),
            anchor_before,
            anchor_after: self.selection_anchor.get(),
            group,
            coalescable,
            label: Cow::Borrowed(label),
        });
    }

    /// R796 §5.52 — step the attached history back one command, restoring the
    /// prior content. `false` (no-op) when no stack is attached or it is
    /// already at the bottom. The restore is a batched whole-content write,
    /// so every subscribed view repaints exactly as the original edit did.
    pub fn undo(&self) -> bool {
        let stack = self.undo.borrow().clone();
        stack.is_some_and(|s| s.undo())
    }

    /// Mirror of [`undo`](Self::undo): re-apply the next undone command.
    pub fn redo(&self) -> bool {
        let stack = self.undo.borrow().clone();
        stack.is_some_and(|s| s.redo())
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

    /// R767 §5.36 §5.22 — current styled runs (rich-text formatting).
    /// Subscribes like [`Self::text`]: a view-fn reading the runs
    /// re-runs when an edit shifts / clips them. Empty for a plain
    /// single-style field. See the [`style_runs`](Self::style_runs)
    /// field doc for the maintenance contract.
    ///
    /// R904 §5.36 — when a [`highlighter`](Self::highlighter) is attached
    /// ([`attach_highlighter`](Self::attach_highlighter)), this **derives** the
    /// runs from the live text instead (subscribing to the text `Signal`, so a
    /// view-fn re-runs on every edit and re-highlights), and the manually
    /// stored runs are shadowed. The single read site — paint, caret geometry,
    /// hit-test, and the `scene/style_runs` RPC all call this — so all four see
    /// the syntax coloring with no per-consumer change (the
    /// [`find_matches`](Self::find_matches) derive shape).
    #[must_use]
    pub fn style_runs(&self) -> Vec<StyleRun> {
        if let Some(highlighter) = self.highlighter.borrow().as_ref() {
            return (highlighter.0)(&self.text.get());
        }
        self.style_runs.get()
    }

    /// R904 §5.36 — attach a **syntax highlighter**: a pure
    /// `Fn(&str) -> Vec<StyleRun>` (a tokeniser — see
    /// [`crate::syntax::highlight_code`]) that derives the displayed runs from
    /// the buffer content. Once attached, [`style_runs`](Self::style_runs)
    /// re-derives on every read and the manual styling path
    /// ([`apply_style_run`](Self::apply_style_run) etc.) is shadowed — a field
    /// is either a code editor (highlighted) or a rich-text editor (manually
    /// styled), not both. The closure is the language-agnostic seam: the
    /// grammar lives in the caller's tokeniser, not in a framework `Language`
    /// trait ([[abstraction-needs-second-consumer]]).
    pub fn attach_highlighter(&self, highlighter: HighlighterFn) {
        *self.highlighter.borrow_mut() = Some(Highlighter(highlighter));
    }

    /// R767 §5.36 §5.22 — replace the styled-run list wholesale. The
    /// substrate seam for seeding a rich field's initial formatting and
    /// (later) for applying / clearing formatting over a range. The
    /// caller supplies fully-resolved, ideally non-overlapping runs in
    /// byte order; the edit mutators maintain whatever shape is set
    /// here across subsequent inserts / deletes.
    pub fn set_style_runs(&self, runs: Vec<StyleRun>) {
        self.style_runs.set(runs);
    }

    /// R768 §5.36 §5.22 — apply `style` to the byte range `[start, end)`
    /// as one styled run: the rich-text "apply formatting to the
    /// selection" primitive (Qt `QTextCursor::setCharFormat` semantics).
    /// Existing runs are carved around the range and a run carrying
    /// `style` is laid in; adjacent runs that end up with an identical
    /// style coalesce, so re-applying the same format across a former
    /// split leaves no seam.
    ///
    /// `start` / `end` are clamped to `[0, text.len()]` and snapped to
    /// `char` boundaries; an empty (or inverted) range is a no-op. The
    /// text buffer is untouched — only the styling changes.
    ///
    /// Reactive: a view-fn reading [`style_runs`](Self::style_runs) (the
    /// field paint) re-runs, so a formatting change with **no** text edit
    /// still repaints — the runs-only mutation path the
    /// [`style_runs`](Self::style_runs) field doc anticipated.
    pub fn apply_style_run(&self, start: usize, end: usize, style: TextStyle) {
        let buf = self.text.get();
        let a = clamp_to_char_boundary(&buf, start);
        let b = clamp_to_char_boundary(&buf, end);
        if a >= b {
            return;
        }
        let mut runs = self.style_runs.get();
        overlay_style_run(
            &mut runs,
            StyleRun::new(
                u32::try_from(a).unwrap_or(u32::MAX),
                u32::try_from(b).unwrap_or(u32::MAX),
                style,
            ),
        );
        self.style_runs.set(runs);
    }

    /// R768 §5.36 §5.22 — remove all styling over the byte range
    /// `[start, end)` (rich-text "clear formatting"), returning those
    /// bytes to the field's base style. Runs straddling a boundary are
    /// split; runs fully inside are dropped. The text is untouched (this
    /// is **not** a deletion — no offset shifts). `start` / `end` are
    /// clamped + `char`-snapped; an empty / inverted range is a no-op.
    pub fn clear_style_runs(&self, start: usize, end: usize) {
        let buf = self.text.get();
        let a = clamp_to_char_boundary(&buf, start);
        let b = clamp_to_char_boundary(&buf, end);
        if a >= b {
            return;
        }
        let mut runs = self.style_runs.get();
        subtract_style_range(
            &mut runs,
            u32::try_from(a).unwrap_or(u32::MAX),
            u32::try_from(b).unwrap_or(u32::MAX),
        );
        self.style_runs.set(runs);
    }

    /// R769 §5.36 §5.22 — merge a per-field style transform over the byte
    /// range `[start, end)` (Qt `mergeCharFormat`): the toolbar "toggle
    /// **bold** / *italic* over the selection while keeping its colour"
    /// primitive. Each affected byte's other styling is preserved; only
    /// the field(s) `mutate` touches change. Covered bytes transform
    /// their run's style; uncovered bytes resolve against `base` (the
    /// field's default char format — pass the same base style the field
    /// paints unstyled text with) before the transform.
    ///
    /// The caller owns the *policy* (which field, and the toggle
    /// direction — read [`Self::style_at`] to decide); this substrate
    /// owns the *mechanics* (sub-span resolution + overlay + merge). The
    /// R768 [`Self::apply_style_run`] is the wholesale `setCharFormat`
    /// peer (replaces every field); this preserves untouched ones.
    /// `start` / `end` are clamped + `char`-snapped; empty range no-op.
    pub fn merge_style_run(
        &self,
        start: usize,
        end: usize,
        base: &TextStyle,
        mutate: impl Fn(&mut TextStyle),
    ) {
        let buf = self.text.get();
        let a = clamp_to_char_boundary(&buf, start);
        let b = clamp_to_char_boundary(&buf, end);
        if a >= b {
            return;
        }
        let mut runs = self.style_runs.get();
        field_merge_runs(
            &mut runs,
            u32::try_from(a).unwrap_or(u32::MAX),
            u32::try_from(b).unwrap_or(u32::MAX),
            base,
            mutate,
        );
        self.style_runs.set(runs);
    }

    /// R769 §5.36 §5.22 — the resolved style of the run covering `byte`,
    /// or `None` if the byte is unstyled (renders with the field base).
    /// A toolbar reads this at the selection start to decide a toggle's
    /// direction (e.g. "is the selection already bold → un-bold it").
    /// `byte` is a raw offset; callers pass an already-`char`-aligned
    /// caret / selection byte.
    #[must_use]
    pub fn style_at(&self, byte: usize) -> Option<TextStyle> {
        let b = u32::try_from(byte).unwrap_or(u32::MAX);
        self.style_runs
            .get()
            .into_iter()
            .find(|r| r.start <= b && b < r.end)
            .map(|r| r.style)
    }

    // ───────────────────────── R903 §5.22 find &amp; replace ─────────────────
    //
    // The find session is two reactive axes (the needle + its case / whole-word
    // flags); the *current match* is the selection (the browser / VS Code
    // model), so there is no persistent cursor to keep clamped against an
    // edit-shifted match list. `find_matches` re-derives from the live text on
    // every call (subscribing in a view-fn), so the highlight paint and the
    // match-count status stay correct through every edit with no manual refresh
    // threaded into the mutators. Replace routes through one splice primitive
    // ([`replace_range`]); *Replace All* wraps the run in an
    // [`UndoStack`](crate::undo::UndoStack) macro so one Ctrl+Z reverses it.

    /// R903 §5.22 — the active find needle (reactive read). Empty is "no search".
    #[must_use]
    pub fn find_query(&self) -> String {
        self.find_query.get()
    }

    /// R903 §5.22 — set the find needle. A pure setter: it never moves the
    /// caret / selection (call [`find_next`](Self::find_next) to navigate), so
    /// the read/write pair stays symmetric and side-effect-free. Equality-skip
    /// suppresses the re-derive when the needle is unchanged.
    pub fn set_find_query(&self, query: &str) {
        self.find_query.set(query.to_string());
    }

    /// R903 §5.22 — whether the find match is case-sensitive (reactive read).
    #[must_use]
    pub fn find_case_sensitive(&self) -> bool {
        self.find_case_sensitive.get()
    }

    /// R903 §5.22 — set case-sensitivity of the find match.
    pub fn set_find_case_sensitive(&self, on: bool) {
        self.find_case_sensitive.set(on);
    }

    /// R903 §5.22 — whether the find match is constrained to whole words
    /// (reactive read).
    #[must_use]
    pub fn find_whole_word(&self) -> bool {
        self.find_whole_word.get()
    }

    /// R903 §5.22 — set the whole-word constraint of the find match.
    pub fn set_find_whole_word(&self, on: bool) {
        self.find_whole_word.set(on);
    }

    /// R903 §5.22 — every match of the current needle in the live buffer as
    /// `(start, end)` byte ranges (reactive read: subscribes to the text + all
    /// three find axes). Empty needle → empty. The single derivation the
    /// highlight paint, the match count, and the navigation all read, so they
    /// never disagree.
    #[must_use]
    pub fn find_matches(&self) -> Vec<(usize, usize)> {
        let query = self.find_query.get();
        let text = self.text.get();
        let cs = self.find_case_sensitive.get();
        let ww = self.find_whole_word.get();
        find_matches_in(&text, &query, cs, ww)
    }

    /// R903 §5.22 — number of current matches (reactive read).
    #[must_use]
    pub fn find_match_count(&self) -> usize {
        self.find_matches().len()
    }

    /// R903 §5.22 — zero-based index of the match the selection currently
    /// coincides with, or `None` when the selection is not exactly on a match
    /// (no search active, caret-only, or an arbitrary selection). Powers the
    /// "{n} of {N}" status; reactive read.
    #[must_use]
    pub fn find_current_index(&self) -> Option<usize> {
        let sel = self.selection_range()?;
        self.find_matches().iter().position(|&m| m == sel)
    }

    /// R903 §5.22 — select the next match at or after the current selection end
    /// (or caret when there is no selection), wrapping to the first match past
    /// the end of the buffer. Returns the selected range, or `None` when there
    /// are no matches. The textbook find-next: the selection *is* the cursor,
    /// so repeated calls walk every match and loop.
    pub fn find_next(&self) -> Option<(usize, usize)> {
        let matches = self.find_matches();
        let first = *matches.first()?;
        let from = self
            .selection_range()
            .map_or_else(|| self.caret_pos.get(), |(_, end)| end);
        let hit = matches
            .iter()
            .copied()
            .find(|&(start, _)| start >= from)
            .unwrap_or(first);
        self.set_selection(hit.0, hit.1);
        Some(hit)
    }

    /// R903 §5.22 — mirror of [`find_next`](Self::find_next): select the
    /// previous match ending at or before the current selection start (or
    /// caret), wrapping to the last match.
    pub fn find_prev(&self) -> Option<(usize, usize)> {
        let matches = self.find_matches();
        let last = *matches.last()?;
        let from = self
            .selection_range()
            .map_or_else(|| self.caret_pos.get(), |(start, _)| start);
        let hit = matches
            .iter()
            .copied()
            .rev()
            .find(|&(_, end)| end <= from)
            .unwrap_or(last);
        self.set_selection(hit.0, hit.1);
        Some(hit)
    }

    /// R903 §5.22 — replace the current match (when the selection sits exactly
    /// on one) and advance to the next, the VS Code "Replace" gesture. Returns
    /// `true` when a replacement happened. When the selection is **not** on a
    /// match, this selects the next match instead (returns `false`) so the
    /// following Replace acts on it — the canonical two-press "find then
    /// replace" flow.
    pub fn replace_current(&self, replacement: &str) -> bool {
        if let Some(sel) = self.selection_range() {
            if self.find_matches().contains(&sel) {
                self.replace_range(sel.0, sel.1, replacement);
                self.find_next();
                return true;
            }
        }
        self.find_next();
        false
    }

    /// R903 §5.22 §5.52 — replace **every** match with `replacement` as one
    /// undo step, returning the count replaced (0 leaves the buffer and the
    /// timeline untouched). The matches are spliced last-to-first so each
    /// earlier match's byte range stays valid as later text shifts, and the run
    /// is bracketed by an [`UndoStack`](crate::undo::UndoStack) macro
    /// ([`begin_macro`](crate::undo::UndoStack::begin_macro) /
    /// [`end_macro`](crate::undo::UndoStack::end_macro)) so a single Ctrl+Z
    /// reverses the whole batch — the first consumer of the macro axis the undo
    /// substrate reserved.
    pub fn replace_all(&self, replacement: &str) -> usize {
        let matches = self.find_matches();
        if matches.is_empty() {
            return 0;
        }
        let count = matches.len();
        let stack = self.undo_stack();
        if let Some(stack) = &stack {
            stack.begin_macro("Replace all");
        }
        for &(start, end) in matches.iter().rev() {
            self.replace_range(start, end, replacement);
        }
        if let Some(stack) = &stack {
            stack.end_macro();
        }
        count
    }

    /// R903 §5.22 — splice `[start, end)` to `s` as one undo step labelled
    /// "Replace". Unlike [`insert`](Self::insert) (which early-returns on an
    /// empty string), this drains the range even when `s` is empty, so
    /// "replace with nothing" deletes the match. The single replace primitive
    /// [`replace_current`](Self::replace_current) and
    /// [`replace_all`](Self::replace_all) share.
    pub fn replace_range(&self, start: usize, end: usize, s: &str) {
        self.record_edit(CoalesceGroup::Boundary, false, "Replace", || {
            self.splice_inner(start, end, s);
        });
    }

    /// R903 §5.22 — apply one `[start, end) -> s` splice to the buffer
    /// (offsets clamped to `char` boundaries), maintaining the style runs
    /// (clip the deleted span, shift the insert) and collapsing the selection,
    /// in one [`batch`]. The bare mutation [`replace_range`](Self::replace_range)
    /// journals.
    fn splice_inner(&self, start: usize, end: usize, s: &str) {
        self.goal_column.set(None);
        let mut buf = self.text.get();
        let start = clamp_to_char_boundary(&buf, start.min(buf.len()));
        let end = clamp_to_char_boundary(&buf, end.min(buf.len())).max(start);
        buf.replace_range(start..end, s);
        let new_caret = start + s.len();
        let mut runs = self.style_runs.get();
        clip_runs_for_delete(&mut runs, start, end);
        shift_runs_for_insert(&mut runs, start, s.len());
        batch(|| {
            self.text.set(buf);
            self.caret_pos.set(new_caret);
            self.selection_anchor.set(None);
            self.style_runs.set(runs);
        });
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
        self.record_edit(CoalesceGroup::Boundary, false, "Replace text", || {
            self.set_text_inner(new_text);
        });
    }

    /// R878 §5.22 — replace the buffer and park the caret at the **end**:
    /// the programmatic "seed an inline editor" sequence (open-at-the-end,
    /// ready to append / Backspace from the trailing edge — the todomvc
    /// R664 begin-edit UX). [`set_text`](Self::set_text) alone clamps the
    /// caret to its *previous* offset (`0` on a first edit, a stale
    /// mid-string offset on later ones), so every seeding binding needed
    /// this exact `set_text` + `set_caret(len)` pair — hand-rolled in 9+
    /// sites before R878 lifted the decision here.
    pub fn seed(&self, text: String) {
        let len = text.len();
        self.set_text(text);
        self.set_caret(len);
    }

    fn set_text_inner(&self, new_text: String) {
        self.goal_column.set(None);
        let new_len = new_text.len();
        let cur_caret = self.caret_pos.get();
        let clamped_caret = clamp_to_char_boundary(&new_text, cur_caret.min(new_len));
        batch(|| {
            self.text.set(new_text);
            self.caret_pos.set(clamped_caret);
            self.selection_anchor.set(None);
            self.preedit_buffer.set(None);
            // R767 — a wholesale text replace invalidates every byte
            // offset the runs referenced; clear them (the caller re-seeds
            // via set_style_runs if the new text is styled).
            self.style_runs.set(Vec::new());
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
        // A selection-replacing insert is its own undo step (boundary); a
        // plain insert coalesces with the typing run unless it is whitespace
        // (which ends the word so the next keystroke opens a fresh step).
        let (group, coalescable) = if self.has_selection() {
            (CoalesceGroup::Boundary, false)
        } else {
            (CoalesceGroup::Insert, !s.chars().any(char::is_whitespace))
        };
        self.record_edit(group, coalescable, "Type", || self.insert_inner(s));
    }

    fn insert_inner(&self, s: &str) {
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
            let mut runs = self.style_runs.get();
            clip_runs_for_delete(&mut runs, start, end);
            shift_runs_for_insert(&mut runs, start, s.len());
            batch(|| {
                self.text.set(buf);
                self.caret_pos.set(new_caret);
                self.selection_anchor.set(None);
                self.style_runs.set(runs);
            });
            return;
        }
        let snapped = clamp_to_char_boundary(&buf, caret);
        buf.insert_str(snapped, s);
        let new_caret = snapped + s.len();
        let mut runs = self.style_runs.get();
        shift_runs_for_insert(&mut runs, snapped, s.len());
        batch(|| {
            self.text.set(buf);
            self.caret_pos.set(new_caret);
            self.style_runs.set(runs);
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
        let group = if self.has_selection() {
            CoalesceGroup::Boundary
        } else {
            CoalesceGroup::DeleteBack
        };
        self.record_edit(group, group == CoalesceGroup::DeleteBack, "Delete", || {
            self.backspace_inner();
        });
    }

    fn backspace_inner(&self) {
        self.goal_column.set(None);
        let mut buf = self.text.get();
        let caret = self.caret_pos.get().min(buf.len());
        if let Some((start, end)) = self.selection_range_against(&buf, caret) {
            buf.drain(start..end);
            let mut runs = self.style_runs.get();
            clip_runs_for_delete(&mut runs, start, end);
            batch(|| {
                self.text.set(buf);
                self.caret_pos.set(start);
                self.selection_anchor.set(None);
                self.style_runs.set(runs);
            });
            return;
        }
        if caret == 0 {
            return;
        }
        let prev = prev_char_boundary(&buf, caret);
        buf.drain(prev..caret);
        let mut runs = self.style_runs.get();
        clip_runs_for_delete(&mut runs, prev, caret);
        batch(|| {
            self.text.set(buf);
            self.caret_pos.set(prev);
            self.style_runs.set(runs);
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
        let group = if self.has_selection() {
            CoalesceGroup::Boundary
        } else {
            CoalesceGroup::DeleteForward
        };
        self.record_edit(
            group,
            group == CoalesceGroup::DeleteForward,
            "Delete forward",
            || self.delete_forward_inner(),
        );
    }

    fn delete_forward_inner(&self) {
        self.goal_column.set(None);
        let mut buf = self.text.get();
        let caret = self.caret_pos.get().min(buf.len());
        if let Some((start, end)) = self.selection_range_against(&buf, caret) {
            buf.drain(start..end);
            let mut runs = self.style_runs.get();
            clip_runs_for_delete(&mut runs, start, end);
            batch(|| {
                self.text.set(buf);
                self.caret_pos.set(start);
                self.selection_anchor.set(None);
                self.style_runs.set(runs);
            });
            return;
        }
        if caret >= buf.len() {
            return;
        }
        let next = next_char_boundary(&buf, caret);
        buf.drain(caret..next);
        let mut runs = self.style_runs.get();
        clip_runs_for_delete(&mut runs, caret, next);
        // Text + runs both change: batch the two `Signal::set`s so a
        // subscriber re-runs once (the R55.G.24 atomic-multi-axis
        // contract — runs maintenance promoted this from a single write).
        batch(|| {
            self.text.set(buf);
            self.style_runs.set(runs);
        });
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

/// R767 §5.36 §5.22 — shift styled runs for an insertion of `len` bytes
/// at byte offset `at` (the [`TextEditState::insert`] maintenance step).
///
/// Bytes at or after `at` move right by `len`. A run that *contains*
/// `at` grows to include the inserted text (typing inside a styled span
/// inherits it); a run that *starts* at `at` is pushed right (the
/// insertion sits before it, so it does not inherit) — the canonical
/// "insert inherits the run to its left" affinity, expressed by the
/// symmetric `>= at` test on both endpoints.
fn shift_runs_for_insert(runs: &mut [StyleRun], at: usize, len: usize) {
    let (at, len) = (
        u32::try_from(at).unwrap_or(u32::MAX),
        u32::try_from(len).unwrap_or(0),
    );
    for r in runs.iter_mut() {
        if r.start >= at {
            r.start = r.start.saturating_add(len);
        }
        if r.end >= at {
            r.end = r.end.saturating_add(len);
        }
    }
}

/// R767 §5.36 §5.22 — clip styled runs against a deletion of the byte
/// range `[start, end)` (the backspace / delete / selection-drain
/// maintenance step). Each run endpoint is interval-subtracted: an
/// endpoint before the range is untouched, one inside is clamped to the
/// range start, one after shifts left by the deleted length. Runs that
/// collapse to empty (fully inside the deleted range) are dropped — a
/// styled span tracks its text, and deleting all of it removes the span.
fn clip_runs_for_delete(runs: &mut Vec<StyleRun>, start: usize, end: usize) {
    let (a, b) = (
        u32::try_from(start).unwrap_or(u32::MAX),
        u32::try_from(end).unwrap_or(u32::MAX),
    );
    let d = b.saturating_sub(a);
    let clip = |p: u32| {
        if p <= a {
            p
        } else if p >= b {
            p - d
        } else {
            a
        }
    };
    for r in runs.iter_mut() {
        r.start = clip(r.start);
        r.end = clip(r.end);
    }
    runs.retain(|r| r.start < r.end);
}

/// R768 §5.36 §5.22 — subtract the byte range `[start, end)` from the
/// styled-run list's *style coverage* (the "clear formatting over a
/// range" core, and the carve-a-hole first half of [`overlay_style_run`]).
///
/// Unlike [`clip_runs_for_delete`] — which models a *text* deletion and
/// pulls every trailing run left by the removed length — this leaves the
/// text (and therefore all offsets outside the range) untouched and only
/// strips the styling inside `[start, end)`. A run straddling a boundary
/// is split: the part before `start` and the part at-or-after `end`
/// survive; the covered middle is dropped. Runs fully inside vanish; runs
/// fully outside are kept verbatim. An empty / inverted range is a no-op.
fn subtract_style_range(runs: &mut Vec<StyleRun>, start: u32, end: u32) {
    if start >= end {
        return;
    }
    let mut out = Vec::with_capacity(runs.len() + 1);
    for r in runs.drain(..) {
        if r.end <= start || r.start >= end {
            out.push(r); // disjoint from the cleared range
        } else {
            if r.start < start {
                out.push(StyleRun::new(r.start, start, r.style.clone()));
            }
            if r.end > end {
                out.push(StyleRun::new(end, r.end, r.style));
            }
            // the [max(start, r.start), min(end, r.end)) middle is dropped
        }
    }
    *runs = out;
}

/// R768 §5.36 §5.22 — coalesce adjacent runs that carry an identical
/// style into one span. Assumes `runs` is sorted ascending by `start`
/// and non-overlapping (the [`overlay_style_run`] post-condition). Two
/// runs merge when the first ends exactly where the second begins and
/// their styles compare equal — the canonical `FormatRange`
/// normalisation that keeps the list minimal, so re-applying the same
/// colour across a former split leaves no redundant seam.
fn merge_adjacent_runs(runs: &mut Vec<StyleRun>) {
    let mut i = 0;
    while i + 1 < runs.len() {
        if runs[i].end == runs[i + 1].start && runs[i].style == runs[i + 1].style {
            let merged_end = runs[i + 1].end;
            runs[i].end = merged_end;
            runs.remove(i + 1);
        } else {
            i += 1;
        }
    }
}

/// R768 §5.36 §5.22 — overlay one [`StyleRun`] onto the list (Qt
/// `QTextCursor::setCharFormat` semantics): the bytes in `new`'s range
/// take `new`'s style wholesale, every previously-styled byte outside it
/// is preserved. Implemented as [`subtract_style_range`] (carve a hole
/// for `new`) + insert + sort-by-start + [`merge_adjacent_runs`], so the
/// list stays ordered, non-overlapping, and minimal. An empty / inverted
/// `new` range is a no-op.
fn overlay_style_run(runs: &mut Vec<StyleRun>, new: StyleRun) {
    if new.start >= new.end {
        return;
    }
    subtract_style_range(runs, new.start, new.end);
    runs.push(new);
    runs.sort_by_key(|r| r.start);
    merge_adjacent_runs(runs);
}

/// R769 §5.36 §5.22 — merge a per-field style transform over the byte
/// range `[start, end)` (Qt `QTextCursor::mergeCharFormat` semantics):
/// every byte keeps its other styling and only the field(s) `mutate`
/// touches change. A byte already covered by a run has `mutate` applied
/// to *that run's* resolved style; an uncovered byte resolves against
/// `base` (the field's default char format — the style unstyled text
/// paints with) before `mutate`, so e.g. bolding plain coloured text
/// yields `base.fg + bold` rather than dropping the colour. The
/// `[start, end)` span is rebuilt from its (possibly several) effective
/// sub-spans, then abutting identical results coalesce. Unlike
/// [`overlay_style_run`] (wholesale `setCharFormat`), this preserves each
/// sub-span's untouched fields. Empty / inverted range is a no-op.
fn field_merge_runs(
    runs: &mut Vec<StyleRun>,
    start: u32,
    end: u32,
    base: &TextStyle,
    mutate: impl Fn(&mut TextStyle),
) {
    if start >= end {
        return;
    }
    runs.sort_by_key(|r| r.start);
    // Split [start, end) into effective sub-spans: a covered slice takes
    // its run's style, a gap takes `base`. `runs` is non-overlapping +
    // sorted, so `cursor` walks left-to-right without backtracking.
    let mut pieces: Vec<StyleRun> = Vec::new();
    let mut cursor = start;
    for r in runs.iter() {
        if r.end <= start || r.start >= end {
            continue;
        }
        let lo = r.start.max(start);
        let hi = r.end.min(end);
        if cursor < lo {
            pieces.push(StyleRun::new(cursor, lo, base.clone()));
        }
        pieces.push(StyleRun::new(lo, hi, r.style.clone()));
        cursor = hi;
    }
    if cursor < end {
        pieces.push(StyleRun::new(cursor, end, base.clone()));
    }
    for p in &mut pieces {
        mutate(&mut p.style);
    }
    subtract_style_range(runs, start, end);
    runs.extend(pieces);
    runs.sort_by_key(|r| r.start);
    merge_adjacent_runs(runs);
}

#[cfg(test)]
mod tests {
    //! R56.1.b §5.38 §5.22 — `TextEditState` regression battery.
    //! Covers ASCII edits, multi-byte UTF-8 caret navigation, atomic
    //! batched-multi-axis subscriber semantics, and the `Owner::cache`
    //! hook integration.

    use super::{
        clamp_to_char_boundary, find_matches_in, next_char_boundary, prev_char_boundary,
        use_text_edit_state, TextEditState,
    };
    use crate::reactive::{Effect, Owner};
    use crate::scene::StyleRun;
    use crate::style::{Color, TextStyle};
    use std::cell::Cell;
    use std::rc::Rc;

    /// R767 — a default-styled run over `[s, e)` for the maintenance
    /// battery (the style itself is irrelevant to shift / clip).
    fn run(s: u32, e: u32) -> StyleRun {
        StyleRun::new(s, e, TextStyle::default())
    }

    /// `(start, end)` pairs of a state's runs, for terse assertions.
    fn run_spans(st: &TextEditState) -> Vec<(u32, u32)> {
        st.style_runs().iter().map(|r| (r.start, r.end)).collect()
    }

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
    fn r878_seed_replaces_text_and_parks_caret_at_the_end() {
        let s = TextEditState::new();
        s.seed("Multiply".to_owned());
        assert_eq!(s.text(), "Multiply");
        assert_eq!(s.caret(), 8, "caret parks at the trailing edge");
        // A re-seed after a mid-string caret still lands at the end —
        // the stale-previous-caret trap `set_text` alone has.
        s.set_caret(2);
        s.seed("Color".to_owned());
        assert_eq!(s.caret(), 5, "re-seed re-parks at the new end");
        // Multi-byte: the end is a byte offset on a char boundary.
        s.seed("caf\u{e9}".to_owned());
        assert_eq!(s.caret(), 5, "UTF-8 end offset is byte-exact");
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

    // ─────────────────────────────────────────────────────────────
    // R767 §5.36 §5.22 — styled-run edit maintenance (FormatRange)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r767_insert_before_run_shifts_it_right() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.set_style_runs(vec![run(6, 11)]); // "world"
        s.set_caret(0);
        s.insert("XX"); // 2 bytes at the buffer start
        assert_eq!(run_spans(&s), vec![(8, 13)], "a run after the insert shifts right by len");
    }

    #[test]
    fn r767_insert_inside_run_grows_it() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![run(0, 5)]);
        s.set_caret(2);
        s.insert("XX"); // inside the run
        assert_eq!(run_spans(&s), vec![(0, 7)], "typing inside a styled span extends it");
    }

    #[test]
    fn r767_insert_at_run_end_inherits_left() {
        let s = TextEditState::with_initial("ab".to_owned());
        s.set_style_runs(vec![run(0, 2)]);
        s.set_caret(2); // at the run end
        s.insert("X");
        assert_eq!(run_spans(&s), vec![(0, 3)], "insert at a run's end extends it (inherit-left)");
    }

    #[test]
    fn r767_insert_at_run_start_does_not_inherit() {
        let s = TextEditState::with_initial("ab".to_owned());
        s.set_style_runs(vec![run(0, 2)]);
        s.set_caret(0); // at the run start
        s.insert("X");
        assert_eq!(
            run_spans(&s),
            vec![(1, 3)],
            "insert at a run's start pushes it right (the new text is outside the span)",
        );
    }

    #[test]
    fn r767_backspace_inside_run_shrinks_it() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![run(0, 5)]);
        s.set_caret(5);
        s.backspace(); // delete byte [4, 5)
        assert_eq!(s.text(), "hell");
        assert_eq!(run_spans(&s), vec![(0, 4)], "deleting a byte inside a run shrinks its end");
    }

    #[test]
    fn r767_deleting_a_whole_run_drops_it() {
        let s = TextEditState::with_initial("ab".to_owned());
        s.set_style_runs(vec![run(0, 2)]);
        s.set_selection(0, 2);
        s.delete_forward(); // drains the whole run's text
        assert!(s.style_runs().is_empty(), "a run whose text is fully deleted is dropped");
    }

    #[test]
    fn r767_delete_spanning_two_runs_clips_both() {
        // "aabbcc": run0 "aa" [0,2), run1 "cc" [4,6). Delete [1,5)
        // ("abbc") -> text "ac", run0 clips to [0,1), run1 to [1,2).
        let s = TextEditState::with_initial("aabbcc".to_owned());
        s.set_style_runs(vec![run(0, 2), run(4, 6)]);
        s.set_selection(1, 5);
        s.delete_forward();
        assert_eq!(s.text(), "ac");
        assert_eq!(
            run_spans(&s),
            vec![(0, 1), (1, 2)],
            "a delete spanning two runs clips the first and shifts/clips the second",
        );
    }

    #[test]
    fn r767_type_to_replace_selection_maintains_runs() {
        // Replace the styled "world" with "WORLD!" (drain then insert at
        // the same offset): the run clips to empty over the drained span,
        // then the inserted text shifts the trailing run.
        let s = TextEditState::with_initial("hi world end".to_owned());
        s.set_style_runs(vec![run(3, 8), run(9, 12)]); // "world", "end"
        s.set_selection(3, 8);
        s.insert("WORLD!"); // 6 bytes replace the 5-byte "world"
        assert_eq!(s.text(), "hi WORLD! end");
        // run0 ("world") drained to empty -> dropped; "end" run shifts +1
        // (net delta +1 from the 5->6 replace) to [10, 13).
        assert_eq!(run_spans(&s), vec![(10, 13)], "trailing run tracks the net length change");
    }

    #[test]
    fn r767_set_text_clears_runs() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![run(0, 5)]);
        s.set_text("brand new".to_owned());
        assert!(s.style_runs().is_empty(), "a wholesale set_text clears the now-invalid runs");
    }

    // ─────────────────────────────────────────────────────────────
    // R768 §5.36 §5.22 — apply / clear styled runs over a range
    // (rich-text formatting; setCharFormat + clearFormat semantics)
    // ─────────────────────────────────────────────────────────────

    /// A colour-distinct run over `[s, e)` so merge / split tests can
    /// tell two spans apart by style (the default-styled [`run`] helper
    /// cannot distinguish adjacent spans for the merge assertions).
    fn crun(s: u32, e: u32, rgb: (u8, u8, u8)) -> StyleRun {
        StyleRun::new(s, e, TextStyle::new().with_fg(Color::rgb(rgb.0, rgb.1, rgb.2)))
    }

    const RED: (u8, u8, u8) = (0xD0, 0x28, 0x28);
    const BLUE: (u8, u8, u8) = (0x26, 0x4C, 0xD8);

    #[test]
    fn r768_apply_over_unstyled_gap_adds_a_run() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.apply_style_run(6, 11, TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)));
        assert_eq!(run_spans(&s), vec![(6, 11)], "applying over plain text adds one run");
    }

    #[test]
    fn r768_apply_inside_a_run_splits_it_into_three() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.set_style_runs(vec![crun(0, 11, RED)]);
        // Recolour the middle "lo wo" → red | blue | red.
        s.apply_style_run(3, 8, TextStyle::new().with_fg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)));
        assert_eq!(
            run_spans(&s),
            vec![(0, 3), (3, 8), (8, 11)],
            "overlaying inside a run carves it into before | new | after",
        );
        let runs = s.style_runs();
        assert_eq!(runs[1].style.fg_color, Color::rgb(BLUE.0, BLUE.1, BLUE.2), "middle is the new ink");
        assert_eq!(runs[0].style.fg_color, Color::rgb(RED.0, RED.1, RED.2), "flanks keep the old ink");
    }

    #[test]
    fn r768_apply_same_style_to_adjacent_merges() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        // Apply the identical red to the abutting "[5, 11)" — the seam
        // dissolves into one span (FormatRange minimisation).
        s.apply_style_run(5, 11, TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)));
        assert_eq!(run_spans(&s), vec![(0, 11)], "adjacent identical styles coalesce");
    }

    #[test]
    fn r768_apply_different_style_to_adjacent_keeps_two() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        s.apply_style_run(5, 11, TextStyle::new().with_fg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)));
        assert_eq!(run_spans(&s), vec![(0, 5), (5, 11)], "abutting distinct styles stay separate");
    }

    #[test]
    fn r768_apply_overlapping_existing_runs_replaces_coverage() {
        let s = TextEditState::with_initial("hello world!!".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED), crun(6, 11, BLUE)]);
        // A wide blue overlay swallows the red run and the gap; the
        // trailing tail of the old blue run survives, then merges.
        s.apply_style_run(2, 9, TextStyle::new().with_fg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)));
        assert_eq!(
            run_spans(&s),
            vec![(0, 2), (2, 11)],
            "overlay carves the red run and merges with the abutting blue tail",
        );
    }

    #[test]
    fn r768_clear_inside_a_run_splits_without_shifting() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.set_style_runs(vec![crun(0, 11, RED)]);
        s.clear_style_runs(3, 8);
        assert_eq!(
            run_spans(&s),
            vec![(0, 3), (8, 11)],
            "clearing a middle range splits the run and drops the covered styling (no offset shift)",
        );
        assert_eq!(s.text(), "hello world", "clear-formatting never edits the text");
    }

    #[test]
    fn r768_clear_whole_coverage_empties_runs() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        s.clear_style_runs(0, 5);
        assert!(s.style_runs().is_empty(), "clearing a run's full extent drops it");
    }

    #[test]
    fn r768_apply_empty_range_is_a_noop() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        s.apply_style_run(3, 3, TextStyle::new().with_fg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)));
        assert_eq!(run_spans(&s), vec![(0, 5)], "a collapsed range leaves the runs untouched");
    }

    #[test]
    fn r768_apply_clamps_out_of_range_bytes() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.apply_style_run(2, 999, TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)));
        assert_eq!(run_spans(&s), vec![(2, 5)], "end clamps to text.len()");
    }

    // ─────────────────────────────────────────────────────────────
    // R769 §5.36 §5.22 — field-level merge (mergeCharFormat) + style_at
    // ─────────────────────────────────────────────────────────────

    /// A plausible field base char format: 16px text in the given ink.
    fn base_ink(rgb: (u8, u8, u8)) -> TextStyle {
        TextStyle::new().with_fg(Color::rgb(rgb.0, rgb.1, rgb.2))
    }

    const INK_BASE: (u8, u8, u8) = (0x10, 0x10, 0x10);

    #[test]
    fn r769_merge_bold_over_run_keeps_its_colour() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        s.merge_style_run(0, 5, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::BOLD;
        });
        let runs = s.style_runs();
        assert_eq!(run_spans(&s), vec![(0, 5)], "the run span is preserved");
        assert_eq!(runs[0].style.fg_color, Color::rgb(RED.0, RED.1, RED.2), "colour kept");
        assert_eq!(runs[0].style.font_weight, crate::style::FontWeight::BOLD, "now bold");
    }

    #[test]
    fn r769_merge_bold_over_unstyled_uses_base() {
        let s = TextEditState::with_initial("hello world".to_owned());
        // No runs: the [6,11) "world" is unstyled → resolves against base.
        s.merge_style_run(6, 11, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::BOLD;
        });
        let runs = s.style_runs();
        assert_eq!(run_spans(&s), vec![(6, 11)], "a run materialises over the bolded gap");
        assert_eq!(runs[0].style.fg_color, Color::rgb(INK_BASE.0, INK_BASE.1, INK_BASE.2), "base ink");
        assert_eq!(runs[0].style.font_weight, crate::style::FontWeight::BOLD, "base + bold");
    }

    #[test]
    fn r769_merge_over_mixed_coverage_preserves_each_subspan() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]); // "hello" red, " world" unstyled
        // Bold [3, 8): covered [3,5) keeps red, gap [5,8) takes base.
        s.merge_style_run(3, 8, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::BOLD;
        });
        assert_eq!(run_spans(&s), vec![(0, 3), (3, 5), (5, 8)], "split into normal | red-bold | base-bold");
        let runs = s.style_runs();
        assert_eq!(runs[0].style.font_weight, crate::style::FontWeight::NORMAL, "untouched flank stays normal");
        assert_eq!(runs[1].style.fg_color, Color::rgb(RED.0, RED.1, RED.2), "covered slice keeps red");
        assert_eq!(runs[1].style.font_weight, crate::style::FontWeight::BOLD, "covered slice is bold");
        assert_eq!(runs[2].style.fg_color, Color::rgb(INK_BASE.0, INK_BASE.1, INK_BASE.2), "gap took base ink");
        assert_eq!(runs[2].style.font_weight, crate::style::FontWeight::BOLD, "gap is bold");
    }

    #[test]
    fn r769_merge_pieces_with_equal_result_coalesce() {
        let s = TextEditState::with_initial("hello".to_owned());
        // Two abutting same-colour runs (set_style_runs does not merge).
        s.set_style_runs(vec![crun(0, 2, RED), crun(2, 5, RED)]);
        s.merge_style_run(0, 5, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::BOLD;
        });
        assert_eq!(run_spans(&s), vec![(0, 5)], "identical bolded pieces coalesce into one span");
    }

    #[test]
    fn r769_merge_is_reversible_round_trips_to_original() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        let original = s.style_runs();
        s.merge_style_run(0, 5, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::BOLD;
        });
        s.merge_style_run(0, 5, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::NORMAL;
        });
        assert_eq!(s.style_runs(), original, "bold then un-bold returns the exact original runs");
    }

    #[test]
    fn r769_merge_italic_independent_of_weight() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        s.merge_style_run(0, 5, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::BOLD;
        });
        s.merge_style_run(0, 5, &base_ink(INK_BASE), |st| {
            st.font_style = crate::style::FontStyle::Italic;
        });
        let runs = s.style_runs();
        assert_eq!(runs[0].style.font_weight, crate::style::FontWeight::BOLD, "weight survives a later italic merge");
        assert_eq!(runs[0].style.font_style, crate::style::FontStyle::Italic, "italic applied");
        assert_eq!(runs[0].style.fg_color, Color::rgb(RED.0, RED.1, RED.2), "colour survives both merges");
    }

    #[test]
    fn r769_style_at_reports_covering_run_or_none() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        assert_eq!(
            s.style_at(2).map(|st| st.fg_color),
            Some(Color::rgb(RED.0, RED.1, RED.2)),
            "a covered byte reports its run style",
        );
        assert!(s.style_at(7).is_none(), "an unstyled byte reports None");
        assert!(s.style_at(5).is_none(), "the exclusive end byte is outside the run");
    }

    #[test]
    fn r769_merge_empty_range_is_a_noop() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        let before = s.style_runs();
        s.merge_style_run(3, 3, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::BOLD;
        });
        assert_eq!(s.style_runs(), before, "a collapsed range leaves the runs untouched");
    }

    #[test]
    fn r768_apply_runs_only_mutation_repaints_via_signal() {
        // A formatting change with no text edit still notifies the
        // style_runs Signal subscribers (the runs-only repaint path).
        let owner = Owner::new();
        let s = Rc::new(TextEditState::with_initial("hello".to_owned()));
        let runs_seen = Rc::new(Cell::new(0usize));
        let st = Rc::clone(&s);
        let seen = Rc::clone(&runs_seen);
        let _e = Effect::new(&owner, move || {
            let _ = st.style_runs();
            seen.set(seen.get() + 1);
        });
        let before = runs_seen.get();
        s.apply_style_run(0, 5, TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)));
        assert!(runs_seen.get() > before, "a runs-only apply re-runs a style_runs subscriber");
    }

    // R796 §5.52 — undo / redo with word-level coalescing.

    fn undoable() -> TextEditState {
        let st = TextEditState::new();
        st.attach_undo(Rc::new(crate::undo::UndoStack::new()));
        st
    }

    #[test]
    fn r796_unattached_undo_is_a_noop() {
        Owner::new().run(|| {
            let st = TextEditState::new();
            st.insert("hi");
            assert!(st.undo_stack().is_none());
            assert!(!st.undo(), "no stack attached -> undo is a no-op");
            assert_eq!(st.text(), "hi", "the edit stands; nothing was journalled");
        });
    }

    #[test]
    fn r796_type_coalesces_into_one_undo_step() {
        Owner::new().run(|| {
            let st = undoable();
            st.insert("a");
            st.insert("b");
            assert_eq!(st.text(), "ab");
            assert!(st.undo());
            assert_eq!(st.text(), "", "one undo reverses the whole typing run");
            assert_eq!(st.caret(), 0, "undo restores the caret too");
            assert!(st.redo());
            assert_eq!(st.text(), "ab", "redo re-applies the coalesced run");
        });
    }

    #[test]
    fn r796_whitespace_ends_the_word() {
        Owner::new().run(|| {
            let st = undoable();
            for s in ["a", "b", " ", "c", "d"] {
                st.insert(s);
            }
            assert_eq!(st.text(), "ab cd");
            assert!(st.undo());
            assert_eq!(st.text(), "ab ", "the second word undoes on its own");
            assert!(st.undo());
            assert_eq!(st.text(), "", "the first word + space is the prior step");
        });
    }

    #[test]
    fn r796_caret_move_breaks_the_run() {
        Owner::new().run(|| {
            let st = undoable();
            st.insert("ab");
            st.move_left();
            st.insert("x");
            assert_eq!(st.text(), "axb");
            assert!(st.undo());
            assert_eq!(st.text(), "ab", "only the post-move insert undoes (caret move broke coalescing)");
        });
    }

    #[test]
    fn r796_backspace_is_a_separate_run_from_typing() {
        Owner::new().run(|| {
            let st = undoable();
            st.insert("abc");
            st.backspace();
            assert_eq!(st.text(), "ab");
            assert!(st.undo());
            assert_eq!(st.text(), "abc", "the delete is its own step (different group)");
            assert!(st.undo());
            assert_eq!(st.text(), "", "the typing run is the prior step");
        });
    }

    #[test]
    fn r796_selection_replace_is_one_step() {
        Owner::new().run(|| {
            let st = undoable();
            st.insert("abc");
            st.set_selection(0, 3);
            st.insert("X");
            assert_eq!(st.text(), "X");
            assert!(st.undo());
            assert_eq!(st.text(), "abc", "type-to-replace undoes back to the selected text in one step");
        });
    }

    // R796.1 §5.52 — granular run reversal: undo must restore style runs
    // byte-exact, not just the text (the risky half of granular undo).

    fn styled(text: &str, runs: Vec<StyleRun>) -> TextEditState {
        let st = TextEditState::new();
        st.set_text(text.to_string());
        st.set_style_runs(runs);
        st.attach_undo(Rc::new(crate::undo::UndoStack::new())); // attach AFTER seeding
        st
    }
    fn red(s: u32, e: u32) -> StyleRun {
        StyleRun::new(s, e, TextStyle::new().with_fg(Color::rgb(0xD0, 0x28, 0x28)))
    }

    #[test]
    fn r796_1_undo_restores_a_clipped_run_exactly() {
        Owner::new().run(|| {
            let st = styled("abcdef", vec![red(0, 6)]);
            st.set_selection(2, 4);
            st.backspace(); // delete "cd" from inside the run
            assert_eq!(st.text(), "abef");
            assert_eq!(st.style_runs(), vec![red(0, 4)], "the run clips to the shorter text");
            assert!(st.undo());
            assert_eq!(st.text(), "abcdef");
            assert_eq!(st.style_runs(), vec![red(0, 6)], "undo restores the run span exactly");
        });
    }

    #[test]
    fn r796_1_undo_restores_a_fully_deleted_run() {
        Owner::new().run(|| {
            let st = styled("first second", vec![red(0, 5)]);
            st.set_selection(0, 5);
            st.backspace();
            assert_eq!(st.text(), " second");
            assert_eq!(st.style_runs(), Vec::new(), "a fully-deleted run drops");
            assert!(st.undo());
            assert_eq!(st.text(), "first second");
            assert_eq!(
                st.style_runs(),
                vec![red(0, 5)],
                "undo restores the dropped run from removed_runs",
            );
        });
    }

    #[test]
    fn r796_1_undo_reverses_the_run_shift_from_insert() {
        Owner::new().run(|| {
            let st = styled("abc", vec![red(0, 3)]);
            st.set_caret(0);
            st.insert("XY"); // shifts the run right by 2
            assert_eq!(st.text(), "XYabc");
            assert_eq!(st.style_runs(), vec![red(2, 5)], "insert before the run shifts it");
            assert!(st.undo());
            assert_eq!(st.text(), "abc");
            assert_eq!(st.style_runs(), vec![red(0, 3)], "undo shifts the run back");
        });
    }

    #[test]
    fn r796_1_consecutive_backspaces_coalesce() {
        Owner::new().run(|| {
            let st = undoable();
            st.insert("word");
            st.backspace();
            st.backspace();
            assert_eq!(st.text(), "wo");
            assert!(st.undo());
            assert_eq!(st.text(), "word", "two backspaces undo as one coalesced step");
            assert!(st.undo());
            assert_eq!(st.text(), "", "the typing run is the prior step");
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R903 §5.22 — find &amp; replace
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r903_find_matches_in_finds_non_overlapping_runs() {
        // "aaa" with needle "aa" yields ONE match (non-overlapping resume).
        assert_eq!(find_matches_in("aaa", "aa", true, false), vec![(0, 2)]);
        assert_eq!(
            find_matches_in("the cat sat", "at", true, false),
            vec![(5, 7), (9, 11)]
        );
        assert_eq!(find_matches_in("abc", "", true, false), Vec::new());
        assert_eq!(find_matches_in("abc", "xyz", true, false), Vec::new());
    }

    #[test]
    fn r903_find_matches_in_ascii_case_insensitive() {
        assert_eq!(
            find_matches_in("Cat cat CAT", "cat", false, false),
            vec![(0, 3), (4, 7), (8, 11)]
        );
        // Case-sensitive sees only the exact one.
        assert_eq!(
            find_matches_in("Cat cat CAT", "cat", true, false),
            vec![(4, 7)]
        );
    }

    #[test]
    fn r903_find_matches_in_whole_word() {
        // "cat" whole-word skips the "cat" inside "cats".
        assert_eq!(
            find_matches_in("cats cat scatter cat", "cat", true, true),
            vec![(5, 8), (17, 20)]
        );
    }

    #[test]
    fn r903_find_matches_in_multibyte_offsets_are_exact() {
        // A multi-byte prefix shifts the byte offset; the match range must land
        // on the source's own byte boundaries.
        let s = "\u{00e9}cole cole"; // "école cole" — 'é' is 2 bytes
        assert_eq!(find_matches_in(s, "cole", true, false), vec![(2, 6), (7, 11)]);
    }

    #[test]
    fn r903_find_next_walks_and_wraps() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("at bat hat".to_string());
            st.set_caret(0);
            st.set_find_query("at");
            assert_eq!(st.find_match_count(), 3);
            assert_eq!(st.find_next(), Some((0, 2)), "first match from caret 0");
            assert_eq!(st.selection_range(), Some((0, 2)));
            assert_eq!(st.find_current_index(), Some(0));
            assert_eq!(st.find_next(), Some((4, 6)), "advances past the selection");
            assert_eq!(st.find_next(), Some((8, 10)));
            assert_eq!(st.find_next(), Some((0, 2)), "wraps to the first");
        });
    }

    #[test]
    fn r903_find_prev_walks_and_wraps() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("at bat hat".to_string());
            st.set_caret(0);
            st.set_find_query("at");
            assert_eq!(st.find_prev(), Some((8, 10)), "from caret 0 wraps to last");
            assert_eq!(st.find_prev(), Some((4, 6)));
            assert_eq!(st.find_prev(), Some((0, 2)));
            assert_eq!(st.find_prev(), Some((8, 10)), "wraps again");
        });
    }

    #[test]
    fn r903_find_current_index_none_off_a_match() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("at bat".to_string());
            st.set_find_query("at");
            // Caret-only (no selection) is not on a match.
            st.set_caret(3);
            assert_eq!(st.find_current_index(), None);
            // An arbitrary selection that is not a match.
            st.set_selection(0, 5);
            assert_eq!(st.find_current_index(), None);
        });
    }

    #[test]
    fn r903_replace_current_replaces_then_advances() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("at bat hat".to_string());
            st.set_caret(0);
            st.set_find_query("at");
            // Not on a match yet: first press selects, replaces nothing.
            assert!(!st.replace_current("X"), "first press only selects");
            assert_eq!(st.selection_range(), Some((0, 2)));
            // On a match now: replace it, advance to next.
            assert!(st.replace_current("X"), "second press replaces");
            assert_eq!(st.text(), "X bat hat");
            assert_eq!(st.selection_range(), Some((3, 5)), "advanced to 'at' in bat");
        });
    }

    #[test]
    fn r903_replace_with_empty_deletes_the_match() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("foofoo".to_string());
            st.set_find_query("foo");
            assert_eq!(st.replace_all(""), 2);
            assert_eq!(st.text(), "", "replace-with-empty deletes every match");
        });
    }

    #[test]
    fn r903_replace_all_counts_and_rewrites() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("red green red blue red".to_string());
            st.set_find_query("red");
            assert_eq!(st.replace_all("X"), 3);
            assert_eq!(st.text(), "X green X blue X");
            // Idempotent: no more matches.
            assert_eq!(st.replace_all("X"), 0);
        });
    }

    #[test]
    fn r903_replace_all_is_one_undo_step() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a a a a".to_string());
            let stack = Rc::new(crate::undo::UndoStack::new());
            st.attach_undo(Rc::clone(&stack));
            st.set_find_query("a");
            assert_eq!(st.replace_all("bb"), 4);
            assert_eq!(st.text(), "bb bb bb bb");
            assert_eq!(stack.len(), 1, "four replacements folded into one step");
            assert!(st.undo());
            assert_eq!(st.text(), "a a a a", "single undo reverses the whole batch");
            assert!(st.redo());
            assert_eq!(st.text(), "bb bb bb bb", "single redo re-applies the batch");
        });
    }

    #[test]
    fn r903_replace_all_no_matches_touches_nothing() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("hello".to_string());
            let stack = Rc::new(crate::undo::UndoStack::new());
            st.attach_undo(Rc::clone(&stack));
            st.set_find_query("zzz");
            assert_eq!(st.replace_all("Q"), 0);
            assert_eq!(st.text(), "hello");
            assert_eq!(stack.len(), 0, "an empty replace-all records no undo step");
        });
    }

    #[test]
    fn r903_set_find_query_does_not_move_the_selection() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("at bat".to_string());
            st.set_caret(2);
            st.set_find_query("at"); // pure setter — no implicit navigation
            assert_eq!(st.caret(), 2, "setting the needle leaves the caret put");
            assert_eq!(st.selection_range(), None);
            assert_eq!(st.find_query(), "at");
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R904 §5.36 — syntax highlighter (derived style_runs)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r904_attach_highlighter_derives_style_runs_from_text() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("let x".to_string());
            // No highlighter → the manual (empty) runs.
            assert!(st.style_runs().is_empty(), "no highlighter → manual runs");
            st.attach_highlighter(std::rc::Rc::new(|t: &str| {
                crate::syntax::highlight_code(t, &["let"], crate::syntax::SyntaxPalette::classic(), 16)
            }));
            let runs = st.style_runs();
            assert_eq!(runs.len(), 1, "highlighter colours the 'let' keyword");
            assert_eq!((runs[0].start, runs[0].end), (0, 3));
        });
    }

    #[test]
    fn r904_highlighter_redrives_after_edit() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial(String::new());
            st.attach_highlighter(std::rc::Rc::new(|t: &str| {
                crate::syntax::highlight_code(t, &["fn"], crate::syntax::SyntaxPalette::classic(), 16)
            }));
            assert!(st.style_runs().is_empty(), "empty buffer → no tokens");
            st.insert("fn");
            assert_eq!(st.style_runs().len(), 1, "typing a keyword re-highlights it");
            // The shadowed manual runs stay empty under a highlighter.
            assert!(st.style_runs.get().is_empty(), "manual runs shadowed, not written");
        });
    }
}
