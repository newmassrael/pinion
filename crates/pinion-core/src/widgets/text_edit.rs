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
//! lives orthogonal to `text`: the canonical platform
//! IME contract (Wayland text-input-v3, macOS `NSTextInputContext`,
//! Windows TSF, GTK `IBus`) keeps preedit display in a separate
//! channel from the committed text so the application paint code
//! stitches the two together (`text[..caret] + preedit + text[caret..]`).
//! Four mutators drive the lifecycle: `preedit_start`,
//! `preedit_update`,
//! `preedit_commit`,
//! `preedit_cancel`. The mutators batch the
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
use std::collections::BTreeSet;
use std::rc::Rc;

use crate::reactive::{Owner, Signal, batch};
use crate::scene::StyleRun;
use crate::style::TextStyle;
use crate::undo::{UndoCommand, UndoStack};

/// R967 §5.36 — which single [`TextStyle`] field a [`TextEditState::toggle_format`]
/// flips (a `mergeCharFormat` toggle: it changes only this field and preserves
/// every other one of the covered run). The discriminator the AI-first
/// `toggle-format` RPC verb and the `hello-textarea` **B** / **I** toolbar share,
/// so the two channels flip identically (a divergence would be a bug, not a
/// style choice — the lift criterion).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatField {
    /// `font_weight` Bold (700) ↔ Normal (400).
    Bold,
    /// `font_style` Italic ↔ Normal.
    Italic,
    /// `underline` on ↔ off.
    Underline,
    /// `strikethrough` on ↔ off.
    Strikethrough,
}

impl FormatField {
    /// Whether `style` currently has this field "on". The Bold check matches
    /// the exact `FontWeight::BOLD` the toolbar reflective state reads, so the
    /// toggle direction agrees across the toolbar + RPC channels.
    #[must_use]
    pub fn is_on(self, style: &TextStyle) -> bool {
        match self {
            FormatField::Bold => style.font_weight == crate::style::FontWeight::BOLD,
            FormatField::Italic => style.font_style == crate::style::FontStyle::Italic,
            FormatField::Underline => style.decoration.underline,
            FormatField::Strikethrough => style.decoration.strikethrough,
        }
    }

    /// Set this field on / off, leaving every OTHER field of `style` untouched
    /// (the `mergeCharFormat` contract — toggling Bold keeps the run's colour).
    pub fn set(self, style: &mut TextStyle, on: bool) {
        match self {
            FormatField::Bold => {
                style.font_weight = if on {
                    crate::style::FontWeight::BOLD
                } else {
                    crate::style::FontWeight::NORMAL
                };
            }
            FormatField::Italic => {
                style.font_style = if on {
                    crate::style::FontStyle::Italic
                } else {
                    crate::style::FontStyle::Normal
                };
            }
            FormatField::Underline => style.decoration.underline = on,
            FormatField::Strikethrough => style.decoration.strikethrough = on,
        }
    }

    /// R967 — parse the AI-first `toggle-format` wire token (the lowercase field
    /// name); `None` for an unknown token.
    #[must_use]
    pub fn from_wire(token: &str) -> Option<Self> {
        match token {
            "bold" => Some(FormatField::Bold),
            "italic" => Some(FormatField::Italic),
            "underline" => Some(FormatField::Underline),
            "strikethrough" => Some(FormatField::Strikethrough),
            _ => None,
        }
    }
}

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
/// logical edit. Mirror of the R55.G.24 [`ScrollState`](crate::widgets::scroll::ScrollState) batched
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
    /// `Selection` maintains across `move_lines`
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
    /// current `text` byte buffer (the Qt `FormatRange`
    /// model — each run is a fully-resolved [`TextStyle`] over a UTF-8
    /// byte range). Empty (the default) is the single-style fast path;
    /// the field's paint threads the runs into the
    /// `layout_with_runs` shaping the visible caret /
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
    /// `text` / [`caret_pos`](Self::caret_pos) /
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
    ///    re-runs it. R767 edits also write `text`, so a
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
    /// [`TextStyle`] family (resolving the
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
    /// `Signal` (content peer of `text`) so the find-highlight
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
    /// R933 §5.36 — **code-folding** state: the set of *collapsed* fold
    /// regions, each keyed by its opening-bracket byte offset (the
    /// [`FoldRegion::open_byte`] anchor). Empty (the default) is "nothing
    /// folded". The foldable regions themselves are **derived on read**
    /// from the live buffer ([`fold_regions`](Self::fold_regions), the
    /// [`matching_bracket`](Self::matching_bracket) /
    /// [`find_matches`](Self::find_matches) re-derive lineage); only the
    /// collapsed *anchors* are stored, so the set survives edits the way a
    /// styled run tracks its text — an anchor whose `{` an edit deleted
    /// simply stops matching a derived region and is pruned on the next
    /// [`fold_regions`](Self::fold_regions) read.
    ///
    /// A reactive [`Signal`] — a content peer of [`style_runs`](Self::style_runs)
    /// / [`find_query`](Self::find_query): paint reads it to hide the
    /// collapsed lines and the `scene/<tag>/external/fold_regions` RPC
    /// exposes it, so a toggle must re-run the field paint. Reactive does
    /// **not** mean journalled: like the find session and the sort / filter
    /// / group view-state of the data widgets, folding is *view* config,
    /// not document content, so it is deliberately **outside** the
    /// [`undo`](Self::undo) journal (the Qt / Unreal convention — Ctrl+Z
    /// reverses edits, never a fold toggle). The journalled content is
    /// exactly `{text, caret, anchor, runs}` ([`TextEditCommand`]).
    folds: Signal<BTreeSet<usize>>,
    /// R938 §5.22 — opt-in: `Tab` / `Shift+Tab` **indent / dedent** the
    /// selected lines (the multi-line code-editor affordance) instead of
    /// being left to the shell's focus-traversal default. `false` (the
    /// default) keeps every existing single-line field byte-unchanged —
    /// `Tab` advances focus as before, because the field reports the key
    /// unhandled and the shell's traversal fallback runs ([`crate::input::forward_key_to_field`]
    /// returns `false`). A multi-line editor calls [`set_tab_indents`](Self::set_tab_indents)`(true)`
    /// to capture `Tab`. The HTML `<textarea>` vs `<input>` distinction:
    /// only the multi-line surface treats `Tab` as text, not navigation.
    ///
    /// A non-reactive [`Cell`] (not a [`Signal`]): the flag is wiring, not
    /// observable content — no view-fn renders "does Tab indent" — so it
    /// shares the interior-mutability shape of [`undo`](Self::undo) /
    /// `highlighter`, not the reactive content shape.
    tab_indents: Cell<bool>,
    /// R1268 §5.22 — opt-in: `Enter` inserts an **auto-indented** newline (a
    /// `\n` followed by a copy of the current line's leading indentation), the
    /// code-editor "keep indentation" affordance. `false` (the default) leaves
    /// `Enter` to the field's own policy — a single-line field submits, a prose
    /// multi-line field inserts a plain newline through its own handler — so
    /// every pre-R1268 caller is byte-unchanged. A multi-line code editor calls
    /// [`set_auto_indent`](Self::set_auto_indent)`(true)` (the sibling of
    /// [`set_tab_indents`](Self::set_tab_indents) / [`set_line_comment`](Self::set_line_comment):
    /// Enter is the third code-editor keystroke the shared field keymap gates on
    /// an opt-in flag). Non-reactive [`Cell`], like `tab_indents`: the flag is
    /// wiring, not observable content (no view-fn renders "does Enter auto-indent").
    auto_indent: Cell<bool>,
    /// R939 §5.22 — opt-in: the line-comment marker `Ctrl+/` toggles on the
    /// selected lines (`Some("//")` for a C-family editor), or `None` (the
    /// default) so `Ctrl+/` falls through to the application — the marker is a
    /// per-*language* fact the substrate cannot know, so the binding supplies
    /// it via [`set_line_comment`](Self::set_line_comment), the way it supplies
    /// the highlighter grammar. `&'static str` because a comment token is, in
    /// practice, a compile-time language constant rather than runtime input. The opt-in peer of
    /// [`tab_indents`](Self::tab_indents): both gate a code-editor keystroke an
    /// `<input>`-style field must not capture, and both are non-reactive
    /// wiring (no view-fn renders "does Ctrl+/ comment").
    line_comment: Cell<Option<&'static str>>,
    /// R951 §5.36 §5.22 — the **active typing mark** armed at a collapsed
    /// caret: when `Some`, the next inserted text carries this style even where
    /// the buffer is otherwise unstyled — the canonical rich-text "press Bold,
    /// then type" affordance (`ProseMirror` `storedMarks` / Slate `editor.marks` /
    /// Word's pending format). `None` (the default) means a fresh insert
    /// *inherits the style of the character to its left* (which
    /// `shift_runs_for_insert` already produces), so an unstyled field is
    /// byte-unchanged. A toolbar reads [`pending_style`](Self::pending_style) to
    /// show that a mark is armed and [`style_at_caret`](Self::style_at_caret)
    /// for the next-char style; the `scene/<tag>/external` RPC mirrors both.
    ///
    /// A reactive [`Signal`] (a toolbar view-fn lights its Bold button as a
    /// mark arms / clears) but **not** journalled: like the find session and
    /// the [`folds`](Self::folds) it is transient caret-adjacent view-state,
    /// never document content — the undo journal stays exactly
    /// `{text, caret, anchor, runs}` ([`TextEditCommand`]).
    ///
    /// Lifecycle (the `ProseMirror` `storedMarks` model): **navigation clears
    /// it** — every *navigation* mover ([`set_caret`](Self::set_caret) /
    /// [`move_left`](Self::move_left) / `select_*` / …) drops it alongside its
    /// [`goal_column`](Self::goal_column) reset (the W3C `selectionchange`
    /// model: relocating the caret discards a pending mark), a
    /// [`clear_selection`](Self::clear_selection) drops it (collapsing a
    /// selection is a selection change), an IME composition start drops it
    /// (`preedit_start`), and a wholesale
    /// [`set_text`](Self::set_text) resets it with the rest of the transient
    /// state. **Edits preserve it** — [`insert`](Self::insert) overlays the
    /// mark onto the typed span yet leaves the signal armed, so a run of
    /// keystrokes all stay styled, and a Backspace-then-type keeps the mark
    /// (the Word convention); an edit that *relocates* the caret (a
    /// selection-replacing splice) preserves it too, since the mark is inert
    /// under the `has_selection` guard until that selection collapses. The [`has_selection`](Self::has_selection) guard
    /// in [`effective_pending`](Self::effective_pending) is the only further
    /// gate: a mark is collapsed-caret state, inert while a selection is active
    /// (where formatting goes straight onto the runs). IME-committed text does
    /// not carry the mark (the mark survives composition and applies to the
    /// next direct keystroke — marking composed text is a deferred IME
    /// interaction).
    pending_style: Signal<Option<TextStyle>>,
}

/// R938 §5.22 — the indentation unit a `Tab` inserts (and `Shift+Tab`
/// removes) in a [`tab_indents`](TextEditState::tab_indents)-enabled editor:
/// four spaces, the space-indent default the keyboard + RPC dispatch share
/// so the two paths land on one width. A binding that wants tab characters
/// passes its own string to [`indent_selection`](TextEditState::indent_selection).
pub const INDENT_UNIT: &str = "    ";

/// R938 §5.22 — the indentation width (in `space` bytes) `Shift+Tab` strips
/// from a line start; the [`INDENT_UNIT`] peer for the dedent path (a single
/// leading `\t` also counts as one level). `4`, matching [`INDENT_UNIT`]'s
/// four spaces.
pub const INDENT_WIDTH: usize = 4;

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
    /// R933.1 §5.36 — the live fold set, shifted (not snapshotted) on
    /// undo/redo so a replayed splice keeps the byte anchors valid. Folds
    /// are non-journal view-state, so the command does not *save/restore*
    /// them — it only applies the same clip+shift the forward mutators do,
    /// the fold peer of the `runs` maintenance below.
    folds: Signal<BTreeSet<usize>>,
    /// Byte offset of the splice.
    offset: usize,
    /// Bytes removed by the edit (empty for a pure insert).
    removed: String,
    /// Bytes inserted by the edit (empty for a pure delete).
    inserted: String,
    /// Style-run fragments covering `[offset, offset + removed.len())` before
    /// the edit (absolute byte positions), restored on undo.
    removed_runs: Vec<StyleRun>,
    /// R951 §5.36 — style-run fragments covering `[offset, offset +
    /// inserted.len())` *after* the edit (absolute byte positions), re-applied
    /// on redo. The symmetric peer of [`removed_runs`](Self::removed_runs): an
    /// insert that *adds* run coverage the surrounding-run clip+shift cannot
    /// re-derive — the R951 active-typing-mark path overlays the armed style
    /// onto the typed span — is removed correctly on undo (clipped away with
    /// its bytes) but would be **lost on redo** (which re-derives runs purely by
    /// clip+shift) without this. For a plain insert (the run merely shifts) the
    /// captured coverage equals what the shift already produces, so re-overlaying
    /// it on redo is idempotent.
    inserted_runs: Vec<StyleRun>,
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
        buf.replace_range(
            self.offset..self.offset + self.removed.len(),
            &self.inserted,
        );
        let mut runs = self.runs.get();
        // The same maintenance the forward mutators apply: drop the deleted
        // span's coverage, then shift trailing runs right for the insert.
        clip_runs_for_delete(&mut runs, self.offset, self.offset + self.removed.len());
        shift_runs_for_insert(&mut runs, self.offset, self.inserted.len());
        // R951 §5.36 — restore run coverage the edit *added* over the inserted
        // span (an active typing mark), which clip+shift alone cannot re-derive.
        // Idempotent for a plain insert (the shifted run already covers it);
        // essential for an overlaid mark — the redo peer of the `removed_runs`
        // restore in `undo`.
        for run in &self.inserted_runs {
            overlay_style_run(&mut runs, run.clone());
        }
        // R933.1 — keep fold anchors valid across the replayed splice.
        let mut folds = self.folds.get();
        clip_folds_for_delete(&mut folds, self.offset, self.offset + self.removed.len());
        shift_folds_for_insert(&mut folds, self.offset, self.inserted.len());
        batch(|| {
            self.text.set(buf);
            self.caret.set(self.caret_after);
            self.anchor.set(self.anchor_after);
            self.runs.set(runs);
            self.folds.set(folds);
        });
    }

    fn undo(&self) {
        let mut buf = self.text.get();
        buf.replace_range(
            self.offset..self.offset + self.inserted.len(),
            &self.removed,
        );
        let mut runs = self.runs.get();
        // Reverse the forward clip+shift, restore the coverage the delete
        // destroyed, and re-fuse any survivor that was extended back over it.
        clip_runs_for_delete(&mut runs, self.offset, self.offset + self.inserted.len());
        shift_runs_for_insert(&mut runs, self.offset, self.removed.len());
        runs.extend(self.removed_runs.iter().cloned());
        normalize_runs(&mut runs);
        // R933.1 — reverse the forward fold clip+shift (folds are
        // non-journal, so only the byte-offset maintenance is mirrored).
        let mut folds = self.folds.get();
        clip_folds_for_delete(&mut folds, self.offset, self.offset + self.inserted.len());
        shift_folds_for_insert(&mut folds, self.offset, self.removed.len());
        batch(|| {
            self.text.set(buf);
            self.caret.set(self.caret_before);
            self.anchor.set(self.anchor_before);
            self.runs.set(runs);
            self.folds.set(folds);
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
                // R951 §5.36 — the continuation's added run coverage is at
                // absolute offsets right after this one's (contiguous inserts),
                // so it appends in order (overlay on redo re-fuses identicals).
                self.inserted_runs
                    .extend(next.inserted_runs.iter().cloned());
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

/// R928 §5.52 §5.36 — a reversible **formatting** edit: one contiguous splice
/// of the styled-run list, the rich-text peer of [`TextEditCommand`]'s text
/// splice. [`TextEditState::apply_style_run`] / [`clear_style_runs`] /
/// [`merge_style_run`] journal one of these so `Ctrl+Z` reverses a Bold / a
/// colour exactly as it reverses typing — closing the funnel-bypass where
/// formatting was the one editable mutation the [`UndoStack`] never saw.
///
/// [`clear_style_runs`]: TextEditState::clear_style_runs
/// [`merge_style_run`]: TextEditState::merge_style_run
///
/// Granular, never a whole-document snapshot ([[granular-undo-not-snapshot]]):
/// a range format touches a single contiguous index span of the sorted run
/// list, so the command stores only the changed middle — the runs `before`
/// the op (`removed`) and `after` it (`inserted`) over `[prefix, prefix + …)`,
/// recovered by [`style_runs_diff`]. The text buffer + caret are untouched (a
/// format moves neither), so unlike [`TextEditCommand`] this carries no
/// text / caret / anchor fields. Non-coalescable: each format is its own undo
/// step (the inherited [`UndoCommand::merge`] default returns `false`), and a
/// text edit never folds into a format (or vice-versa) because the two
/// concrete types never downcast into each other.
pub(crate) struct StyleRunCommand {
    runs: Signal<Vec<StyleRun>>,
    /// Count of leading runs the format left untouched (the common prefix).
    prefix: usize,
    /// Runs occupying `[prefix, prefix + removed.len())` *before* the format
    /// (restored on undo).
    removed: Vec<StyleRun>,
    /// Runs occupying `[prefix, prefix + inserted.len())` *after* the format
    /// (re-applied on redo).
    inserted: Vec<StyleRun>,
    label: Cow<'static, str>,
}

impl UndoCommand for StyleRunCommand {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        let mut runs = self.runs.get();
        runs.splice(
            self.prefix..self.prefix + self.removed.len(),
            self.inserted.iter().cloned(),
        );
        self.runs.set(runs);
    }

    fn undo(&self) {
        let mut runs = self.runs.get();
        runs.splice(
            self.prefix..self.prefix + self.inserted.len(),
            self.removed.iter().cloned(),
        );
        self.runs.set(runs);
    }
}

/// R928 §5.52 §5.36 — the minimal single contiguous splice between two run
/// lists: strip the common prefix + suffix and return `(prefix, removed,
/// inserted)`. A range format (`apply` / `clear` / `merge`) carves and
/// coalesces a single contiguous index span of the sorted run list, so this
/// recovers exactly that span for `StyleRunCommand` — the run-vector twin of
/// [`text_diff`]'s byte splice. R930.1 — both share the element-generic
/// prefix / suffix scan ([`common_prefix_len`] / [`common_suffix_len`]); only
/// `text_diff`'s `char`-boundary backtracking is text-specific (runs need
/// none). The caller guards `before == after` first, so the delta is never
/// empty.
fn style_runs_diff(
    before: &[StyleRun],
    after: &[StyleRun],
) -> (usize, Vec<StyleRun>, Vec<StyleRun>) {
    let p = common_prefix_len(before, after);
    let s = common_suffix_len(before, after, (before.len() - p).min(after.len() - p));
    (
        p,
        before[p..before.len() - s].to_vec(),
        after[p..after.len() - s].to_vec(),
    )
}

/// R930.1 §5.52 — count of leading elements `before` and `after` share: the
/// element-generic prefix scan both [`text_diff`] (over bytes) and
/// [`style_runs_diff`] (over runs) compute. A divergence between the two would
/// corrupt the granular splice, so the scan is one SSOT and each caller layers
/// its own type-specific framing (char-boundary snapping / run slicing) on top.
fn common_prefix_len<T: PartialEq>(before: &[T], after: &[T]) -> usize {
    let max = before.len().min(after.len());
    let mut p = 0;
    while p < max && before[p] == after[p] {
        p += 1;
    }
    p
}

/// R930.1 §5.52 — count of trailing shared elements, scanned within the `max`
/// elements left after the prefix (so the suffix never overlaps the prefix
/// region). The peer of [`common_prefix_len`]; `text_diff` passes a `max`
/// bounded by its already-`char`-snapped prefix so the two scans stay
/// consistent.
fn common_suffix_len<T: PartialEq>(before: &[T], after: &[T], max: usize) -> usize {
    let mut s = 0;
    while s < max && before[before.len() - 1 - s] == after[after.len() - 1 - s] {
        s += 1;
    }
    s
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
    // R930.1 — shared byte scan ([`common_prefix_len`]), then the text-specific
    // `char`-boundary backtracking (runs need none). The suffix scan is bounded
    // by the *snapped* prefix so a multi-byte char is never split across the
    // splice.
    let mut p = common_prefix_len(bb, ab);
    while p > 0 && !before.is_char_boundary(p) {
        p -= 1;
    }
    let mut s = common_suffix_len(bb, ab, (bb.len() - p).min(ab.len() - p));
    while s > 0 && (!before.is_char_boundary(bb.len() - s) || !after.is_char_boundary(ab.len() - s))
    {
        s -= 1;
    }
    (
        p,
        before[p..bb.len() - s].to_string(),
        after[p..ab.len() - s].to_string(),
    )
}

/// R938 §5.22 — the byte offset of every **line start** a `[start, end]`
/// span touches (a line is delimited by `\n`). Always ≥ 1 entry: the first
/// is the start of the line containing `start` (the byte after the previous
/// `\n`, or `0`); a span ending exactly at a line start does **not** include
/// that empty trailing line (the VS Code "select to column 0 of line N
/// leaves line N untouched" rule), while a collapsed caret yields exactly its
/// own line. The line-operation SSOT — [`TextEditState::indent_selection`] /
/// [`dedent_selection`](TextEditState::dedent_selection) /
/// [`toggle_line_comment`](TextEditState::toggle_line_comment) all iterate it.
/// Built on the `line_starts` / [`line_of`] line-index SSOT (no second `\n`
/// scan) — the only range-specific logic is the end-boundary rule.
fn line_starts_in_range(text: &str, start: usize, end: usize) -> Vec<usize> {
    let len = text.len();
    let start = start.min(len);
    let end = end.min(len).max(start);
    let starts = line_starts(text);
    let first = line_of(&starts, start);
    let mut last = line_of(&starts, end);
    // A span ending exactly at a line start does not include that (empty
    // trailing) line — unless it is also the first line (a collapsed caret at
    // a line start dedents that one line).
    if last > first && starts[last] == end {
        last -= 1;
    }
    starts[first..=last].to_vec()
}

/// R945 §5.22 — byte index one past the line starting at `ls`, **including** its
/// trailing `\n` when the line has one (else the buffer end). The whole-line
/// extent the line-manipulation ops ([`TextEditState::move_lines`] /
/// [`TextEditState::duplicate_lines`]) cut on — a line "owns" its newline, so a
/// move / duplicate carries the line break with the text.
fn line_extent_end(text: &str, ls: usize) -> usize {
    text[ls..].find('\n').map_or(text.len(), |n| ls + n + 1)
}

/// R938 §5.22 — how many leading bytes a dedent strips from the line at `ls`:
/// one byte for a leading `\t` (a tab is one indent level), else the count of
/// leading spaces up to `width`. `0` (no removal) for a line that starts with
/// neither — so [`TextEditState::dedent_selection`] never deletes content.
fn dedent_remove_len(text: &str, ls: usize, width: usize) -> usize {
    let bytes = text.as_bytes();
    if bytes.get(ls) == Some(&b'\t') {
        return 1;
    }
    let mut n = 0;
    while n < width && bytes.get(ls + n) == Some(&b' ') {
        n += 1;
    }
    n
}

/// R939 §5.22 — the byte offset of the first non-whitespace byte on the line
/// starting at `ls` (scanning to its `\n` or EOF), or `None` for a **blank**
/// line (only spaces / tabs / `\r`, so a bare `\r\n` line reads as blank too).
/// The comment-toggle insert / detect column: a blank line takes no marker and
/// is excluded from the "is every line already commented?" verdict, so a toggle
/// never comments an empty line. The returned offset is a `char` boundary — the
/// scan only steps over single-byte ASCII whitespace, so it stops at the first
/// lead byte of any multi-byte char.
fn line_first_non_ws(text: &str, ls: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = ls;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'\n' => return None,
            // `\r` counts as leading whitespace so a CRLF-only line (`\r\n`)
            // reads as blank — the toggle never comments an empty CRLF line
            // (the editor is otherwise LF-oriented, treating `\r` as content).
            b' ' | b'\t' | b'\r' => i += 1,
            _ => return Some(i),
        }
    }
    None
}

/// R1268 §5.22 — the byte offset where the leading indentation of the line
/// starting at `ls` ends, bounded by `at` — the copy bound for an auto-indent
/// newline ([`TextEditState::insert_newline`]). Scans only ASCII ` ` / `\t`
/// (indent characters — never `\r` or content, unlike the comment-toggle's
/// [`line_first_non_ws`] blank detection) and stops at `at`, so the copied
/// slice `text[ls..line_indent_end]` is the indentation up to the insertion
/// point: a caret inside the indent copies only what precedes it, a caret past
/// the indent copies the whole indent, and neither ever reaches code. The
/// returned offset is a `char` boundary (the scan steps over single-byte ASCII
/// only).
fn line_indent_end(text: &str, ls: usize, at: usize) -> usize {
    let bytes = text.as_bytes();
    let mut end = ls;
    while end < at && matches!(bytes.get(end), Some(b' ' | b'\t')) {
        end += 1;
    }
    end
}

/// R939 §5.22 — re-anchor a caret / selection offset across a set of
/// non-overlapping `(offset, removed_len, inserted_len)` edits. A position at
/// or after an edit's removed run moves by `inserted_len - removed_len`; a
/// position *inside* a removed run clamps into the replacement; a position
/// before is unchanged. The edits are independent (never overlap), so order
/// does not matter — the bare-offset peer of the `shift_runs_for_insert` /
/// [`clip_runs_for_delete`] byte maintenance. Generalises the dedent-only
/// removal shift at its 2nd consumer (the comment toggle): a pure insert is
/// `removed_len == 0`, a pure delete is `inserted_len == 0`.
///
/// Computed with `usize` arithmetic (no signed casts): the inserted and
/// removed lengths of edits ending *at or before* `pos` are summed separately,
/// and a net-left shift never underflows because those removed runs all end at or
/// before `pos`, so their total is `<= pos` (and `<= o` for the straddled run,
/// which they precede without overlap). At most one run straddles `pos`; the
/// result then clamps into that run's replacement.
fn shift_pos_for_edits(pos: usize, edits: &[(usize, usize, usize)]) -> usize {
    let mut add = 0;
    let mut rem = 0;
    let mut straddle = None;
    for &(o, removed, inserted) in edits {
        if pos >= o + removed {
            add += inserted;
            rem += removed;
        } else if pos > o {
            // `pos` falls inside this removed run (only one run can, since they
            // never overlap) — clamp into its replacement, after the net shift.
            straddle = Some((o, inserted));
        }
    }
    match straddle {
        Some((o, inserted)) => o + add - rem + (pos - o).min(inserted),
        None => pos + add - rem,
    }
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
    let after_ok =
        end == haystack.len() || !haystack[end..].chars().next().is_some_and(is_word_char);
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

/// R926 §5.22 — the three structural bracket pairs the matcher
/// recognizes, as `(open, close)` ASCII bytes. Angle brackets `<>` are
/// intentionally excluded: without a tokenizer they are ambiguous with
/// the comparison / shift operators, so highlighting them would fire on
/// `a < b` (the same reason VS Code matches them only under a language
/// configuration). All six bytes are ASCII, so a raw byte scan is
/// UTF-8-safe — an ASCII byte never occurs inside a multi-byte char.
const BRACKET_PAIRS: [(u8, u8); 3] = [(b'(', b')'), (b'[', b']'), (b'{', b'}')];

/// Classify a byte as a bracket: `Some((is_open, mate))` where `mate`
/// is the opposite-side byte of the same pair, or `None` for a
/// non-bracket byte.
fn bracket_role(b: u8) -> Option<(bool, u8)> {
    for &(open, close) in &BRACKET_PAIRS {
        if b == open {
            return Some((true, close));
        }
        if b == close {
            return Some((false, open));
        }
    }
    None
}

/// R926 §5.22 — the matching bracket of the bracket the caret sits
/// adjacent to, as `(open_byte, close_byte)` with `open_byte <
/// close_byte`, or `None` when the caret is not next to a bracket or
/// the bracket is unbalanced.
///
/// **Active bracket** (the VS Code rule): the bracket immediately
/// *before* the caret takes precedence (you just typed / stepped past
/// it), else the bracket immediately *at* the caret. So a caret between
/// `(` and `)` resolves to the opener it follows.
///
/// **Same-type depth scan**: from an opener, scan forward counting
/// nesting of *that pair only* until depth returns to zero; from a
/// closer, scan backward symmetrically. Counting one pair at a time is
/// correct for well-formed code (inner pairs of other types are
/// balanced, so they cannot shadow the match) and best-effort while
/// code is mid-edit and unbalanced — the baseline every editor ships
/// before language services. **Deferred refinements** (a separate
/// axis): a full multi-type bracket *stack* for strict interleave
/// detection (`([)]`), and skipping brackets inside string / comment
/// tokens — both need the tokenizer the §5.22 syntax layer owns.
///
/// Byte offsets, not char offsets: brackets are single ASCII bytes, and
/// `caret_byte` is a char-boundary byte offset, so the scan compares
/// raw bytes safely (ASCII bracket bytes cannot appear inside a
/// multi-byte UTF-8 sequence).
fn matching_bracket_in(text: &str, caret_byte: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    // Active bracket: the one just before the caret wins, else the one
    // at the caret. `caret_byte` is clamped to the buffer by the caller
    // (it is a live caret position); the bounds checks defend a stale
    // or out-of-range argument rather than panicking.
    let pos = if caret_byte > 0
        && caret_byte <= bytes.len()
        && bracket_role(bytes[caret_byte - 1]).is_some()
    {
        caret_byte - 1
    } else if caret_byte < bytes.len() && bracket_role(bytes[caret_byte]).is_some() {
        caret_byte
    } else {
        return None;
    };
    let (is_open, mate) = bracket_role(bytes[pos])?;
    if is_open {
        // Forward same-type depth scan — shared with fold-region
        // enumeration via [`match_forward`] so the active-bracket
        // highlight and a block's fold extent never disagree on where the
        // block ends.
        return match_forward(bytes, pos).map(|close| (pos, close));
    }
    // Closer: backward same-type depth scan. The active-bracket resolve is
    // its only consumer (fold enumeration only walks openers), so it stays
    // inline rather than lifting a `match_backward` peer with no second
    // caller.
    let this = bytes[pos];
    let mut depth: u32 = 0;
    let mut i = pos + 1;
    while i > 0 {
        i -= 1;
        let b = bytes[i];
        if b == this {
            depth += 1;
        } else if b == mate {
            depth -= 1;
            if depth == 0 {
                return Some((i, pos));
            }
        }
    }
    None
}

/// R933 §5.36 — from an opening bracket byte at `pos` (`{` / `[` / `(`),
/// the byte offset of its matching closer, or `None` when `pos` is not an
/// opener or the block is unbalanced. The forward half of the R926
/// matcher, lifted so [`matching_bracket_in`] (active-bracket highlight)
/// and `fold_regions_in` (block enumeration) run the **one** scan — a
/// divergence between "where the highlight says the block ends" and
/// "where the fold collapses to" would be a bug. Same-type depth scan
/// (counting this pair only) is correct for well-formed code and
/// best-effort mid-edit, exactly as the matcher documents. Byte offsets
/// are UTF-8-safe (bracket bytes are ASCII).
fn match_forward(bytes: &[u8], pos: usize) -> Option<usize> {
    let (is_open, mate) = bracket_role(*bytes.get(pos)?)?;
    if !is_open {
        return None;
    }
    let this = bytes[pos];
    let mut depth: u32 = 0;
    for (i, &b) in bytes.iter().enumerate().skip(pos) {
        if b == this {
            depth += 1;
        } else if b == mate {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// R933 §5.36 — byte offsets at which each **logical** (newline-delimited)
/// line begins: `0`, then the offset just past every `'\n'`. The textbook
/// byte→line index — `line_of(&starts, byte)` is then `O(log lines)`.
/// Logical, not parley *visual*, lines: code folding and the gutter number
/// the source's own lines independent of soft wrap, and the brackets that
/// bound a fold live at fixed byte offsets, so a pure newline scan (no
/// shaped `Layout`) is both sufficient and the correct unit.
fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    starts.extend(
        text.bytes()
            .enumerate()
            .filter(|&(_, b)| b == b'\n')
            .map(|(i, _)| i + 1),
    );
    starts
}

/// R958.1 §5.22 — byte offset of 1-based logical `line`'s start, given the
/// `line_starts` index, clamped to `1..=starts.len()` (`starts` is never
/// empty — always `[0]` at minimum). The shared indexing the public
/// [`TextEditState::line_start_byte`] and [`TextEditState::go_to_line`] both
/// resolve through, so neither re-derives the clamp inline and each walks
/// the document only once.
fn line_start_byte_at(starts: &[usize], line: usize) -> usize {
    starts[line.clamp(1, starts.len()) - 1]
}

/// R933 §5.36 — zero-based logical line containing `byte`, given the
/// `line_starts` index. `partition_point` finds the first start strictly
/// past `byte`; the line is the one before it.
fn line_of(starts: &[usize], byte: usize) -> usize {
    starts.partition_point(|&s| s <= byte).saturating_sub(1)
}

/// R933 §5.36 — a **foldable region**: one bracket-delimited block
/// (`{ }`, `[ ]`, or `( )`) whose opener and closer sit on *different*
/// logical lines (a same-line `{}` is not foldable). When collapsed, the
/// opener line stays visible (a `…` placeholder) and the interior lines
/// `start_line + 1 ..= end_line` hide. This folds the closer `}` line into
/// the summary too (the simplest rule); some editors keep the closer line
/// visible after the `…` — a deliberate divergence, not a copy of any one
/// editor's exact behaviour. Derived on read from the buffer by
/// [`TextEditState::fold_regions`]; the stored fold set keys only on
/// [`open_byte`](Self::open_byte).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FoldRegion {
    /// Byte offset of the opening bracket — the stable fold anchor the
    /// collapsed set keys on.
    pub open_byte: usize,
    /// Byte offset of the matching closing bracket.
    pub close_byte: usize,
    /// Zero-based logical line of the opener (stays visible when folded).
    pub start_line: usize,
    /// Zero-based logical line of the closer (the last hidden line when
    /// folded).
    pub end_line: usize,
    /// Whether this region is currently collapsed (filled by
    /// [`TextEditState::fold_regions`] from the live fold set;
    /// `fold_regions_in` leaves it `false`).
    pub collapsed: bool,
}

impl FoldRegion {
    /// R955.1 §5.36 — whether logical `line` is in this region's **interior**:
    /// after the opener, through the closer (`start_line < line <= end_line`).
    /// The single definition of the fold boundary rule — [`TextEditState::is_line_hidden`],
    /// the paint gutter's per-line hidden-check, and caret reanchoring all
    /// defer to it, so a future boundary change (e.g. some editors keep the
    /// closer line visible) lands in exactly one place rather than desyncing
    /// the painted gutter from keyboard navigation.
    #[must_use]
    pub fn contains_interior(&self, line: usize) -> bool {
        line > self.start_line && line <= self.end_line
    }

    /// R955.1 §5.36 — whether this region currently **hides** `line`: it is
    /// [`collapsed`](Self::collapsed) and `line` is in its
    /// [`interior`](Self::contains_interior).
    #[must_use]
    pub fn hides(&self, line: usize) -> bool {
        self.collapsed && self.contains_interior(line)
    }
}

/// R933 §5.36 — every foldable region in `text`, in opener (byte) order:
/// each bracket block spanning ≥ 2 logical lines. Reuses [`match_forward`]
/// for the extent and the single-pass `line_starts` index for the line
/// mapping. `collapsed` is left `false` — [`TextEditState::fold_regions`]
/// fills it from the live set. Cost is `O(text · openers)` per call — each
/// opener runs a forward [`match_forward`] scan — of the
/// [`find_matches`](TextEditState::find_matches) / matching-bracket
/// derive-on-read lineage, paid on read. There is **no** memoization here:
/// a caller needing the regions more than once per paint (e.g. a per-line
/// hidden-check) must call [`TextEditState::fold_regions`] once and reuse
/// the `Vec`, not re-derive per line. An incremental fold tree is the
/// scale answer; this baseline is honestly `O(text · openers)` each call.
fn fold_regions_in(text: &str) -> Vec<FoldRegion> {
    let bytes = text.as_bytes();
    let starts = line_starts(text);
    let mut out = Vec::new();
    for (pos, &b) in bytes.iter().enumerate() {
        if !matches!(bracket_role(b), Some((true, _))) {
            continue;
        }
        let Some(close) = match_forward(bytes, pos) else {
            continue;
        };
        let start_line = line_of(&starts, pos);
        let end_line = line_of(&starts, close);
        if end_line > start_line {
            out.push(FoldRegion {
                open_byte: pos,
                close_byte: close,
                start_line,
                end_line,
                collapsed: false,
            });
        }
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
            folds: Signal::new(BTreeSet::new()),
            tab_indents: Cell::new(false),
            auto_indent: Cell::new(false),
            line_comment: Cell::new(None),
            pending_style: Signal::new(None),
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
            folds: Signal::new(BTreeSet::new()),
            tab_indents: Cell::new(false),
            auto_indent: Cell::new(false),
            line_comment: Cell::new(None),
            pending_style: Signal::new(None),
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

    /// R938 §5.22 — opt this field into the `Tab` / `Shift+Tab` indent /
    /// dedent affordance ([`tab_indents`](Self::tab_indents)). Call once at
    /// wiring time for a multi-line code editor; single-line fields leave it
    /// off so `Tab` keeps advancing focus. The default is `false`, so every
    /// pre-R938 caller is byte-unchanged.
    pub fn set_tab_indents(&self, on: bool) {
        self.tab_indents.set(on);
    }

    /// R938 §5.22 — whether `Tab` indents the selection here (vs. the shell's
    /// focus-traversal default). The dispatch gate the `Tab` keystroke
    /// consults before calling [`indent_selection`](Self::indent_selection).
    #[must_use]
    pub fn tab_indents(&self) -> bool {
        self.tab_indents.get()
    }

    /// R1268 §5.22 — opt this field into `Enter` inserting an **auto-indented**
    /// newline ([`auto_indent`](Self::auto_indent) — the [`insert_newline`](Self::insert_newline)
    /// behaviour). Call once at wiring time for a multi-line code editor; a
    /// single-line field leaves it off so `Enter` keeps submitting, and a prose
    /// multi-line field that wants a plain newline leaves it off and drives its
    /// own handler. The default is `false`, so every pre-R1268 caller is
    /// byte-unchanged.
    pub fn set_auto_indent(&self, on: bool) {
        self.auto_indent.set(on);
    }

    /// R1268 §5.22 — whether `Enter` inserts an auto-indented newline here (vs.
    /// the field's own submit / plain-newline policy). The dispatch gate the
    /// shared field keymap consults before calling [`insert_newline`](Self::insert_newline)
    /// (the sibling of [`tab_indents`](Self::tab_indents) for the `Tab` key).
    #[must_use]
    pub fn auto_indent(&self) -> bool {
        self.auto_indent.get()
    }

    /// R939 §5.22 — opt this field into `Ctrl+/` line-comment toggling, with
    /// `marker` the language's line-comment token (`"//"` for C-family, `"#"`
    /// for shell / Python). Call once at wiring time for a code editor; a
    /// field that never calls this leaves [`line_comment`](Self::line_comment)
    /// `None`, so `Ctrl+/` falls through unhandled (the keymap returns `false`)
    /// and the application keeps the chord. The peer of
    /// [`set_tab_indents`](Self::set_tab_indents).
    pub fn set_line_comment(&self, marker: &'static str) {
        self.line_comment.set(Some(marker));
    }

    /// R939 §5.22 — the configured line-comment marker, or `None` when this
    /// field did not opt into `Ctrl+/` toggling. Both the keyboard keymap and
    /// the `toggle-comment` RPC verb read it, so an AI-driven toggle and a
    /// `Ctrl+/` press land the same edit ([`toggle_line_comment`](Self::toggle_line_comment)).
    #[must_use]
    pub fn line_comment(&self) -> Option<&'static str> {
        self.line_comment.get()
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
        // R951 §5.36 — capture run coverage the edit *added* over the inserted
        // span (post-edit), the redo peer of `removed_runs` (pre-edit, removed
        // span). For a plain insert this is the shifted surrounding run; for an
        // active-typing-mark insert it is the overlaid mark redo must restore.
        let after_runs = self.style_runs.get();
        let inserted_runs = runs_over_range(&after_runs, offset, offset + inserted.len());
        stack.push_applied(TextEditCommand {
            text: self.text.clone(),
            caret: self.caret_pos.clone(),
            anchor: self.selection_anchor.clone(),
            runs: self.style_runs.clone(),
            folds: self.folds.clone(),
            offset,
            removed,
            inserted,
            removed_runs,
            inserted_runs,
            caret_before,
            caret_after: self.caret_pos.get(),
            anchor_before,
            anchor_after: self.selection_anchor.get(),
            group,
            coalescable,
            label: Cow::Borrowed(label),
        });
    }

    /// R928 §5.52 §5.36 — the formatting peer of [`record_edit`](Self::record_edit):
    /// run a styled-run mutation `f` (an `apply` / `clear` / `merge`) and
    /// journal the run delta onto the attached [`UndoStack`] as one
    /// `StyleRunCommand`, so `Ctrl+Z` reverses formatting just like it
    /// reverses typing. No stack attached → `f` runs plain (unchanged
    /// behaviour for fields with no history). A format that leaves the runs
    /// untouched (empty / inverted / no-effect range) records nothing — the
    /// `before == after` guard, mirroring `record_edit`'s `before_text ==
    /// after_text`. The diff (and thus the snapshot) covers only the changed
    /// contiguous span ([`style_runs_diff`]), never the whole list.
    fn record_style_edit(&self, label: &'static str, f: impl FnOnce()) {
        let Some(stack) = self.undo.borrow().clone() else {
            f();
            return;
        };
        let before = self.style_runs.get();
        f();
        let after = self.style_runs.get();
        if before == after {
            return;
        }
        let (prefix, removed, inserted) = style_runs_diff(&before, &after);
        stack.push_applied(StyleRunCommand {
            runs: self.style_runs.clone(),
            prefix,
            removed,
            inserted,
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
    /// snap any anchor input through `clamp_to_char_boundary`.
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
        self.clear_pending_style();
        let text = self.text.get();
        let len = text.len();
        let snapped_anchor = clamp_to_char_boundary(&text, anchor.min(len));
        let snapped_focus = clamp_to_char_boundary(&text, focus.min(len));
        batch(|| {
            self.caret_pos.set(snapped_focus);
            self.selection_anchor
                .set(if snapped_anchor == snapped_focus {
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
        // R952.1 §5.36 — collapsing the selection is a selection change, so it
        // drops any pending typing mark too (the W3C `selectionchange` model,
        // the peer of the navigation movers). Without this, a mark armed via
        // `set_pending_style` while a selection was active (inert under the
        // `has_selection` guard) would resurrect when the selection collapses
        // here, applying to text it was never meant to.
        self.clear_pending_style();
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
    /// R904 §5.36 — when a `highlighter` is attached
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
    ///
    /// R928 §5.52 — journals onto the attached [`UndoStack`] as one
    /// `StyleRunCommand`, so a `Ctrl+Z` reverses the formatting (a discrete
    /// step from any surrounding typing); no-op ranges record nothing.
    pub fn apply_style_run(&self, start: usize, end: usize, style: TextStyle) {
        self.record_style_edit("Apply formatting", || {
            self.apply_style_run_inner(start, end, style);
        });
    }

    fn apply_style_run_inner(&self, start: usize, end: usize, style: TextStyle) {
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
    ///
    /// R928 §5.52 — undoable via `StyleRunCommand` (the
    /// `record_style_edit` funnel), so clearing
    /// formatting is one reversible step.
    pub fn clear_style_runs(&self, start: usize, end: usize) {
        self.record_style_edit("Clear formatting", || {
            self.clear_style_runs_inner(start, end);
        });
    }

    fn clear_style_runs_inner(&self, start: usize, end: usize) {
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
    ///
    /// R928 §5.52 — undoable via `StyleRunCommand`: toggling **bold** over a
    /// selection is one `Ctrl+Z` step, the same as the wholesale
    /// [`apply_style_run`](Self::apply_style_run) and `clear`. The `mutate`
    /// transform is applied inside the `record_style_edit`
    /// span; the recorded delta is the net run change, whatever fields it set.
    pub fn merge_style_run(
        &self,
        start: usize,
        end: usize,
        base: &TextStyle,
        mutate: impl Fn(&mut TextStyle),
    ) {
        self.record_style_edit("Format", || {
            self.merge_style_run_inner(start, end, base, mutate);
        });
    }

    fn merge_style_run_inner(
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

    /// R951 §5.36 §5.22 — the **armed typing mark** at a collapsed caret, or
    /// `None` when the next insert would merely inherit the surrounding style.
    /// `Some` only while the caret still sits where the mark was armed and no
    /// selection is active (the collapsed-caret invariant the
    /// [`pending_style`](Self::pending_style) field documents), so a toolbar can
    /// show "Bold is armed" distinctly from "the text here is already bold". The
    /// reactive read peer of [`set_pending_style`](Self::set_pending_style): a
    /// view-fn calling it re-runs when a mark arms, clears, or the caret leaves.
    #[must_use]
    pub fn pending_style(&self) -> Option<TextStyle> {
        self.effective_pending()
    }

    /// R951 §5.36 §5.22 — the style the **next inserted char** would carry: the
    /// armed [`pending_style`](Self::pending_style) if one is set, else the
    /// style inherited from the character to the caret's left (`None` = the
    /// field base). A toolbar's Bold button lights from this (bold whether
    /// *armed* or merely *inherited*); the AI-first peer reads it over RPC
    /// before typing. At offset `0` with no mark there is no left character, so
    /// the next char is the field base (`None`) — matching what
    /// `shift_runs_for_insert` produces for a leading insert.
    #[must_use]
    pub fn style_at_caret(&self) -> Option<TextStyle> {
        if let Some(style) = self.effective_pending() {
            return Some(style);
        }
        let text = self.text.get();
        let caret = self.caret_pos.get().min(text.len());
        if caret == 0 {
            return None;
        }
        self.style_at(prev_char_boundary(&text, caret))
    }

    /// The armed mark, honoured only at a collapsed caret (a mark is
    /// collapsed-caret state; with a selection, formatting goes onto the runs).
    /// Reads `pending_style` + `selection_anchor` (via
    /// [`has_selection`](Self::has_selection)) so the public reactive readers
    /// built on it subscribe to both — a view-fn re-runs when the mark changes
    /// or a selection forms / collapses.
    fn effective_pending(&self) -> Option<TextStyle> {
        let style = self.pending_style.get()?;
        if self.has_selection() {
            return None;
        }
        Some(style)
    }

    /// R951 §5.36 §5.22 — arm (or clear, with `None`) the active typing mark so
    /// the next inserted text carries `style`: the absolute setter, the AI-first
    /// peer of the toolbar toggle. An agent reads
    /// [`style_at_caret`](Self::style_at_caret), mutates a field, and writes the
    /// whole style back (the `apply-style` round-trip shape). The mark stays
    /// armed until a caret **navigation** clears it (the
    /// [`pending_style`](Self::pending_style) lifecycle). Arming with a
    /// selection active has no effect until the selection collapses — with a
    /// selection, format straight onto the runs via
    /// [`apply_style_run`](Self::apply_style_run) /
    /// [`merge_style_run`](Self::merge_style_run); use
    /// [`format_at_caret_or_selection`](Self::format_at_caret_or_selection) for
    /// the unified "works selected-or-not" toggle.
    pub fn set_pending_style(&self, style: Option<TextStyle>) {
        self.pending_style.set(style);
    }

    /// R951 §5.36 §5.22 — the unified **toggle a character format** command
    /// (Ctrl+B / Ctrl+I), working selected-or-not: the one entry a toolbar /
    /// keymap calls. With a selection it merges the transform over the selected
    /// runs ([`merge_style_run`](Self::merge_style_run), one undo step); at a
    /// collapsed caret it arms a pending mark — seeded from
    /// [`style_at_caret`](Self::style_at_caret) so the toggle flips relative to
    /// what would otherwise be typed (Bold over already-bold-inheriting text
    /// un-bolds the next keystrokes) — so the format takes effect on the
    /// following keystrokes. `base` is the field's default char format (the same
    /// `base` `merge_style_run` resolves uncovered bytes against); `mutate`
    /// flips the field(s) (e.g. toggle `font_weight`).
    pub fn format_at_caret_or_selection(&self, base: &TextStyle, mutate: impl Fn(&mut TextStyle)) {
        if let Some((start, end)) = self.selection_range() {
            self.merge_style_run(start, end, base, mutate);
            return;
        }
        let mut style = self.style_at_caret().unwrap_or_else(|| base.clone());
        mutate(&mut style);
        self.set_pending_style(Some(style));
    }

    /// R967.1 §5.36 — the "reflective" style a format toggle reads to decide its
    /// direction AND a toolbar reads to light its pressed-state: the selection
    /// start's style when selecting, else the next-typed-char style
    /// ([`style_at_caret`](Self::style_at_caret)) at a collapsed caret (so the
    /// toggles light for an armed mark / inherited style too — the R799
    /// reflective model). ONE substrate home so the toggle's flip-direction and
    /// the toolbar's lit/unlit state cannot diverge (a session-review found the
    /// read byte-duplicated between `toggle_format` and the example's
    /// `toolbar_active_style` — divergence-is-a-bug, since they MUST agree).
    #[must_use]
    pub fn reflective_style(&self) -> Option<TextStyle> {
        self.selection_range()
            .map_or_else(|| self.style_at_caret(), |(start, _)| self.style_at(start))
    }

    /// R967 §5.36 — toggle one [`FormatField`] over the selection (or arm it at a
    /// collapsed caret), preserving the covered runs' OTHER fields
    /// (`mergeCharFormat`). The toggle DIRECTION reads [`reflective_style`](Self::reflective_style)
    /// (the same read the toolbar's pressed-state uses), so it flips relative to
    /// what is / would-be styled; unstyled bytes resolve against `base`. Returns
    /// the new on-state. The SSOT shared by the `hello-textarea` **B** / **I**
    /// toolbar (`apply_format`) and the AI-first `toggle-format` RPC verb — both
    /// flip via this one method. Routes through
    /// [`format_at_caret_or_selection`](Self::format_at_caret_or_selection), so
    /// the toggle is one undoable [`UndoStack`] step like every other format edit.
    pub fn toggle_format(&self, field: FormatField, base: &TextStyle) -> bool {
        let target = !self.reflective_style().is_some_and(|st| field.is_on(&st));
        self.format_at_caret_or_selection(base, move |st| field.set(st, target));
        target
    }

    /// Drop any armed typing mark — the caret-navigation maintenance step (a
    /// pending mark is collapsed-caret state; moving the caret discards it, the
    /// W3C `selectionchange` model). Called by every navigation mover alongside
    /// its [`goal_column`](Self::goal_column) reset; equality-skips via the
    /// signal, so a move with no mark armed never notifies subscribers.
    fn clear_pending_style(&self) {
        self.pending_style.set(None);
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
    // [`UndoStack`] macro so one Ctrl+Z reverses it.

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

    /// R926 §5.22 — the bracket pair the caret currently sits adjacent
    /// to, as `(open_byte, close_byte)`, or `None` when the caret is not
    /// next to a bracket (or the bracket is unbalanced). A derive-on-read
    /// accessor in the [`find_matches`](Self::find_matches) /
    /// [`style_runs`](Self::style_runs) lineage: it subscribes to the
    /// `text` + `caret` Signals and re-derives from the live buffer, so
    /// the paint highlight bands, the caret geometry, and the
    /// `scene/<tag>/external/bracket_match` RPC all read the one
    /// derivation and never disagree. Re-derives on every caret move
    /// (the scan is `O(distance to the mate)`, paid only on read).
    #[must_use]
    pub fn matching_bracket(&self) -> Option<(usize, usize)> {
        let text = self.text.get();
        let caret = self.caret_pos.get();
        matching_bracket_in(&text, caret)
    }

    /// R933 §5.36 — every foldable region in the live buffer (each bracket
    /// block spanning ≥ 2 logical lines), opener-ordered, each carrying its
    /// current `collapsed` flag from the fold set. A derive-on-read accessor
    /// in the [`matching_bracket`](Self::matching_bracket) /
    /// [`find_matches`](Self::find_matches) lineage: it subscribes to `text`
    /// (region geometry) and `folds` (collapsed flags) so the field paint,
    /// the gutter chevrons, and the `scene/<tag>/external/fold_regions` RPC
    /// all read **one** derivation and never disagree. Pruning of stale
    /// anchors (a `{` an edit deleted) is implicit — a collapsed anchor with
    /// no matching derived region simply contributes no `FoldRegion`.
    #[must_use]
    pub fn fold_regions(&self) -> Vec<FoldRegion> {
        let text = self.text.get();
        let set = self.folds.get();
        let mut regions = fold_regions_in(&text);
        for r in &mut regions {
            r.collapsed = set.contains(&r.open_byte);
        }
        regions
    }

    /// R933 §5.36 — whether logical `line` is hidden by a collapsed region,
    /// i.e. it lies in some collapsed region's interior
    /// `start_line + 1 ..= end_line`. The opener line is never hidden (it
    /// carries the `…` placeholder); an outer collapse hides an inner
    /// region's opener too. Derived, so it always matches
    /// [`fold_regions`](Self::fold_regions); paint skips hidden lines.
    #[must_use]
    pub fn is_line_hidden(&self, line: usize) -> bool {
        self.fold_regions().iter().any(|r| r.hides(line))
    }

    /// R933 §5.36 — toggle the fold of the region that *opens* on logical
    /// `line` (the gutter-chevron gesture). When several brackets open on
    /// the same line the outermost (widest) block toggles — the block the
    /// gutter chevron represents. No region opens there ⇒ no-op returning
    /// `false`. **Collapsing reanchors the caret** out of the now-hidden
    /// interior (see `reanchor_caret_out_of_folds`):
    /// a fold must never strand the caret on an invisible line — the same
    /// view-state reanchor discipline the data widgets apply to sort /
    /// filter / group changes. Returns `true` when a region toggled.
    pub fn toggle_fold(&self, line: usize) -> bool {
        let Some(region) = self
            .fold_regions()
            .into_iter()
            .filter(|r| r.start_line == line)
            .max_by_key(|r| r.end_line)
        else {
            return false;
        };
        let mut set = self.folds.get();
        // `insert` returns `true` when the anchor was absent → we just
        // collapsed it; `false` when it was present → expand by removing.
        let now_collapsed = set.insert(region.open_byte);
        if !now_collapsed {
            set.remove(&region.open_byte);
        }
        self.folds.set(set);
        if now_collapsed {
            self.reanchor_caret_out_of_folds();
        }
        true
    }

    /// R933 §5.36 — collapse every foldable region (the VS Code "Fold All"
    /// gesture). Reanchors the caret out of any interior it hides.
    pub fn fold_all(&self) {
        let text = self.text.get();
        let anchors: BTreeSet<usize> = fold_regions_in(&text)
            .into_iter()
            .map(|r| r.open_byte)
            .collect();
        self.folds.set(anchors);
        self.reanchor_caret_out_of_folds();
    }

    /// R933 §5.36 — expand every region ("Unfold All"). No caret reanchor:
    /// unfolding only reveals lines, it never hides the caret.
    pub fn unfold_all(&self) {
        self.folds.set(BTreeSet::new());
    }

    /// R933 §5.36 — pull the caret to a visible line when a collapse has
    /// hidden the line it sits on. Lands it at the end of the *outermost*
    /// hiding region's opener line (the visible row the fold collapses to).
    /// Reads the already-written fold set, so callers set `folds` first.
    /// Mirrors the data-widget cursor-reanchor invariant — a view-state
    /// change must never leave the cursor on an invisible row.
    fn reanchor_caret_out_of_folds(&self) {
        let text = self.text.get();
        let starts = line_starts(&text);
        let caret_line = line_of(&starts, self.caret_pos.get());
        let set = self.folds.get();
        // Opener order ⇒ the first match is the outermost hiding region,
        // whose opener line is the visible row the fold summarises to.
        let hiding = fold_regions_in(&text)
            .into_iter()
            // `fold_regions_in` leaves `collapsed` false, so the collapse source
            // is the raw `set`; the boundary geometry defers to the SSOT.
            .find(|r| set.contains(&r.open_byte) && r.contains_interior(caret_line));
        if let Some(r) = hiding {
            let opener_end = if r.start_line + 1 < starts.len() {
                // End of the opener's logical line: the byte just before
                // its trailing newline (a char boundary — `\n` is ASCII).
                starts[r.start_line + 1] - 1
            } else {
                text.len()
            };
            self.set_caret(opener_end);
        }
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
    /// is bracketed by an [`UndoStack`] macro
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

    /// R938 §5.22 — **indent** the selected lines by one `indent` unit (the
    /// `Tab` editor command). Two cases, the macOS / VS Code split:
    ///
    /// * A selection spanning **≥ 2 lines** → block indent: `indent` is
    ///   inserted at the start of *every* touched line, the whole run grouped
    ///   into **one** undo step via the [`UndoStack::begin_macro`] /
    ///   [`end_macro`](UndoStack::end_macro) transaction (the 2nd consumer of
    ///   that substrate after `replace_all`). Each per-line insert is a
    ///   contiguous [`replace_range`](Self::replace_range) splice, so the
    ///   style runs **and** the fold anchors shift line-by-line and survive
    ///   the indent (the R933.1 byte-offset-state discipline — a single
    ///   whole-block splice would instead clip every interior fold). The
    ///   selection re-covers the indented block.
    /// * Otherwise (collapsed caret, or a selection **within one line**) →
    ///   `indent` is inserted at the caret, replacing any selection — `Tab`
    ///   as text, the single-line behaviour ([`insert`](Self::insert)).
    ///
    /// Returns whether the buffer changed (`false` for an empty `indent`).
    /// Lines are processed bottom-to-top so each insert leaves the earlier
    /// (smaller) line-start offsets valid.
    pub fn indent_selection(&self, indent: &str) -> bool {
        if indent.is_empty() {
            return false;
        }
        let text = self.text.get();
        let (start, end) = self.selection_range().unwrap_or_else(|| {
            let c = self.caret_pos.get().min(text.len());
            (c, c)
        });
        // A within-line selection / collapsed caret inserts at the caret;
        // only a multi-line span indents per line.
        if !text.get(start..end).is_some_and(|s| s.contains('\n')) {
            let before = self.text.get();
            self.insert(indent);
            return self.text.get() != before;
        }
        let line_starts = line_starts_in_range(&text, start, end);
        if let Some(stack) = self.undo_stack() {
            stack.begin_macro("Indent");
        }
        for &ls in line_starts.iter().rev() {
            self.replace_range(ls, ls, indent);
        }
        if let Some(stack) = self.undo_stack() {
            stack.end_macro();
        }
        // Re-cover the block: every insert sat at a line start strictly
        // before `end`, so `end` shifted right by `count * indent.len()`; the
        // first line start did not move (the insert lands at it).
        let new_end = end + line_starts.len() * indent.len();
        self.set_selection(line_starts[0], new_end);
        true
    }

    /// R938 §5.22 — **dedent** the selected lines by up to one `width`-space
    /// unit (the `Shift+Tab` editor command); the [`indent_selection`](Self::indent_selection)
    /// inverse. Always line-based (a collapsed caret dedents its own line): a
    /// leading `\t` strips as one level, otherwise up to `width` leading
    /// spaces strip, and a line with no leading whitespace is left untouched
    /// — a dedent never deletes content. The removals group into one undo step
    /// (the macro transaction, labelled "Dedent"). Each
    /// removal is a contiguous splice, so runs + fold anchors clip-and-shift
    /// per line (R933.1). The caret / selection re-anchors against the
    /// removed runs (a position inside a removed run clamps to the line
    /// start). Returns whether the buffer changed (`false` when no line had
    /// removable leading whitespace).
    pub fn dedent_selection(&self, width: usize) -> bool {
        if width == 0 {
            return false;
        }
        let text = self.text.get();
        let had_selection = self.selection_anchor.get().is_some();
        let (start, end) = self.selection_range().unwrap_or_else(|| {
            let c = self.caret_pos.get().min(text.len());
            (c, c)
        });
        let line_starts = line_starts_in_range(&text, start, end);
        let edits: Vec<(usize, usize)> = line_starts
            .iter()
            .filter_map(|&ls| {
                let n = dedent_remove_len(&text, ls, width);
                (n > 0).then_some((ls, n))
            })
            .collect();
        if edits.is_empty() {
            return false;
        }
        if let Some(stack) = self.undo_stack() {
            stack.begin_macro("Dedent");
        }
        for &(ls, n) in edits.iter().rev() {
            self.replace_range(ls, ls + n, "");
        }
        if let Some(stack) = self.undo_stack() {
            stack.end_macro();
        }
        // Re-anchor across the removals (each a delete: `inserted_len == 0`).
        let shift: Vec<(usize, usize, usize)> = edits.iter().map(|&(ls, n)| (ls, n, 0)).collect();
        let new_end = shift_pos_for_edits(end, &shift);
        if had_selection {
            self.set_selection(line_starts[0], new_end);
        } else {
            self.set_caret(new_end);
        }
        true
    }

    /// R939 §5.22 — **toggle line comments** on the selected lines with the
    /// `marker` token (`"//"` for C-family), the `Ctrl+/` editor command and
    /// the AI-first `toggle-comment` RPC verb's shared core. The VS Code
    /// "Toggle Line Comment" rule:
    ///
    /// * The touched lines are `line_starts_in_range` — the line-op SSOT that
    ///   [`indent_selection`](Self::indent_selection) /
    ///   [`dedent_selection`](Self::dedent_selection) also iterate (a collapsed
    ///   caret toggles its own line). Blank lines (whitespace only, including a
    ///   bare `\r\n`) take no marker and are excluded from the verdict, so a
    ///   toggle never comments an empty line.
    /// * **Verdict**: if *every* non-blank line already begins (after its
    ///   indent) with `marker`, the toggle **removes** it (and one following
    ///   space, the inverse of the inserted `"{marker} "`); otherwise it
    ///   **adds** `"{marker} "` at each non-blank line's first non-whitespace
    ///   column, so the marker hugs the code and the indent is preserved.
    /// * Each per-line edit is a contiguous [`replace_range`](Self::replace_range)
    ///   splice grouped into **one** undo step via the
    ///   [`UndoStack::begin_macro`] / [`end_macro`](UndoStack::end_macro)
    ///   transaction (reused from `replace_all` / indent / dedent — no new
    ///   primitive), so the style runs **and** fold anchors shift line-by-line
    ///   and survive the toggle (R933.1). Lines are spliced bottom-to-top so
    ///   the earlier offsets stay valid.
    ///
    /// Returns whether the buffer changed (`false` for an empty `marker`, or a
    /// selection of only blank lines). The prefix match is literal — `marker`
    /// inside a string or a longer token (`"///"`) is not distinguished, the
    /// every-editor baseline before a language service (the
    /// [`matching_bracket`](Self::matching_bracket) string-awareness defer
    /// applies here too).
    pub fn toggle_line_comment(&self, marker: &str) -> bool {
        if marker.is_empty() {
            return false;
        }
        let text = self.text.get();
        let had_selection = self.selection_anchor.get().is_some();
        let (start, end) = self.selection_range().unwrap_or_else(|| {
            let c = self.caret_pos.get().min(text.len());
            (c, c)
        });
        let line_starts = line_starts_in_range(&text, start, end);
        // The comment column of every non-blank touched line (blank lines drop
        // out, so they are neither commented nor counted in the verdict).
        let cols: Vec<usize> = line_starts
            .iter()
            .filter_map(|&ls| line_first_non_ws(&text, ls))
            .collect();
        if cols.is_empty() {
            return false;
        }
        // Remove iff *every* non-blank line is already commented; else add.
        let removing = cols.iter().all(|&p| text[p..].starts_with(marker));
        let added = format!("{marker} ");
        // `(offset, removed_len, inserted)` per non-blank line, ascending.
        let edits: Vec<(usize, usize, String)> = cols
            .iter()
            .map(|&p| {
                if removing {
                    // Strip the marker plus one following space if present (the
                    // inverse of the inserted `"{marker} "`).
                    let after = p + marker.len();
                    let strip =
                        marker.len() + usize::from(text.as_bytes().get(after) == Some(&b' '));
                    (p, strip, String::new())
                } else {
                    (p, 0, added.clone())
                }
            })
            .collect();
        if let Some(stack) = self.undo_stack() {
            stack.begin_macro("Toggle comment");
        }
        for (p, removed, ins) in edits.iter().rev() {
            self.replace_range(*p, *p + *removed, ins);
        }
        if let Some(stack) = self.undo_stack() {
            stack.end_macro();
        }
        let shift: Vec<(usize, usize, usize)> = edits
            .iter()
            .map(|(p, removed, ins)| (*p, *removed, ins.len()))
            .collect();
        let new_end = shift_pos_for_edits(end, &shift);
        if had_selection {
            self.set_selection(line_starts[0], new_end);
        } else {
            self.set_caret(new_end);
        }
        true
    }

    /// R945 §5.22 — the byte extent `[start, end)` of the whole-line block the
    /// current selection (or collapsed caret) touches: from the first touched
    /// line's start to the last touched line's end, INCLUDING that line's
    /// trailing `\n` (`line_extent_end` — a line owns its newline). The shared
    /// preamble of [`move_lines`](Self::move_lines) /
    /// [`duplicate_lines`](Self::duplicate_lines): both cut on this block.
    fn line_block_extent(&self, text: &str) -> (usize, usize) {
        let (start, end) = self.selection_range().unwrap_or_else(|| {
            let c = self.caret_pos.get().min(text.len());
            (c, c)
        });
        let touched = line_starts_in_range(text, start, end);
        let block_start = touched[0];
        let block_end = line_extent_end(text, *touched.last().unwrap_or(&block_start));
        (block_start, block_end)
    }

    /// R945 §5.22 — move the current line (or selected line block) one line up
    /// (`down == false`) or down (`down == true`) — the editor "move line"
    /// command (VS Code `Alt+Up` / `Alt+Down`), the AI-first peer of those
    /// chords. A boundary move (the first line up, the last line down) is a
    /// `false` no-op.
    ///
    /// The touched lines are the `line_starts_in_range` block; the line
    /// swapped past is the adjacent one, taken with its newline
    /// (`line_extent_end` — a line owns its trailing `\n`). The reorder is one
    /// [`replace_range`](Self::replace_range) over the two-line region with the
    /// lines re-sequenced, so the buffer's newline structure stays exact even
    /// when the move crosses the final, newline-less line (the swapped pair
    /// trade which one ends the buffer). The caret / selection rides the moved
    /// block. Returns whether the buffer changed.
    ///
    /// View-state across the reorder (R945.1, honest scope — not an
    /// impossibility): the single whole-region splice clips the moved region's
    /// style runs and fold anchors. **Folds** are non-journal best-effort
    /// view-state (R933 — `set_text` clears them, undo does not restore them),
    /// so dropping a fold on a structural reorder is consistent with the fold
    /// model, not a regression; the block re-derives its foldable regions on
    /// the next read. **Derived** syntax runs re-scan, so a highlighted code
    /// buffer is correct after a move. A *manual* `apply-style` run inside a
    /// moved line is the one gap — it is not yet carried across the reorder
    /// (there is no multi-line-move manual-run consumer; the textbook follow-up
    /// shifts the moved block's anchors by its byte delta, exactly as the caret
    /// already rides). Newlines are LF-internal: a synthesized separator is `\n`
    /// (the editor does not normalize / round-trip CRLF, matching every other
    /// edit path — it is `\r`-tolerant on read, not CRLF-preserving on write).
    pub fn move_lines(&self, down: bool) -> bool {
        let text = self.text.get();
        let (block_start, block_end) = self.line_block_extent(&text);
        let had_selection = self.selection_anchor.get().is_some();
        let caret = self.caret_pos.get().min(text.len());

        // Re-sequence the block + the adjacent line over [region_start,
        // region_end); `block_len_after` is the moved block's byte length once
        // it lands (it loses or gains a `\n` only when it crosses the final
        // line), and `new_block_start` is where it lands.
        let (region_start, region_end, reordered, new_block_start, block_len_after) = if down {
            if block_end >= text.len() {
                return false; // no line below
            }
            let next_end = line_extent_end(&text, block_end);
            let block = &text[block_start..block_end]; // ends with `\n` (a line follows)
            let next = &text[block_end..next_end];
            let (seq, block_len) = if next.ends_with('\n') {
                (format!("{next}{block}"), block.len())
            } else {
                // `next` is the last line (no `\n`): it becomes a middle line
                // (gains `\n`); the block becomes the last line (drops its `\n`).
                let block_nl = block.strip_suffix('\n').unwrap_or(block);
                (format!("{next}\n{block_nl}"), block_nl.len())
            };
            let new_start = block_start + (seq.len() - block_len);
            (block_start, next_end, seq, new_start, block_len)
        } else {
            if block_start == 0 {
                return false; // no line above
            }
            let starts = line_starts(&text);
            let bi = line_of(&starts, block_start); // block_start is a line start
            let prev_start = starts[bi - 1];
            let block = &text[block_start..block_end]; // may or may not end with `\n`
            let prev = &text[prev_start..block_start]; // ends with `\n`
            let (seq, block_len) = if block.ends_with('\n') {
                (format!("{block}{prev}"), block.len())
            } else {
                // The block is the last line (no `\n`): it becomes a middle line
                // (gains `\n`); `prev` becomes the last line (drops its `\n`).
                let prev_nl = prev.strip_suffix('\n').unwrap_or(prev);
                (format!("{block}\n{prev_nl}"), block.len() + 1)
            };
            (prev_start, block_end, seq, prev_start, block_len)
        };

        let rel = caret.saturating_sub(block_start).min(block_len_after);
        self.replace_range(region_start, region_end, &reordered);
        if had_selection {
            self.set_selection(new_block_start, new_block_start + block_len_after);
        } else {
            self.set_caret(new_block_start + rel);
        }
        true
    }

    /// R945 §5.22 — duplicate the current line (or selected line block),
    /// inserting a copy directly below. `down` chooses where the caret lands —
    /// on the lower copy (`true`) or the upper (`false`) — the VS Code
    /// `Shift+Alt+Down` / `Shift+Alt+Up` "copy line" split, the AI-first peer of
    /// those chords. One undo step (a single insertion). A block that ends in
    /// `\n` is already newline-separated; the last-line block (no trailing `\n`)
    /// gets a separator `\n` before its copy (LF-internal, like every other edit
    /// path — see [`move_lines`](Self::move_lines)). Always inserts a copy, so
    /// returns `true`.
    pub fn duplicate_lines(&self, down: bool) -> bool {
        let text = self.text.get();
        let (block_start, block_end) = self.line_block_extent(&text);
        let block = text[block_start..block_end].to_owned();
        let rel = self
            .caret_pos
            .get()
            .min(text.len())
            .saturating_sub(block_start)
            .min(block.len());
        let (insert_text, copy_start) = if block.ends_with('\n') {
            (block.clone(), block_end)
        } else {
            (format!("\n{block}"), block_end + 1)
        };
        self.replace_range(block_end, block_end, &insert_text);
        let new_caret = if down {
            copy_start + rel
        } else {
            block_start + rel
        };
        self.set_caret(new_caret);
        true
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
        // R933.1 — fold anchors track the edit the same way runs do.
        let mut folds = self.folds.get();
        clip_folds_for_delete(&mut folds, start, end);
        shift_folds_for_insert(&mut folds, start, s.len());
        batch(|| {
            self.text.set(buf);
            self.caret_pos.set(new_caret);
            self.selection_anchor.set(None);
            self.style_runs.set(runs);
            self.folds.set(folds);
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
        // R951 §5.36 — a wholesale replace resets the transient typing mark too
        // (the peer of clearing the selection / runs / folds below): its byte
        // context is gone with the old buffer.
        self.clear_pending_style();
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
            // R933 §5.36 — the same wholesale-replace invalidation applies
            // to the fold set: its collapsed anchors are byte offsets into
            // the *old* buffer, meaningless against new content (a mirror of
            // the style-run clear above). Incremental edits keep their folds
            // via the derive-on-read prune; only a full `set_text` resets —
            // otherwise replacing a buffer with different code whose braces
            // land on the old anchor bytes would silently re-collapse them.
            self.folds.set(BTreeSet::new());
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
        self.clear_pending_style();
        let text = self.text.get();
        let clamped = clamp_to_char_boundary(&text, pos.min(text.len()));
        batch(|| {
            self.caret_pos.set(clamped);
            self.selection_anchor.set(None);
        });
    }

    /// R941 §5.22 — the number of logical (newline-delimited) lines, 1-based:
    /// the count of `line_starts` entries. Always `>= 1` (an empty buffer is
    /// one line). The clamp bound for [`go_to_line`](Self::go_to_line) and the
    /// peer of a line-number gutter / a "go to line" prompt's max.
    #[must_use]
    pub fn line_count(&self) -> usize {
        line_starts(&self.text.get()).len()
    }

    /// R955.1 §5.22 — the 0-based logical (newline-delimited) line the caret
    /// currently sits on: the count of `\n`s before [`caret`](Self::caret).
    /// The read peer of [`go_to_line`](Self::go_to_line) (which positions by
    /// 1-based line) and the current-line input for a gutter highlight / fold
    /// navigation; in `0..line_count()`. Lifts the `line_of(&line_starts, …)`
    /// the state already computes internally (caret reanchoring) so a binding
    /// reads the current line from the substrate rather than re-deriving it.
    #[must_use]
    pub fn caret_line(&self) -> usize {
        line_of(&line_starts(&self.text.get()), self.caret())
    }

    /// R941 §5.22 — move the caret to the start of 1-based logical line `line`,
    /// collapsing any selection (the editor "go to line" navigation, the AI-first
    /// peer of a `Ctrl+G` prompt). `line` is clamped to `1..=line_count` (a `0`
    /// or `1` lands on the first line; a line past the end lands on the last
    /// line's start), so an out-of-range jump goes to the nearest valid line
    /// rather than failing. Returns the resolved 1-based line the caret landed
    /// on, so an RPC / keyboard caller can echo the actual destination (the
    /// setter-returns-the-read wire symmetry). Delegates to
    /// [`set_caret`](Self::set_caret), so the destination is clamped to a `char`
    /// boundary and the selection collapses identically to every caret move.
    ///
    /// R941.1 — this is the caret-POSITIONING primitive, not a scroll command: a
    /// field that scrolls (the R765 `scroll_into_view` paint path) follows the
    /// caret into view on the next paint, so the target line becomes visible
    /// automatically; a field that renders all its lines (no scroll viewport, e.g.
    /// `hello-syntax-highlight`) needs no scroll. Viewport scroll-to-caret is the
    /// field's reactive concern, kept orthogonal to caret placement.
    pub fn go_to_line(&self, line: usize) -> usize {
        // One document walk: derive both the clamped line number (echoed) and
        // its start byte from a single `line_starts`, via the shared
        // `line_start_byte_at` indexing (the SSOT `line_start_byte` also uses).
        let starts = line_starts(&self.text.get());
        let resolved = line.clamp(1, starts.len());
        self.set_caret(line_start_byte_at(&starts, resolved));
        resolved
    }

    /// R957 §5.22 — UTF-8 byte offset of the start of 1-based logical line
    /// `line`, clamped to `1..=line_count` (a `0` / `1` lands on the first
    /// line; a line past the end lands on the last line's start). The pure
    /// *byte-positioning* SSOT under [`go_to_line`](Self::go_to_line)
    /// (which is `set_caret(line_start_byte(line))` + the resolved-line
    /// echo) — exposed so a caller that must position **without** moving
    /// the caret can address a line directly: a gutter `Shift`+click
    /// extends the selection to a line's start
    /// (`set_selection(anchor, line_start_byte(line))`), where
    /// `go_to_line` would wrongly collapse the selection first. The
    /// returned offset is a `char` boundary (a line start always is).
    #[must_use]
    pub fn line_start_byte(&self, line: usize) -> usize {
        line_start_byte_at(&line_starts(&self.text.get()), line)
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
            // R906 — a selection-replacing insert IS the splice primitive
            // (`replace_range`'s inner): drain `[start, end)`, lay in `s`, clip
            // + shift runs, collapse the selection. One SSOT, not a 4th copy.
            self.splice_inner(start, end, s);
            return;
        }
        // R951 §5.36 — an active typing mark (Bold armed at a collapsed caret,
        // ProseMirror `storedMarks`) styles the text typed next. Read it before
        // the buffer changes; it is honoured only at this exact caret.
        let mark = self.effective_pending();
        let snapped = clamp_to_char_boundary(&buf, caret);
        buf.insert_str(snapped, s);
        let new_caret = snapped + s.len();
        let mut runs = self.style_runs.get();
        shift_runs_for_insert(&mut runs, snapped, s.len());
        if let Some(style) = &mark {
            // Overlay the armed mark over exactly the inserted span; adjacent
            // identical runs coalesce so a run of keystrokes is one run.
            overlay_style_run(
                &mut runs,
                StyleRun::new(
                    u32::try_from(snapped).unwrap_or(u32::MAX),
                    u32::try_from(new_caret).unwrap_or(u32::MAX),
                    style.clone(),
                ),
            );
        }
        // R933.1 — fold anchors shift with the insert (peer of the runs).
        let mut folds = self.folds.get();
        shift_folds_for_insert(&mut folds, snapped, s.len());
        // R951 — the mark stays armed (the signal is untouched here): a run of
        // keystrokes all carry it; only a caret navigation clears it.
        batch(|| {
            self.text.set(buf);
            self.caret_pos.set(new_caret);
            self.style_runs.set(runs);
            self.folds.set(folds);
        });
    }

    /// R1268 §5.22 — insert a newline, copying the current line's leading
    /// indentation when [`auto_indent`](Self::auto_indent) is set (the
    /// code-editor "keep indentation" behaviour). The single newline-insert
    /// entry point a handler that turns `Enter` into a newline routes through
    /// (a single-line field's `Enter` submits and never reaches here); with
    /// `auto_indent` off it is a plain `\n`, with it on it inserts `"\n"` + the
    /// copied indent, leaving the caret past the copied indent on the new line.
    ///
    /// R1270 §5.22 — the newline is an undo **boundary** (the VS Code / Sublime
    /// model, C3 audit fix): `Ctrl+Z` after `Enter` removes just the `\n` +
    /// copied indent, and never merges into the preceding typing run — so
    /// `Enter` is its own undo step whether or not indentation was copied.
    /// (Delegating to [`insert`](Self::insert) coalesced the whitespace insert
    /// *backward* into a prior character run, so undo wiped the preceding word.)
    ///
    /// The copied indent is the run of ASCII space / tab bytes at the insertion
    /// line's start, up to the insertion point (the selection start when a
    /// selection is being replaced, else the caret — where the text is
    /// spliced). Clamping to the insertion point means a caret parked inside
    /// the indent copies only the indentation before it, and a caret past the
    /// indent copies the whole indent but never any code; only ` ` / `\t` are
    /// copied (never a trailing `\r` or content).
    pub fn insert_newline(&self) {
        let text = self.text.get();
        // The byte the newline splices at: the selection start when replacing a
        // selection, else the (clamped) caret.
        let at = self
            .selection_range()
            .map_or_else(|| self.caret_pos.get().min(text.len()), |(start, _)| start);
        let indent = if self.auto_indent.get() {
            let starts = line_starts(&text);
            let ls = starts[line_of(&starts, at)];
            &text[ls..line_indent_end(&text, ls, at)]
        } else {
            ""
        };
        let inserted = format!("\n{indent}");
        // A newline is an undo BOUNDARY — its own step, never coalesced into a
        // prior typing run (unlike the character-insert coalescing `insert`
        // uses). `insert_inner` still drains an active selection first.
        self.record_edit(CoalesceGroup::Boundary, false, "New line", || {
            self.insert_inner(&inserted);
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
            // R906 — selection-delete is `splice_inner(start, end, "")` (the SSOT).
            self.splice_inner(start, end, "");
            return;
        }
        if caret == 0 {
            return;
        }
        let prev = prev_char_boundary(&buf, caret);
        buf.drain(prev..caret);
        let mut runs = self.style_runs.get();
        clip_runs_for_delete(&mut runs, prev, caret);
        // R933.1 — fold anchors clip with the delete (peer of the runs).
        let mut folds = self.folds.get();
        clip_folds_for_delete(&mut folds, prev, caret);
        batch(|| {
            self.text.set(buf);
            self.caret_pos.set(prev);
            self.style_runs.set(runs);
            self.folds.set(folds);
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
            // R906 — selection-delete is `splice_inner(start, end, "")` (the SSOT).
            self.splice_inner(start, end, "");
            return;
        }
        if caret >= buf.len() {
            return;
        }
        let next = next_char_boundary(&buf, caret);
        buf.drain(caret..next);
        let mut runs = self.style_runs.get();
        clip_runs_for_delete(&mut runs, caret, next);
        // R933.1 — fold anchors clip with the delete (peer of the runs).
        let mut folds = self.folds.get();
        clip_folds_for_delete(&mut folds, caret, next);
        // Text + runs + folds all change: batch the `Signal::set`s so a
        // subscriber re-runs once (the R55.G.24 atomic-multi-axis
        // contract — runs maintenance promoted this from a single write).
        batch(|| {
            self.text.set(buf);
            self.style_runs.set(runs);
            self.folds.set(folds);
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
        self.clear_pending_style();
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
        self.clear_pending_style();
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
        self.clear_pending_style();
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
        self.clear_pending_style();
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
        self.clear_pending_style();
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
        self.clear_pending_style();
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
        self.clear_pending_style();
        let caret = self.caret_pos.get();
        let anchor = self.selection_anchor.get().unwrap_or(caret);
        batch(|| {
            self.caret_pos.set(0);
            self.selection_anchor
                .set(if anchor == 0 { None } else { Some(anchor) });
        });
    }

    /// R56.1.f §5.22 — extend the selection to the end of the
    /// buffer. Shift+End canonical (single-line fields).
    pub fn select_end(&self) {
        self.clear_pending_style();
        let caret = self.caret_pos.get();
        let len = self.text.get().len();
        let anchor = self.selection_anchor.get().unwrap_or(caret);
        batch(|| {
            self.caret_pos.set(len);
            self.selection_anchor
                .set(if anchor == len { None } else { Some(anchor) });
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
    /// `pinion_text::LayoutCache` key (`(text, style, max_width)`)
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
        // R952.1 §5.36 — starting an IME composition drops any pending typing
        // mark, so the deferral "IME-committed text does not carry the mark" is
        // *consistent*: the composed text is unstyled AND the next direct
        // keystroke after the commit is too (until re-armed). Without this the
        // mark survived the bypassing `preedit_commit` and mis-applied to the
        // post-commit keystroke while skipping the composed text itself.
        self.clear_pending_style();
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

/// R933.1 §5.36 — shift fold anchors right for a text insert of `len`
/// bytes at `at` (the buffer-mutation maintenance step, the fold peer of
/// `shift_runs_for_insert`). An anchor at or after the insertion point
/// moves with its `{`; one before is untouched. Mirrors the style-run
/// maintenance so a collapsed block tracks *its* text across an edit
/// rather than stranding on a stale byte (which a coincidental brace
/// collision could otherwise turn into hiding the wrong block).
fn shift_folds_for_insert(folds: &mut BTreeSet<usize>, at: usize, len: usize) {
    if folds.is_empty() || len == 0 {
        return;
    }
    *folds = folds
        .iter()
        .map(|&a| if a >= at { a.saturating_add(len) } else { a })
        .collect();
}

/// R933.1 §5.36 — clip fold anchors against a delete of `[start, end)`
/// (the fold peer of [`clip_runs_for_delete`]). An anchor before the range
/// is kept; one *inside* it (its `{` was deleted) is dropped — the fold
/// ceases to exist; one after shifts left by the removed length.
fn clip_folds_for_delete(folds: &mut BTreeSet<usize>, start: usize, end: usize) {
    if folds.is_empty() || start >= end {
        return;
    }
    let d = end - start;
    *folds = folds
        .iter()
        .filter_map(|&a| {
            if a < start {
                Some(a)
            } else if a < end {
                None
            } else {
                Some(a - d)
            }
        })
        .collect();
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
        FormatField, INDENT_UNIT, INDENT_WIDTH, TextEditState, clamp_to_char_boundary,
        dedent_remove_len, find_matches_in, fold_regions_in, line_first_non_ws, line_indent_end,
        line_of, line_starts, line_starts_in_range, matching_bracket_in, next_char_boundary,
        prev_char_boundary, shift_pos_for_edits, style_runs_diff, use_text_edit_state,
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
        assert_eq!(
            vc, 3,
            "splice clamps to text.len(), preedit_end = 2 + 1 = 3"
        );
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
        assert_eq!(
            run_spans(&s),
            vec![(8, 13)],
            "a run after the insert shifts right by len"
        );
    }

    #[test]
    fn r767_insert_inside_run_grows_it() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![run(0, 5)]);
        s.set_caret(2);
        s.insert("XX"); // inside the run
        assert_eq!(
            run_spans(&s),
            vec![(0, 7)],
            "typing inside a styled span extends it"
        );
    }

    #[test]
    fn r767_insert_at_run_end_inherits_left() {
        let s = TextEditState::with_initial("ab".to_owned());
        s.set_style_runs(vec![run(0, 2)]);
        s.set_caret(2); // at the run end
        s.insert("X");
        assert_eq!(
            run_spans(&s),
            vec![(0, 3)],
            "insert at a run's end extends it (inherit-left)"
        );
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
        assert_eq!(
            run_spans(&s),
            vec![(0, 4)],
            "deleting a byte inside a run shrinks its end"
        );
    }

    #[test]
    fn r767_deleting_a_whole_run_drops_it() {
        let s = TextEditState::with_initial("ab".to_owned());
        s.set_style_runs(vec![run(0, 2)]);
        s.set_selection(0, 2);
        s.delete_forward(); // drains the whole run's text
        assert!(
            s.style_runs().is_empty(),
            "a run whose text is fully deleted is dropped"
        );
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
        assert_eq!(
            run_spans(&s),
            vec![(10, 13)],
            "trailing run tracks the net length change"
        );
    }

    #[test]
    fn r767_set_text_clears_runs() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![run(0, 5)]);
        s.set_text("brand new".to_owned());
        assert!(
            s.style_runs().is_empty(),
            "a wholesale set_text clears the now-invalid runs"
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R768 §5.36 §5.22 — apply / clear styled runs over a range
    // (rich-text formatting; setCharFormat + clearFormat semantics)
    // ─────────────────────────────────────────────────────────────

    /// A colour-distinct run over `[s, e)` so merge / split tests can
    /// tell two spans apart by style (the default-styled [`run`] helper
    /// cannot distinguish adjacent spans for the merge assertions).
    fn crun(s: u32, e: u32, rgb: (u8, u8, u8)) -> StyleRun {
        StyleRun::new(
            s,
            e,
            TextStyle::new().with_fg(Color::rgb(rgb.0, rgb.1, rgb.2)),
        )
    }

    const RED: (u8, u8, u8) = (0xD0, 0x28, 0x28);
    const BLUE: (u8, u8, u8) = (0x26, 0x4C, 0xD8);

    #[test]
    fn r768_apply_over_unstyled_gap_adds_a_run() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.apply_style_run(
            6,
            11,
            TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)),
        );
        assert_eq!(
            run_spans(&s),
            vec![(6, 11)],
            "applying over plain text adds one run"
        );
    }

    #[test]
    fn r768_apply_inside_a_run_splits_it_into_three() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.set_style_runs(vec![crun(0, 11, RED)]);
        // Recolour the middle "lo wo" → red | blue | red.
        s.apply_style_run(
            3,
            8,
            TextStyle::new().with_fg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)),
        );
        assert_eq!(
            run_spans(&s),
            vec![(0, 3), (3, 8), (8, 11)],
            "overlaying inside a run carves it into before | new | after",
        );
        let runs = s.style_runs();
        assert_eq!(
            runs[1].style.fg_color,
            Color::rgb(BLUE.0, BLUE.1, BLUE.2),
            "middle is the new ink"
        );
        assert_eq!(
            runs[0].style.fg_color,
            Color::rgb(RED.0, RED.1, RED.2),
            "flanks keep the old ink"
        );
    }

    #[test]
    fn r768_apply_same_style_to_adjacent_merges() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        // Apply the identical red to the abutting "[5, 11)" — the seam
        // dissolves into one span (FormatRange minimisation).
        s.apply_style_run(
            5,
            11,
            TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)),
        );
        assert_eq!(
            run_spans(&s),
            vec![(0, 11)],
            "adjacent identical styles coalesce"
        );
    }

    #[test]
    fn r768_apply_different_style_to_adjacent_keeps_two() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        s.apply_style_run(
            5,
            11,
            TextStyle::new().with_fg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)),
        );
        assert_eq!(
            run_spans(&s),
            vec![(0, 5), (5, 11)],
            "abutting distinct styles stay separate"
        );
    }

    #[test]
    fn r768_apply_overlapping_existing_runs_replaces_coverage() {
        let s = TextEditState::with_initial("hello world!!".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED), crun(6, 11, BLUE)]);
        // A wide blue overlay swallows the red run and the gap; the
        // trailing tail of the old blue run survives, then merges.
        s.apply_style_run(
            2,
            9,
            TextStyle::new().with_fg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)),
        );
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
        assert_eq!(
            s.text(),
            "hello world",
            "clear-formatting never edits the text"
        );
    }

    #[test]
    fn r768_clear_whole_coverage_empties_runs() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        s.clear_style_runs(0, 5);
        assert!(
            s.style_runs().is_empty(),
            "clearing a run's full extent drops it"
        );
    }

    #[test]
    fn r768_apply_empty_range_is_a_noop() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        s.apply_style_run(
            3,
            3,
            TextStyle::new().with_fg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)),
        );
        assert_eq!(
            run_spans(&s),
            vec![(0, 5)],
            "a collapsed range leaves the runs untouched"
        );
    }

    #[test]
    fn r768_apply_clamps_out_of_range_bytes() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.apply_style_run(
            2,
            999,
            TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)),
        );
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
        assert_eq!(
            runs[0].style.fg_color,
            Color::rgb(RED.0, RED.1, RED.2),
            "colour kept"
        );
        assert_eq!(
            runs[0].style.font_weight,
            crate::style::FontWeight::BOLD,
            "now bold"
        );
    }

    #[test]
    fn r769_merge_bold_over_unstyled_uses_base() {
        let s = TextEditState::with_initial("hello world".to_owned());
        // No runs: the [6,11) "world" is unstyled → resolves against base.
        s.merge_style_run(6, 11, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::BOLD;
        });
        let runs = s.style_runs();
        assert_eq!(
            run_spans(&s),
            vec![(6, 11)],
            "a run materialises over the bolded gap"
        );
        assert_eq!(
            runs[0].style.fg_color,
            Color::rgb(INK_BASE.0, INK_BASE.1, INK_BASE.2),
            "base ink"
        );
        assert_eq!(
            runs[0].style.font_weight,
            crate::style::FontWeight::BOLD,
            "base + bold"
        );
    }

    #[test]
    fn r769_merge_over_mixed_coverage_preserves_each_subspan() {
        let s = TextEditState::with_initial("hello world".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]); // "hello" red, " world" unstyled
        // Bold [3, 8): covered [3,5) keeps red, gap [5,8) takes base.
        s.merge_style_run(3, 8, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::BOLD;
        });
        assert_eq!(
            run_spans(&s),
            vec![(0, 3), (3, 5), (5, 8)],
            "split into normal | red-bold | base-bold"
        );
        let runs = s.style_runs();
        assert_eq!(
            runs[0].style.font_weight,
            crate::style::FontWeight::NORMAL,
            "untouched flank stays normal"
        );
        assert_eq!(
            runs[1].style.fg_color,
            Color::rgb(RED.0, RED.1, RED.2),
            "covered slice keeps red"
        );
        assert_eq!(
            runs[1].style.font_weight,
            crate::style::FontWeight::BOLD,
            "covered slice is bold"
        );
        assert_eq!(
            runs[2].style.fg_color,
            Color::rgb(INK_BASE.0, INK_BASE.1, INK_BASE.2),
            "gap took base ink"
        );
        assert_eq!(
            runs[2].style.font_weight,
            crate::style::FontWeight::BOLD,
            "gap is bold"
        );
    }

    #[test]
    fn r769_merge_pieces_with_equal_result_coalesce() {
        let s = TextEditState::with_initial("hello".to_owned());
        // Two abutting same-colour runs (set_style_runs does not merge).
        s.set_style_runs(vec![crun(0, 2, RED), crun(2, 5, RED)]);
        s.merge_style_run(0, 5, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::BOLD;
        });
        assert_eq!(
            run_spans(&s),
            vec![(0, 5)],
            "identical bolded pieces coalesce into one span"
        );
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
        assert_eq!(
            s.style_runs(),
            original,
            "bold then un-bold returns the exact original runs"
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R967 §5.36 — toggle_format: the toggle-direction SSOT shared by the
    // hello-textarea B / I toolbar and the AI-first `toggle-format` RPC verb.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r967_toggle_format_flips_one_field_and_keeps_colour() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        s.set_selection(0, 5);
        // First toggle reads not-bold at the start -> targets bold.
        let now = s.toggle_format(FormatField::Bold, &base_ink(INK_BASE));
        assert!(now, "toggle reports the new on-state (bold)");
        let runs = s.style_runs();
        assert_eq!(
            runs[0].style.font_weight,
            crate::style::FontWeight::BOLD,
            "now bold"
        );
        assert_eq!(
            runs[0].style.fg_color,
            Color::rgb(RED.0, RED.1, RED.2),
            "colour preserved (mergeCharFormat)"
        );
        // Second toggle reads bold -> targets normal, round-tripping the colour.
        let now2 = s.toggle_format(FormatField::Bold, &base_ink(INK_BASE));
        assert!(!now2, "the second toggle reports off");
        let runs = s.style_runs();
        assert_eq!(
            runs[0].style.font_weight,
            crate::style::FontWeight::NORMAL,
            "un-bolded"
        );
        assert_eq!(
            runs[0].style.fg_color,
            Color::rgb(RED.0, RED.1, RED.2),
            "colour still preserved"
        );
    }

    #[test]
    fn r967_toggle_format_each_field_is_orthogonal() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        s.set_selection(0, 5);
        assert!(s.toggle_format(FormatField::Bold, &base_ink(INK_BASE)));
        assert!(s.toggle_format(FormatField::Italic, &base_ink(INK_BASE)));
        assert!(s.toggle_format(FormatField::Underline, &base_ink(INK_BASE)));
        let st = s.style_runs()[0].style.clone();
        assert_eq!(st.font_weight, crate::style::FontWeight::BOLD, "bold set");
        assert_eq!(
            st.font_style,
            crate::style::FontStyle::Italic,
            "italic set, weight untouched"
        );
        assert!(
            st.decoration.underline,
            "underline set, weight + style untouched"
        );
        assert_eq!(
            st.fg_color,
            Color::rgb(RED.0, RED.1, RED.2),
            "colour preserved through all three toggles"
        );
    }

    #[test]
    fn r967_toggle_format_at_collapsed_caret_arms_pending_mark() {
        let s = TextEditState::with_initial("hello".to_owned());
        // No selection -> the toggle arms a pending typing mark (Word's "press
        // Bold then type"), not a run over existing text.
        s.set_caret(5);
        let now = s.toggle_format(FormatField::Bold, &base_ink(INK_BASE));
        assert!(now, "caret toggle reports bold armed");
        assert_eq!(
            s.pending_style().map(|st| st.font_weight),
            Some(crate::style::FontWeight::BOLD),
            "the next typed char would be bold",
        );
        assert!(
            s.style_runs().is_empty(),
            "no run materialised at a collapsed caret"
        );
    }

    #[test]
    fn r967_format_field_from_wire_round_trips_and_rejects_unknown() {
        assert_eq!(FormatField::from_wire("bold"), Some(FormatField::Bold));
        assert_eq!(FormatField::from_wire("italic"), Some(FormatField::Italic));
        assert_eq!(
            FormatField::from_wire("underline"),
            Some(FormatField::Underline)
        );
        assert_eq!(
            FormatField::from_wire("strikethrough"),
            Some(FormatField::Strikethrough)
        );
        assert_eq!(
            FormatField::from_wire("rainbow"),
            None,
            "unknown token rejected"
        );
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
        assert_eq!(
            runs[0].style.font_weight,
            crate::style::FontWeight::BOLD,
            "weight survives a later italic merge"
        );
        assert_eq!(
            runs[0].style.font_style,
            crate::style::FontStyle::Italic,
            "italic applied"
        );
        assert_eq!(
            runs[0].style.fg_color,
            Color::rgb(RED.0, RED.1, RED.2),
            "colour survives both merges"
        );
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
        assert!(
            s.style_at(5).is_none(),
            "the exclusive end byte is outside the run"
        );
    }

    #[test]
    fn r769_merge_empty_range_is_a_noop() {
        let s = TextEditState::with_initial("hello".to_owned());
        s.set_style_runs(vec![crun(0, 5, RED)]);
        let before = s.style_runs();
        s.merge_style_run(3, 3, &base_ink(INK_BASE), |st| {
            st.font_weight = crate::style::FontWeight::BOLD;
        });
        assert_eq!(
            s.style_runs(),
            before,
            "a collapsed range leaves the runs untouched"
        );
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
        s.apply_style_run(
            0,
            5,
            TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)),
        );
        assert!(
            runs_seen.get() > before,
            "a runs-only apply re-runs a style_runs subscriber"
        );
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
            assert_eq!(
                st.text(),
                "ab",
                "only the post-move insert undoes (caret move broke coalescing)"
            );
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
            assert_eq!(
                st.text(),
                "abc",
                "the delete is its own step (different group)"
            );
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
            assert_eq!(
                st.text(),
                "abc",
                "type-to-replace undoes back to the selected text in one step"
            );
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
            assert_eq!(
                st.style_runs(),
                vec![red(0, 4)],
                "the run clips to the shorter text"
            );
            assert!(st.undo());
            assert_eq!(st.text(), "abcdef");
            assert_eq!(
                st.style_runs(),
                vec![red(0, 6)],
                "undo restores the run span exactly"
            );
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
            assert_eq!(
                st.style_runs(),
                vec![red(2, 5)],
                "insert before the run shifts it"
            );
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
            assert_eq!(
                st.text(),
                "word",
                "two backspaces undo as one coalesced step"
            );
            assert!(st.undo());
            assert_eq!(st.text(), "", "the typing run is the prior step");
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R928 §5.52 §5.36 — formatting is an undoable edit. Closes the
    // funnel-bypass where apply / clear / merge wrote `style_runs`
    // directly, the one editable mutation `Ctrl+Z` never saw.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r928_style_runs_diff_is_the_minimal_contiguous_splice() {
        // [A, B, C] → [A, B', C]: prefix skips the unchanged A, suffix skips
        // the unchanged C, so the delta is just the middle run — granular,
        // never the whole list ([[granular-undo-not-snapshot]]).
        let before = vec![crun(0, 2, RED), crun(2, 4, RED), crun(4, 6, RED)];
        let after = vec![crun(0, 2, RED), crun(2, 4, BLUE), crun(4, 6, RED)];
        let (prefix, removed, inserted) = style_runs_diff(&before, &after);
        assert_eq!(prefix, 1, "the leading equal run is outside the splice");
        assert_eq!(
            removed,
            vec![crun(2, 4, RED)],
            "only the changed run is removed"
        );
        assert_eq!(
            inserted,
            vec![crun(2, 4, BLUE)],
            "only the changed run is inserted"
        );
    }

    #[test]
    fn r928_apply_then_undo_reverts_formatting() {
        Owner::new().run(|| {
            let st = styled("hello", vec![]);
            st.apply_style_run(
                0,
                3,
                TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)),
            );
            assert_eq!(run_spans(&st), vec![(0, 3)], "the format applied");
            assert!(st.undo(), "Ctrl+Z reverses the format");
            assert_eq!(
                run_spans(&st),
                Vec::<(u32, u32)>::new(),
                "formatting undone"
            );
            assert_eq!(st.text(), "hello", "the text was never touched");
            assert!(st.redo(), "redo re-applies the format");
            assert_eq!(run_spans(&st), vec![(0, 3)], "the run is back");
        });
    }

    #[test]
    fn r928_clear_then_undo_restores_the_run() {
        Owner::new().run(|| {
            let st = styled("hello", vec![red(0, 5)]);
            st.clear_style_runs(0, 5);
            assert_eq!(
                run_spans(&st),
                Vec::<(u32, u32)>::new(),
                "the run was cleared"
            );
            assert!(st.undo());
            assert_eq!(
                st.style_runs(),
                vec![red(0, 5)],
                "undo restores the cleared run exactly"
            );
            assert!(st.redo());
            assert_eq!(run_spans(&st), Vec::<(u32, u32)>::new(), "redo re-clears");
        });
    }

    #[test]
    fn r928_merge_bold_then_undo() {
        Owner::new().run(|| {
            let st = styled("hello", vec![]);
            let base = TextStyle::new();
            st.merge_style_run(0, 5, &base, |s| {
                s.font_weight = crate::style::FontWeight::BOLD;
            });
            let runs = st.style_runs();
            assert_eq!(runs.len(), 1, "the bold toggle laid one run");
            assert_eq!(
                runs[0].style.font_weight,
                crate::style::FontWeight::BOLD,
                "selection is bold"
            );
            assert!(st.undo(), "Ctrl+Z un-bolds");
            assert_eq!(
                run_spans(&st),
                Vec::<(u32, u32)>::new(),
                "the bold run is gone"
            );
            assert!(st.redo());
            assert_eq!(
                st.style_runs()[0].style.font_weight,
                crate::style::FontWeight::BOLD,
                "redo re-bolds",
            );
        });
    }

    #[test]
    fn r928_format_is_a_discrete_step_from_typing() {
        // Typing then formatting are TWO undo steps — a format never folds
        // into the surrounding typing run (the concrete command types never
        // downcast into each other). One Ctrl+Z drops the format and leaves
        // the text; a second drops the text.
        Owner::new().run(|| {
            let st = undoable();
            st.insert("ab");
            st.apply_style_run(
                0,
                1,
                TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)),
            );
            assert_eq!(run_spans(&st), vec![(0, 1)], "the format is on");
            assert!(st.undo(), "first undo");
            assert_eq!(
                run_spans(&st),
                Vec::<(u32, u32)>::new(),
                "the format alone reverted"
            );
            assert_eq!(st.text(), "ab", "the typing still stands — a separate step");
            assert!(st.undo(), "second undo");
            assert_eq!(st.text(), "", "now the typing reverts");
        });
    }

    #[test]
    fn r928_consecutive_formats_are_separate_steps() {
        Owner::new().run(|| {
            let st = styled("hello world", vec![]);
            st.apply_style_run(
                0,
                5,
                TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)),
            );
            st.apply_style_run(
                6,
                11,
                TextStyle::new().with_fg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)),
            );
            assert_eq!(run_spans(&st), vec![(0, 5), (6, 11)], "two formats applied");
            assert!(st.undo());
            assert_eq!(
                run_spans(&st),
                vec![(0, 5)],
                "one undo drops only the last format"
            );
            assert!(st.undo());
            assert_eq!(
                run_spans(&st),
                Vec::<(u32, u32)>::new(),
                "the first format is the prior step"
            );
        });
    }

    #[test]
    fn r928_noop_format_records_nothing() {
        // An empty / inverted range changes no runs, so it must not push a
        // phantom undo step — otherwise the first Ctrl+Z would be wasted.
        Owner::new().run(|| {
            let st = undoable();
            st.insert("ab");
            st.apply_style_run(
                2,
                2,
                TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)),
            );
            assert!(st.undo(), "the only journalled step is the typing");
            assert_eq!(
                st.text(),
                "",
                "the no-op format pushed nothing; one undo cleared the text"
            );
        });
    }

    #[test]
    fn r928_unattached_format_still_applies() {
        // No undo stack → the format still happens (graceful, like an
        // unattached text edit); there is simply nothing to reverse.
        Owner::new().run(|| {
            let st = TextEditState::with_initial("hello".to_owned());
            assert!(st.undo_stack().is_none());
            st.apply_style_run(
                0,
                3,
                TextStyle::new().with_fg(Color::rgb(RED.0, RED.1, RED.2)),
            );
            assert_eq!(
                run_spans(&st),
                vec![(0, 3)],
                "the format applied with no stack"
            );
            assert!(!st.undo(), "no stack -> undo is a no-op");
            assert_eq!(
                run_spans(&st),
                vec![(0, 3)],
                "the format stands; nothing was journalled"
            );
        });
    }

    #[test]
    fn r930_1_format_undo_survives_an_interleaved_text_edit() {
        // R930.1 — the StyleRunCommand splice indexes the runs vector by an
        // absolute prefix; an interleaved TEXT edit shifts the runs between a
        // format and its undo. Prove the round-trip stays byte-exact (the
        // splice never indexes a stale position) — the SUSPECT a session review
        // flagged, turned into a guard.
        Owner::new().run(|| {
            let st = styled("hello world", vec![red(0, 5)]); // "hello" is red
            st.set_caret(0);
            // (1) text edit shifts the run right by the insert.
            st.insert("XY");
            assert_eq!(
                st.style_runs(),
                vec![red(2, 7)],
                "the run shifted right by the insert"
            );
            // (2) a format edit over the *shifted* text.
            st.apply_style_run(
                0,
                2,
                TextStyle::new().with_fg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)),
            );
            assert_eq!(
                run_spans(&st),
                vec![(0, 2), (2, 7)],
                "blue prefix + shifted red"
            );
            // (3) undo the format: blue gone, the shifted red intact, text untouched.
            assert!(st.undo());
            assert_eq!(
                st.style_runs(),
                vec![red(2, 7)],
                "format undone against the shifted runs"
            );
            assert_eq!(
                st.text(),
                "XYhello world",
                "the text edit still stands (separate step)"
            );
            // (4) undo the text edit: text + runs back to the seed exactly.
            assert!(st.undo());
            assert_eq!(st.text(), "hello world");
            assert_eq!(
                st.style_runs(),
                vec![red(0, 5)],
                "runs restored to the seed"
            );
            // (5) redo both, in order.
            assert!(st.redo());
            assert!(st.redo());
            assert_eq!(
                run_spans(&st),
                vec![(0, 2), (2, 7)],
                "both edits re-applied"
            );
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
        assert_eq!(
            find_matches_in(s, "cole", true, false),
            vec![(2, 6), (7, 11)]
        );
    }

    // ── R926 §5.22 — matching-bracket derivation ─────────────────────

    #[test]
    fn r926_matching_bracket_from_opener_scans_forward() {
        // Caret immediately after the opener (the just-passed position)
        // and caret AT the opener both match the closer.
        assert_eq!(matching_bracket_in("(a)", 1), Some((0, 2)));
        assert_eq!(matching_bracket_in("(a)", 0), Some((0, 2)));
    }

    #[test]
    fn r926_matching_bracket_from_closer_scans_backward() {
        // Caret after the closer (the just-typed `)`) and caret AT the
        // closer both match back to the opener.
        assert_eq!(matching_bracket_in("(a)", 3), Some((0, 2)));
        assert_eq!(matching_bracket_in("(a)", 2), Some((0, 2)));
    }

    #[test]
    fn r926_matching_bracket_nesting_same_type() {
        let s = "((()))";
        // After the outer opener -> outer closer.
        assert_eq!(matching_bracket_in(s, 1), Some((0, 5)));
        // After the innermost opener -> innermost closer.
        assert_eq!(matching_bracket_in(s, 3), Some((2, 3)));
    }

    #[test]
    fn r926_matching_bracket_before_caret_takes_precedence() {
        // "(){}", caret at 2 sits between `)` (before) and `{` (at). The
        // closer just before the caret wins, matching back to `(`.
        assert_eq!(matching_bracket_in("(){}", 2), Some((0, 1)));
    }

    #[test]
    fn r926_matching_bracket_all_three_pair_types() {
        assert_eq!(matching_bracket_in("[x]", 1), Some((0, 2)));
        assert_eq!(matching_bracket_in("{x}", 1), Some((0, 2)));
        assert_eq!(matching_bracket_in("(x)", 1), Some((0, 2)));
    }

    #[test]
    fn r926_matching_bracket_mixed_types_count_per_pair() {
        // "{[()]}" — same-type counting reaches the right mate through
        // balanced inner pairs of other types.
        let s = "{[()]}";
        assert_eq!(matching_bracket_in(s, 1), Some((0, 5)));
        assert_eq!(matching_bracket_in(s, 2), Some((1, 4)));
        assert_eq!(matching_bracket_in(s, 3), Some((2, 3)));
    }

    #[test]
    fn r926_matching_bracket_none_when_not_adjacent() {
        assert_eq!(matching_bracket_in("a b c", 2), None);
        assert_eq!(matching_bracket_in("", 0), None);
        // Inside "(abc)" but not next to a bracket.
        assert_eq!(matching_bracket_in("(abc)", 2), None);
    }

    #[test]
    fn r926_matching_bracket_none_when_unbalanced() {
        assert_eq!(matching_bracket_in("(a", 1), None, "lone opener");
        assert_eq!(matching_bracket_in("a)", 2), None, "lone closer");
    }

    #[test]
    fn r926_matching_bracket_multibyte_offsets_exact() {
        // "(é)" — '(' at byte 0, 'é' at bytes 1..3, ')' at byte 3. The
        // multi-byte char's bytes are all >= 0x80, never an ASCII
        // bracket, so the byte scan is exact.
        let s = "(\u{00e9})";
        assert_eq!(matching_bracket_in(s, 1), Some((0, 3)));
        assert_eq!(matching_bracket_in(s, 4), Some((0, 3)));
    }

    #[test]
    fn r926_matching_bracket_accessor_reads_buffer_and_caret() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("f(x)".to_string());
            // Caret at end (after ')') -> matches '(' at 1.
            st.set_caret(4);
            assert_eq!(st.matching_bracket(), Some((1, 3)));
            // Caret away from any bracket -> None.
            st.set_caret(0);
            assert_eq!(st.matching_bracket(), None);
            // Caret just after '(' -> matches ')'.
            st.set_caret(2);
            assert_eq!(st.matching_bracket(), Some((1, 3)));
        });
    }

    #[test]
    fn r933_line_index_maps_bytes_to_logical_lines() {
        // "ab\ncd\n\nef": newlines at 2, 5, 6 → line starts [0, 3, 6, 7].
        let s = "ab\ncd\n\nef";
        let starts = line_starts(s);
        assert_eq!(starts, vec![0, 3, 6, 7]);
        assert_eq!(line_of(&starts, 0), 0);
        assert_eq!(
            line_of(&starts, 2),
            0,
            "the trailing '\\n' belongs to its line"
        );
        assert_eq!(line_of(&starts, 3), 1);
        assert_eq!(line_of(&starts, 6), 2, "an empty line still indexes");
        assert_eq!(line_of(&starts, 8), 3);
    }

    #[test]
    fn r933_fold_regions_enumerates_multiline_blocks_only() {
        // The `()` on line 0 is same-line → not foldable; the `{}` spans
        // line 0 (opener) to line 3 (closer) → the one foldable region.
        let src = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
        let regions = fold_regions_in(src);
        assert_eq!(regions.len(), 1, "only the multi-line brace block folds");
        let r = regions[0];
        assert_eq!(r.start_line, 0);
        assert_eq!(r.end_line, 3);
        assert!(!r.collapsed);
        assert_eq!(src.as_bytes()[r.open_byte], b'{');
        assert_eq!(src.as_bytes()[r.close_byte], b'}');
    }

    #[test]
    fn r933_fold_regions_nested_both_enumerated_outer_first() {
        let src = "a {\n  b {\n    c\n  }\n}\n";
        let regions = fold_regions_in(src);
        assert_eq!(regions.len(), 2, "outer and inner brace blocks");
        assert_eq!(
            (regions[0].start_line, regions[0].end_line),
            (0, 4),
            "outer first"
        );
        assert_eq!(
            (regions[1].start_line, regions[1].end_line),
            (1, 3),
            "inner second"
        );
    }

    #[test]
    fn r933_fold_extent_agrees_with_matching_bracket() {
        // A fold region's [open, close] is exactly what the active-bracket
        // matcher resolves from the opener — they share `match_forward`, so
        // the gutter fold and the highlight can never disagree.
        let src = "{\n  a\n}\n";
        let regions = fold_regions_in(src);
        assert_eq!(regions.len(), 1);
        let r = regions[0];
        assert_eq!(
            matching_bracket_in(src, r.open_byte + 1),
            Some((r.open_byte, r.close_byte)),
            "fold extent == active-bracket resolve",
        );
    }

    #[test]
    fn r933_collapse_hides_interior_keeps_opener_visible() {
        Owner::new().run(|| {
            let src = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n".to_string();
            let st = TextEditState::with_initial(src);
            assert!(st.toggle_fold(0), "a region opens on line 0");
            assert!(!st.is_line_hidden(0), "opener stays visible");
            assert!(st.is_line_hidden(1), "interior hides");
            assert!(st.is_line_hidden(2), "interior hides");
            assert!(st.is_line_hidden(3), "closer line joins the folded summary");
            assert!(st.fold_regions()[0].collapsed);
        });
    }

    #[test]
    fn r933_toggle_fold_round_trips() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("x {\n y\n}\n".to_string());
            assert!(st.toggle_fold(0));
            assert!(st.fold_regions()[0].collapsed);
            assert!(st.toggle_fold(0), "toggling again expands");
            assert!(!st.fold_regions()[0].collapsed);
            assert!(!st.is_line_hidden(1), "interior revealed");
        });
    }

    #[test]
    fn r933_collapse_reanchors_caret_out_of_hidden_interior() {
        Owner::new().run(|| {
            // "fn f() {\n" — '{' at 7, first '\n' at 8 (opener line end).
            // Line 1 "    body" starts at 9; the caret at 13 sits on "body".
            let src = "fn f() {\n    body\n}\n".to_string();
            let st = TextEditState::with_initial(src);
            st.set_caret(13);
            assert!(st.toggle_fold(0));
            assert_eq!(st.caret(), 8, "caret reanchored to opener line end");
            let starts = line_starts(&st.text());
            assert!(
                !st.is_line_hidden(line_of(&starts, st.caret())),
                "reanchored caret is on a visible line",
            );
        });
    }

    #[test]
    fn r933_fold_all_then_unfold_all() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a {\n b {\n  c\n }\n}\n".to_string());
            st.fold_all();
            assert!(
                st.fold_regions().iter().all(|r| r.collapsed),
                "all collapsed"
            );
            st.unfold_all();
            assert!(
                st.fold_regions().iter().all(|r| !r.collapsed),
                "all expanded"
            );
            assert!(!st.is_line_hidden(2));
        });
    }

    #[test]
    fn r933_stale_fold_anchor_pruned_when_brace_edited_away() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("x {\n y\n}\n".to_string());
            assert!(st.toggle_fold(0));
            assert_eq!(st.fold_regions().len(), 1);
            // Clearing the buffer removes every brace, so the collapsed
            // anchor matches no derived region — pruned, not panicking.
            st.set_text(String::new());
            assert!(
                st.fold_regions().is_empty(),
                "stale anchor yields no region"
            );
            assert!(!st.is_line_hidden(0));
        });
    }

    #[test]
    fn r933_set_text_clears_fold_set() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a {\n b\n}\n".to_string());
            assert!(st.toggle_fold(0));
            assert!(st.fold_regions()[0].collapsed);
            // A wholesale replace whose braces land on the SAME byte offsets
            // must not inherit the old collapse — `set_text` resets the set
            // (mirror of its style-run clear), else identical-shape content
            // would silently re-collapse.
            st.set_text("a {\n b\n}\n".to_string());
            assert!(
                !st.fold_regions()[0].collapsed,
                "set_text reset the fold set"
            );
            assert!(!st.is_line_hidden(1), "nothing hidden after the reset");
        });
    }

    #[test]
    fn r933_insert_before_collapsed_fold_tracks_the_anchor() {
        Owner::new().run(|| {
            // "fn f() {\n  x\n}" — '{' at byte 7. Collapse, then type before it.
            let st = TextEditState::with_initial("fn f() {\n  x\n}".to_string());
            assert!(st.toggle_fold(0));
            assert!(st.is_line_hidden(1), "interior hidden");
            st.set_caret(0);
            st.insert("pub ");
            // The anchor shifted with its '{'; the fold did NOT spring open.
            assert!(
                st.fold_regions()[0].collapsed,
                "fold survives an edit before it"
            );
            assert!(st.is_line_hidden(1), "interior still hidden");
        });
    }

    #[test]
    fn r933_insert_does_not_collapse_the_wrong_block() {
        Owner::new().run(|| {
            // Two sibling blocks: block-1 '{' at byte 2, block-2 '{' at byte 11
            // (distance 9). Collapse ONLY block-2, then insert exactly 9 bytes
            // before everything — without anchor-shifting the stale anchor (11)
            // would collide onto block-1's now-shifted '{' and hide the WRONG
            // block. The shift keeps the collapse on block-2.
            let st = TextEditState::with_initial("a {\n b\n}\nc {\n d\n}".to_string());
            assert_eq!(st.fold_regions().len(), 2);
            assert!(st.toggle_fold(3), "collapse block-2 (opens on line 3)");
            st.set_caret(0);
            st.insert("123456789"); // 9 bytes = block2_open - block1_open
            let regions = st.fold_regions();
            let b1 = regions.iter().find(|r| r.start_line == 0).unwrap();
            let b2 = regions.iter().find(|r| r.start_line == 3).unwrap();
            assert!(
                !b1.collapsed,
                "block-1 must NOT collapse (no anchor collision)"
            );
            assert!(b2.collapsed, "block-2 stays collapsed, tracking its brace");
        });
    }

    #[test]
    fn r933_deleting_the_opener_brace_drops_the_fold() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a {\n b\n}".to_string());
            assert!(st.toggle_fold(0));
            assert_eq!(st.fold_regions().len(), 1);
            // Delete the '{' (bytes [2, 3)). The anchor is inside the deleted
            // range → dropped; no foldable region remains.
            st.set_selection(2, 3);
            st.backspace();
            assert!(
                st.fold_regions().is_empty(),
                "deleting the opener brace prunes the fold"
            );
            assert!(!st.is_line_hidden(1), "nothing hidden");
        });
    }

    #[test]
    fn r933_undo_keeps_the_fold_anchor_valid() {
        Owner::new().run(|| {
            let st = undoable();
            st.set_text("fn f() {\n  x\n}".to_string());
            assert!(st.toggle_fold(0));
            assert!(st.fold_regions()[0].collapsed);
            // Type before the fold (anchor shifts forward), then undo (anchor
            // shifts back) — the fold stays valid across the round-trip.
            st.set_caret(0);
            st.insert("pub ");
            assert!(st.fold_regions()[0].collapsed, "fold tracks the insert");
            assert!(st.undo(), "undo the insert");
            assert!(
                st.fold_regions()[0].collapsed,
                "fold still valid after undo"
            );
            assert!(st.is_line_hidden(1));
        });
    }

    // ───────────────────────────────────────────────────────────────
    // R938 §5.22 — indent / dedent (Tab / Shift+Tab)
    // ───────────────────────────────────────────────────────────────

    #[test]
    fn r938_line_starts_in_range_covers_touched_lines() {
        // "ab\ncd\nef": line starts at 0, 3, 6.
        let t = "ab\ncd\nef";
        // A multi-line span touches the first two lines.
        assert_eq!(line_starts_in_range(t, 0, 4), vec![0, 3]);
        // A span ending exactly at a line start does NOT include that line
        // (the VS Code "select to column 0 of line N leaves N untouched" rule).
        assert_eq!(line_starts_in_range(t, 0, 6), vec![0, 3]);
        // One byte into the third line includes it.
        assert_eq!(line_starts_in_range(t, 0, 7), vec![0, 3, 6]);
        // A collapsed caret yields exactly its own line.
        assert_eq!(line_starts_in_range(t, 4, 4), vec![3]);
        assert_eq!(line_starts_in_range(t, 0, 0), vec![0]);
    }

    #[test]
    fn r938_dedent_remove_len_and_pos_shift() {
        assert_eq!(dedent_remove_len("    x", 0, 4), 4, "four leading spaces");
        assert_eq!(dedent_remove_len("  x", 0, 4), 2, "fewer than width");
        assert_eq!(
            dedent_remove_len("\tx", 0, 4),
            1,
            "a leading tab is one level"
        );
        assert_eq!(dedent_remove_len("x", 0, 4), 0, "no leading whitespace");
        // Re-anchor across removals (`inserted_len == 0`): after a run shifts
        // left; inside clamps to start.
        let edits = [(0_usize, 4_usize, 0_usize), (7, 2, 0)];
        assert_eq!(shift_pos_for_edits(11, &edits), 5, "after both removals");
        assert_eq!(
            shift_pos_for_edits(2, &edits),
            0,
            "inside the first run clamps"
        );
        assert_eq!(shift_pos_for_edits(0, &edits), 0, "before everything");
    }

    #[test]
    fn r938_tab_indents_flag_defaults_off() {
        let st = TextEditState::new();
        assert!(
            !st.tab_indents(),
            "off by default — single-line fields keep Tab=traverse"
        );
        st.set_tab_indents(true);
        assert!(st.tab_indents());
    }

    #[test]
    fn r1268_auto_indent_flag_defaults_off() {
        let st = TextEditState::new();
        assert!(
            !st.auto_indent(),
            "off by default — single-line fields keep Enter=submit"
        );
        st.set_auto_indent(true);
        assert!(st.auto_indent());
    }

    #[test]
    fn r1268_line_indent_end_scans_spaces_and_tabs_clamped() {
        // "    foo": four leading spaces, then 'f' at 4.
        let t = "    foo";
        assert_eq!(
            line_indent_end(t, 0, 7),
            4,
            "stops at the first non-indent byte"
        );
        assert_eq!(
            line_indent_end(t, 0, 2),
            2,
            "clamps to `at` inside the indent"
        );
        assert_eq!(line_indent_end(t, 0, 0), 0, "`at == ls` copies nothing");
        // Tabs are indent bytes; a flush-left line has none.
        assert_eq!(line_indent_end("\t\tx", 0, 3), 2, "leading tabs are indent");
        assert_eq!(
            line_indent_end("x", 0, 1),
            0,
            "flush-left line has no indent"
        );
        // The second line's own indent (ls past the first `\n`).
        assert_eq!(
            line_indent_end("a\n    b", 2, 7),
            6,
            "second line's 4-space indent"
        );
        // A carriage return ends the scan (never copied — the LF-oriented editor).
        assert_eq!(
            line_indent_end("  \r", 0, 3),
            2,
            "`\\r` is not an indent byte"
        );
    }

    #[test]
    fn r1268_insert_newline_plain_when_flag_off() {
        Owner::new().run(|| {
            // Flag defaults off → a plain newline, byte-identical to insert("\n").
            let st = TextEditState::with_initial("    foo".to_string());
            st.insert_newline();
            assert_eq!(
                st.text(),
                "    foo\n",
                "no indent copied when auto-indent is off"
            );
            assert_eq!(st.caret(), 8);
        });
    }

    #[test]
    fn r1268_insert_newline_copies_leading_indent() {
        Owner::new().run(|| {
            // Caret at the end of an indented line.
            let st = TextEditState::with_initial("    foo".to_string());
            st.set_auto_indent(true);
            st.insert_newline();
            assert_eq!(
                st.text(),
                "    foo\n    ",
                "the new line copies the 4-space indent"
            );
            assert_eq!(st.caret(), 12, "caret lands past the copied indent");
        });
    }

    #[test]
    fn r1268_insert_newline_clamps_copied_indent_to_the_caret() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("    foo".to_string());
            st.set_auto_indent(true);
            st.set_caret(2); // parked inside the 4-space indent
            st.insert_newline();
            // Split at 2: "  " + "\n  " + "  foo" — only the indent BEFORE the
            // caret is copied, never doubling the total indentation.
            assert_eq!(
                st.text(),
                "  \n    foo",
                "copies only the indent before the caret"
            );
            assert_eq!(
                st.caret(),
                5,
                "caret sits just past the 2-space copied indent"
            );
        });
    }

    #[test]
    fn r1268_insert_newline_no_indent_on_a_flush_left_line() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("foo".to_string());
            st.set_auto_indent(true);
            st.insert_newline();
            assert_eq!(
                st.text(),
                "foo\n",
                "a flush-left line yields a plain newline"
            );
            assert_eq!(st.caret(), 4);
        });
    }

    #[test]
    fn r1268_insert_newline_uses_the_caret_line_indent_not_the_first() {
        Owner::new().run(|| {
            // Caret at the end of the indented SECOND line.
            let st = TextEditState::with_initial("fn f() {\n        bar".to_string());
            st.set_auto_indent(true);
            st.insert_newline();
            assert_eq!(
                st.text(),
                "fn f() {\n        bar\n        ",
                "the newline copies the caret line's 8-space indent",
            );
        });
    }

    #[test]
    fn r1268_insert_newline_over_a_selection_uses_the_start_line_indent() {
        Owner::new().run(|| {
            // Select the whole tail from the indented first line; Enter replaces it.
            let st = TextEditState::with_initial("    ab\ncd".to_string());
            st.set_auto_indent(true);
            st.set_selection(4, 9);
            st.insert_newline();
            // The selection drains, the insert lands at byte 4 (line-1 start),
            // so the copied indent is line 1's "    " — not the collapsed caret's.
            assert_eq!(
                st.text(),
                "    \n    ",
                "indent copied from the selection-start line"
            );
            assert_eq!(st.caret(), 9);
        });
    }

    #[test]
    fn r1268_insert_newline_is_one_undo_step() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("    x".to_string());
            st.set_auto_indent(true);
            st.attach_undo(Rc::new(crate::undo::UndoStack::new()));
            st.insert_newline();
            assert_eq!(st.text(), "    x\n    ", "auto-indented newline");
            assert!(st.undo(), "one undo reverses the whole newline + indent");
            assert_eq!(
                st.text(),
                "    x",
                "the \\n and the copied indent vanish together"
            );
        });
    }

    #[test]
    fn r1270_insert_newline_is_an_undo_boundary_not_merged_into_typing() {
        // C3 audit fix: Enter is its own undo step. Typing a word then Enter,
        // one Ctrl+Z removes ONLY the newline — never the preceding word (the
        // pre-R1270 delegation to `insert` coalesced the `\n` backward into the
        // typing run, so undo wiped "foo" too).
        Owner::new().run(|| {
            let st = TextEditState::new();
            st.set_auto_indent(true);
            st.attach_undo(Rc::new(crate::undo::UndoStack::new()));
            st.insert("foo"); // a coalescable typing run (one Insert command)
            st.insert_newline(); // its own boundary step (flush-left: plain "\n")
            assert_eq!(st.text(), "foo\n");
            assert!(st.undo(), "one undo removes the newline");
            assert_eq!(
                st.text(),
                "foo",
                "the preceding typed word survives the boundary"
            );
            assert!(st.undo(), "a second undo removes the typed word");
            assert_eq!(st.text(), "");
        });
    }

    #[test]
    fn r938_indent_single_line_inserts_at_caret() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("abc".to_string());
            st.set_caret(1);
            assert!(st.indent_selection(INDENT_UNIT), "buffer changed");
            assert_eq!(st.text(), "a    bc", "Tab inserts the unit at the caret");
            assert_eq!(st.caret(), 5, "caret follows past the inserted unit");
        });
    }

    #[test]
    fn r938_indent_empty_unit_is_a_noop() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb".to_string());
            st.set_selection(0, 3);
            assert!(!st.indent_selection(""), "an empty unit changes nothing");
            assert_eq!(st.text(), "a\nb");
        });
    }

    #[test]
    fn r938_indent_multiline_indents_each_touched_line() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("ab\ncd".to_string());
            st.set_selection(0, 5);
            assert!(st.indent_selection(INDENT_UNIT));
            assert_eq!(st.text(), "    ab\n    cd", "every line gains one unit");
            // The selection re-covers the block (first line start → shifted end).
            assert_eq!(st.selection_range(), Some((0, 13)));
        });
    }

    #[test]
    fn r938_indent_is_one_undo_step() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb\nc".to_string());
            let stack = Rc::new(crate::undo::UndoStack::new());
            st.attach_undo(Rc::clone(&stack)); // attach AFTER seeding (no seed journal)
            st.set_selection(0, 5);
            assert!(st.indent_selection(INDENT_UNIT));
            assert_eq!(st.text(), "    a\n    b\n    c");
            assert_eq!(
                st.selection_range(),
                Some((0, 17)),
                "the block re-covers after indent"
            );
            assert_eq!(stack.len(), 1, "three line inserts fold into one undo step");
            assert!(st.undo(), "one undo reverses the whole block indent");
            assert_eq!(st.text(), "a\nb\nc");
            // R938.1 — undo restores the ORIGINAL selection: the first-applied
            // child (bottom line) carries the pre-indent caret/anchor and
            // undoes last (MacroCommand reverses children), so the macro's undo
            // — not the post-macro set_selection — fixes the final state.
            assert_eq!(
                st.selection_range(),
                Some((0, 5)),
                "undo restores the original selection"
            );
            assert!(st.redo());
            assert_eq!(st.text(), "    a\n    b\n    c", "one redo re-applies it");
        });
    }

    #[test]
    fn r938_indent_preserves_interior_fold() {
        // The R933.1 discipline: indenting a block that contains a collapsed
        // fold must SHIFT the fold anchor line-by-line, never clip it. A naive
        // whole-block splice would delete the interior fold; the per-line
        // macro keeps it.
        Owner::new().run(|| {
            let st = TextEditState::with_initial("fn a() {\n  x\n}\nz".to_string());
            st.attach_undo(Rc::new(crate::undo::UndoStack::new()));
            assert!(st.toggle_fold(0), "collapse the function body");
            assert!(st.is_line_hidden(1), "interior hidden");
            st.set_selection(0, st.text().len());
            assert!(st.indent_selection(INDENT_UNIT));
            assert_eq!(st.text(), "    fn a() {\n      x\n    }\n    z");
            assert!(st.fold_regions()[0].collapsed, "fold survives the indent");
            assert!(st.is_line_hidden(1), "interior still hidden");
            // And one undo restores both the text and the fold.
            assert!(st.undo());
            assert_eq!(st.text(), "fn a() {\n  x\n}\nz");
            assert!(
                st.fold_regions()[0].collapsed,
                "fold still valid after undo"
            );
        });
    }

    #[test]
    fn r938_indent_shifts_style_runs() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("ab\ncd".to_string());
            st.set_style_runs(vec![crun(0, 2, RED)]); // "ab" red
            st.set_selection(0, 5);
            assert!(st.indent_selection(INDENT_UNIT));
            // The run shifts with its text ("ab" is now at bytes [4, 6)),
            // never destroyed by the block edit.
            assert_eq!(st.style_runs(), vec![crun(4, 6, RED)]);
        });
    }

    #[test]
    fn r938_dedent_removes_leading_whitespace_per_line() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("    ab\n  cd".to_string());
            st.set_selection(0, st.text().len());
            assert!(st.dedent_selection(INDENT_WIDTH));
            assert_eq!(st.text(), "ab\ncd", "each line loses up to one unit");
            assert_eq!(st.selection_range(), Some((0, 5)));
        });
    }

    #[test]
    fn r938_dedent_is_a_noop_with_no_leading_whitespace() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("ab\ncd".to_string());
            let stack = Rc::new(crate::undo::UndoStack::new());
            st.attach_undo(Rc::clone(&stack));
            st.set_selection(0, 5);
            assert!(!st.dedent_selection(INDENT_WIDTH), "nothing to strip");
            assert_eq!(st.text(), "ab\ncd");
            assert_eq!(stack.len(), 0, "a no-op dedent journals nothing");
        });
    }

    #[test]
    fn r938_dedent_strips_a_leading_tab_as_one_level() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("\tab".to_string());
            st.set_caret(0);
            assert!(st.dedent_selection(INDENT_WIDTH));
            assert_eq!(st.text(), "ab", "a leading tab strips as one level");
        });
    }

    #[test]
    fn r938_dedent_collapsed_caret_dedents_its_own_line() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("x\n    yz".to_string());
            st.set_caret(st.text().len()); // caret at end, on the second line
            assert!(st.dedent_selection(INDENT_WIDTH));
            assert_eq!(st.text(), "x\nyz");
            assert_eq!(st.caret(), 4, "caret shifts left by the removed unit");
            assert!(!st.has_selection(), "a collapsed dedent stays collapsed");
        });
    }

    #[test]
    fn r938_dedent_is_one_undo_step() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("    a\n    b".to_string());
            let stack = Rc::new(crate::undo::UndoStack::new());
            st.attach_undo(Rc::clone(&stack));
            st.set_selection(0, st.text().len());
            assert!(st.dedent_selection(INDENT_WIDTH));
            assert_eq!(st.text(), "a\nb");
            assert_eq!(stack.len(), 1, "two line removals fold into one undo step");
            assert!(st.undo());
            assert_eq!(st.text(), "    a\n    b", "one undo restores both lines");
        });
    }

    // R939 §5.22 — line-comment toggle.

    #[test]
    fn r939_line_comment_flag_defaults_off() {
        let st = TextEditState::new();
        assert_eq!(
            st.line_comment(),
            None,
            "off by default — Ctrl+/ falls through"
        );
        st.set_line_comment("//");
        assert_eq!(st.line_comment(), Some("//"));
    }

    #[test]
    fn r939_line_first_non_ws_finds_column_or_none_for_blank() {
        assert_eq!(
            line_first_non_ws("  ab", 0),
            Some(2),
            "skips leading spaces"
        );
        assert_eq!(line_first_non_ws("\t\tx", 0), Some(2), "skips leading tabs");
        assert_eq!(line_first_non_ws("ab", 0), Some(0), "no indent");
        assert_eq!(
            line_first_non_ws("   \nx", 0),
            None,
            "whitespace-only line is blank"
        );
        assert_eq!(line_first_non_ws("", 0), None, "EOF is blank");
        // Mid-buffer: the line starting at byte 3 ("  y").
        assert_eq!(line_first_non_ws("a\n  y", 2), Some(4));
    }

    #[test]
    fn r939_comment_empty_marker_is_a_noop() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb".to_string());
            st.set_selection(0, 3);
            assert!(
                !st.toggle_line_comment(""),
                "an empty marker changes nothing"
            );
            assert_eq!(st.text(), "a\nb");
        });
    }

    #[test]
    fn r939_comment_collapsed_caret_toggles_its_own_line() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("ab\ncd".to_string());
            st.set_caret(4); // on the second line
            assert!(st.toggle_line_comment("//"), "comments the caret's line");
            assert_eq!(st.text(), "ab\n// cd", "marker + space at the column");
            assert!(!st.has_selection(), "a collapsed toggle stays collapsed");
            // Toggling again removes the marker this toggle just added (a
            // round-trip on toggle-produced text, not a general involution).
            st.set_caret(st.text().len());
            assert!(st.toggle_line_comment("//"));
            assert_eq!(st.text(), "ab\ncd", "second toggle uncomments");
        });
    }

    #[test]
    fn r939_comment_adds_at_first_non_ws_preserving_indent() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("  ab\n    cd".to_string());
            st.set_selection(0, st.text().len());
            assert!(st.toggle_line_comment("//"));
            // The marker hugs the code, each line keeps its own indent.
            assert_eq!(st.text(), "  // ab\n    // cd");
            assert_eq!(
                st.selection_range(),
                Some((0, st.text().len())),
                "block re-covers"
            );
        });
    }

    #[test]
    fn r939_comment_round_trips_over_mixed_indents() {
        // Comment-then-uncomment restores the original — a round-trip on clean
        // (un-commented) input. NOT a general involution: a line already
        // commented with no space (`"//x"`) does not survive two toggles (see
        // r939_comment_remove_strips_only_one_following_space) — the toggle is
        // a round-trip only for text its own add-path produced.
        Owner::new().run(|| {
            let original = "x\n  y\n    z".to_string();
            let st = TextEditState::with_initial(original.clone());
            st.set_selection(0, st.text().len());
            assert!(st.toggle_line_comment("//"), "comment all");
            assert_eq!(st.text(), "// x\n  // y\n    // z");
            st.set_selection(0, st.text().len());
            assert!(st.toggle_line_comment("//"), "uncomment all");
            assert_eq!(st.text(), original, "round-trips to the original");
        });
    }

    #[test]
    fn r939_comment_skips_a_blank_crlf_line() {
        // R939.1 — a CRLF-only line (`\r\n`) is blank: `\r` counts as leading
        // whitespace, so the toggle never inserts a marker before it (the
        // "never comments an empty line" contract holds for CRLF too).
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\r\n\r\nb".to_string());
            st.set_selection(0, st.text().len());
            assert!(st.toggle_line_comment("//"));
            assert_eq!(
                st.text(),
                "// a\r\n\r\n// b",
                "the bare CRLF line takes no marker"
            );
        });
    }

    #[test]
    fn r939_comment_partial_block_adds_to_all() {
        Owner::new().run(|| {
            // One line already commented, one not → NOT all-commented → add to
            // both (VS Code "Toggle Line Comment" — the second marker stacks).
            let st = TextEditState::with_initial("// a\nb".to_string());
            st.set_selection(0, st.text().len());
            assert!(st.toggle_line_comment("//"));
            assert_eq!(
                st.text(),
                "// // a\n// b",
                "adds to every line when not all are commented"
            );
        });
    }

    #[test]
    fn r939_comment_skips_blank_lines() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\n\nb".to_string());
            st.set_selection(0, st.text().len());
            assert!(st.toggle_line_comment("//"));
            assert_eq!(st.text(), "// a\n\n// b", "the blank line takes no marker");
            // The blank line is excluded from the verdict, so a re-toggle still
            // sees "all non-blank commented" and removes.
            st.set_selection(0, st.text().len());
            assert!(st.toggle_line_comment("//"));
            assert_eq!(st.text(), "a\n\nb");
        });
    }

    #[test]
    fn r939_comment_only_blank_lines_is_a_noop() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("   \n\t".to_string());
            st.set_selection(0, st.text().len());
            assert!(!st.toggle_line_comment("//"), "nothing to comment");
            assert_eq!(st.text(), "   \n\t");
        });
    }

    #[test]
    fn r939_comment_remove_strips_only_one_following_space() {
        Owner::new().run(|| {
            // A doubly-spaced comment keeps the extra space on uncomment (only
            // the one space the toggle itself inserts is stripped).
            let st = TextEditState::with_initial("//  ab".to_string());
            st.set_caret(0);
            assert!(st.toggle_line_comment("//"));
            assert_eq!(
                st.text(),
                " ab",
                "marker + one space removed, extra space kept"
            );
        });
    }

    #[test]
    fn r939_comment_is_one_undo_step() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb\nc".to_string());
            let stack = Rc::new(crate::undo::UndoStack::new());
            st.attach_undo(Rc::clone(&stack)); // attach AFTER seeding
            st.set_selection(0, st.text().len());
            assert!(st.toggle_line_comment("//"));
            assert_eq!(st.text(), "// a\n// b\n// c");
            assert_eq!(stack.len(), 1, "three line inserts fold into one undo step");
            assert!(st.undo(), "one undo reverses the whole block comment");
            assert_eq!(st.text(), "a\nb\nc");
            assert!(st.redo());
            assert_eq!(st.text(), "// a\n// b\n// c", "one redo re-applies it");
        });
    }

    #[test]
    fn r939_comment_shifts_style_runs() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("ab\ncd".to_string());
            st.set_style_runs(vec![crun(3, 5, RED)]); // "cd" red
            st.set_selection(0, st.text().len());
            assert!(st.toggle_line_comment("//"));
            // "cd" gained a "// " (3 bytes) before it on its line, plus the
            // first line's "// " (3 bytes) earlier → run shifts right by 6.
            assert_eq!(st.text(), "// ab\n// cd");
            assert_eq!(st.style_runs(), vec![crun(9, 11, RED)]);
        });
    }

    #[test]
    fn r939_comment_preserves_interior_fold() {
        // The R933.1 discipline (cross-applied from indent): toggling comments
        // on a block containing a collapsed fold must SHIFT the fold anchor
        // line-by-line, never clip it.
        Owner::new().run(|| {
            let st = TextEditState::with_initial("fn a() {\n  x\n}\nz".to_string());
            st.attach_undo(Rc::new(crate::undo::UndoStack::new()));
            assert!(st.toggle_fold(0), "collapse the function body");
            assert!(st.is_line_hidden(1), "interior hidden");
            st.set_selection(0, st.text().len());
            assert!(st.toggle_line_comment("//"));
            assert_eq!(st.text(), "// fn a() {\n  // x\n// }\n// z");
            assert!(
                st.fold_regions()[0].collapsed,
                "fold survives the comment toggle"
            );
            assert!(st.is_line_hidden(1), "interior still hidden");
            assert!(st.undo());
            assert_eq!(st.text(), "fn a() {\n  x\n}\nz");
            assert!(
                st.fold_regions()[0].collapsed,
                "fold still valid after undo"
            );
        });
    }

    #[test]
    fn r939_shift_pos_for_edits_handles_inserts_and_mixed() {
        // Pure inserts (the comment-add case): a position after an insert
        // shifts right by its length; before is unchanged.
        let inserts = [(2_usize, 0_usize, 3_usize), (8, 0, 3)];
        assert_eq!(shift_pos_for_edits(0, &inserts), 0, "before everything");
        assert_eq!(shift_pos_for_edits(5, &inserts), 8, "past the first insert");
        assert_eq!(shift_pos_for_edits(10, &inserts), 16, "past both inserts");
        // A mixed edit (replace 4 bytes [4,8) with 2): a position inside the
        // removed run clamps *into* the 2-byte replacement [4,6); after the run
        // shifts by the net. (Neither real consumer produces a mixed edit —
        // dedent / comment-remove have `inserted == 0`, so this clamps to the
        // run start there; comment-add has `removed == 0`, so no straddle.)
        let mixed = [(4_usize, 4_usize, 2_usize)];
        assert_eq!(
            shift_pos_for_edits(5, &mixed),
            5,
            "one byte into the run → into the replacement"
        );
        assert_eq!(
            shift_pos_for_edits(7, &mixed),
            6,
            "deep in the run clamps to the replacement end"
        );
        assert_eq!(
            shift_pos_for_edits(10, &mixed),
            8,
            "after the run shifts by net -2"
        );
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
            assert_eq!(
                st.selection_range(),
                Some((3, 5)),
                "advanced to 'at' in bat"
            );
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
                crate::syntax::highlight_code(
                    t,
                    &["let"],
                    crate::syntax::SyntaxPalette::classic(),
                    16,
                )
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
                crate::syntax::highlight_code(
                    t,
                    &["fn"],
                    crate::syntax::SyntaxPalette::classic(),
                    16,
                )
            }));
            assert!(st.style_runs().is_empty(), "empty buffer → no tokens");
            st.insert("fn");
            assert_eq!(
                st.style_runs().len(),
                1,
                "typing a keyword re-highlights it"
            );
            // The shadowed manual runs stay empty under a highlighter.
            assert!(
                st.style_runs.get().is_empty(),
                "manual runs shadowed, not written"
            );
        });
    }

    // ─── R941 go-to-line ──────────────────────────────────────────

    #[test]
    fn r941_line_count_counts_logical_lines() {
        let st = TextEditState::new();
        assert_eq!(st.line_count(), 1, "empty buffer is one line");
        st.set_text("solo".to_string());
        assert_eq!(st.line_count(), 1, "a single line, no newline");
        st.set_text("a\nb\nc".to_string());
        assert_eq!(st.line_count(), 3);
        st.set_text("a\nb\n".to_string());
        assert_eq!(
            st.line_count(),
            3,
            "a trailing newline opens an empty last line"
        );
    }

    #[test]
    fn r941_go_to_line_jumps_caret_to_line_start() {
        let st = TextEditState::new();
        st.set_text("zero\none\ntwo\nthree".to_string()); // starts [0, 5, 9, 13]
        assert_eq!(st.go_to_line(1), 1, "line 1 resolves to itself");
        assert_eq!(st.caret(), 0, "line 1 starts at byte 0");
        assert_eq!(st.go_to_line(3), 3);
        assert_eq!(st.caret(), 9, "line 3 (\"two\") starts at byte 9");
        assert_eq!(st.go_to_line(4), 4);
        assert_eq!(st.caret(), 13, "line 4 (\"three\") starts at byte 13");
    }

    #[test]
    fn r941_go_to_line_clamps_out_of_range() {
        let st = TextEditState::new();
        st.set_text("a\nb\nc".to_string()); // 3 lines, starts [0, 2, 4]
        assert_eq!(st.go_to_line(0), 1, "0 clamps up to the first line");
        assert_eq!(st.caret(), 0);
        assert_eq!(st.go_to_line(99), 3, "past the end clamps to the last line");
        assert_eq!(st.caret(), 4, "the last line's start");
    }

    #[test]
    fn r941_go_to_line_collapses_selection() {
        let st = TextEditState::new();
        st.set_text("a\nbb\nccc".to_string()); // starts [0, 2, 5]
        st.set_selection(0, 4); // a selection spanning lines 1-2
        assert!(st.has_selection());
        assert_eq!(st.go_to_line(3), 3);
        assert_eq!(st.caret(), 5, "caret at line 3 start");
        assert!(
            !st.has_selection(),
            "go_to_line collapses the selection (a caret move)"
        );
    }

    #[test]
    fn r957_line_start_byte_addresses_line_without_moving_caret() {
        // R957 — the pure byte-positioning SSOT under go_to_line. Same
        // line starts go_to_line jumps to, but a pure read: the caret /
        // selection are untouched, so a gutter Shift+click can extend to
        // a line's start without the collapse go_to_line forces.
        let st = TextEditState::new();
        st.set_text("zero\none\ntwo\nthree".to_string()); // starts [0, 5, 9, 13]
        st.set_caret(2);
        assert_eq!(st.line_start_byte(1), 0, "line 1 starts at byte 0");
        assert_eq!(
            st.line_start_byte(3),
            9,
            "line 3 (\"two\") starts at byte 9"
        );
        assert_eq!(
            st.line_start_byte(4),
            13,
            "line 4 (\"three\") starts at byte 13"
        );
        assert_eq!(
            st.caret(),
            2,
            "line_start_byte is a pure read — the caret did not move"
        );
        // Clamps like go_to_line: 0 / past-end land on the first / last line.
        assert_eq!(st.line_start_byte(0), 0, "0 clamps to the first line");
        assert_eq!(
            st.line_start_byte(99),
            13,
            "past the end clamps to the last line"
        );
    }

    // ─── R945 §5.22 — move-line / duplicate-line ────────────────────────────

    #[test]
    fn r945_move_line_down_swaps_with_next() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb\nc".to_string());
            st.set_caret(0); // caret on line "a"
            assert!(st.move_lines(true), "moved");
            assert_eq!(st.text(), "b\na\nc", "line a swaps below line b");
            assert_eq!(st.caret(), 2, "caret rides the moved line to its new start");
        });
    }

    #[test]
    fn r945_move_line_up_swaps_with_prev() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb\nc".to_string());
            st.set_caret(2); // caret on line "b"
            assert!(st.move_lines(false), "moved");
            assert_eq!(st.text(), "b\na\nc", "line b swaps above line a");
            assert_eq!(st.caret(), 0, "caret rides the moved line up");
        });
    }

    #[test]
    fn r945_move_line_is_a_noop_at_the_boundaries() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb".to_string());
            st.set_caret(0);
            assert!(!st.move_lines(false), "first line cannot move up");
            st.set_caret(2);
            assert!(!st.move_lines(true), "last line cannot move down");
            assert_eq!(st.text(), "a\nb", "no boundary move changed the buffer");
        });
    }

    #[test]
    fn r945_move_line_down_across_the_final_newlineless_line() {
        // The Case-B newline juggle: moving the second-to-last line down past the
        // last (newline-less) line. The lone `\n` relocates so the buffer keeps
        // exactly one line break and no trailing newline.
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb".to_string()); // "b" has no trailing \n
            st.set_caret(0); // caret on "a" (second-to-last)
            assert!(st.move_lines(true));
            assert_eq!(
                st.text(),
                "b\na",
                "newline relocates; no trailing newline added"
            );
            assert_eq!(st.caret(), 2, "caret rides the now-last line");
        });
    }

    #[test]
    fn r945_move_line_up_of_the_final_newlineless_line() {
        // The mirror Case-B: moving the last (newline-less) line up. It gains a
        // `\n` (now a middle line) and the previous line loses its `\n` (now last).
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb".to_string());
            st.set_caret(2); // caret on the last line "b"
            assert!(st.move_lines(false));
            assert_eq!(
                st.text(),
                "b\na",
                "the pair swap which line ends the buffer"
            );
            assert_eq!(st.caret(), 0, "caret rides 'b' to the top");
        });
    }

    #[test]
    fn r945_move_line_multiline_selection_moves_the_block() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb\nc\nd".to_string());
            st.set_selection(0, 3); // lines "a" and "b"
            assert!(st.move_lines(true), "block moves down past 'c'");
            assert_eq!(st.text(), "c\na\nb\nd", "the two-line block swaps past 'c'");
            // The re-cover spans the whole moved block "a\nb\n" = [2, 6); the
            // trailing newline is included (the natural "these two lines" extent,
            // and `line_starts_in_range` trims it on a repeated move).
            assert_eq!(
                st.selection_range(),
                Some((2, 6)),
                "the selection re-covers the moved block"
            );
        });
    }

    #[test]
    fn r945_move_line_is_one_undo_step() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb\nc".to_string());
            let stack = Rc::new(crate::undo::UndoStack::new());
            st.attach_undo(Rc::clone(&stack));
            st.set_caret(0);
            assert!(st.move_lines(true));
            assert_eq!(st.text(), "b\na\nc");
            assert_eq!(stack.len(), 1, "the reorder is one undo step");
            assert!(st.undo(), "one undo restores the order");
            assert_eq!(st.text(), "a\nb\nc");
        });
    }

    #[test]
    fn r945_duplicate_line_down_copies_below_caret_on_copy() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb".to_string());
            st.set_caret(0); // line "a"
            assert!(st.duplicate_lines(true));
            assert_eq!(st.text(), "a\na\nb", "a copy of 'a' is inserted below");
            assert_eq!(st.caret(), 2, "caret lands on the lower copy");
        });
    }

    #[test]
    fn r945_duplicate_line_up_copies_below_caret_on_original() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb".to_string());
            st.set_caret(0);
            assert!(st.duplicate_lines(false));
            assert_eq!(
                st.text(),
                "a\na\nb",
                "the buffer gains an identical adjacent copy"
            );
            assert_eq!(st.caret(), 0, "caret stays on the upper instance");
        });
    }

    #[test]
    fn r945_duplicate_final_newlineless_line_adds_a_separator() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb".to_string()); // "b" has no trailing \n
            st.set_caret(2); // line "b"
            assert!(st.duplicate_lines(true));
            assert_eq!(
                st.text(),
                "a\nb\nb",
                "a separator newline precedes the copy"
            );
            assert_eq!(st.caret(), 4, "caret lands on the lower copy");
        });
    }

    #[test]
    fn r945_duplicate_line_is_one_undo_step() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("a\nb\nc".to_string());
            let stack = Rc::new(crate::undo::UndoStack::new());
            st.attach_undo(Rc::clone(&stack));
            st.set_caret(0);
            assert!(st.duplicate_lines(true));
            assert_eq!(st.text(), "a\na\nb\nc");
            assert_eq!(stack.len(), 1, "the insertion is one undo step");
            assert!(st.undo());
            assert_eq!(st.text(), "a\nb\nc", "one undo removes the copy");
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R951 §5.36 — active typing marks (collapsed-caret formatting)
    // ─────────────────────────────────────────────────────────────

    /// A distinct, non-default mark for the typing-attribute battery.
    fn bold_style() -> TextStyle {
        let mut s = TextStyle::new();
        s.font_weight = crate::style::FontWeight::BOLD;
        s
    }

    #[test]
    fn r951_typing_mark_styles_inserted_text() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial(String::new());
            st.set_pending_style(Some(bold_style()));
            st.insert("X");
            assert_eq!(st.text(), "X");
            let runs = st.style_runs();
            assert_eq!(runs.len(), 1, "the armed mark styles the typed char");
            assert_eq!((runs[0].start, runs[0].end), (0, 1));
            assert_eq!(
                runs[0].style,
                bold_style(),
                "the run carries the armed style"
            );
        });
    }

    #[test]
    fn r951_typing_mark_continues_across_keystrokes() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial(String::new());
            st.set_pending_style(Some(bold_style()));
            st.insert("a");
            st.insert("b");
            assert_eq!(st.text(), "ab");
            let runs = st.style_runs();
            assert_eq!(
                runs.len(),
                1,
                "consecutive marked keystrokes coalesce to one run"
            );
            assert_eq!((runs[0].start, runs[0].end), (0, 2));
        });
    }

    #[test]
    fn r951_navigation_clears_typing_mark() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("abc".to_string());
            st.set_caret(1);
            st.set_pending_style(Some(bold_style()));
            assert!(st.pending_style().is_some(), "armed at the collapsed caret");
            st.move_right();
            assert!(
                st.pending_style().is_none(),
                "moving the caret drops the mark"
            );
            st.insert("Z"); // at caret 2 in "abc" -> "abZc"; left neighbour 'b' is unstyled
            assert!(
                st.style_runs().is_empty(),
                "no mark + unstyled neighbour -> unstyled insert"
            );
        });
    }

    #[test]
    fn r951_edit_preserves_typing_mark() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial(String::new());
            st.set_pending_style(Some(bold_style()));
            st.insert("a"); // bold "a", mark stays armed
            st.backspace(); // an edit, not a navigation
            assert_eq!(st.text(), "");
            assert!(
                st.pending_style().is_some(),
                "an edit keeps the mark armed (the Word convention)"
            );
            st.insert("b");
            let runs = st.style_runs();
            assert_eq!(runs.len(), 1, "the preserved mark styles the re-typed char");
            assert_eq!((runs[0].start, runs[0].end), (0, 1));
        });
    }

    #[test]
    fn r951_style_at_caret_inherits_from_left() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("abcd".to_string());
            st.set_style_runs(vec![StyleRun::new(0, 2, bold_style())]); // "ab" bold
            st.set_caret(2); // right edge of the bold run
            assert_eq!(
                st.style_at_caret(),
                Some(bold_style()),
                "caret after a bold char inherits bold"
            );
            st.set_caret(3); // after the unstyled 'c'
            assert_eq!(
                st.style_at_caret(),
                None,
                "caret after unstyled inherits the base"
            );
            st.set_caret(0); // no char to the left
            assert_eq!(
                st.style_at_caret(),
                None,
                "caret at the start is the field base"
            );
        });
    }

    #[test]
    fn r951_pending_distinguishes_armed_from_inherited() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("abcd".to_string());
            st.set_style_runs(vec![StyleRun::new(0, 2, bold_style())]);
            st.set_caret(2);
            assert_eq!(st.style_at_caret(), Some(bold_style()), "inheriting bold");
            assert_eq!(
                st.pending_style(),
                None,
                "inherited is not the same as armed"
            );
            let mut big = TextStyle::new();
            big.font_size_px = 24;
            st.set_pending_style(Some(big.clone()));
            assert_eq!(
                st.pending_style(),
                Some(big.clone()),
                "the armed mark is reported"
            );
            assert_eq!(
                st.style_at_caret(),
                Some(big),
                "the armed mark overrides inherited"
            );
        });
    }

    #[test]
    fn r951_format_toggle_routes_selection_vs_caret() {
        Owner::new().run(|| {
            let base = TextStyle::new();
            // selection path -> merge onto the runs, no pending mark.
            let st = TextEditState::with_initial("hello".to_string());
            st.set_selection(0, 3);
            st.format_at_caret_or_selection(&base, |s| {
                s.font_weight = crate::style::FontWeight::BOLD;
            });
            assert!(
                st.pending_style().is_none(),
                "the selection path arms no mark"
            );
            let runs = st.style_runs();
            assert_eq!(runs.len(), 1, "the selection got a bold run");
            assert_eq!((runs[0].start, runs[0].end), (0, 3));
            assert_eq!(runs[0].style.font_weight, crate::style::FontWeight::BOLD);

            // collapsed path -> arm a pending mark, runs untouched.
            let st2 = TextEditState::with_initial("hello".to_string());
            st2.set_caret(2);
            st2.format_at_caret_or_selection(&base, |s| {
                s.font_weight = crate::style::FontWeight::BOLD;
            });
            assert!(
                st2.style_runs().is_empty(),
                "the collapsed path touches no runs"
            );
            assert_eq!(
                st2.pending_style().map(|s| s.font_weight),
                Some(crate::style::FontWeight::BOLD),
                "the collapsed path arms the mark"
            );
        });
    }

    #[test]
    fn r951_styled_typing_undo_redo_roundtrip() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial(String::new());
            let stack = Rc::new(crate::undo::UndoStack::new());
            st.attach_undo(Rc::clone(&stack));
            st.set_pending_style(Some(bold_style()));
            st.insert("a");
            st.insert("b");
            assert_eq!(st.text(), "ab");
            assert_eq!(st.style_runs().len(), 1, "typed bold run");
            assert_eq!(stack.len(), 1, "coalesced typing is one undo step");
            // Undo removes the text *and* the overlaid mark (clipped with the bytes).
            assert!(st.undo());
            assert_eq!(st.text(), "");
            assert!(
                st.style_runs().is_empty(),
                "undo removes the bold run with the bytes"
            );
            // Redo restores the text *and* the mark — the `inserted_runs` peer
            // (clip+shift alone cannot re-derive an added run).
            assert!(st.redo());
            assert_eq!(st.text(), "ab");
            let runs = st.style_runs();
            assert_eq!(
                runs.len(),
                1,
                "redo restores the overlaid mark, not just the text"
            );
            assert_eq!((runs[0].start, runs[0].end), (0, 2));
            assert_eq!(runs[0].style, bold_style());
        });
    }

    #[test]
    fn r951_set_pending_style_none_clears() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial(String::new());
            st.set_pending_style(Some(bold_style()));
            st.set_pending_style(None);
            st.insert("a");
            assert!(
                st.style_runs().is_empty(),
                "a cleared mark -> unstyled insert"
            );
        });
    }

    #[test]
    fn r951_mark_inert_while_selection_active() {
        Owner::new().run(|| {
            let st = TextEditState::with_initial("hello".to_string());
            st.set_pending_style(Some(bold_style()));
            st.set_selection(1, 3);
            assert!(
                st.pending_style().is_none(),
                "forming a selection clears / masks the mark"
            );
        });
    }

    #[test]
    fn r952_1_clear_selection_drops_a_mark_no_resurrection() {
        // R952.1 — a mark armed while a selection is active (inert under the
        // has_selection guard) must NOT resurrect when the selection collapses
        // via clear_selection.
        Owner::new().run(|| {
            let st = TextEditState::with_initial("hello".to_string());
            st.set_selection(1, 3);
            st.set_pending_style(Some(bold_style())); // armed during selection -> inert
            assert!(
                st.pending_style().is_none(),
                "inert while the selection is active"
            );
            st.clear_selection(); // collapse
            assert!(
                st.pending_style().is_none(),
                "the mark does not resurrect on collapse"
            );
            st.set_caret(0);
            st.insert("X");
            assert!(
                st.style_runs().is_empty(),
                "the next typed char is unstyled"
            );
        });
    }

    #[test]
    fn r952_1_ime_composition_start_drops_the_mark() {
        // R952.1 — starting an IME composition clears a pending mark, so the
        // deferral is consistent: composed text and the next direct keystroke
        // are both unstyled (the mark no longer skips the composed text only to
        // re-apply to the following char).
        Owner::new().run(|| {
            let st = TextEditState::with_initial("ab".to_string());
            st.set_caret(2);
            st.set_pending_style(Some(bold_style()));
            assert!(st.pending_style().is_some(), "armed before composition");
            st.preedit_start();
            assert!(st.pending_style().is_none(), "IME start drops the mark");
            st.preedit_commit("c");
            assert!(st.style_runs().is_empty(), "the committed text is unstyled");
            st.insert("d");
            assert!(
                st.style_runs().is_empty(),
                "the next direct keystroke is also unstyled"
            );
        });
    }
}
