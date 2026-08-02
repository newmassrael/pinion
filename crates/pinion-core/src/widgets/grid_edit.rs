//! R1544 §5.27 — the **view's half** of Model/View cell editing: which cell
//! has an open editor, what opens one, and how an edit ends.
//!
//! ## What was missing
//!
//! R1532 gave the virtualized grid a per-column **paint** delegate (Qt
//! `QStyledItemDelegate::paint`) and R1535 / R1536 gave the model its
//! decoration role. The other half of Qt's delegate — `createEditor`,
//! `setEditorData`, `setModelData` — did not exist, so *the grid's cell path
//! could not host an editor at all*.
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
//! Qt's, kept: the **model** owns the datum (is this cell editable, what does
//! an editor open with, does the write succeed), the **delegate** owns the
//! editor's appearance, and the **view** owns the latch and the triggers. This
//! module is the view's third; the model's is
//! [`CellEdit`] and `GridModel::edit`, and the
//! delegate's is `VirtualTableData::editor`.
//!
//! It is a state substrate rather than paint for the same reason
//! [`GridSortState`](crate::widgets::grid_sort::GridSortState) and
//! [`ScrollState`](crate::widgets::scroll::ScrollState) are: the binding's
//! `External` **mutates** it on input and the view / a11y tree **read** it
//! through the same `Rc`, so the cell that paints an editor is the cell the
//! keystrokes route to, by construction rather than by two agreeing copies.
//!
//! ## Past Qt 6.11
//!
//! - **A rejected write keeps the editor open.**
//!   `QAbstractItemDelegate::setModelData` returns `void` and
//!   `QAbstractItemView::commitData` ignores what the model did, so a
//!   `setData` that returns `false` still closes the editor and the user's
//!   typing is discarded with no feedback. [`GridEditState::commit_with`]
//!   propagates the model's verdict: a refused write leaves the editor open
//!   with the text intact, which is what a validating DCC grid needs and what
//!   Qt cannot express.
//! - **The editing state is data.** Qt has no public way to ask a view whether
//!   a *transient* editor is open (`isPersistentEditorOpen` covers only the
//!   persistent kind), and the in-flight text lives inside an opaque
//!   `QWidget`. Here the latch is a signal and the editor is scene nodes, so
//!   `scene/snapshot` and an `ExternalIntrospect` slot both see them (§2 #7).
//!
//! ## Scope (honest boundaries)
//!
//! **One editor at a time** — Qt's *transient* editor, which is the default
//! and the only one `EditTrigger` opens. Qt's `openPersistentEditor(index)`
//! keeps N editors open simultaneously; that needs N independent text-edit
//! states, and [`use_text_edit_state`]
//! is keyed by `&'static str`. It is a property of the **view**, not of the
//! delegate this round closes, and it is recorded as a remaining item of the
//! DCC axis rather than built here on speculation about its shape.

use std::rc::Rc;

#[cfg(test)]
use crate::cell_value::CellValue;
use crate::cell_value::{CellEdit, CellKind};
use crate::model_index::{CellIndex, GridExtent};
use crate::reactive::{Owner, Signal};
use crate::widgets::text_edit::use_text_edit_state;

/// R1544 §5.27 — one reason an editor opens: Qt's
/// `QAbstractItemView::EditTrigger`.
///
/// Qt's `CurrentChanged` and `NoEditTriggers` are absent by construction
/// rather than by omission: the first is not a discrete event a binding
/// dispatches (it is the *absence* of a gate — a view that edits whatever the
/// cursor lands on calls [`GridEditState::begin`] from its cursor move, which
/// is what "no trigger gate" means), and the second is the empty set, spelled
/// [`EditTriggers::NONE`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EditTrigger {
    /// A double-click on the cell (Qt `DoubleClicked`).
    DoubleClicked,
    /// A single click on an already-selected cell (Qt `SelectedClicked`) —
    /// the slow-double-click rename gesture a file browser has.
    SelectedClicked,
    /// The dedicated edit key on the current cell (Qt `EditKeyPressed`);
    /// <kbd>F2</kbd> on every desktop platform.
    EditKeyPressed,
    /// Any printable keystroke on the current cell (Qt `AnyKeyPressed`) — the
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

/// R1544 §5.27 — which gestures open an editor: Qt's
/// `QAbstractItemView::EditTriggers` flag set.
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
    /// No gesture opens an editor (Qt `NoEditTriggers`) — a read-only view.
    /// The [`Default`], so a grid is not accidentally editable.
    pub const NONE: Self = Self(0);

    /// Qt's own default for `QAbstractItemView`: double-click, click on the
    /// selected cell, and the edit key. Type-to-replace is **not** in it,
    /// there or here — a grid whose arrow keys navigate cannot also treat
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

    /// Whether **no** gesture opens an editor — Qt's `NoEditTriggers` as a
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

/// R1544 §5.27 — where the cursor goes when an edit ends: Qt's
/// `QAbstractItemDelegate::EndEditHint`.
///
/// Qt's `SubmitModelCache` / `RevertModelCache` are absent: they exist for
/// `QDataWidgetMapper`'s buffered submit policy, which has no analogue here —
/// [`GridEditState::commit_with`] writes through to the model at the moment of
/// commit, so there is no cache to submit or revert. Shipping the arms would
/// name two behaviours nothing implements.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, Hash)]
pub enum EndEditHint {
    /// Close the editor and leave the cursor where it is (Qt `NoHint`) —
    /// <kbd>Enter</kbd>.
    #[default]
    NoHint,
    /// Open an editor on the next editable cell (Qt `EditNextItem`) —
    /// <kbd>Tab</kbd>.
    EditNextItem,
    /// Open an editor on the previous editable cell (Qt `EditPreviousItem`) —
    /// <kbd>Shift+Tab</kbd>.
    EditPreviousItem,
}

/// R1544 §5.27 — the editor currently open: which cell, what it was seeded
/// with, and which editor kind is hosting it.
///
/// One value rather than three parallel signals, so opening and closing are
/// single transitions. Three signals could hold a latched index whose kind
/// belongs to the previously edited column — a state with no meaning that
/// nothing would reject.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OpenEditor {
    /// The cell the editor is open on (Qt: the view's editing `QModelIndex`).
    pub index: CellIndex,
    /// The editor kind, from the model's [`CellEdit::kind`]: the keystroke
    /// gate ([`CellKind::accepts_keystroke`]) and the commit parser
    /// ([`CellKind::parse`]) both read it.
    pub kind: CellKind,
    /// The `Qt::EditRole` text the editor was opened with — kept so a commit
    /// can tell an untouched editor from an edited one, and so a delegate's
    /// editor can render the original beside the in-flight value.
    pub seed: String,
}

/// R1544 §5.27 — the grid's editing latch and trigger gate: the half of Qt's
/// editing decomposition that belongs to `QAbstractItemView`.
///
/// Created once through [`use_grid_edit`] and shared by the binding's
/// `External` (which mutates it) and the view / a11y tree (which read it)
/// through the same `Rc` — the [`ScrollState`](crate::widgets::scroll::ScrollState)
/// pattern. Reading [`open`](Self::open) inside a view-fn auto-subscribes, so
/// opening an editor repaints exactly like a scroll-offset change.
///
/// The in-flight text is **not** held here. It lives in the
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
    open: Signal<Option<OpenEditor>>,
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
            open: Signal::new(None),
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

    /// The open editor, or `None` when the grid is not editing. Reading this
    /// in a view-fn subscribes it to open / close.
    #[must_use]
    pub fn open(&self) -> Option<OpenEditor> {
        self.open.get()
    }

    /// The cell an editor is open on — [`open`](Self::open) narrowed to the
    /// question the paint layer asks once per painted cell.
    #[must_use]
    pub fn editing(&self) -> Option<CellIndex> {
        self.open.get().map(|e| e.index)
    }

    /// Whether an editor is open on `index`.
    #[must_use]
    pub fn is_editing(&self, index: CellIndex) -> bool {
        self.editing() == Some(index)
    }

    /// The open editor's kind — the [`edit_field_keymap`](crate::input::edit_field_keymap)
    /// keystroke gate's argument. `None` when not editing.
    #[must_use]
    pub fn kind(&self) -> Option<CellKind> {
        self.open.get().map(|e| e.kind)
    }

    /// The in-flight editor text: the inline field's live buffer, **not** the
    /// seed. Empty when no editor is open.
    #[must_use]
    pub fn text(&self) -> String {
        if self.open.get().is_none() {
            return String::new();
        }
        self.field.text()
    }

    /// Whether the in-flight text differs from what the editor opened with.
    /// `false` when nothing is open.
    ///
    /// The question a close-without-commit path asks and Qt answers only by
    /// re-reading the editor widget: `QAbstractItemView` keeps no record of
    /// what `setEditorData` seeded, so a Qt view cannot distinguish an
    /// untouched editor from one edited back to its original value.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.open.get().is_some_and(|e| self.text() != e.seed)
    }

    /// The active trigger set.
    #[must_use]
    pub fn triggers(&self) -> EditTriggers {
        self.triggers.get()
    }

    /// Replace the trigger set (Qt `setEditTriggers`).
    pub fn set_triggers(&self, triggers: EditTriggers) {
        self.triggers.set(triggers);
    }

    /// Open an editor on `index` seeded from the model's `edit` answer,
    /// unconditionally — Qt's `QAbstractItemView::edit(const QModelIndex&)`,
    /// the programmatic open that bypasses the trigger gate.
    ///
    /// Taking a [`CellEdit`] rather than an index alone is what makes "an
    /// editor open on a cell the model will not edit" unrepresentable: only
    /// the model can produce one, and it produces `None` for a read-only cell.
    ///
    /// The field's buffer is seeded and fully selected, so the first printable
    /// keystroke replaces it — the type-to-replace behaviour Qt gets from
    /// `QLineEdit::selectAll` on editor focus, and the reason
    /// [`EditTrigger::AnyKeyPressed`] needs no special seeding path.
    pub fn begin(&self, index: CellIndex, edit: &CellEdit) {
        self.field.set_text(edit.text.clone());
        self.field.set_selection(0, edit.text.chars().count());
        self.open.set(Some(OpenEditor {
            index,
            kind: edit.kind,
            seed: edit.text.clone(),
        }));
    }

    /// Open an editor on `index` **if** `trigger` is in the active set — Qt's
    /// `QAbstractItemView::edit(index, trigger, event)`. Returns whether it
    /// opened.
    #[must_use]
    pub fn begin_on(&self, trigger: EditTrigger, index: CellIndex, edit: &CellEdit) -> bool {
        if !self.triggers.get().contains(trigger) {
            return false;
        }
        self.begin(index, edit);
        true
    }

    /// Abandon the edit, discarding the in-flight text — <kbd>Escape</kbd>,
    /// Qt's `closeEditor(editor, RevertModelCache)` on a write-through model.
    pub fn cancel(&self) {
        self.close();
    }

    /// Write the in-flight text back through `set` and, **if the model
    /// accepts it**, close the editor. Qt's `commitData` +
    /// `setModelData`.
    ///
    /// Returns the committed [`CellIndex`] on success, and `None` both when
    /// no editor is open and when the model refused the write. The refusal
    /// case is the divergence from Qt this seam exists for: `setModelData`
    /// returns `void` there, so a rejected value closes the editor anyway and
    /// the typing is lost. Here the editor stays open holding the text the
    /// user typed, which is the only state from which they can correct it.
    ///
    /// `set` receives the raw buffer; parsing is its job, through the same
    /// [`CellKind::parse`] the seed's [`CellValue::edit_text`](crate::cell_value::CellValue::edit_text)
    /// is the inverse of. Returning `false` from an unparseable commit is
    /// what keeps a malformed number from silently reverting.
    pub fn commit_with(&self, set: impl FnOnce(CellIndex, &str) -> bool) -> Option<CellIndex> {
        let editor = self.open.get()?;
        let text = self.field.text();
        if !set(editor.index, &text) {
            return None;
        }
        self.close();
        Some(editor.index)
    }

    /// Honour an [`EndEditHint`] by opening an editor on the next / previous
    /// **editable** cell after `from` — Qt's `closeEditor(editor, hint)` move
    /// half, which walks with `moveCursor(MoveNext)` and edits if the landing
    /// index is editable.
    ///
    /// Returns whether an editor opened. [`EndEditHint::NoHint`] always
    /// returns `false` — there is nothing to move to.
    ///
    /// The walk is row-major over the **model** extent (not the painted
    /// window — Qt moves through indices, and a grid windowing 5 of 200
    /// columns must not stop at the window edge), and it **wraps**: past the
    /// last cell it resumes at the first. Qt's `QTableView::moveCursor` stops
    /// there instead; wrapping is the spreadsheet behaviour and it costs
    /// nothing, because the walk is bounded by
    /// [`GridExtent::cell_count`] and so terminates on a model with no
    /// editable cell at all rather than spinning.
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

    /// Clear the latch and the field buffer. The one place the editor closes,
    /// so a close cannot leave the previous edit's text in the buffer for the
    /// next open to inherit.
    fn close(&self) {
        self.open.set(None);
        self.field.set_text(String::new());
    }
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
        open: Signal::new(None),
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
        CellEdit::new(kind, s)
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
            assert_eq!(state.open(), None);
            state.begin(CellIndex::new(3, 1), &text(CellKind::Text, "Bravo"));
            let open = state.open().expect("open");
            assert_eq!(open.index, CellIndex::new(3, 1));
            assert_eq!(open.kind, CellKind::Text);
            assert_eq!(open.seed, "Bravo");
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
                "type-to-replace is not in Qt's default set"
            );
            assert_eq!(state.open(), None, "a refused trigger opens nothing");
            assert!(state.begin_on(EditTrigger::DoubleClicked, at, &edit));
            assert!(state.open().is_some());
            state.cancel();
            state.set_triggers(EditTriggers::NONE);
            assert!(
                !state.begin_on(EditTrigger::DoubleClicked, at, &edit),
                "NoEditTriggers refuses every gesture"
            );
            assert!(
                {
                    state.begin(at, &edit);
                    state.open().is_some()
                },
                "the programmatic open bypasses the gate (Qt `edit(index)`)"
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
            assert_eq!(state.open(), None);
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
            // This is the divergence from Qt: `setModelData` returns void, so
            // there a rejected value closes the editor and the typing is gone.
            assert_eq!(
                state.commit_with(|_, _| false),
                None,
                "a refused write reports no committed index"
            );
            assert_eq!(state.open().map(|e| e.index), Some(at), "still editing");
            assert_eq!(
                state.text(),
                "999",
                "the user's text survives so they can correct it"
            );
            let mut seen = String::new();
            assert_eq!(
                state.commit_with(|_, t| {
                    seen = t.to_string();
                    true
                }),
                Some(at)
            );
            assert_eq!(seen, "999", "the model is handed the live buffer");
            assert_eq!(state.open(), None, "an accepted write closes the editor");
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
                None
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
                "edited back to the original is not dirty — Qt cannot tell"
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
            assert_eq!(state.open().map(|e| e.index), Some(CellIndex::new(0, 2)));
            // From the LAST cell, forward wraps to the first.
            assert!(state.advance(
                CellIndex::new(2, 2),
                EndEditHint::EditNextItem,
                extent,
                sparse_edit
            ));
            assert_eq!(state.open().map(|e| e.index), Some(CellIndex::new(0, 0)));
            // Backward from the first wraps to the last.
            assert!(state.advance(
                CellIndex::new(0, 0),
                EndEditHint::EditPreviousItem,
                extent,
                sparse_edit
            ));
            assert_eq!(state.open().map(|e| e.index), Some(CellIndex::new(2, 2)));
            // (0,2) backward is (0,0), not the read-only (0,1).
            assert!(state.advance(
                CellIndex::new(0, 2),
                EndEditHint::EditPreviousItem,
                extent,
                sparse_edit
            ));
            assert_eq!(state.open().map(|e| e.index), Some(CellIndex::new(0, 0)));
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
            assert_eq!(state.open(), None);
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
            assert_eq!(state.open(), None);
        });
    }

    #[test]
    fn advance_can_land_on_the_cell_it_started_from() {
        with_owner(|| {
            let state = GridEditState::new("t.self");
            // One editable cell in the whole model: a full lap comes back to
            // it, which is what Qt's Tab does in a single-editable-cell view.
            let only = CellIndex::new(1, 1);
            assert!(state.advance(
                only,
                EndEditHint::EditNextItem,
                GridExtent::new(3, 3),
                |i| { (i == only).then(|| text(CellKind::Text, "v")) }
            ));
            assert_eq!(state.open().map(|e| e.index), Some(only));
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
        assert_eq!(edit.kind, CellKind::Float);
        assert_eq!(edit.text, value.edit_text());
        assert_eq!(
            CellKind::Float.parse(&edit.text),
            Some(CellValue::Float(1234.5)),
            "the seed parses back to the value it came from"
        );
    }
}
