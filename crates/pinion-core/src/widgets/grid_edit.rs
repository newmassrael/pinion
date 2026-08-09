//! R1544 §5.27 — the **view's half** of Model/View cell editing: which cell
//! has an open editor, what opens one, and how an edit ends.
//!
//! ## What was missing
//!
//! R1532 gave the virtualized grid a per-column **paint** delegate (the
//! toolkit `paint`) and R1535 / R1536 gave the model its decoration role. The
//! other half of the toolkit's delegate — `createEditor`, `setEditorData`, `setModelData` — did not exist, so
//! *the grid's cell path could not host an editor at all*.
//!
//! The absence showed up the way an absent extension point always does: as
//! workaround code. Four bindings in this tree edit cells — the data grid, the
//! property grid, the inspector and the node editor — and **not one of them
//! uses the grid's cell path**; two do not use the grid painter at all. Each
//! carries its own copy of the same five things: a latch signal holding the
//! open `(row, col)`, a rule for what opens it, an inline field painted in
//! place of the label, a seed read off the model, and a commit that parses the
//! buffer back. The pieces those copies are built from were already lifted —
//! [`CellKind::accepts_keystroke`](crate::cell_value::CellKind::accepts_keystroke),
//! [`CellKind::parse`](crate::cell_value::CellKind::parse),
//! [`CellValue::edit_text`](crate::cell_value::CellValue::edit_text),
//! [`edit_field_keymap`](crate::input::edit_field_keymap) — so what was absent
//! was never the mechanism. It was the **seam** that lets the grid reach them.
//!
//! ## The decomposition
//!
//! The toolkit's, kept: the **model** owns the datum (is this cell editable,
//! what does an editor open with, does the write succeed), the **delegate**
//! owns the editor's appearance, and the **view** owns the latch and the
//! triggers. This module is the view's third; the model's is [`CellEdit`] and `GridModel::edit`,
//! and the delegate's is `VirtualTableData::editor`.
//!
//! It is a state substrate rather than paint for the same reason
//! [`GridSortState`](crate::widgets::grid_sort::GridSortState) and
//! [`ScrollState`](crate::widgets::scroll::ScrollState) are: the binding's
//! `External` **mutates** it on input and the view / a11y tree **read** it
//! through the same `Rc`, so the cell that paints an editor is the cell the
//! keystrokes route to, by construction rather than by two agreeing copies.
//!
//! ## Past the toolkit 6.11
//!
//! - **A rejected write keeps the editor open.**
//!   `setModelData` returns `void` and
//!   `commitData` ignores what the model did, so a
//!   `setData` that returns `false` still closes the editor and the user's
//!   typing is discarded with no feedback. [`GridEditState::commit_with`]
//!   propagates the model's verdict: a refused write leaves the editor open
//!   with the text intact, which is what a validating DCC grid needs and what
//!   the toolkit cannot express.
//! - **The editing state is data.** the toolkit has no public way to ask a view whether
//!   a *transient* editor is open (`isPersistentEditorOpen` covers only the
//!   persistent kind), and the in-flight text lives inside an opaque
//!   widget. Here the latch is a signal and the editor is scene nodes, so
//!   `scene/snapshot` and an `ExternalIntrospect` slot both see them (§2 #7).
//! - **Every commit outcome is named** (R1555, [`CommitOutcome`]). The toolkit's
//!   `commitData` discards `setData`'s verdict, and its editors' validators mean
//!   a malformed value never reaches the commit at all — so "that is not a
//!   number" and "the model will not take 500" are the same event there, and
//!   neither is reported.
//!
//! ## R1555 — the editor a cell opens follows from its datum
//!
//! The delegate above is the **override** half of the toolkit's editing
//! decomposition (`setItemDelegateForColumn`). The other half is item editor factory: a registry from
//! the datum's type to an editor, which styled item delegate consults when no
//! delegate overrides it. That half did not exist, so one inline text field
//! was the built-in editor for all six [`CellKind`]s — including the two that refuse
//! every keystroke and parse to nothing. [`CellKind::editor_form`] is the registry, [`EditorForm`] its
//! answer, and this module's job is what follows from that answer: which
//! buffer holds the in-flight value ([`EditBuffer`]), which gesture verbs the form
//! accepts ([`toggle`](GridEditState::toggle) / [`select`](GridEditState::select) /
//! [`step`](GridEditState::step)), and one commit arc that serves all five forms.
//!
//! ## R1571 — an editor's persistence is a property of the editor
//!
//! R1544 and R1555 both closed with the same remaining item: the toolkit's
//! `openPersistentEditor(index)` keeps N editors open at once, and this held
//! one. R1555 also wrote down the prescription — widen
//! [`use_text_edit_state`]'s `&'static str` key so a per-cell buffer can be
//! cached at a runtime id, which [`Owner::cache`] has accepted since R685.C.
//!
//! **That prescription is wrong, and building it is how you find out.**
//! [`Owner::cache`] has `cache`, `cache_contains` and `cache_get_by_str` and
//! **no removal of any kind**: a slot lives until its owner drops. Keying an
//! editor's buffer by its cell would therefore retain one `TextEditState` per
//! cell *ever edited*, for the life of the window — unbounded growth on the
//! million-row models this axis is named for, and exactly the class R1550 built
//! `scene/memory` to see. An editor's buffer has to die with the editor, so the
//! buffers belong to the **editor set**, not to the owner's cache.
//!
//! What the set needs instead follows from a fact about *this* framework
//! rather than about the toolkit: there is exactly **one keyboard focus**. The
//! toolkit can afford N live line edits because each editor is a real widget
//! with its own focus; here only the editor that holds the keyboard can be
//! typed into, so only it needs a live buffer. Every other open editor's
//! in-flight text is **parked** in the latch ([`EditBuffer::Parked`]) and swapped back into the
//! field when focus returns to it. One field tag, one focus stop, one
//! composition target — the keystroke, IME and clipboard machinery is
//! untouched — and the editor set is plain data that can be windowed,
//! enumerated and published.
//!
//! ### Past the toolkit 6.11
//!
//! - **The set is enumerable.** `persistent` is a
//!   private `set<widget *>` and the only public question is
//!   `isPersistentEditorOpen(index)` — one index at a time, so a toolkit view cannot
//!   be asked *what* it has open; you must already know in order to ask.
//!   [`GridEditState::editors`] answers with the whole set, and
//!   `scene/grid_editors` puts it on the wire.
//! - **Focus is data.** Which open editor has the keyboard is
//!   [`OpenEditors::focused`]. In the toolkit it is `focusWidget()`
//!   reverse-mapped through the private `indexEditorHash` — that is, not
//!   answerable through any public API of the view.
//! - **<kbd>Escape</kbd> reverts a persistent editor.**
//!   `eventFilter` emits `closeEditor(editor,
//!   RevertModelCache)`, and `closeEditor` early-returns for
//!   a persistent editor — so in the toolkit <kbd>Escape</kbd> on one does **nothing at
//!   all**, the typed text stays, and the original is unrecoverable.
//!   [`GridEditState::cancel`] restores the seed and leaves the editor open.
//! - **The cost is windowed.** A persistent editor outside the painted window
//!   contributes no scene node and keeps its in-flight value.
//!   `updateEditorGeometries()` walks *every* editor on
//!   every scroll, so N persistent editors on a virtualized model are N live
//!   widgets repositioned per scroll event whether or not one of them is on
//!   screen.
//! - **A cell has at most one editor, by construction.** the toolkit keeps an
//!   index → widget hash and a separate persistence set, and `edit()` reuses
//!   whatever the hash holds; here [`OpenEditors`] is keyed by the cell and
//!   rejects a second entry for it, at construction and again on deserialize.

use std::ops::Range;
use std::rc::Rc;

use crate::cell_value::{CellEdit, CellKind, CellValue, EditorForm};
use crate::model_index::{CellIndex, GridExtent};
use crate::reactive::{Owner, Signal};
use crate::widgets::text_edit::use_text_edit_state;

/// R1544 §5.27 — one reason an editor opens: the toolkit's
/// `EditTrigger`.
///
/// The toolkit's `CurrentChanged` and `NoEditTriggers` are absent by construction rather than by
/// omission: the first is not a discrete event a binding dispatches (it is the
/// *absence* of a gate — a view that edits whatever the cursor lands on calls
/// [`GridEditState::begin`] from its cursor move, which is what "no trigger gate" means), and the
/// second is the empty set, spelled [`EditTriggers::NONE`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EditTrigger {
    /// A double-click on the cell (the toolkit `DoubleClicked`).
    DoubleClicked,
    /// A single click on an already-selected cell (the toolkit `SelectedClicked`) — the
    /// slow-double-click rename gesture a file browser has.
    SelectedClicked,
    /// The dedicated edit key on the current cell (the toolkit `EditKeyPressed`);
    /// <kbd>F2</kbd> on every desktop platform.
    EditKeyPressed,
    /// Any printable keystroke on the current cell (the toolkit `AnyKeyPressed`) — the
    /// spreadsheet type-to-replace gesture.
    AnyKeyPressed,
}

impl EditTrigger {
    /// The wire token, for the introspection surface a binding publishes.
    #[must_use]
    pub fn wire_token(self) -> &'static str {
        match self {
            EditTrigger::DoubleClicked => "double_clicked",
            EditTrigger::SelectedClicked => "selected_clicked",
            EditTrigger::EditKeyPressed => "edit_key",
            EditTrigger::AnyKeyPressed => "any_key",
        }
    }

    /// Parse a [`wire_token`](Self::wire_token). `None` for an unknown token.
    #[must_use]
    pub fn from_wire(token: &str) -> Option<Self> {
        match token {
            "double_clicked" => Some(EditTrigger::DoubleClicked),
            "selected_clicked" => Some(EditTrigger::SelectedClicked),
            "edit_key" => Some(EditTrigger::EditKeyPressed),
            "any_key" => Some(EditTrigger::AnyKeyPressed),
            _ => None,
        }
    }

    /// Every trigger, in wire order — the census a `to_wire` / `from_wire`
    /// round-trip test and a schema enumeration both walk, so neither can
    /// silently miss an arm a later round adds.
    pub const ALL: [EditTrigger; 4] = [
        EditTrigger::DoubleClicked,
        EditTrigger::SelectedClicked,
        EditTrigger::EditKeyPressed,
        EditTrigger::AnyKeyPressed,
    ];
}

/// R1544 §5.27 — which gestures open an editor: the toolkit's
/// `EditTriggers` flag set.
///
/// A set rather than a single mode because the gestures are independent — a
/// grid that opens on <kbd>F2</kbd> and on double-click has said two things,
/// not chosen between them.
///
/// # Why it persists as its wire form
///
/// `Signal<EditTriggers>` needs serde (the R36 §5.31 hot-reload bound), and
/// the obvious derive would persist the private bitmask. That makes the bit
/// *layout* a compatibility surface: inserting an [`EditTrigger`] arm ahead of
/// another shifts every bit above it, and a persisted `5` would silently come
/// back meaning a different pair of gestures. Serializing through
/// [`to_wire`](Self::to_wire) / [`from_wire`](Self::from_wire) persists the
/// **names**, which is what the value means and what the introspection surface
/// already publishes — one form, not two that can disagree.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Default, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(into = "String", try_from = "String")]
pub struct EditTriggers(u8);

/// The failure of [`EditTriggers::from_wire`] as an error type, for the
/// `try_from = "String"` serde bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditTriggersParseError(String);

impl core::fmt::Display for EditTriggersParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "not an edit-trigger set: {:?}", self.0)
    }
}

impl std::error::Error for EditTriggersParseError {}

impl From<EditTriggers> for String {
    fn from(triggers: EditTriggers) -> Self {
        triggers.to_wire()
    }
}

impl TryFrom<String> for EditTriggers {
    type Error = EditTriggersParseError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        EditTriggers::from_wire(&s).ok_or(EditTriggersParseError(s))
    }
}

impl EditTriggers {
    /// No gesture opens an editor (the toolkit `NoEditTriggers`) — a read-only view. The
    /// [`Default`], so a grid is not accidentally editable.
    pub const NONE: Self = Self(0);

    /// The toolkit's own default for abstract item view: double-click, click
    /// on the selected cell, and the edit key. Type-to-replace is **not** in
    /// it, there or here — a grid whose arrow keys navigate cannot also treat
    /// every letter as the start of an edit unless it says so.
    pub const DEFAULT: Self = Self(
        Self::bit(EditTrigger::DoubleClicked)
            | Self::bit(EditTrigger::SelectedClicked)
            | Self::bit(EditTrigger::EditKeyPressed),
    );

    const fn bit(trigger: EditTrigger) -> u8 {
        1 << (trigger as u8)
    }

    /// This set plus `trigger`.
    #[must_use]
    pub const fn with(self, trigger: EditTrigger) -> Self {
        Self(self.0 | Self::bit(trigger))
    }

    /// This set minus `trigger`.
    #[must_use]
    pub const fn without(self, trigger: EditTrigger) -> Self {
        Self(self.0 & !Self::bit(trigger))
    }

    /// Whether `trigger` opens an editor.
    #[must_use]
    pub const fn contains(self, trigger: EditTrigger) -> bool {
        self.0 & Self::bit(trigger) != 0
    }

    /// Whether **no** gesture opens an editor — the toolkit's `NoEditTriggers` as a
    /// question rather than as a value to compare against.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The wire form: the members' tokens, `'|'`-joined in
    /// [`EditTrigger::ALL`] order, or `"none"` for the empty set.
    ///
    /// Ordered by the census rather than by insertion so the same set always
    /// prints the same string — an introspection slot whose value depended on
    /// the order a binding happened to build it in would make an assertion on
    /// it a test of the binding, not of the state.
    #[must_use]
    pub fn to_wire(self) -> String {
        if self.is_empty() {
            return "none".to_string();
        }
        EditTrigger::ALL
            .iter()
            .filter(|t| self.contains(**t))
            .map(|t| t.wire_token())
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Parse a [`to_wire`](Self::to_wire) form. `None` if any token is
    /// unknown — a partially-understood trigger set is a silently weaker
    /// gate, so it is rejected whole.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        if s == "none" {
            return Some(Self::NONE);
        }
        s.split('|').try_fold(Self::NONE, |acc, token| {
            EditTrigger::from_wire(token).map(|t| acc.with(t))
        })
    }
}

/// R1544 §5.27 — where the cursor goes when an edit ends: the toolkit's
/// `EndEditHint`.
///
/// The toolkit's `SubmitModelCache` / `RevertModelCache` are absent: they exist for data widget mapper's
/// buffered submit policy, which has no analogue here — [`GridEditState::commit_with`] writes through
/// to the model at the moment of commit, so there is no cache to submit or
/// revert. Shipping the arms would name two behaviours nothing implements.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, Hash)]
pub enum EndEditHint {
    /// Close the editor and leave the cursor where it is (the toolkit `NoHint`) —
    /// <kbd>Enter</kbd>.
    #[default]
    NoHint,
    /// Open an editor on the next editable cell (the toolkit `EditNextItem`) —
    /// <kbd>Tab</kbd>.
    EditNextItem,
    /// Open an editor on the previous editable cell (the toolkit `EditPreviousItem`) —
    /// <kbd>Shift+Tab</kbd>.
    EditPreviousItem,
}

/// R1571 §5.27 — whether an editor survives a commit and an <kbd>Escape</kbd>:
/// The toolkit's `persistent` membership.
///
/// A **property of the editor**, which is what makes "a cell has at most one
/// editor" true by construction. The toolkit models the same fact as a second
/// collection — an index → widget hash plus a `set<widget *>` of the ones
/// that survive — so the two can disagree about an editor that is in one and
/// not the other, and `openPersistentEditor` on an index that already has a
/// transient editor quietly *promotes* that widget by inserting it into the
/// set.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EditorPersistence {
    /// Closed by a successful commit and by <kbd>Escape</kbd>, and replaced
    /// when another cell opens one — the toolkit's default editor, the only
    /// kind an [`EditTrigger`] opens. At most one is open at a time.
    Transient,
    /// Stays open across commits and <kbd>Escape</kbd> until [`GridEditState::close_persistent`] — the
    /// toolkit's `openPersistentEditor`. Any number may be open.
    Persistent,
}

impl EditorPersistence {
    /// The wire token, for the introspection surface.
    #[must_use]
    pub fn wire_token(self) -> &'static str {
        match self {
            EditorPersistence::Transient => "transient",
            EditorPersistence::Persistent => "persistent",
        }
    }

    /// Parse a [`wire_token`](Self::wire_token). `None` for an unknown token.
    #[must_use]
    pub fn from_wire(token: &str) -> Option<Self> {
        match token {
            "transient" => Some(EditorPersistence::Transient),
            "persistent" => Some(EditorPersistence::Persistent),
            _ => None,
        }
    }

    /// Both arms, in wire order — the census a round-trip test walks.
    pub const ALL: [EditorPersistence; 2] =
        [EditorPersistence::Transient, EditorPersistence::Persistent];
}

/// R1555 §5.27 — **where** an open editor's in-flight value lives.
///
/// A function of the editor's [`EditorForm`] and — since R1571 — of whether it
/// holds the keyboard. The forms whose buffer is text
/// ([`EditorForm::buffer_is_text`]) put the value the user is producing in the
/// inline field's [`TextEditState`](crate::widgets::text_edit::TextEditState),
/// because that is the state the caret, the selection, the IME preedit and the
/// clipboard already act on. A toggle and a selector have no text at all, so
/// for them the latch holds the value.
///
/// The toolkit has the first split and states it nowhere: a line edit editor's
/// in-flight value is its text and a combo box editor's is its current index,
/// and `setModelData` reaches each through a `qobject_cast` the delegate author has to get right.
/// Here it is one exhaustive answer per form, so the latch cannot look for the
/// value in the half that does not hold it.
///
/// R1571 adds the second split, and it exists because this framework has one
/// keyboard focus where the toolkit has one focusable widget per editor: a
/// text-buffered editor that does **not** hold the field parks its text here
/// instead.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EditBuffer {
    /// The inline field's buffer is the authority — a text-buffered form
    /// ([`EditorForm::Field`], [`EditorForm::Stepper`], and the hex half of
    /// [`EditorForm::Swatch`]) that currently holds the keyboard. At most one
    /// editor in a [`OpenEditors`] is in this state, and it is the focused one.
    Live,
    /// A text-buffered form that does **not** hold the keyboard: its in-flight
    /// text, parked until focus returns to it.
    Parked(String),
    /// The latch holds the in-flight value — [`EditorForm::Toggle`] and
    /// [`EditorForm::Selector`], focused or not, because neither has any text.
    Value(CellValue),
}

/// R1544 §5.27 — the editor currently open: which cell, what it was seeded
/// with, and which editor kind is hosting it.
///
/// One value rather than three parallel signals, so opening and closing are
/// single transitions. Three signals could hold a latched index whose kind
/// belongs to the previously edited column — a state with no meaning that
/// nothing would reject.
///
/// R1555 — the seed is the **datum**, not its text, for the reason
/// [`CellEdit`] carries one: a [`EditorForm::Selector`]'s editor needs the
/// option domain, which no string carries. [`kind`](Self::kind) and
/// [`form`](Self::form) are derivations of it, and its private `buffer` field is
/// built only by [`GridEditState::begin`] — so a latch whose buffer does not
/// match its form is not a state a caller can reach.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OpenEditor {
    /// The cell the editor is open on (the toolkit: the view's editing model
    /// index).
    pub index: CellIndex,
    /// The `EditRole` datum the editor was opened with — kept so a commit
    /// can tell an untouched editor from an edited one, and so a delegate's
    /// editor can render the original beside the in-flight value.
    seed: CellValue,
    /// Where the in-flight value lives, from [`EditorForm::buffer_is_text`]
    /// and — R1571 — from whether this editor holds the keyboard.
    buffer: EditBuffer,
    /// R1571 — whether a commit or an <kbd>Escape</kbd> closes it.
    persistence: EditorPersistence,
}

impl OpenEditor {
    /// The datum the editor opened with.
    #[must_use]
    pub fn seed(&self) -> &CellValue {
        &self.seed
    }

    /// R1571 — whether a commit or an <kbd>Escape</kbd> closes this editor
    /// (the toolkit: whether the view's private `persistent` set contains its widget).
    #[must_use]
    pub fn persistence(&self) -> EditorPersistence {
        self.persistence
    }

    /// R1571 — whether this editor's buffer **is** the inline field, which is
    /// true exactly when it is the focused text-buffered editor.
    #[must_use]
    pub fn holds_the_field(&self) -> bool {
        matches!(self.buffer, EditBuffer::Live)
    }

    /// R1571 — the parked in-flight text of a text-buffered editor that does
    /// not hold the keyboard. `None` for the focused one (read it through
    /// [`GridEditState::text`]) and for the latch-buffered forms.
    #[must_use]
    pub fn parked_text(&self) -> Option<&str> {
        match &self.buffer {
            EditBuffer::Parked(text) => Some(text),
            EditBuffer::Live | EditBuffer::Value(_) => None,
        }
    }

    /// The editor kind: the keystroke gate
    /// ([`CellKind::accepts_keystroke`]) and the commit parser
    /// ([`CellKind::parse`]) both read it.
    #[must_use]
    pub fn kind(&self) -> CellKind {
        self.seed.kind()
    }

    /// R1555 — which editor form is hosting this edit (the toolkit
    /// item editor factory's answer for the cell's datum).
    #[must_use]
    pub fn form(&self) -> EditorForm {
        self.seed.kind().editor_form()
    }

    /// R1555 — the latch-held in-flight value, for the forms whose buffer is
    /// not text. `None` for the text-buffered forms, whose in-flight value is
    /// the inline field's — read it through [`GridEditState::state`].
    #[must_use]
    pub fn pending(&self) -> Option<&CellValue> {
        match &self.buffer {
            EditBuffer::Live | EditBuffer::Parked(_) => None,
            EditBuffer::Value(value) => Some(value),
        }
    }
}

/// R1571 §5.27 — every editor a grid has open, and which of them holds the
/// keyboard: the toolkit's `indexEditorHash` and its `persistent` subset, as one value.
///
/// # The invariants, and why they are the type's rather than the caller's
///
/// 1. **At most one editor per cell**, sorted by [`CellIndex`] — so
///    [`OpenEditors::get`] is a decision rather than a first-match, and two
///    editors cannot end up racing for one cell's keystrokes.
/// 2. **At most one [`EditorPersistence::Transient`] editor** — the toolkit maintains
///    this by having `edit()` close the previous one, which is a convention its
///    data structure does not hold it to.
/// 3. **`focused` names a member, or nothing** — so "the focused editor" is
///    never a dangling index into a set that has closed it. This is the
///    argument [`OpenEditor`] itself makes about its own three fields, one
///    level up.
/// 4. **The focused editor holds the shared field exactly when its form is
///    text-buffered** — *both* directions. One is obvious: the field is a
///    single shared buffer, so an unfocused editor claiming it would be reading
///    somebody else's typing. The converse is the one a counterfactual found
///    missing at R1571, and it is the load-bearing half: with only the first
///    rule, "nobody holds the field" satisfies every check while the focused
///    editor's cell still *paints* one, so that cell's tag would show a buffer
///    belonging to no editor. Stating it both ways makes the paint's question
///    ("does this editor own the field") and the state's ("who has the
///    keyboard") one fact rather than two that agree until they do not.
///
/// All four are re-checked on **deserialize** ([`OpenEditors::from_parts`]), so
/// a restored session or a hand-written `intervene` payload cannot reach a
/// state the constructors refuse to build — R1561's rule for `IndexRuns`,
/// applied to a set whose members are richer.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "OpenEditorsParts", into = "OpenEditorsParts")]
pub struct OpenEditors {
    /// Sorted by [`OpenEditor::index`], at most one entry per cell.
    open: Vec<OpenEditor>,
    /// The cell whose editor holds the keyboard; always a member of `open`.
    focused: Option<CellIndex>,
}

/// The serde form of [`OpenEditors`] — the two fields, before validation.
///
/// A separate type rather than `#[serde(deny_unknown_fields)]` on the real one
/// because the invariants are *between* the fields: no derive can say "this
/// index names a member of that vector".
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OpenEditorsParts {
    /// The open editors, in any order — [`OpenEditors::from_parts`] sorts them.
    pub open: Vec<OpenEditor>,
    /// The focused cell.
    pub focused: Option<CellIndex>,
}

/// Why a [`OpenEditorsParts`] is not a valid [`OpenEditors`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenEditorsError {
    /// Two editors name the same cell.
    DuplicateCell(CellIndex),
    /// More than one [`EditorPersistence::Transient`] editor.
    TwoTransientEditors,
    /// `focused` names a cell with no open editor.
    FocusOnNothing(CellIndex),
    /// The shared inline field's owner is not the focused text-buffered
    /// editor: either an unfocused editor claims it, or the focused
    /// text-buffered one does not hold it.
    FieldOwnerIsNotTheFocus(CellIndex),
}

impl core::fmt::Display for OpenEditorsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            OpenEditorsError::DuplicateCell(at) => {
                write!(f, "two editors open on cell {}_{}", at.row, at.col)
            }
            OpenEditorsError::TwoTransientEditors => {
                write!(f, "more than one transient editor is open")
            }
            OpenEditorsError::FocusOnNothing(at) => {
                write!(
                    f,
                    "focus names cell {}_{}, which has no editor",
                    at.row, at.col
                )
            }
            OpenEditorsError::FieldOwnerIsNotTheFocus(at) => write!(
                f,
                "cell {}_{} is not the shared field's owner but is recorded as one, \
                 or is its owner and is not recorded as one",
                at.row, at.col
            ),
        }
    }
}

impl std::error::Error for OpenEditorsError {}

impl From<OpenEditors> for OpenEditorsParts {
    fn from(editors: OpenEditors) -> Self {
        Self {
            open: editors.open,
            focused: editors.focused,
        }
    }
}

impl TryFrom<OpenEditorsParts> for OpenEditors {
    type Error = OpenEditorsError;

    fn try_from(parts: OpenEditorsParts) -> Result<Self, Self::Error> {
        OpenEditors::from_parts(parts.open, parts.focused)
    }
}

impl OpenEditors {
    /// Build from the two fields, checking every invariant the type's doc
    /// states. The one constructor, so the deserialize path and the verbs below
    /// cannot enforce different rules.
    ///
    /// # Errors
    ///
    /// [`OpenEditorsError`], naming the cell responsible where there is one.
    pub fn from_parts(
        mut open: Vec<OpenEditor>,
        focused: Option<CellIndex>,
    ) -> Result<Self, OpenEditorsError> {
        open.sort_by_key(|e| e.index);
        if let Some(dup) = open.windows(2).find(|w| w[0].index == w[1].index) {
            return Err(OpenEditorsError::DuplicateCell(dup[0].index));
        }
        if open
            .iter()
            .filter(|e| e.persistence == EditorPersistence::Transient)
            .count()
            > 1
        {
            return Err(OpenEditorsError::TwoTransientEditors);
        }
        if let Some(at) = focused
            && !open.iter().any(|e| e.index == at)
        {
            return Err(OpenEditorsError::FocusOnNothing(at));
        }
        for editor in &open {
            // Both directions — see invariant 4. `should_hold` is the whole
            // rule, written once, so a transition cannot satisfy half of it.
            let should_hold =
                focused == Some(editor.index) && editor.seed.kind().editor_form().buffer_is_text();
            if editor.holds_the_field() != should_hold {
                return Err(OpenEditorsError::FieldOwnerIsNotTheFocus(editor.index));
            }
        }
        Ok(Self { open, focused })
    }

    /// How many editors are open — the toolkit has no accessor at all for
    /// this, since the set it would count is private.
    #[must_use]
    pub fn len(&self) -> usize {
        self.open.len()
    }

    /// Whether no editor is open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// The editor open on `index`, or `None` — the toolkit's `isPersistentEditorOpen` generalized to both
    /// persistences, and answering with the editor rather than with a bool.
    #[must_use]
    pub fn get(&self, index: CellIndex) -> Option<&OpenEditor> {
        self.open
            .binary_search_by_key(&index, |e| e.index)
            .ok()
            .map(|i| &self.open[i])
    }

    /// Whether `index` has an editor open.
    #[must_use]
    pub fn contains(&self, index: CellIndex) -> bool {
        self.get(index).is_some()
    }

    /// Every open editor, in [`CellIndex`] order.
    pub fn iter(&self) -> impl Iterator<Item = &OpenEditor> {
        self.open.iter()
    }

    /// The editors whose cells are in `rows` — the **window** a paint pass
    /// needs, so an editor outside it costs nothing to draw.
    ///
    /// The property the toolkit cannot have: its persistent editors are
    /// widgets that exist and are repositioned by `updateEditorGeometries()` whether or not their row
    /// is on screen.
    pub fn in_rows(&self, rows: Range<usize>) -> impl Iterator<Item = &OpenEditor> {
        self.open
            .iter()
            .filter(move |e| rows.contains(&e.index.row))
    }

    /// The editor holding the keyboard, or `None`.
    #[must_use]
    pub fn focused(&self) -> Option<&OpenEditor> {
        self.focused.and_then(|at| self.get(at))
    }

    /// The cell whose editor holds the keyboard.
    #[must_use]
    pub fn focused_index(&self) -> Option<CellIndex> {
        self.focused
    }
}

/// R1555 §5.27 — the in-flight state of a grid's editor: whether one is open
/// and, if so, whether its buffer currently holds a value of the cell's kind.
///
/// One answer rather than an `Option<CellValue>`, because "no value" has two
/// causes that a caller must treat differently — nothing is being edited, and
/// something is being edited but is not yet a number — and a single `None`
/// would make a half-typed `-` indistinguishable from a closed editor.
#[derive(Clone, Debug, PartialEq)]
pub enum EditState {
    /// No editor is open.
    Closed,
    /// An editor is open and its buffer does not hold a value of the cell's
    /// kind: a half-typed number, a malformed `#RRGGBB`. The model is never
    /// asked to store one of these.
    Malformed,
    /// An editor is open holding this value.
    Value(CellValue),
}

/// R1555 §5.27 — what happened to a commit: the toolkit's `commitData` /
/// `setModelData` pair, with the outcomes named.
///
/// The toolkit's path answers nothing. `setModelData` returns `void`, `commitData` ignores what `setData`
/// did, and the editor closes either way — so a rejected value and an accepted
/// one are indistinguishable to the caller and the user's typing is gone in
/// the rejected case. R1544 kept the editor open on a refusal; this names
/// *why* a commit did not land, which is what lets a binding put a different
/// message on screen for "that is not a number" than for "the model will not
/// take 500".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitOutcome {
    /// No editor was open — nothing was written and nothing closed.
    NotEditing,
    /// The buffer does not hold a value of the cell's kind, so **the model was
    /// never asked**. The editor stays open holding the text.
    ///
    /// The toolkit cannot reach this state and pays for it: the editor's own
    /// validator keeps a malformed value from ever reaching `commitData`, which means
    /// the committed value is silently not the one the user typed.
    Malformed,
    /// The model refused a well-formed value. The editor stays open holding it,
    /// which is the only state the user can correct it from.
    Refused,
    /// Written through, and the editor closed.
    Committed(CellIndex),
}

impl CommitOutcome {
    /// The committed cell, or `None` for every non-landing outcome — the narrow
    /// question a caller that only needs "did it land" asks.
    #[must_use]
    pub fn committed(self) -> Option<CellIndex> {
        match self {
            CommitOutcome::Committed(index) => Some(index),
            CommitOutcome::NotEditing | CommitOutcome::Malformed | CommitOutcome::Refused => None,
        }
    }

    /// The wire token, for the introspection surface a binding publishes.
    #[must_use]
    pub fn wire_token(self) -> &'static str {
        match self {
            CommitOutcome::NotEditing => "not_editing",
            CommitOutcome::Malformed => "malformed",
            CommitOutcome::Refused => "refused",
            CommitOutcome::Committed(_) => "committed",
        }
    }
}

/// R1571 §5.27 — what [`GridEditState::open_persistent`] did: the toolkit's
/// `openPersistentEditor`, which returns `void`.
///
/// There is no failure arm, and its absence is the argument R1544 made about
/// the transient path, now on this one: the call takes a [`CellEdit`], which
/// only the model produces and which it produces `None` for on a cell it will
/// not edit — so "a persistent editor open on a read-only cell" is not a state
/// the types can express. The toolkit's `openPersistentEditor` reaches
/// `createEditor`, which consults item editor factory and
/// **never looks at `flags() & ItemIsEditable`**: it opens a live editor on
/// a read-only cell, the user types into it, `setModelData` calls a `setData`
/// that returns `false`, and nothing anywhere reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenOutcome {
    /// A new persistent editor opened on a cell that had none.
    Opened,
    /// The cell's [`EditorPersistence::Transient`] editor was **promoted**, keeping whatever the user had
    /// already typed into it. The toolkit reaches the same end by a different
    /// route: `editor()` hands back the widget already in its hash and `openPersistentEditor` inserts
    /// that widget into the persistence set — an outcome it cannot report.
    Promoted,
    /// The cell already had a persistent editor; nothing changed, and in
    /// particular its in-flight value was **not** reseeded from the model.
    AlreadyOpen,
}

impl OpenOutcome {
    /// The wire token, for the introspection surface.
    #[must_use]
    pub fn wire_token(self) -> &'static str {
        match self {
            OpenOutcome::Opened => "opened",
            OpenOutcome::Promoted => "promoted",
            OpenOutcome::AlreadyOpen => "already_open",
        }
    }
}

/// R1544 §5.27 — the grid's editing latch and trigger gate: the half of the
/// toolkit's editing decomposition that belongs to abstract item view.
///
/// Created once through [`use_grid_edit`] and shared by the binding's
/// `External` (which mutates it) and the view / a11y tree (which read it)
/// through the same `Rc` — the [`ScrollState`](crate::widgets::scroll::ScrollState)
/// pattern. Reading [`editors`](Self::editors) inside a view-fn auto-subscribes,
/// so opening an editor repaints exactly like a scroll-offset change.
///
/// R1571 — the latch became a **set** ([`OpenEditors`]), because the toolkit's `openPersistentEditor` keeps
/// N editors open at once. The invariants that set maintains are stated on it;
/// every verb below rebuilds it through one private publisher that re-checks
/// all four.
///
/// The **focused** editor's in-flight text is **not** held here. It lives in the
/// [`TextEditState`](crate::widgets::text_edit::TextEditState) of the inline
/// field the editor paints, keyed by [`field_tag`](Self::field_tag), because
/// that is the state the caret, the selection, the IME preedit and the
/// clipboard already act on. A second copy here would be the buffer the user
/// is *not* typing into.
pub struct GridEditState {
    /// The `use_grid_edit` cache key, for symmetry with
    /// [`ScrollState::with_tag`](crate::widgets::scroll::ScrollState::with_tag).
    tag: Option<&'static str>,
    /// The inline editor field's tag — the `use_text_edit_state` key whose
    /// buffer is the in-flight text.
    field_tag: &'static str,
    /// The editor field's buffer, resolved **once** at construction.
    ///
    /// Held rather than re-resolved per verb because the verbs run wherever
    /// input is dispatched — an `External`'s send arc has no ambient
    /// [`Owner`], so a `use_text_edit_state` call inside `begin` panicked the
    /// first time a pointer gesture opened an editor. Resolving at
    /// construction is also the shape every other state substrate here has:
    /// the `Rc` is captured once, in the scope that owns it.
    field: Rc<crate::widgets::text_edit::TextEditState>,
    /// R1571 — every open editor, and which one holds the field.
    editors: Signal<OpenEditors>,
    triggers: Signal<EditTriggers>,
}

impl GridEditState {
    /// Construct over the inline editor field tagged `field_tag`, with no
    /// editor open and [`EditTriggers::DEFAULT`].
    ///
    /// `field_tag` is the one transient editor's field — see the module's
    /// scope note on why there is exactly one.
    ///
    /// # Panics
    ///
    /// Panics if no current [`Owner`] is set: the editor field's shared
    /// buffer is resolved here, once, so that the editing verbs can run from
    /// an input arc that has no ambient scope.
    #[must_use]
    pub fn new(field_tag: &'static str) -> Self {
        Self {
            tag: None,
            field_tag,
            field: use_text_edit_state(field_tag),
            editors: Signal::new(OpenEditors::default()),
            triggers: Signal::new(EditTriggers::DEFAULT),
        }
    }

    /// As [`new`](Self::new) but records the [`use_grid_edit`] cache key.
    #[must_use]
    pub fn with_tag(key: &'static str, field_tag: &'static str) -> Self {
        Self {
            tag: Some(key),
            ..Self::new(field_tag)
        }
    }

    /// The [`use_grid_edit`] cache key, or `None` when constructed directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// The inline editor field's `use_text_edit_state` key.
    #[must_use]
    pub fn field_tag(&self) -> &'static str {
        self.field_tag
    }

    /// R1571 — every open editor and which of them holds the keyboard. Reading
    /// this in a view-fn subscribes it to every open / close / focus move.
    #[must_use]
    pub fn editors(&self) -> OpenEditors {
        self.editors.get()
    }

    /// The editor open on `index`, or `None` — the question the paint layer
    /// asks once per painted cell.
    #[must_use]
    pub fn editor_at(&self, index: CellIndex) -> Option<OpenEditor> {
        self.editors.get().get(index).cloned()
    }

    /// The editor holding the keyboard, or `None` when nothing is focused.
    ///
    /// R1571 renamed this from `open`: with N editors, "the open editor" is not
    /// a thing, and the one every keystroke verb below acts on is the focused
    /// one.
    #[must_use]
    pub fn focused(&self) -> Option<OpenEditor> {
        self.editors.get().focused().cloned()
    }

    /// The cell whose editor holds the keyboard — [`focused`](Self::focused)
    /// narrowed to its index.
    #[must_use]
    pub fn editing(&self) -> Option<CellIndex> {
        self.editors.get().focused_index()
    }

    /// Whether an editor — of either persistence — is open on `index`.
    #[must_use]
    pub fn is_editing(&self, index: CellIndex) -> bool {
        self.editors.get().contains(index)
    }

    /// R1571 — whether a **persistent** editor is open on `index`: the toolkit's
    /// `isPersistentEditorOpen`, which is the only question the toolkit's private editor set
    /// answers.
    #[must_use]
    pub fn is_persistent_editor_open(&self, index: CellIndex) -> bool {
        self.editors
            .get()
            .get(index)
            .is_some_and(|e| e.persistence() == EditorPersistence::Persistent)
    }

    /// R1571 — whether the editor on `index` holds the keyboard.
    #[must_use]
    pub fn is_focused(&self, index: CellIndex) -> bool {
        self.editors.get().focused_index() == Some(index)
    }

    /// The focused editor's kind — the
    /// [`edit_field_keymap`](crate::input::edit_field_keymap) keystroke gate's
    /// argument. `None` when nothing is focused.
    #[must_use]
    pub fn kind(&self) -> Option<CellKind> {
        self.focused().map(|e| e.kind())
    }

    /// R1555 — the focused editor's form (the toolkit item editor factory's
    /// answer for the cell's datum). `None` when nothing is focused.
    #[must_use]
    pub fn form(&self) -> Option<EditorForm> {
        self.focused().map(|e| e.form())
    }

    /// The in-flight editor text: the inline field's live buffer, **not** the
    /// seed. Empty when no editor holds the keyboard.
    #[must_use]
    pub fn text(&self) -> String {
        if self.editors.get().focused().is_none() {
            return String::new();
        }
        self.field.text()
    }

    /// R1571 — the in-flight text of the editor on `index`, whichever buffer
    /// holds it: the live field for the focused one, the parked string for the
    /// rest. `None` when `index` has no editor or its form has no text.
    #[must_use]
    pub fn text_at(&self, index: CellIndex) -> Option<String> {
        let editors = self.editors.get();
        let editor = editors.get(index)?;
        match editor.parked_text() {
            Some(parked) => Some(parked.to_string()),
            None if editor.holds_the_field() => Some(self.field.text()),
            None => None,
        }
    }

    /// R1555 §5.27 — the in-flight state: whether an editor is open and whether
    /// its buffer holds a value of the cell's kind.
    ///
    /// The one place the two buffer halves are read, so a caller never has to
    /// know which half a form uses: a text-buffered form is parsed back out of
    /// the inline field through [`CellKind::parse`], and a
    /// latch-buffered one answers with the value it holds.
    ///
    /// R1571 — three halves rather than two, because a text-buffered editor
    /// that does not hold the keyboard reads its text back out of the latch.
    #[must_use]
    pub fn state(&self) -> EditState {
        match self.editors.get().focused_index() {
            Some(at) => self.state_at(at),
            None => EditState::Closed,
        }
    }

    /// R1571 — [`state`](Self::state) for a named cell, so an unfocused
    /// editor's in-flight value is readable without focusing it first.
    #[must_use]
    pub fn state_at(&self, index: CellIndex) -> EditState {
        let editors = self.editors.get();
        let Some(editor) = editors.get(index) else {
            return EditState::Closed;
        };
        if let Some(value) = editor.pending() {
            return EditState::Value(value.clone());
        }
        let text = match editor.parked_text() {
            Some(parked) => parked.to_string(),
            None => self.field.text(),
        };
        match editor.kind().parse(&text) {
            Some(value) => EditState::Value(value),
            None => EditState::Malformed,
        }
    }

    /// Whether the in-flight value differs from what the editor opened with.
    /// `false` when nothing is open.
    ///
    /// The question a close-without-commit path asks and the toolkit answers
    /// only by re-reading the editor widget: abstract item view keeps no
    /// record of what `setEditorData` seeded, so a toolkit view cannot distinguish an
    /// untouched editor from one edited back to its original value.
    ///
    /// R1555 — a [`EditState::Malformed`] buffer counts as dirty. The user typed
    /// something that is not the seed, which is exactly the state a "discard
    /// your changes?" prompt exists for; treating it as clean would drop work
    /// silently.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        match self.editors.get().focused_index() {
            Some(at) => self.is_dirty_at(at),
            None => false,
        }
    }

    /// R1571 — [`is_dirty`](Self::is_dirty) for a named cell. The question a
    /// binding asks of *every* open editor before closing a document, which is
    /// unaskable in the toolkit for the reason above, N times over.
    #[must_use]
    pub fn is_dirty_at(&self, index: CellIndex) -> bool {
        let editors = self.editors.get();
        let Some(editor) = editors.get(index) else {
            return false;
        };
        match self.state_at(index) {
            EditState::Closed => false,
            EditState::Malformed => true,
            EditState::Value(value) => &value != editor.seed(),
        }
    }

    /// The active trigger set.
    #[must_use]
    pub fn triggers(&self) -> EditTriggers {
        self.triggers.get()
    }

    /// Replace the trigger set (the toolkit `setEditTriggers`).
    pub fn set_triggers(&self, triggers: EditTriggers) {
        self.triggers.set(triggers);
    }

    /// Open an editor on `index` seeded from the model's `edit` answer,
    /// unconditionally — the toolkit's `edit(const model index&)`,
    /// the programmatic open that bypasses the trigger gate.
    ///
    /// Taking a [`CellEdit`] rather than an index alone is what makes "an
    /// editor open on a cell the model will not edit" unrepresentable: only
    /// the model can produce one, and it produces `None` for a read-only cell.
    ///
    /// For a text-buffered form the field's buffer is seeded and fully
    /// selected, so the first printable keystroke replaces it — the
    /// type-to-replace behaviour the toolkit gets from `selectAll` on editor focus,
    /// and the reason [`EditTrigger::AnyKeyPressed`] needs no special seeding path.
    ///
    /// R1555 — which buffer is seeded follows from the datum's
    /// [`EditorForm`]. A toggle or a selector holds its in-flight value in the
    /// latch, and its field buffer is **cleared**, so nothing can read a
    /// previous edit's text as this editor's value.
    ///
    /// R1571 — a cell that already has a **persistent** editor is *focused*
    /// rather than reopened, so a trigger on it does not discard what the user
    /// has already typed there. The toolkit reaches the same behaviour through
    /// `editor()`, which hands back the widget its hash already holds instead of
    /// asking the delegate for a new one. Any other transient editor is
    /// closed, discarding its buffer — this is what it has always done, and a
    /// binding that wants the toolkit's commit-on-focus-out runs that first
    /// (`blur_committing_field_extra`).
    pub fn begin(&self, index: CellIndex, edit: &CellEdit) {
        if self.is_persistent_editor_open(index) {
            let _focused = self.focus_editor(index);
            return;
        }
        let prev = self.editors.get();
        // Read before `seed_field` overwrites it, so the departing editor parks
        // what the user actually typed rather than this editor's seed.
        let live = self.field.text();
        let mut open: Vec<OpenEditor> = prev
            .iter()
            .filter(|e| e.index != index && e.persistence != EditorPersistence::Transient)
            .cloned()
            .collect();
        let _handed = hand_over_field(&mut open, &live, None);
        let buffer = self.seed_field(edit);
        open.push(OpenEditor {
            index,
            seed: edit.value().clone(),
            buffer,
            persistence: EditorPersistence::Transient,
        });
        self.store_editors(open, Some(index));
    }

    /// Publish a rebuilt editor set.
    ///
    /// The **one** place [`OpenEditors::from_parts`]'s verdict is consumed, and
    /// therefore the only place its `expect` lives. Every verb above hands over
    /// a set built from a valid one by a single structural step — adding a
    /// cell, removing one, moving the shared field — and `from_parts` re-checks
    /// all four invariants, so a step that broke one fails here loudly instead
    /// of shipping a set whose paint and whose keystroke routing disagree.
    ///
    /// Private, because the proof obligation is this module's: no caller can
    /// build a `Vec<OpenEditor>` to hand in, so the panic is unreachable from
    /// outside and belongs on no public method's `# Panics` section.
    ///
    /// # Panics
    ///
    /// If a verb in this module built a set that breaks an [`OpenEditors`]
    /// invariant — a bug here, never a caller's.
    fn store_editors(&self, open: Vec<OpenEditor>, focused: Option<CellIndex>) {
        self.editors.set(
            OpenEditors::from_parts(open, focused)
                .expect("a grid edit verb rebuilds its set from an already valid one"),
        );
    }

    /// The buffer a freshly opened editor starts with, and the field mutation
    /// that goes with it. The one place a seed is written, so the field and the
    /// latch cannot disagree about which of them holds the value.
    fn seed_field(&self, edit: &CellEdit) -> EditBuffer {
        if edit.form().buffer_is_text() {
            let text = edit.text();
            let chars = text.chars().count();
            // `set_text` clears the selection, so the whole-buffer select has
            // to follow it — the ordering is load-bearing and a test pins it.
            self.field.set_text(text);
            self.field.set_selection(0, chars);
            EditBuffer::Live
        } else {
            self.field.set_text(String::new());
            EditBuffer::Value(edit.value().clone())
        }
    }

    /// R1571 §5.27 — open a **persistent** editor on `index`: the toolkit's
    /// `openPersistentEditor`.
    ///
    /// It takes the keyboard, because that is what a caller opening an editor
    /// means and because the toolkit's own `openPersistentEditor` shows the widget in a state
    /// ready to receive input. Every other open editor keeps its in-flight
    /// value, parked.
    ///
    /// See [`OpenOutcome`] for what the three answers mean and for what the toolkit
    /// answers instead (`void`).
    pub fn open_persistent(&self, index: CellIndex, edit: &CellEdit) -> OpenOutcome {
        let prev = self.editors.get();
        match prev.get(index).map(OpenEditor::persistence) {
            Some(EditorPersistence::Persistent) => OpenOutcome::AlreadyOpen,
            Some(EditorPersistence::Transient) => {
                let live = self.field.text();
                let mut open: Vec<OpenEditor> = prev.iter().cloned().collect();
                for editor in &mut open {
                    if editor.index == index {
                        editor.persistence = EditorPersistence::Persistent;
                    }
                }
                self.hand_the_field_to(&mut open, &live, index);
                self.store_editors(open, Some(index));
                OpenOutcome::Promoted
            }
            None => {
                let live = self.field.text();
                let mut open: Vec<OpenEditor> = prev.iter().cloned().collect();
                let _handed = hand_over_field(&mut open, &live, None);
                let buffer = self.seed_field(edit);
                open.push(OpenEditor {
                    index,
                    seed: edit.value().clone(),
                    buffer,
                    persistence: EditorPersistence::Persistent,
                });
                self.store_editors(open, Some(index));
                OpenOutcome::Opened
            }
        }
    }

    /// Hand the shared field to the editor on `index` and write the field to
    /// match: the paired half of [`hand_over_field`], which owns the latch side
    /// but must not touch the field from inside a set-building pass.
    fn hand_the_field_to(&self, open: &mut [OpenEditor], live: &str, index: CellIndex) {
        match hand_over_field(open, live, Some(index)) {
            // `seed` is the R878 "replace the buffer and park the caret at the
            // end" pair — see `focus_editor`'s doc for why this is not a
            // select-all the way a fresh open is.
            Some(text) => self.field.seed(text),
            None => self.field.set_text(String::new()),
        }
    }

    /// R1571 §5.27 — close the editor on `index` whatever its persistence:
    /// The toolkit's `closePersistentEditor`, widened to the
    /// transient kind because "close this cell's editor" is one question.
    ///
    /// Returns whether one was open. Its in-flight value is **discarded** —
    /// the caller commits first if it wants it, which is the same order the
    /// toolkit's `closePersistentEditor` imposes (it deletes the widget, taking the text with it,
    /// and reports nothing).
    ///
    /// When the closed editor held the keyboard, focus moves to no editor
    /// rather than to an arbitrary survivor: which one a user meant next is not
    /// a fact this state has.
    #[must_use]
    pub fn close_persistent(&self, index: CellIndex) -> bool {
        let prev = self.editors.get();
        if !prev.contains(index) {
            return false;
        }
        if prev.focused_index() == Some(index) {
            self.field.set_text(String::new());
        }
        let open: Vec<OpenEditor> = prev.iter().filter(|e| e.index != index).cloned().collect();
        let focused = prev.focused_index().filter(|at| *at != index);
        self.store_editors(open, focused);
        true
    }

    /// R1571 §5.27 — close every open editor, discarding every in-flight
    /// value. The model-reset path: the toolkit destroys its editor widgets
    /// when the rows under them go away, and reports nothing about what was
    /// lost.
    pub fn close_all(&self) {
        self.field.set_text(String::new());
        self.editors.set(OpenEditors::default());
    }

    /// R1571 §5.27 — give the editor on `index` the keyboard, parking the
    /// previously focused editor's text and restoring this one's.
    ///
    /// Returns whether it moved — `false` when `index` has no editor. Focusing
    /// the already-focused editor is a no-op that answers `true`.
    ///
    /// The restored buffer's caret lands at the **end** with nothing selected,
    /// where [`begin`](Self::begin) selects the whole seed. The difference is
    /// the point: a fresh editor is type-to-replace, and an editor you are
    /// coming back to holds work the next keystroke must not erase.
    #[must_use]
    pub fn focus_editor(&self, index: CellIndex) -> bool {
        let prev = self.editors.get();
        if !prev.contains(index) {
            return false;
        }
        if prev.focused_index() == Some(index) {
            return true;
        }
        let live = self.field.text();
        let mut open: Vec<OpenEditor> = prev.iter().cloned().collect();
        self.hand_the_field_to(&mut open, &live, index);
        self.store_editors(open, Some(index));
        true
    }

    /// R1555 §5.27 — flip an open [`EditorForm::Toggle`] editor's in-flight
    /// bool: the checkbox gesture, <kbd>Space</kbd> or a click.
    ///
    /// Returns whether it flipped — `false` when no editor is open and when the
    /// open editor is not a toggle, so a Space arriving while a text field is
    /// open cannot silently rewrite the cell.
    ///
    /// # Why the toggle is in-flight and not a write-through
    ///
    /// The toolkit's `editorEvent` handles a check-state click by calling `setModelData`
    /// **immediately**, so there is nothing to escape: a mis-click on a
    /// toolkit check column is already committed. Here the toggle edits the
    /// latch, so <kbd>Escape</kbd> reverts it and <kbd>Enter</kbd> commits it
    /// — the same arc every other form has.
    #[must_use]
    pub fn toggle(&self) -> bool {
        let editors = self.editors.get();
        let Some(editor) = editors.focused() else {
            return false;
        };
        let Some(CellValue::Bool(current)) = editor.pending() else {
            return false;
        };
        self.set_focused_buffer(&EditBuffer::Value(CellValue::Bool(!current)));
        true
    }

    /// Replace the focused editor's buffer, leaving the set's shape and its
    /// focus alone — the one write path the three gesture verbs share, so none
    /// of them can rebuild the set and lose a sibling editor.
    fn set_focused_buffer(&self, buffer: &EditBuffer) {
        let prev = self.editors.get();
        let Some(at) = prev.focused_index() else {
            return;
        };
        let mut open: Vec<OpenEditor> = prev.iter().cloned().collect();
        for editor in &mut open {
            if editor.index == at {
                editor.buffer = buffer.clone();
            }
        }
        self.store_editors(open, Some(at));
    }

    /// R1555 §5.27 — set an open [`EditorForm::Selector`] editor's in-flight
    /// option index: the combo-box gesture.
    ///
    /// Returns whether it selected — `false` when no editor is open, when the
    /// open editor is not a selector, and when `selected` is past the datum's
    /// own option list. That last check is the past-the toolkit half:
    /// `setCurrentIndex` accepts an out-of-range index by silently
    /// clearing the selection, so a stale index there produces an empty combo
    /// rather than a rejected write.
    #[must_use]
    pub fn select(&self, selected: usize) -> bool {
        let editors = self.editors.get();
        let Some(editor) = editors.focused() else {
            return false;
        };
        let Some(CellValue::Choice { options, .. }) = editor.pending() else {
            return false;
        };
        if selected >= options.len() {
            return false;
        }
        self.set_focused_buffer(&EditBuffer::Value(CellValue::Choice {
            selected,
            options: options.clone(),
        }));
        true
    }

    /// R1555 §5.27 — step an open [`EditorForm::Stepper`] editor's buffer by
    /// `delta` steps: the toolkit `stepBy`, reached from the editor's
    /// up / down affordances.
    ///
    /// Returns whether it stepped — `false` when no editor is open, when the
    /// open editor has no stepper, and when the buffer does not currently hold a
    /// value of the cell's kind ([`CellKind::step_text`]).
    ///
    /// The stepped text lands in the same buffer a keystroke would, and the
    /// caret is put at its end, so stepping and typing compose instead of
    /// fighting over the selection.
    #[must_use]
    pub fn step(&self, delta: i64) -> bool {
        let editors = self.editors.get();
        let Some(editor) = editors.focused() else {
            return false;
        };
        if editor.form() != EditorForm::Stepper {
            return false;
        }
        let Some(stepped) = editor.kind().step_text(&self.field.text(), delta) else {
            return false;
        };
        // `seed` is the R878 "replace the buffer and park the caret at the end"
        // pair; `set_text` alone would clamp the caret to its previous offset.
        self.field.seed(stepped);
        true
    }

    /// Open an editor on `index` **if** `trigger` is in the active set — the toolkit's
    /// `edit(index, trigger, event)`. Returns whether it opened.
    #[must_use]
    pub fn begin_on(&self, trigger: EditTrigger, index: CellIndex, edit: &CellEdit) -> bool {
        if !self.triggers.get().contains(trigger) {
            return false;
        }
        self.begin(index, edit);
        true
    }

    /// Abandon the focused edit — <kbd>Escape</kbd>, the toolkit's
    /// `closeEditor(editor, RevertModelCache)` on a write-through model.
    ///
    /// R1571 — what "abandon" means follows from the editor's persistence, and
    /// this is where the toolkit's own decomposition breaks down. A
    /// **transient** editor closes, discarding its in-flight text, as it
    /// always has. A **persistent** one is *reverted to its seed and stays
    /// open*, because closing it would be a second, undeclared way for `openPersistentEditor` to
    /// be undone.
    ///
    /// The toolkit does neither: `eventFilter` emits
    /// `closeEditor(editor, RevertModelCache)`, and
    /// `closeEditor` checks `d->persistent.contains(editor)`
    /// and **returns without touching it** — so <kbd>Escape</kbd> on a toolkit
    /// persistent editor does nothing at all, the typed text stays on screen,
    /// and the original value is unrecoverable from the view.
    pub fn cancel(&self) {
        let prev = self.editors.get();
        let Some(editor) = prev.focused().cloned() else {
            return;
        };
        match editor.persistence() {
            EditorPersistence::Transient => {
                let _closed = self.close_persistent(editor.index);
            }
            EditorPersistence::Persistent => {
                let reverted = self.seed_field(&CellEdit::from(editor.seed().clone()));
                let mut open: Vec<OpenEditor> = prev.iter().cloned().collect();
                for open_editor in &mut open {
                    if open_editor.index == editor.index {
                        open_editor.buffer = reverted.clone();
                    }
                }
                self.store_editors(open, Some(editor.index));
            }
        }
    }

    /// Write the in-flight **value** back through `set` and, **if the model
    /// accepts it**, close the editor. The toolkit's `commitData` + `setModelData`.
    ///
    /// Every outcome is named — see [`CommitOutcome`]. The toolkit's path answers nothing:
    /// `setModelData` returns `void`, `commitData` ignores what `setData` did, and the editor closes either
    /// way.
    ///
    /// R1555 — `set` receives the **datum**, not the raw buffer. The framework
    /// parses once, through [`CellKind::parse`], which is the documented inverse
    /// of the [`CellValue::edit_text`](crate::cell_value::CellValue::edit_text)
    /// the editor was seeded from — so every model no longer re-derives that
    /// parse, and a malformed buffer is [`CommitOutcome::Malformed`] rather than
    /// a `false` the model has to return for a reason it cannot distinguish from
    /// its own validation. A toggle and a selector reach this path with the same
    /// shape, which is what lets one commit arc serve all five forms.
    pub fn commit_with(&self, set: impl FnOnce(CellIndex, &CellValue) -> bool) -> CommitOutcome {
        match self.editors.get().focused_index() {
            Some(at) => self.commit_at_with(at, set),
            None => CommitOutcome::NotEditing,
        }
    }

    /// R1571 — [`commit_with`](Self::commit_with) for a named cell, so an editor that
    /// does not hold the keyboard can be written through without being focused
    /// first. The "save every open editor" verb, which in the toolkit means
    /// iterating a private hash you cannot reach.
    ///
    /// A **transient** editor closes on a successful write, as it always has.
    /// A **persistent** one stays open, and the committed value becomes its
    /// new seed — so [`is_dirty_at`](Self::is_dirty_at) is `false` immediately after. The
    /// toolkit leaves the widget's text alone and keeps no record of what the
    /// editor was seeded with, so there is nothing there for a second commit
    /// to compare against.
    pub fn commit_at_with(
        &self,
        index: CellIndex,
        set: impl FnOnce(CellIndex, &CellValue) -> bool,
    ) -> CommitOutcome {
        let prev = self.editors.get();
        let Some(editor) = prev.get(index).cloned() else {
            return CommitOutcome::NotEditing;
        };
        let value = match self.state_at(index) {
            EditState::Closed => return CommitOutcome::NotEditing,
            EditState::Malformed => return CommitOutcome::Malformed,
            EditState::Value(value) => value,
        };
        if !set(index, &value) {
            return CommitOutcome::Refused;
        }
        match editor.persistence() {
            EditorPersistence::Transient => {
                let _closed = self.close_persistent(index);
            }
            EditorPersistence::Persistent => {
                let mut open: Vec<OpenEditor> = prev.iter().cloned().collect();
                for open_editor in &mut open {
                    if open_editor.index == index {
                        open_editor.seed = value.clone();
                    }
                }
                self.store_editors(open, prev.focused_index());
            }
        }
        CommitOutcome::Committed(index)
    }

    /// Honour an [`EndEditHint`] by opening an editor on the next / previous
    /// **editable** cell after `from` — the toolkit's `closeEditor(editor, hint)` move half, which walks
    /// with `moveCursor(MoveNext)` and edits if the landing index is editable.
    ///
    /// Returns whether an editor opened. [`EndEditHint::NoHint`] always
    /// returns `false` — there is nothing to move to.
    ///
    /// The walk is row-major over the **model** extent (not the painted window
    /// — the toolkit moves through indices, and a grid windowing 5 of 200
    /// columns must not stop at the window edge), and it **wraps**: past the
    /// last cell it resumes at the first. The toolkit's `moveCursor` stops there
    /// instead; wrapping is the spreadsheet behaviour and it costs nothing,
    /// because the walk is bounded by [`GridExtent::cell_count`] and so terminates on a model with
    /// no editable cell at all rather than spinning.
    pub fn advance(
        &self,
        from: CellIndex,
        hint: EndEditHint,
        extent: GridExtent,
        edit_at: impl Fn(CellIndex) -> Option<CellEdit>,
    ) -> bool {
        let forward = match hint {
            EndEditHint::NoHint => return false,
            EndEditHint::EditNextItem => true,
            EndEditHint::EditPreviousItem => false,
        };
        let count = extent.cell_count();
        if count == 0 || !extent.contains(from) {
            return false;
        }
        let start = from.row * extent.cols + from.col;
        for step in 1..=count {
            let flat = if forward {
                (start + step) % count
            } else {
                (start + count - (step % count)) % count
            };
            let index = CellIndex::new(flat / extent.cols, flat % extent.cols);
            if let Some(edit) = edit_at(index) {
                self.begin(index, &edit);
                return true;
            }
        }
        false
    }

    /// R1571 §5.27 — the open editors whose cells lie in `rows`, paired with
    /// the model's edit role for each, as the **paint window** needs them.
    ///
    /// A helper here rather than in each binding because it is the seam that
    /// makes N editors cost what the window costs: an editor on a row that is
    /// not painted contributes nothing to the scene, and the toolkit has no
    /// equivalent at all — its persistent editors are widgets that exist and
    /// are repositioned by `updateEditorGeometries()` on every scroll whether or not their row is on
    /// screen.
    ///
    /// An editor whose cell the model no longer edits is **dropped from the
    /// answer** rather than painted against a stale seed: a model can turn a
    /// cell read-only under an open editor, and the honest paint for that is
    /// the display path. It stays *open* — closing it here would make a paint
    /// pass mutate state, which is exactly the §6.3 purity the view-fn rests
    /// on.
    #[must_use]
    pub fn open_cells(
        &self,
        rows: Range<usize>,
        edit_at: impl Fn(CellIndex) -> Option<CellEdit>,
    ) -> Vec<OpenCell> {
        let editors = self.editors.get();
        let focused = editors.focused_index();
        editors
            .in_rows(rows)
            .filter_map(|editor| {
                Some(OpenCell {
                    focused: focused == Some(editor.index),
                    edit: edit_at(editor.index)?,
                    editor: editor.clone(),
                })
            })
            .collect()
    }
}

/// R1571 §5.27 — one open editor as a paint pass receives it: the editor, the
/// model's edit role for its cell, and whether it holds the keyboard.
///
/// The third field is the one that cannot be derived from the first two: a
/// latch-buffered form's buffer is [`EditBuffer::Value`] whether or not it is
/// focused, so "does this editor own the caret" has to be carried.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenCell {
    /// The editor itself — its index, seed, form, persistence and parked text.
    pub editor: OpenEditor,
    /// The model's `EditRole` answer for that cell, re-asked this frame.
    pub edit: CellEdit,
    /// Whether this editor holds the shared inline field, and so the caret,
    /// the selection and the IME preedit.
    pub focused: bool,
}

/// Move the shared inline field's ownership between editors, on the **latch**
/// side only.
///
/// The single author of [`EditBuffer::Live`], which is what makes invariant 4
/// of [`OpenEditors`] hold: whoever holds the field is parked with `live` (the
/// field's text, read before any of this), and `to` — if it is a text-buffered
/// member — takes it.
///
/// Answers with the text the field should now contain, or `None` when the
/// arriving editor is latch-buffered (or absent), in which case the field is
/// cleared. The field itself is written by
/// [`GridEditState::hand_the_field_to`]: a function that both rebuilds the set
/// and mutates a signal would be doing two things one of its callers does not
/// want (`begin` seeds the field from the model instead).
fn hand_over_field(open: &mut [OpenEditor], live: &str, to: Option<CellIndex>) -> Option<String> {
    for editor in open.iter_mut() {
        if editor.holds_the_field() {
            editor.buffer = EditBuffer::Parked(live.to_string());
        }
    }
    let index = to?;
    let editor = open.iter_mut().find(|e| e.index == index)?;
    let EditBuffer::Parked(text) = &editor.buffer else {
        // A latch-buffered form has no text and never claims the field.
        return None;
    };
    let text = text.clone();
    editor.buffer = EditBuffer::Live;
    Some(text)
}

/// R1544 §5.27 — the shared [`GridEditState`] for `key`, created on first use
/// over the inline editor field tagged `field_tag`.
///
/// The [`use_grid_sort`](crate::widgets::grid_sort::use_grid_sort) /
/// [`use_scroll_state`](crate::widgets::scroll::use_scroll_state) accessor:
/// the `External` and the view both call it and get the same `Rc`, which is
/// what makes the cell that paints an editor and the cell the keystrokes
/// route to one fact.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` / a
/// `create_extra_externals` hook — both run inside a `root_owner.run`).
#[must_use]
pub fn use_grid_edit(key: &'static str, field_tag: &'static str) -> Rc<GridEditState> {
    let owner = Owner::current().expect("use_grid_edit requires an active Owner scope");
    // The editor field's slot is resolved BEFORE the factory runs: an
    // `Owner::cache` factory may not call `Owner::cache` again, and
    // `use_text_edit_state` is one. Pre-resolving is the documented shape for
    // a cached value that depends on another cached value.
    let field = use_text_edit_state(field_tag);
    owner.cache(key, move || GridEditState {
        tag: Some(key),
        field_tag,
        field,
        editors: Signal::new(OpenEditors::default()),
        triggers: Signal::new(EditTriggers::DEFAULT),
    })
}

#[cfg(test)]
mod tests {
    //! R1544 §5.27 — the editing latch's contract, pinned at the substrate
    //! level so a drift surfaces here rather than in a binding.
    use super::*;
    use crate::reactive::Owner;

    fn text(kind: CellKind, s: &str) -> CellEdit {
        // R1555 — an edit role is a datum, so a test fixture states one. The
        // scalar kinds parse their own seed text, which is the same round trip
        // `CellValue::edit_text` / `CellKind::parse` are documented as.
        CellEdit::from(kind.parse(s).expect("a seed of this kind"))
    }

    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    #[test]
    fn triggers_wire_round_trips_every_arm_and_the_empty_set() {
        // The census drives the loop, so an arm added without a token fails
        // here rather than silently serializing as a missing member.
        for trigger in EditTrigger::ALL {
            assert_eq!(
                EditTrigger::from_wire(trigger.wire_token()),
                Some(trigger),
                "{trigger:?} round-trips"
            );
            let set = EditTriggers::NONE.with(trigger);
            assert_eq!(EditTriggers::from_wire(&set.to_wire()), Some(set));
        }
        assert_eq!(EditTriggers::NONE.to_wire(), "none");
        assert_eq!(EditTriggers::from_wire("none"), Some(EditTriggers::NONE));
        // A partially-understood set is a silently weaker gate, so it is
        // rejected whole rather than parsed down to its known members.
        assert_eq!(EditTriggers::from_wire("double_clicked|nope"), None);
    }

    #[test]
    fn triggers_wire_order_is_the_census_not_the_insertion_order() {
        let a = EditTriggers::NONE
            .with(EditTrigger::AnyKeyPressed)
            .with(EditTrigger::DoubleClicked);
        let b = EditTriggers::NONE
            .with(EditTrigger::DoubleClicked)
            .with(EditTrigger::AnyKeyPressed);
        assert_eq!(a.to_wire(), b.to_wire(), "same set, same string");
        assert_eq!(a.to_wire(), "double_clicked|any_key");
    }

    #[test]
    fn triggers_persist_as_names_not_as_a_bitmask() {
        // The serde form is the wire form, so inserting an `EditTrigger` arm
        // cannot reinterpret a persisted value (see the type's doc).
        let set = EditTriggers::DEFAULT;
        let json = serde_json::to_string(&set).expect("serialize");
        assert_eq!(json, "\"double_clicked|selected_clicked|edit_key\"");
        let back: EditTriggers = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, set);
        assert!(
            serde_json::from_str::<EditTriggers>("\"bogus\"").is_err(),
            "an unparseable name fails rather than defaulting to some set"
        );
    }

    #[test]
    fn qt_default_trigger_set_excludes_type_to_replace() {
        assert!(EditTriggers::DEFAULT.contains(EditTrigger::DoubleClicked));
        assert!(EditTriggers::DEFAULT.contains(EditTrigger::SelectedClicked));
        assert!(EditTriggers::DEFAULT.contains(EditTrigger::EditKeyPressed));
        assert!(!EditTriggers::DEFAULT.contains(EditTrigger::AnyKeyPressed));
        assert!(EditTriggers::NONE.is_empty());
        assert!(!EditTriggers::DEFAULT.is_empty());
        assert!(
            EditTriggers::default().is_empty(),
            "a grid is not accidentally editable"
        );
    }

    #[test]
    fn begin_seeds_the_field_and_selects_it_whole() {
        with_owner(|| {
            let state = GridEditState::new("t.begin");
            assert_eq!(state.focused(), None);
            state.begin(CellIndex::new(3, 1), &text(CellKind::Text, "Bravo"));
            let open = state.focused().expect("open");
            assert_eq!(open.index, CellIndex::new(3, 1));
            assert_eq!(open.kind(), CellKind::Text);
            assert_eq!(open.seed(), &CellValue::Text("Bravo".into()));
            assert_eq!(open.form(), EditorForm::Field);
            assert_eq!(open.pending(), None, "a field's buffer is its text");
            let field = use_text_edit_state("t.begin");
            assert_eq!(field.text(), "Bravo", "the field carries the seed");
            assert_eq!(
                field.selection_range(),
                Some((0, 5)),
                "seeded text is fully selected, so the first keystroke replaces it"
            );
            assert!(state.is_editing(CellIndex::new(3, 1)));
            assert!(!state.is_editing(CellIndex::new(3, 0)));
        });
    }

    #[test]
    fn begin_on_honours_the_trigger_gate() {
        with_owner(|| {
            let state = GridEditState::new("t.gate");
            let edit = text(CellKind::Text, "x");
            let at = CellIndex::new(0, 0);
            assert!(
                !state.begin_on(EditTrigger::AnyKeyPressed, at, &edit),
                "type-to-replace is not in the toolkit's default set"
            );
            assert_eq!(state.focused(), None, "a refused trigger opens nothing");
            assert!(state.begin_on(EditTrigger::DoubleClicked, at, &edit));
            assert!(state.focused().is_some());
            state.cancel();
            state.set_triggers(EditTriggers::NONE);
            assert!(
                !state.begin_on(EditTrigger::DoubleClicked, at, &edit),
                "NoEditTriggers refuses every gesture"
            );
            assert!(
                {
                    state.begin(at, &edit);
                    state.focused().is_some()
                },
                "the programmatic open bypasses the gate (the toolkit `edit(index)`)"
            );
        });
    }

    #[test]
    fn cancel_clears_both_the_latch_and_the_buffer() {
        with_owner(|| {
            let state = GridEditState::new("t.cancel");
            state.begin(CellIndex::new(1, 1), &text(CellKind::Text, "seed"));
            use_text_edit_state("t.cancel").set_text("typed".to_string());
            state.cancel();
            assert_eq!(state.focused(), None);
            assert_eq!(
                use_text_edit_state("t.cancel").text(),
                "",
                "the next open must not inherit the abandoned text"
            );
            assert_eq!(state.text(), "", "no editor open reads as no text");
        });
    }

    #[test]
    fn a_refused_commit_keeps_the_editor_open_with_the_typed_text() {
        with_owner(|| {
            let state = GridEditState::new("t.refuse");
            let at = CellIndex::new(2, 2);
            state.begin(at, &text(CellKind::Int, "7"));
            use_text_edit_state("t.refuse").set_text("999".to_string());
            assert!(state.is_dirty(), "the buffer differs from the seed");
            // This is the divergence from the toolkit: `setModelData` returns void, so
            // there a rejected value closes the editor and the typing is gone.
            assert_eq!(
                state.commit_with(|_, _| false),
                CommitOutcome::Refused,
                "a refused write names its refusal"
            );
            assert_eq!(state.focused().map(|e| e.index), Some(at), "still editing");
            assert_eq!(
                state.text(),
                "999",
                "the user's text survives so they can correct it"
            );
            let mut seen = None;
            assert_eq!(
                state.commit_with(|_, v| {
                    seen = Some(v.clone());
                    true
                }),
                CommitOutcome::Committed(at)
            );
            assert_eq!(
                seen,
                Some(CellValue::Int(999)),
                "R1555 — the model is handed the parsed DATUM, not the buffer"
            );
            assert_eq!(state.focused(), None, "an accepted write closes the editor");
        });
    }

    #[test]
    fn commit_on_a_closed_editor_never_calls_the_model() {
        with_owner(|| {
            let state = GridEditState::new("t.closed");
            let mut called = false;
            assert_eq!(
                state.commit_with(|_, _| {
                    called = true;
                    true
                }),
                CommitOutcome::NotEditing
            );
            assert!(!called, "no open editor means no write");
        });
    }

    #[test]
    fn is_dirty_is_false_for_an_untouched_editor_and_for_a_restored_one() {
        with_owner(|| {
            let state = GridEditState::new("t.dirty");
            assert!(!state.is_dirty(), "nothing open is not dirty");
            state.begin(CellIndex::new(0, 0), &text(CellKind::Text, "abc"));
            assert!(!state.is_dirty());
            use_text_edit_state("t.dirty").set_text("abcd".to_string());
            assert!(state.is_dirty());
            use_text_edit_state("t.dirty").set_text("abc".to_string());
            assert!(
                !state.is_dirty(),
                "edited back to the original is not dirty — the toolkit cannot tell"
            );
        });
    }

    /// A 3x3 model whose middle column is read-only.
    fn sparse_edit(index: CellIndex) -> Option<CellEdit> {
        (index.col != 1).then(|| text(CellKind::Text, "v"))
    }

    #[test]
    fn advance_skips_read_only_cells_and_wraps() {
        with_owner(|| {
            let state = GridEditState::new("t.adv");
            let extent = GridExtent::new(3, 3);
            // (0,0) -> next editable is (0,2): (0,1) is read-only.
            assert!(state.advance(
                CellIndex::new(0, 0),
                EndEditHint::EditNextItem,
                extent,
                sparse_edit
            ));
            assert_eq!(state.focused().map(|e| e.index), Some(CellIndex::new(0, 2)));
            // From the LAST cell, forward wraps to the first.
            assert!(state.advance(
                CellIndex::new(2, 2),
                EndEditHint::EditNextItem,
                extent,
                sparse_edit
            ));
            assert_eq!(state.focused().map(|e| e.index), Some(CellIndex::new(0, 0)));
            // Backward from the first wraps to the last.
            assert!(state.advance(
                CellIndex::new(0, 0),
                EndEditHint::EditPreviousItem,
                extent,
                sparse_edit
            ));
            assert_eq!(state.focused().map(|e| e.index), Some(CellIndex::new(2, 2)));
            // (0,2) backward is (0,0), not the read-only (0,1).
            assert!(state.advance(
                CellIndex::new(0, 2),
                EndEditHint::EditPreviousItem,
                extent,
                sparse_edit
            ));
            assert_eq!(state.focused().map(|e| e.index), Some(CellIndex::new(0, 0)));
        });
    }

    #[test]
    fn advance_terminates_on_a_model_with_no_editable_cell() {
        with_owner(|| {
            let state = GridEditState::new("t.none");
            // The walk is bounded by the cell count, so a fully read-only
            // model returns rather than spinning forever looking for a target.
            assert!(!state.advance(
                CellIndex::new(1, 1),
                EndEditHint::EditNextItem,
                GridExtent::new(4, 4),
                |_| None
            ));
            assert_eq!(state.focused(), None);
        });
    }

    #[test]
    fn advance_is_a_no_op_without_a_hint_or_outside_the_extent() {
        with_owner(|| {
            let state = GridEditState::new("t.nohint");
            let extent = GridExtent::new(2, 2);
            assert!(!state.advance(
                CellIndex::new(0, 0),
                EndEditHint::NoHint,
                extent,
                sparse_edit
            ));
            assert!(
                !state.advance(
                    CellIndex::new(9, 9),
                    EndEditHint::EditNextItem,
                    extent,
                    sparse_edit
                ),
                "an index outside the model has no successor in it"
            );
            assert!(!state.advance(
                CellIndex::new(0, 0),
                EndEditHint::EditNextItem,
                GridExtent::new(0, 0),
                sparse_edit
            ));
            assert_eq!(state.focused(), None);
        });
    }

    #[test]
    fn advance_can_land_on_the_cell_it_started_from() {
        with_owner(|| {
            let state = GridEditState::new("t.self");
            // One editable cell in the whole model: a full lap comes back to
            // it, which is what the toolkit's Tab does in a
            // single-editable-cell view.
            let only = CellIndex::new(1, 1);
            assert!(state.advance(
                only,
                EndEditHint::EditNextItem,
                GridExtent::new(3, 3),
                |i| { (i == only).then(|| text(CellKind::Text, "v")) }
            ));
            assert_eq!(state.focused().map(|e| e.index), Some(only));
        });
    }

    #[test]
    fn use_grid_edit_shares_one_state_across_callers() {
        with_owner(|| {
            let a = use_grid_edit("t.shared", "t.shared.field");
            let b = use_grid_edit("t.shared", "t.shared.field");
            a.begin(CellIndex::new(4, 0), &text(CellKind::Text, "s"));
            assert_eq!(
                b.editing(),
                Some(CellIndex::new(4, 0)),
                "the External and the view see one latch, not two that agree"
            );
            assert_eq!(a.tag(), Some("t.shared"));
            assert_eq!(a.field_tag(), "t.shared.field");
        });
    }

    #[test]
    fn cell_edit_derives_from_a_value_through_its_edit_role_not_its_display() {
        // The seed is `edit_text`, whose documented inverse is `CellKind::parse`
        // — the property that makes a committed value round-trip.
        let value = CellValue::Float(1234.5);
        let edit = CellEdit::from(&value);
        assert_eq!(edit.kind(), CellKind::Float);
        assert_eq!(edit.text(), value.edit_text());
        assert_eq!(
            CellKind::Float.parse(&edit.text()),
            Some(CellValue::Float(1234.5)),
            "the seed parses back to the value it came from"
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R1555 §5.27 — the five forms the factory opens
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r1555_a_toggle_holds_its_value_in_the_latch_not_in_the_field() {
        with_owner(|| {
            let state = GridEditState::new("t.toggle");
            let at = CellIndex::new(1, 1);
            state.begin(at, &CellEdit::from(CellValue::Bool(false)));
            let open = state.focused().expect("open");
            assert_eq!(open.form(), EditorForm::Toggle);
            assert_eq!(open.pending(), Some(&CellValue::Bool(false)));
            assert_eq!(
                use_text_edit_state("t.toggle").text(),
                "",
                "a toggle's field buffer is cleared, so nothing can read a \
                 previous edit's text as this editor's value"
            );
            assert_eq!(state.state(), EditState::Value(CellValue::Bool(false)));
            assert!(!state.is_dirty());

            assert!(state.toggle(), "the checkbox gesture flips the latch");
            assert_eq!(state.state(), EditState::Value(CellValue::Bool(true)));
            assert!(state.is_dirty(), "flipped away from the seed");

            // Past the toolkit: `editorEvent` calls `setModelData` on the click, so a mis-click on
            // a toolkit check column is already committed. Escape reverts
            // here.
            state.cancel();
            assert_eq!(state.focused(), None);
            assert_eq!(state.state(), EditState::Closed);
        });
    }

    #[test]
    fn r1555_a_gesture_verb_is_refused_by_a_form_that_does_not_have_it() {
        with_owner(|| {
            let state = GridEditState::new("t.wrongform");
            // Nothing open: every verb refuses rather than inventing a latch.
            assert!(!state.toggle());
            assert!(!state.select(0));
            assert!(!state.step(1));

            state.begin(CellIndex::new(0, 0), &text(CellKind::Text, "abc"));
            assert!(
                !state.toggle(),
                "a Space arriving while a text field is open must not rewrite \
                 the cell"
            );
            assert!(!state.select(0), "a field has no options");
            assert!(!state.step(1), "a field has no stepper");
            assert_eq!(
                state.state(),
                EditState::Value(CellValue::Text("abc".into()))
            );
        });
    }

    #[test]
    fn r1555_a_selector_bounds_checks_against_its_own_option_list() {
        with_owner(|| {
            let state = GridEditState::new("t.select");
            let value = CellValue::Choice {
                selected: 0,
                options: vec!["Alpha".into(), "Bravo".into()],
            };
            state.begin(CellIndex::new(4, 2), &CellEdit::from(value));
            assert_eq!(state.form(), Some(EditorForm::Selector));
            assert!(state.select(1));
            assert_eq!(
                state.state(),
                EditState::Value(CellValue::Choice {
                    selected: 1,
                    options: vec!["Alpha".into(), "Bravo".into()],
                }),
                "selecting preserves the domain — an option list is part of \
                 the value's identity"
            );
            // Past the toolkit: `setCurrentIndex` accepts an out-of-range
            // index by silently clearing the selection.
            assert!(!state.select(2), "past the end is refused, not cleared");
            assert!(!state.select(usize::MAX));
            assert_eq!(
                state.state(),
                EditState::Value(CellValue::Choice {
                    selected: 1,
                    options: vec!["Alpha".into(), "Bravo".into()],
                }),
                "a refused selection leaves the in-flight value alone"
            );
        });
    }

    #[test]
    fn r1555_a_stepper_steps_the_same_buffer_a_keystroke_feeds() {
        with_owner(|| {
            let state = GridEditState::new("t.step");
            state.begin(CellIndex::new(0, 3), &CellEdit::from(CellValue::Int(41)));
            assert_eq!(state.form(), Some(EditorForm::Stepper));
            assert!(state.step(1));
            assert_eq!(state.text(), "42");
            assert_eq!(state.state(), EditState::Value(CellValue::Int(42)));
            let field = use_text_edit_state("t.step");
            assert_eq!(
                field.caret(),
                2,
                "the caret lands at the end, so stepping and typing compose"
            );
            assert_eq!(
                field.selection_range(),
                None,
                "and nothing is selected, so the next keystroke appends rather \
                 than replacing what the step just produced"
            );
            assert!(state.step(-2));
            assert_eq!(state.state(), EditState::Value(CellValue::Int(40)));

            // A half-typed buffer is not stepped from: the toolkit's spin box
            // would step from whatever its validator last accepted.
            use_text_edit_state("t.step").set_text("4-".to_string());
            assert!(!state.step(1));
            assert_eq!(state.text(), "4-", "the user's text is left alone");
            assert_eq!(state.state(), EditState::Malformed);
        });
    }

    #[test]
    fn r1555_a_malformed_buffer_never_reaches_the_model() {
        with_owner(|| {
            let state = GridEditState::new("t.malformed");
            let at = CellIndex::new(2, 1);
            state.begin(at, &CellEdit::from(CellValue::Int(7)));
            use_text_edit_state("t.malformed").set_text("12a".to_string());
            let mut asked = false;
            assert_eq!(
                state.commit_with(|_, _| {
                    asked = true;
                    true
                }),
                CommitOutcome::Malformed,
            );
            assert!(
                !asked,
                "the model is not asked to store something that is not a value \
                 of its cell's kind — and the outcome says WHICH failure it was, \
                 where the toolkit's validator makes this state unreachable at the price \
                 of committing a value the user did not type"
            );
            assert!(state.is_dirty(), "a malformed buffer is unsaved work");
            assert_eq!(state.focused().map(|e| e.index), Some(at), "still editing");
            assert_eq!(CommitOutcome::Malformed.committed(), None);
            assert_eq!(CommitOutcome::Committed(at).committed(), Some(at));
        });
    }

    #[test]
    fn r1555_a_swatch_reads_its_value_back_out_of_the_hex_field() {
        with_owner(|| {
            let state = GridEditState::new("t.swatch");
            let red = crate::style::Color::from_hex("#ff0000").expect("hex");
            state.begin(CellIndex::new(0, 0), &CellEdit::from(CellValue::Color(red)));
            let open = state.focused().expect("open");
            assert_eq!(open.form(), EditorForm::Swatch);
            assert_eq!(
                open.pending(),
                None,
                "a swatch's in-flight value is its hex field's text"
            );
            assert_eq!(state.text(), red.to_hex(), "seeded with the hex form");
            assert_eq!(state.state(), EditState::Value(CellValue::Color(red)));
            let blue = crate::style::Color::from_hex("#0000ff").expect("hex");
            use_text_edit_state("t.swatch").set_text(blue.to_hex());
            assert_eq!(state.state(), EditState::Value(CellValue::Color(blue)));
            use_text_edit_state("t.swatch").set_text("#zz".to_string());
            assert_eq!(state.state(), EditState::Malformed);
        });
    }

    #[test]
    fn r1555_one_commit_arc_serves_every_form() {
        // The property that makes the factory a seam rather than five paths:
        // whatever form the datum's kind opened, the commit is the same call.
        with_owner(|| {
            let state = GridEditState::new("t.allforms");
            let seeds = [
                CellValue::Text("t".into()),
                CellValue::Int(1),
                CellValue::Float(1.5),
                CellValue::Bool(false),
                CellValue::Choice {
                    selected: 0,
                    options: vec!["a".into()],
                },
                CellValue::Color(crate::style::Color::from_hex("#010203").expect("hex")),
            ];
            assert_eq!(seeds.len(), CellKind::ALL.len(), "one seed per kind");
            for (i, seed) in seeds.into_iter().enumerate() {
                let at = CellIndex::new(i, 0);
                let kind = seed.kind();
                state.begin(at, &CellEdit::from(seed.clone()));
                let mut written = None;
                assert_eq!(
                    state.commit_with(|_, v| {
                        written = Some(v.clone());
                        true
                    }),
                    CommitOutcome::Committed(at),
                    "{kind:?} commits through the same arc"
                );
                assert_eq!(
                    written,
                    Some(seed),
                    "{kind:?} — an untouched open-and-commit does not change \
                     the datum (the toolkit's default double editor rounds to 2 decimals)"
                );
                assert_eq!(state.focused(), None);
            }
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R1571 §5.27 — N editors, and persistence as a property of one
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r1571_persistence_round_trips_every_arm() {
        for persistence in EditorPersistence::ALL {
            assert_eq!(
                EditorPersistence::from_wire(persistence.wire_token()),
                Some(persistence),
            );
        }
        assert_eq!(EditorPersistence::from_wire("sticky"), None);
        assert_eq!(OpenOutcome::Opened.wire_token(), "opened");
        assert_eq!(OpenOutcome::Promoted.wire_token(), "promoted");
        assert_eq!(OpenOutcome::AlreadyOpen.wire_token(), "already_open");
    }

    #[test]
    fn r1571_the_owner_cache_never_releases_a_runtime_key() {
        // The finding that decides where an editor's buffer lives, stated as a
        // property rather than as an argument. R1555's audit prescribed keying
        // one `TextEditState` per cell into `Owner::cache` — which has accepted
        // runtime ids since R685.C — and the cache has `cache`,
        // `cache_contains` and `cache_get_by_str` and **no removal of any
        // kind**, so every slot outlives every handle to it. On the models this
        // axis is named for that is one buffer per cell ever edited, for the
        // life of the window.
        let owner = Owner::new();
        let keys: Vec<String> = (0..64).map(|i| format!("r1571.buffer#{i}")).collect();
        for key in &keys {
            drop(owner.cache(key.clone(), crate::widgets::text_edit::TextEditState::new));
        }
        let retained = keys
            .iter()
            .filter(|key| {
                owner.cache_contains::<crate::widgets::text_edit::TextEditState>((*key).clone())
            })
            .count();
        assert_eq!(
            retained,
            keys.len(),
            "every runtime-keyed slot outlived its handle — which is why an \
             editor's buffer lives in the editor set, not in the owner's cache"
        );
    }

    #[test]
    fn r1571_a_persistent_editor_survives_a_commit_and_is_reseeded() {
        with_owner(|| {
            let state = GridEditState::new("t.p.commit");
            let at = CellIndex::new(2, 1);
            assert_eq!(
                state.open_persistent(at, &text(CellKind::Text, "seed")),
                OpenOutcome::Opened
            );
            assert!(state.is_persistent_editor_open(at));
            assert!(state.is_focused(at));
            use_text_edit_state("t.p.commit").set_text("typed".to_string());
            assert!(state.is_dirty_at(at));

            let mut written = None;
            assert_eq!(
                state.commit_at_with(at, |_, v| {
                    written = Some(v.clone());
                    true
                }),
                CommitOutcome::Committed(at)
            );
            assert_eq!(written, Some(CellValue::Text("typed".into())));
            assert!(
                state.is_persistent_editor_open(at),
                "the toolkit's persistent editor survives commitData, and so does this one"
            );
            assert!(
                !state.is_dirty_at(at),
                "the committed value is the new seed — the toolkit keeps no seed at all, \
                 so a second commit there has nothing to compare against"
            );
            assert_eq!(state.text_at(at).as_deref(), Some("typed"));
        });
    }

    #[test]
    fn r1571_escape_reverts_a_persistent_editor_and_closes_a_transient_one() {
        with_owner(|| {
            let state = GridEditState::new("t.p.escape");
            let persistent = CellIndex::new(0, 0);
            let transient = CellIndex::new(1, 0);
            assert_eq!(
                state.open_persistent(persistent, &text(CellKind::Text, "keep")),
                OpenOutcome::Opened
            );
            use_text_edit_state("t.p.escape").set_text("scribble".to_string());
            // Past the toolkit: `closeEditor` returns early for a
            // persistent editor, so Escape there does nothing at all and the
            // original value cannot be recovered from the view.
            state.cancel();
            assert!(state.is_persistent_editor_open(persistent), "still open");
            assert_eq!(state.text_at(persistent).as_deref(), Some("keep"));
            assert!(!state.is_dirty_at(persistent));

            state.begin(transient, &text(CellKind::Text, "gone"));
            assert_eq!(state.editors().len(), 2);
            state.cancel();
            assert!(!state.is_editing(transient), "a transient editor closes");
            assert_eq!(state.editors().len(), 1);
            assert_eq!(
                state.editing(),
                None,
                "closing the focused editor focuses nothing, rather than \
                 guessing which survivor the user meant"
            );
        });
    }

    #[test]
    fn r1571_each_editor_parks_its_own_text_and_gets_it_back() {
        with_owner(|| {
            let state = GridEditState::new("t.p.park");
            let a = CellIndex::new(0, 0);
            let b = CellIndex::new(5, 2);
            assert_eq!(
                state.open_persistent(a, &text(CellKind::Text, "a")),
                OpenOutcome::Opened
            );
            use_text_edit_state("t.p.park").set_text("alpha".to_string());
            assert_eq!(
                state.open_persistent(b, &text(CellKind::Text, "b")),
                OpenOutcome::Opened
            );
            use_text_edit_state("t.p.park").set_text("bravo".to_string());

            // Both in-flight values exist at once, which is the whole point:
            // one lives in the shared field, the other is parked.
            assert_eq!(state.text_at(a).as_deref(), Some("alpha"));
            assert_eq!(state.text_at(b).as_deref(), Some("bravo"));
            assert_eq!(
                state.state_at(a),
                EditState::Value(CellValue::Text("alpha".into()))
            );
            assert!(state.is_dirty_at(a) && state.is_dirty_at(b));

            let editors = state.editors();
            assert_eq!(editors.len(), 2);
            assert_eq!(editors.focused_index(), Some(b));
            assert_eq!(
                editors.iter().filter(|e| e.holds_the_field()).count(),
                1,
                "exactly one editor owns the shared buffer"
            );
            assert_eq!(
                editors.get(a).and_then(OpenEditor::parked_text),
                Some("alpha")
            );

            assert!(state.focus_editor(a));
            assert_eq!(state.text(), "alpha", "the parked text came back");
            let field = use_text_edit_state("t.p.park");
            assert_eq!(
                field.caret(),
                5,
                "the caret lands at the end — an editor you are returning to \
                 holds work the next keystroke must not erase"
            );
            assert_eq!(field.selection_range(), None);
            assert_eq!(state.text_at(b).as_deref(), Some("bravo"), "b parked");
            assert!(state.focus_editor(a), "focusing the focused one is a no-op");
            assert!(
                !state.focus_editor(CellIndex::new(9, 9)),
                "a cell with no editor cannot take the keyboard"
            );
        });
    }

    #[test]
    fn r1571_a_second_transient_editor_replaces_the_first_but_not_a_persistent_one() {
        with_owner(|| {
            let state = GridEditState::new("t.p.replace");
            let first = CellIndex::new(0, 0);
            let second = CellIndex::new(1, 0);
            let kept = CellIndex::new(2, 0);
            assert_eq!(
                state.open_persistent(kept, &text(CellKind::Text, "kept")),
                OpenOutcome::Opened
            );
            state.begin(first, &text(CellKind::Text, "one"));
            state.begin(second, &text(CellKind::Text, "two"));
            assert!(!state.is_editing(first), "one transient editor at a time");
            assert!(state.is_editing(second));
            assert!(
                state.is_persistent_editor_open(kept),
                "a persistent editor is not what `edit(index)` replaces"
            );
            assert_eq!(state.editors().len(), 2);

            // A trigger on a cell that already has a persistent editor focuses
            // it rather than reseeding — the toolkit's `editor()` hands back the
            // widget its hash already holds.
            assert!(state.focus_editor(second));
            use_text_edit_state("t.p.replace").set_text("edited".to_string());
            assert!(state.focus_editor(kept));
            use_text_edit_state("t.p.replace").set_text("in progress".to_string());
            assert!(state.focus_editor(second));
            state.begin(kept, &text(CellKind::Text, "kept"));
            assert_eq!(
                state.text_at(kept).as_deref(),
                Some("in progress"),
                "the user's typing survived the trigger"
            );
            assert!(state.is_focused(kept));
        });
    }

    #[test]
    fn r1571_open_persistent_promotes_a_transient_editor_and_is_idempotent() {
        with_owner(|| {
            let state = GridEditState::new("t.p.promote");
            let at = CellIndex::new(3, 3);
            state.begin(at, &text(CellKind::Text, "seed"));
            use_text_edit_state("t.p.promote").set_text("half typed".to_string());
            assert_eq!(
                state.open_persistent(at, &text(CellKind::Text, "seed")),
                OpenOutcome::Promoted,
                "the toolkit reaches the same end by inserting the existing widget into \
                 its persistence set — an outcome `void` cannot report"
            );
            assert!(state.is_persistent_editor_open(at));
            assert_eq!(
                state.text_at(at).as_deref(),
                Some("half typed"),
                "promotion keeps the in-flight value"
            );
            assert_eq!(
                state.open_persistent(at, &text(CellKind::Text, "different")),
                OpenOutcome::AlreadyOpen
            );
            assert_eq!(
                state.text_at(at).as_deref(),
                Some("half typed"),
                "an already-open editor is not reseeded from the model"
            );
            assert_eq!(state.editors().len(), 1);
        });
    }

    #[test]
    fn r1571_commit_writes_an_editor_that_does_not_hold_the_keyboard() {
        with_owner(|| {
            let state = GridEditState::new("t.p.remote");
            let a = CellIndex::new(1, 1);
            let b = CellIndex::new(2, 2);
            state.open_persistent(a, &CellEdit::from(CellValue::Int(1)));
            use_text_edit_state("t.p.remote").set_text("41".to_string());
            state.open_persistent(b, &CellEdit::from(CellValue::Int(2)));

            let mut written = Vec::new();
            assert_eq!(
                state.commit_at_with(a, |at, v| {
                    written.push((at, v.clone()));
                    true
                }),
                CommitOutcome::Committed(a),
                "the 'save every open editor' verb — a private hash in the toolkit"
            );
            assert_eq!(written, vec![(a, CellValue::Int(41))]);
            assert!(state.is_focused(b), "committing a is not focusing a");

            // A malformed parked buffer is named, and the model is never asked.
            assert!(state.focus_editor(a));
            use_text_edit_state("t.p.remote").set_text("4x".to_string());
            assert!(state.focus_editor(b));
            let mut asked = false;
            assert_eq!(
                state.commit_at_with(a, |_, _| {
                    asked = true;
                    true
                }),
                CommitOutcome::Malformed
            );
            assert!(!asked);
            assert_eq!(
                state.commit_at_with(CellIndex::new(7, 7), |_, _| true),
                CommitOutcome::NotEditing
            );
        });
    }

    #[test]
    fn r1571_open_cells_is_windowed_and_skips_a_cell_the_model_stopped_editing() {
        with_owner(|| {
            let state = GridEditState::new("t.p.window");
            for row in [0usize, 4, 40, 400] {
                state.open_persistent(CellIndex::new(row, 0), &text(CellKind::Text, "v"));
            }
            assert_eq!(state.editors().len(), 4);
            let window = state.open_cells(0..8, |i| Some(text(CellKind::Text, &i.row.to_string())));
            assert_eq!(
                window
                    .iter()
                    .map(|c| c.editor.index.row)
                    .collect::<Vec<_>>(),
                vec![0, 4],
                "an editor outside the painted rows costs the paint nothing — \
                 the toolkit repositions every persistent editor on every scroll"
            );
            assert!(window.iter().filter(|c| c.focused).count() <= 1);
            // The model turning a cell read-only under an open editor: the
            // honest paint is the display path, and the editor STAYS open —
            // making a paint pass close it would be a §6.3 purity violation.
            let narrowed =
                state.open_cells(0..8, |i| (i.row != 0).then(|| text(CellKind::Text, "v")));
            assert_eq!(narrowed.len(), 1);
            assert!(state.is_persistent_editor_open(CellIndex::new(0, 0)));
        });
    }

    #[test]
    fn r1571_the_set_rejects_every_state_its_invariants_forbid() {
        use EditorPersistence::{Persistent, Transient};
        let editor = |row: usize, persistence: EditorPersistence, buffer: EditBuffer| OpenEditor {
            index: CellIndex::new(row, 0),
            seed: CellValue::Text("s".into()),
            buffer,
            persistence,
        };
        let at = |row: usize| CellIndex::new(row, 0);

        assert_eq!(
            OpenEditors::from_parts(
                vec![
                    editor(1, Persistent, EditBuffer::Parked(String::new())),
                    editor(1, Persistent, EditBuffer::Parked(String::new())),
                ],
                None,
            ),
            Err(OpenEditorsError::DuplicateCell(at(1)))
        );
        assert_eq!(
            OpenEditors::from_parts(
                vec![
                    editor(1, Transient, EditBuffer::Parked(String::new())),
                    editor(2, Transient, EditBuffer::Parked(String::new())),
                ],
                None,
            ),
            Err(OpenEditorsError::TwoTransientEditors)
        );
        assert_eq!(
            OpenEditors::from_parts(vec![], Some(at(3))),
            Err(OpenEditorsError::FocusOnNothing(at(3)))
        );
        assert_eq!(
            OpenEditors::from_parts(
                vec![
                    editor(1, Persistent, EditBuffer::Live),
                    editor(2, Persistent, EditBuffer::Live),
                ],
                Some(at(2)),
            ),
            Err(OpenEditorsError::FieldOwnerIsNotTheFocus(at(1))),
            "an unfocused editor claiming the shared field"
        );
        // The converse half of invariant 4, which a counterfactual found
        // missing: nobody holding the field satisfies the first direction while
        // the focused editor's cell still paints one.
        assert_eq!(
            OpenEditors::from_parts(
                vec![editor(2, Persistent, EditBuffer::Parked("x".into()))],
                Some(at(2)),
            ),
            Err(OpenEditorsError::FieldOwnerIsNotTheFocus(at(2))),
            "a focused text-buffered editor that does NOT hold the field"
        );

        // Sorted by construction, whatever order the caller supplied.
        let ok = OpenEditors::from_parts(
            vec![
                editor(9, Persistent, EditBuffer::Parked("z".into())),
                editor(2, Persistent, EditBuffer::Live),
            ],
            Some(at(2)),
        )
        .expect("valid");
        assert_eq!(
            ok.iter().map(|e| e.index.row).collect::<Vec<_>>(),
            vec![2, 9]
        );
        assert_eq!(ok.get(at(9)).and_then(OpenEditor::parked_text), Some("z"));
        assert!(ok.get(at(3)).is_none());

        // R1561's rule: the wire cannot smuggle in a state the constructors
        // refuse to build.
        let json = serde_json::to_string(&ok).expect("serialize");
        assert_eq!(
            serde_json::from_str::<OpenEditors>(&json).expect("round trip"),
            ok
        );
        let forged = json.replace("\"row\":9", "\"row\":2");
        assert!(
            serde_json::from_str::<OpenEditors>(&forged).is_err(),
            "a forged payload with two editors on one cell is rejected, not \
             silently kept"
        );
    }

    #[test]
    fn r1571_close_all_drops_every_editor_and_its_buffer() {
        with_owner(|| {
            let state = GridEditState::new("t.p.reset");
            for row in 0..32 {
                state.open_persistent(CellIndex::new(row, 0), &text(CellKind::Text, "v"));
            }
            assert_eq!(state.editors().len(), 32);
            state.close_all();
            assert!(state.editors().is_empty());
            assert_eq!(state.editing(), None);
            assert_eq!(
                use_text_edit_state("t.p.reset").text(),
                "",
                "the shared field is cleared with the set that owned it"
            );
            assert!(!state.close_persistent(CellIndex::new(0, 0)));
        });
    }
}
