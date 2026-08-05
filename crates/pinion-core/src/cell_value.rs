//! R837 §5.38 — `CellValue`: a typed editable-cell value.
//!
//! The unifying data model behind every editable-grid widget — a property
//! grid value (`hello-property-grid`, R836, the 1st consumer) and a data
//! grid cell (`hello-data-grid`, R837, the 2nd consumer). One value, four
//! editor renderings, dispatched by [`CellValue::kind`].
//!
//! This is the pure-logic half (kind dispatch, display / edit formatting,
//! parse, the keystroke gate, the introspect read / intervene write) — no
//! paint, no a11y. It was lifted here from `hello-property-grid` at the 2nd
//! consumer per the SSOT "lift at the 2nd consumer" discipline
//! (`[[abstraction-needs-second-consumer]]`): a typed-value model is *pure
//! logic*, where a divergence between consumers would be a bug, not a style
//! choice, so it is the textbook lift (the R703 "pure introspection +
//! mapping = 2nd-consumer immediate lift" rule). The *paint* of a cell (how
//! a bool draws a checkbox, where the inline editor sits) stays per-binding
//! — that is the opinionated half the Rule of Three governs.

use serde::{Deserialize, Serialize};

use crate::external::{InterveneError, IntrospectValue};
use crate::style::Color;
use crate::widgets::grid_sort::FilterOp;

/// A typed editable-cell value. `Signal<T>` requires `Serialize +
/// DeserializeOwned` (the R36 §5.31 hot-reload bound), so the model derives
/// serde — editable-grid bindings hold the value model in a `Signal`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CellValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    /// An enumerated value — one of a fixed list of option labels (the
    /// property-grid combobox cell, R867). Unlike the scalar kinds, the
    /// options are part of the value's identity, not derivable from the
    /// [`CellKind`], so the editor is a popup listbox (not text or toggle)
    /// and the `intervene` write addresses an option by index
    /// ([`CellValue::with_intervene`]).
    Choice {
        selected: usize,
        options: Vec<String>,
    },
    /// An sRGB colour (the property-grid colour cell, R869). Edited by a
    /// popup — a preset swatch palette plus a hex field for an arbitrary
    /// value — not inline text or a toggle. The `intervene` write takes a
    /// `#RRGGBB[AA]` hex string ([`CellValue::with_intervene`]). A 2-D HSV
    /// pad in the cell is a deferred follow-up: it needs a `Color`→HSV
    /// decomposition (the inverse of `Color::from_hsv`) to seed the pad from
    /// a stored colour, which the substrate does not yet have — the
    /// standalone `hello-color-picker` starts from picker state, not a colour.
    Color(Color),
}

/// R1555 §5.27 — which editor a [`CellKind`] opens: Qt's
/// `QItemEditorFactory`, as a pure function of the datum's type.
///
/// # The axis this is
///
/// Qt reaches this decision through
/// `QItemEditorFactory::createEditor(QMetaType::Type, parent)` — a registry
/// from a value's type to an editor widget, which `QStyledItemDelegate`
/// consults and `setItemDelegateForColumn` overrides. pinion had the
/// **override** (R1532's per-column paint delegate, R1544's editor delegate)
/// and not the registry, so one inline text field was the built-in editor for
/// every kind. For two of the six that editor cannot work at all:
/// [`CellKind::Bool`] and [`CellKind::Choice`] refuse every keystroke
/// ([`CellKind::accepts_keystroke`]) and parse to nothing
/// ([`CellKind::parse`]), so the seam opened a field that could not be typed
/// into and whose commit could never produce a value. The tree already *knew*
/// this — [`CellKind::is_text_editable`] has said so since R837 — and the
/// factory that would have acted on it did not exist.
///
/// # Why a form and not a widget
///
/// `createEditor` **instantiates a `QWidget`**, so "what editor would this
/// cell get" is a question Qt cannot answer without constructing one, and the
/// registry cannot be enumerated at all (`creatorMap` is private and there is
/// no accessor for which types are registered). A form is a *value*, which is
/// what lets `scene/cell_editors` publish the whole mapping, an a11y pass
/// derive the editor's role, and a test assert the census — none of which
/// requires painting anything.
///
/// # Where each form is past Qt's default factory
///
/// - [`Toggle`](Self::Toggle) — Qt's `QMetaType::Bool` creator is a two-item
///   `QComboBox` reading "False" / "True", which is why a Qt application that
///   wants a checkbox writes a delegate. A checkbox is the shipped form here.
/// - [`Stepper`](Self::Stepper) — Qt's `QMetaType::Double` creator is a
///   `QDoubleSpinBox` left at its default `decimals() == 2`, so opening the
///   default editor on a cell holding `1.234` and committing writes `1.23`.
///   The seed here is [`CellValue::edit_text`] and the commit is
///   [`CellKind::parse`], documented inverses, so an untouched open-and-commit
///   cannot change the datum.
/// - [`Selector`](Self::Selector) — Qt's factory is keyed by `QVariant` type,
///   and an enumerated value **is an int** to `QVariant`, so no registration
///   can produce a combo box populated with that cell's options.
///   [`CellValue::Choice`] carries its own domain, so the form can.
/// - [`Swatch`](Self::Swatch) — Qt's default factory has **no** `QColor`
///   creator; `createEditor` answers `nullptr`, `QStyledItemDelegate` passes
///   it through, and `QAbstractItemView::edit` then silently does nothing. A
///   colour cell in a plain `QTableView` is simply not editable.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorForm {
    /// An inline text field (Qt `QLineEdit` / `QExpandingLineEdit`).
    Field,
    /// An inline text field with step affordances (Qt `QSpinBox` /
    /// `QDoubleSpinBox`).
    Stepper,
    /// An inline checkbox.
    Toggle,
    /// A closed selector showing the current option (Qt `QComboBox`), which
    /// the binding opens into a list.
    Selector,
    /// A colour chip beside a hex field.
    Swatch,
}

impl EditorForm {
    /// Every form, in wire order — the census a round-trip test and the
    /// `scene/cell_editors` enumeration both walk, so a form added without a
    /// token fails here rather than serializing as a missing member.
    pub const ALL: [EditorForm; 5] = [
        EditorForm::Field,
        EditorForm::Stepper,
        EditorForm::Toggle,
        EditorForm::Selector,
        EditorForm::Swatch,
    ];

    /// The wire token.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            EditorForm::Field => "field",
            EditorForm::Stepper => "stepper",
            EditorForm::Toggle => "toggle",
            EditorForm::Selector => "selector",
            EditorForm::Swatch => "swatch",
        }
    }

    /// Parse a [`name`](Self::name). `None` for an unknown token.
    #[must_use]
    pub fn from_wire(token: &str) -> Option<Self> {
        EditorForm::ALL.into_iter().find(|f| f.name() == token)
    }

    /// Whether the **cell's own box** is a text field the keystroke gate
    /// feeds — the begin-edit guard for an inline type-in.
    ///
    /// [`Swatch`](Self::Swatch) answers `false` even though it contains a hex
    /// field: that field is a *part* of the editor the binding focuses, not
    /// the cell's box, so a printable key arriving at the cell must not be
    /// treated as the start of a hex edit.
    #[must_use]
    pub fn inline_text(self) -> bool {
        matches!(self, EditorForm::Field | EditorForm::Stepper)
    }

    /// Whether the in-flight value is read back out of a **text buffer**
    /// (through [`CellKind::parse`]) rather than held as a typed value by the
    /// editing latch.
    ///
    /// This is the axis Qt has too and states nowhere: a `QLineEdit` editor's
    /// in-flight value is its text and a `QComboBox` editor's is its current
    /// index, and `setModelData` reaches each by a `qobject_cast` the delegate
    /// author has to get right. Here it is one exhaustive answer per form, so
    /// the latch cannot look for a value in the half that does not hold it.
    #[must_use]
    pub fn buffer_is_text(self) -> bool {
        match self {
            EditorForm::Field | EditorForm::Stepper | EditorForm::Swatch => true,
            EditorForm::Toggle | EditorForm::Selector => false,
        }
    }
}

/// The static descriptor of a [`CellValue`] — drives editor behaviour
/// (toggle vs text-edit vs popup), the keystroke gate, and parse / format.
/// `Copy` (per-column kind arrays live in `[CellKind; N]`); the option list a
/// [`CellKind::Choice`] needs lives on the *value*, not the kind, so `Copy`
/// is preserved.
///
/// R1544 — derives serde for the reason [`CellValue`] does: the grid's editing
/// latch ([`OpenEditor`](crate::widgets::grid_edit::OpenEditor)) records the
/// open editor's kind and lives in a `Signal`, whose R36 §5.31 hot-reload
/// bound is `Serialize + DeserializeOwned`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellKind {
    Bool,
    Int,
    Float,
    Text,
    Choice,
    Color,
}

impl CellValue {
    /// The value's kind.
    #[must_use]
    pub fn kind(&self) -> CellKind {
        match self {
            CellValue::Bool(_) => CellKind::Bool,
            CellValue::Int(_) => CellKind::Int,
            CellValue::Float(_) => CellKind::Float,
            CellValue::Text(_) => CellKind::Text,
            CellValue::Choice { .. } => CellKind::Choice,
            CellValue::Color(_) => CellKind::Color,
        }
    }

    /// The wire value for `scene/query` (AI-first introspection). A `Choice`
    /// reads as a structured object — `{ selected, label, options }` — so the
    /// AI sees the current index, its label, and the whole domain in one
    /// query (the richest honest representation of an enumerated cell).
    #[must_use]
    pub fn to_introspect(&self) -> IntrospectValue {
        match self {
            CellValue::Bool(b) => IntrospectValue::Bool(*b),
            CellValue::Int(i) => IntrospectValue::Int(*i),
            CellValue::Float(f) => IntrospectValue::Float(*f),
            CellValue::Text(s) => IntrospectValue::Text(s.clone()),
            CellValue::Choice { selected, options } => IntrospectValue::Json(serde_json::json!({
                "selected": selected,
                "label": selected_label(*selected, options),
                "options": options,
            })),
            CellValue::Color(c) => IntrospectValue::Json(serde_json::json!({
                "hex": c.to_hex(),
                "r": c.r,
                "g": c.g,
                "b": c.b,
                "a": c.a,
            })),
        }
    }

    /// The value shown in a non-edited cell. Bools render as a checkbox
    /// affordance (not this text), but the spoken / AT name reuses the
    /// `On` / `Off` wording so the value reads naturally; a `Choice` reads
    /// as its selected option label.
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            CellValue::Bool(b) => if *b { "On" } else { "Off" }.to_owned(),
            CellValue::Int(i) => i.to_string(),
            CellValue::Float(f) => format_float(*f),
            CellValue::Text(s) => s.clone(),
            CellValue::Choice { selected, options } => selected_label(*selected, options),
            CellValue::Color(c) => c.to_hex(),
        }
    }

    /// R886.1 §5.40 — the typed sort comparison for a sortable grid
    /// column. Same-kind cells (the homogeneous-column invariant every
    /// `CellKind`-columned grid holds) compare by their TYPE: `Bool`
    /// `false < true` (semantic, not the accident of `"Off" < "On"`
    /// label spelling), `Int` exactly (no f64 round-trip loss past
    /// 2^53), `Float` via `total_cmp`, `Text` / `Choice` through the
    /// numeric-aware [`cell_cmp`](crate::widgets::table::cell_cmp)
    /// string SSOT. Cross-kind pairs (defensive — a well-formed column
    /// never mixes) fall back to `cell_cmp` over the display text. A
    /// total order in every arm, so `slice::sort_by` is panic-free.
    ///
    /// The typed model IS the sort SSOT for an editable grid; sorting
    /// through `display()` stringification would re-derive the value a
    /// layer down and tie the order to presentation labels.
    #[must_use]
    pub fn sort_cmp(&self, other: &Self) -> core::cmp::Ordering {
        use crate::widgets::table::cell_cmp;
        match (self, other) {
            (CellValue::Bool(a), CellValue::Bool(b)) => a.cmp(b),
            (CellValue::Int(a), CellValue::Int(b)) => a.cmp(b),
            (CellValue::Float(a), CellValue::Float(b)) => a.total_cmp(b),
            (CellValue::Text(a), CellValue::Text(b)) => cell_cmp(a, b),
            (a, b) => cell_cmp(&a.display(), &b.display()),
        }
    }

    /// R920 §5.40 — whether two values are equal by the substrate's TOTAL order
    /// ([`sort_cmp`](Self::sort_cmp)), NOT the derived IEEE `PartialEq`. A `Float`
    /// of `NaN` is equal to `NaN` under this (where `==` would be `false`), so a
    /// no-op guard ("re-setting the same value journals nothing") and a
    /// modified-from-default check ("is this property changed?") are both correct
    /// for every kind. The single home for "same typed value (NaN-safe)" — the
    /// node-editor's `apply_set_default` no-op guard (R900), the property grid's
    /// modified indicator (R919), and the inspector's multi-object "Multiple
    /// Values" detection (R922, "do the selected objects agree?") are its three
    /// consumers; all must avoid the `#[derive(PartialEq)]` on this type, which
    /// is right there and wrong for `NaN`. Peer of [`matches_filter`](Self::matches_filter) (typed equality vs
    /// a wire string) and [`sort_cmp`](Self::sort_cmp) (the ordering it builds on).
    #[must_use]
    pub fn value_eq(&self, other: &Self) -> bool {
        self.sort_cmp(other) == core::cmp::Ordering::Equal
    }

    /// R891 §5.40 — whether this cell passes an equality filter whose wire
    /// value is `value`. The typed peer of [`sort_cmp`](Self::sort_cmp): a
    /// filter matches by the cell's TYPED value, never its display label, so
    /// the wire value is interpreted the way a committed edit would be — a
    /// numeric column compares the parsed number (`"024"` matches `Int(24)`,
    /// `"2.50"` matches `Float(2.5)`, whitespace-tolerant like
    /// [`CellKind::parse`]), and a bool compares the canonical `"true"` /
    /// `"false"` the AI / `edit_text` speak (not the `"On"` / `"Off"`
    /// presentation accident the sort lesson warns of). A `Text` cell matches
    /// the value EXACTLY (the cross-grid [`GridFilter`] "exact cell text"
    /// contract for the one string kind); a `Choice` matches its selected
    /// option label; a `Color` matches a parsed `#RRGGBB[AA]` hex (so case /
    /// shorthand fold). An unparseable numeric / colour value matches
    /// nothing (`"Count=abc"` keeps no rows) — the honest equality result.
    ///
    /// The match SEMANTICS are this consumer's policy, not the wire vocab:
    /// the read-only [`GridSortState`](crate::widgets::grid_sort::GridSortState)
    /// matches its `String` cells with raw text equality, while the typed grid
    /// matches by value here — the same R778 family ruling sort follows (the
    /// shared part is the [`GridFilter`] wire form, not the comparison).
    ///
    /// [`GridFilter`]: crate::widgets::grid_sort::GridFilter
    #[must_use]
    pub fn matches_filter(&self, value: &str) -> bool {
        match self {
            CellValue::Bool(b) => value.trim().parse::<bool>().is_ok_and(|v| v == *b),
            CellValue::Int(i) => value.trim().parse::<i64>().is_ok_and(|v| v == *i),
            CellValue::Float(f) => value
                .trim()
                .parse::<f64>()
                .is_ok_and(|v| v.total_cmp(f) == core::cmp::Ordering::Equal),
            CellValue::Text(s) => s == value,
            CellValue::Choice { selected, options } => selected_label(*selected, options) == value,
            CellValue::Color(c) => Color::from_hex(value.trim()).is_some_and(|v| v == *c),
        }
    }

    /// R997 §5.40 — whether this cell satisfies the [`FilterOp`] facet
    /// `self op value`: the typed-grid peer of the at-scale `GridSortState`'s
    /// text-based [`FilterOp::matches`](crate::widgets::grid_sort::FilterOp::matches).
    /// The wire VOCAB (the op + value string) is shared across grids; the
    /// COMPARISON is this typed consumer's policy (the same R778-family ruling
    /// [`matches_filter`](Self::matches_filter) / [`sort_cmp`](Self::sort_cmp)
    /// follow): `Eq` / `Ne` reuse the type-aware `matches_filter` (so `"024"`
    /// still matches `Int(24)`); the ordered ops (`Lt`..`Ge`) parse `value`
    /// into this cell's own kind via [`CellKind::parse`] and compare through
    /// the typed `sort_cmp` (a numeric / text column orders by number / text).
    /// A kind [`CellKind::parse`] never text-parses (a `Bool` toggles, a
    /// `Choice` popup-selects), or any unparseable numeric operand, matches
    /// nothing for an ordered op — the honest result; such columns filter by
    /// `Eq`, not `<`. `Contains` is a substring test over the
    /// [`display`](Self::display) label — the one op that is inherently textual.
    #[must_use]
    pub fn matches_facet(&self, op: FilterOp, value: &str) -> bool {
        match op {
            FilterOp::Eq => self.matches_filter(value),
            FilterOp::Ne => !self.matches_filter(value),
            FilterOp::Contains => self.display().contains(value),
            FilterOp::Lt | FilterOp::Le | FilterOp::Gt | FilterOp::Ge => {
                // The typed comparator (sort_cmp against a same-kind operand);
                // the op-vs-Ordering truth table is shared with the text path.
                self.kind()
                    .parse(value)
                    .is_some_and(|other| op.ordering_matches(self.sort_cmp(&other)))
            }
        }
    }

    /// The text the inline editor is seeded with when the cell enters edit
    /// mode (the round-trip inverse of [`CellKind::parse`]). A `Choice` is
    /// popup-edited, not text-edited, so this is unused for it — it returns
    /// the selected label for completeness.
    #[must_use]
    pub fn edit_text(&self) -> String {
        match self {
            CellValue::Bool(b) => b.to_string(),
            CellValue::Int(i) => i.to_string(),
            CellValue::Float(f) => format_float(*f),
            CellValue::Text(s) => s.clone(),
            CellValue::Choice { selected, options } => selected_label(*selected, options),
            CellValue::Color(c) => c.to_hex(),
        }
    }

    /// Apply an `intervene` payload, producing the updated value — the
    /// value-level write path (the kind-level [`CellKind::coerce`] cannot
    /// build a [`CellValue::Choice`] because the options live on the value).
    /// Scalar kinds delegate to `coerce`.
    ///
    /// R1253 — the write form is **read/write symmetric**: for the two rich
    /// kinds whose [`to_introspect`](Self::to_introspect) emits a JSON object, a
    /// `query value.<i>` -> `intervene value.<i>` round-trip now works. A
    /// `Choice` accepts either the ergonomic bare [`IntrospectValue::Int`] index
    /// OR the emitted `{selected,…}` JSON; a `Color` accepts either the bare hex
    /// [`IntrospectValue::Text`] (trimmed, matching [`CellKind::parse`]) OR the
    /// emitted `{hex,…}` JSON. Both preserve the value's own domain (a Choice
    /// keeps its options).
    ///
    /// # Errors
    ///
    /// [`InterveneError::TypeMismatch`] when the payload variant is wrong for
    /// this value's kind; [`InterveneError::OutOfRange`] when a `Choice`
    /// index is negative or past the option list, or a `Color` hex string is
    /// malformed.
    pub fn with_intervene(&self, value: IntrospectValue) -> Result<CellValue, InterveneError> {
        match self {
            CellValue::Choice { options, .. } => {
                // R1253 — accept the ergonomic bare `Int` index OR the JSON shape
                // `to_introspect` emits (`{selected,label,options}`), so a
                // `query value.<i>` -> `intervene value.<i>` round-trip works and
                // the read/write wire forms are symmetric.
                let idx = match value {
                    IntrospectValue::Int(i) => usize::try_from(i).map_err(|_| {
                        InterveneError::out_of_range(format!("{i} is not an option index"))
                    })?,
                    IntrospectValue::Json(v) => {
                        let sel = v
                            .get("selected")
                            .and_then(serde_json::Value::as_u64)
                            .ok_or(InterveneError::TypeMismatch)?;
                        usize::try_from(sel).map_err(|_| {
                            InterveneError::out_of_range(format!("{sel} is not an option index"))
                        })?
                    }
                    _ => return Err(InterveneError::TypeMismatch),
                };
                if idx >= options.len() {
                    return Err(InterveneError::out_of_range(format!(
                        "no option {idx} on this cell (it offers {}: {})",
                        options.len(),
                        options.join(", ")
                    )));
                }
                Ok(CellValue::Choice {
                    selected: idx,
                    options: options.clone(),
                })
            }
            CellValue::Color(_) => {
                // R1253 — accept the ergonomic bare hex `Text` OR the JSON shape
                // `to_introspect` emits (`{hex,r,g,b,a}`), so query -> intervene
                // round-trips. Trim the hex to match [`CellKind::parse`] (the
                // type-in commit path), so both AI-first colour-write surfaces
                // share one acceptance set (no whitespace drift).
                let hex = match value {
                    IntrospectValue::Text(s) => s,
                    IntrospectValue::Json(v) => v
                        .get("hex")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .ok_or(InterveneError::TypeMismatch)?,
                    _ => return Err(InterveneError::TypeMismatch),
                };
                Color::from_hex(hex.trim())
                    .map(CellValue::Color)
                    .ok_or_else(|| {
                        InterveneError::out_of_range(format!(
                            "{:?} is not a colour (expected #rgb, #rrggbb or #rrggbbaa)",
                            hex.trim()
                        ))
                    })
            }
            _ => self.kind().coerce(value),
        }
    }
}

impl CellKind {
    /// R1555 — every kind, in wire order: the census
    /// [`editor_form`](Self::editor_form)'s coverage test and the
    /// `scene/cell_editors` enumeration both walk, so a kind added without a
    /// form is a compile error in the `match` and a missing row here.
    pub const ALL: [CellKind; 6] = [
        CellKind::Bool,
        CellKind::Int,
        CellKind::Float,
        CellKind::Text,
        CellKind::Choice,
        CellKind::Color,
    ];

    /// The wire vocab token (`"bool"` / `"int"` / `"float"` / `"text"`) —
    /// the `kind` introspect slot's value.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            CellKind::Bool => "bool",
            CellKind::Int => "int",
            CellKind::Float => "float",
            CellKind::Text => "text",
            CellKind::Choice => "choice",
            CellKind::Color => "color",
        }
    }

    /// R1555 §5.27 — which editor this kind opens: Qt's
    /// `QItemEditorFactory::createEditor(QMetaType::Type, parent)`, as a pure
    /// function. See [`EditorForm`] for what each form is and where it is past
    /// Qt's default factory.
    ///
    /// Total by construction — the `match` is exhaustive over
    /// [`CellKind::ALL`], so a seventh kind cannot be added without stating
    /// the editor it opens. Qt's registry is a `QHash` with a `nullptr`
    /// fallthrough, which is why a `QColor` cell there silently opens nothing.
    #[must_use]
    pub fn editor_form(self) -> EditorForm {
        match self {
            CellKind::Bool => EditorForm::Toggle,
            CellKind::Int | CellKind::Float => EditorForm::Stepper,
            CellKind::Text => EditorForm::Field,
            CellKind::Choice => EditorForm::Selector,
            CellKind::Color => EditorForm::Swatch,
        }
    }

    /// Whether the kind is edited as text (text / int / float) rather than
    /// toggled (bool) or popup-edited (choice / colour). The begin-edit guard
    /// for the inline text field.
    ///
    /// R1555 — **derived** from [`editor_form`](Self::editor_form) rather than
    /// matched again. The two answers are the same classification, and the
    /// pre-R1555 duplicate is exactly the shape that decays: this predicate
    /// already said Bool and Choice are not text-edited while the seam opened a
    /// text field for them anyway.
    #[must_use]
    pub fn is_text_editable(self) -> bool {
        self.editor_form().inline_text()
    }

    /// R1555 §5.27 — step an editor buffer by `delta` steps, for the kinds
    /// whose form is a [`EditorForm::Stepper`]: Qt
    /// `QAbstractSpinBox::stepBy(int steps)`.
    ///
    /// Text in, text out, because the buffer is what the step acts on — the
    /// same buffer the keystroke gate feeds and [`parse`](Self::parse) reads
    /// back, so a stepped value and a typed one are the same state.
    ///
    /// `None` when the kind has no stepper, and also when the buffer does not
    /// currently hold a value of this kind. That second case is a divergence
    /// worth stating: a `QSpinBox` always holds a validated value, so Qt's
    /// `stepBy` on a half-typed field silently steps from whatever its
    /// validator last accepted. Here a step from a malformed buffer is refused
    /// and the user's text is left alone.
    ///
    /// A single step is `1` for both numeric kinds, which is Qt's default for
    /// `QSpinBox` and for `QDoubleSpinBox`. A per-column step is a property of
    /// the column, so it belongs to a delegate — in Qt too: the default factory
    /// sets no `singleStep`.
    #[must_use]
    pub fn step_text(self, text: &str, delta: i64) -> Option<String> {
        match self {
            CellKind::Int => text
                .trim()
                .parse::<i64>()
                .ok()
                .map(|n| n.saturating_add(delta).to_string()),
            #[expect(
                clippy::cast_precision_loss,
                reason = "a step count past 2^53 cannot arrive from a step \
                          gesture, and the alternative — refusing the step — \
                          would be a worse answer than a rounded one"
            )]
            CellKind::Float => text
                .trim()
                .parse::<f64>()
                .ok()
                .map(|f| format_float(f + delta as f64)),
            CellKind::Bool | CellKind::Text | CellKind::Choice | CellKind::Color => None,
        }
    }

    /// Whether a single printable keystroke is allowed into this kind's
    /// inline editor — the `<input type=number>` keystroke gate. Text
    /// accepts any single character; int accepts digits / sign; float adds
    /// the decimal point; bool accepts none (bools toggle). Caret / named
    /// keys are handled by the caller before this gate.
    #[must_use]
    pub fn accepts_keystroke(self, key: &str) -> bool {
        match self {
            CellKind::Bool | CellKind::Choice => false,
            // A colour's popup hex field accepts hex digits + the leading `#`.
            CellKind::Color => single_char(key, |c| c.is_ascii_hexdigit() || c == '#'),
            CellKind::Text => single_char(key, |_| true),
            CellKind::Int => single_char(key, |c| c.is_ascii_digit() || c == '-'),
            CellKind::Float => single_char(key, |c| c.is_ascii_digit() || c == '-' || c == '.'),
        }
    }

    /// Parse committed editor text into a typed value. `None` on a malformed
    /// numeric commit (the caller keeps the prior value — no data loss) or
    /// for a bool (bools toggle, they are never text-parsed).
    #[must_use]
    pub fn parse(self, text: &str) -> Option<CellValue> {
        let trimmed = text.trim();
        match self {
            // Bools toggle and choices popup-select — neither is text-parsed.
            CellKind::Bool | CellKind::Choice => None,
            // A colour parses from a `#RRGGBB[AA]` hex string — the popup's
            // hex field commits through this path.
            CellKind::Color => Color::from_hex(trimmed).map(CellValue::Color),
            CellKind::Int => trimmed.parse::<i64>().ok().map(CellValue::Int),
            CellKind::Float => trimmed.parse::<f64>().ok().map(CellValue::Float),
            CellKind::Text => Some(CellValue::Text(trimmed.to_owned())),
        }
    }

    /// Validate a programmatic `intervene` payload against this kind — the
    /// AI-first typed-set path for the **scalar** kinds. Strict per kind (no
    /// silent coercion) so an RPC writes exactly the type the cell holds. A
    /// `Choice` cannot be rebuilt from a kind alone (the options live on the
    /// value), so it falls through to `TypeMismatch` here — write a `Choice`
    /// through [`CellValue::with_intervene`] instead.
    ///
    /// # Errors
    ///
    /// [`InterveneError::TypeMismatch`] when `value`'s variant does not
    /// match this kind (and for every `Choice` payload).
    pub fn coerce(self, value: IntrospectValue) -> Result<CellValue, InterveneError> {
        match (self, value) {
            (CellKind::Bool, IntrospectValue::Bool(b)) => Ok(CellValue::Bool(b)),
            (CellKind::Int, IntrospectValue::Int(i)) => Ok(CellValue::Int(i)),
            (CellKind::Float, IntrospectValue::Float(f)) => Ok(CellValue::Float(f)),
            (CellKind::Text, IntrospectValue::Text(s)) => Ok(CellValue::Text(s)),
            _ => Err(InterveneError::TypeMismatch),
        }
    }
}

/// f64 → canonical text (`12.5`, `-4`, `1`). The cell display + the
/// inline-editor seed; the parse round-trips it.
fn format_float(value: f64) -> String {
    format!("{value}")
}

/// The label of the selected option, or `""` if the index is stale — the
/// `Choice` SSOT shared by `display`, `edit_text`, and `to_introspect`.
fn selected_label(selected: usize, options: &[String]) -> String {
    options.get(selected).cloned().unwrap_or_default()
}

/// R1544 §5.27 — a cell's `Qt::EditRole` answer: what an editor opened on
/// that cell is seeded with, and which editor to open.
///
/// # Why it is one type and not two accessors
///
/// Qt splits the question in half. `data(index, Qt::EditRole)` answers *what
/// value*, `flags(index) & Qt::ItemIsEditable` answers *whether at all*, and
/// `QItemEditorFactory` maps the value's `QVariant::Type` to *which widget*.
/// Three places, and a model that sets the flag but forgets the role opens an
/// empty editor over a populated cell — a defect Qt cannot make
/// unrepresentable because `QVariant()` is a legal answer.
///
/// Here the model answers `Option<CellEdit>` once: `None` **is** "not
/// editable", and a `Some` carries both the seed and the [`CellKind`] that
/// selects the editor. Producing one requires having a value, so "opened an
/// editor on a cell the model cannot edit" is a state the type system rejects
/// rather than one the view must remember to check.
///
/// The seed's text form is deliberately the [`CellValue::edit_text`] one,
/// **not** the display form: `1234.5` is edited as `1234.5` and displayed as
/// whatever the model's display role formats it to. That is exactly Qt's
/// Edit/Display distinction, and it is the reason a currency or unit column can
/// be edited at all.
///
/// # Why it holds the datum and not `(kind, String)` (R1555)
///
/// It held a [`CellKind`] and a seed string until R1555, and that shape could
/// not describe the one kind whose editor most needs its datum: a
/// [`CellValue::Choice`]'s **options are part of the value's identity, not
/// derivable from the kind** (this module has said so since R867), so a
/// `(Choice, "Bravo")` edit role could not tell a selector what to select
/// between — or even which index `"Bravo"` is, since two options may share a
/// label. It also made `(CellKind::Int, "not a number")` a representable
/// edit-role answer that nothing rejected.
///
/// One field fixes both, and it is Qt's own shape: `Qt::EditRole` answers with
/// a `QVariant`, not with a type tag beside a string. [`kind`](Self::kind) and
/// [`text`](Self::text) are now derivations of the datum, so they cannot
/// disagree with it.
#[derive(Clone, Debug, PartialEq)]
pub struct CellEdit {
    /// The datum. Private so every construction goes through
    /// [`From<CellValue>`](CellEdit::from) and the kind / seed pair stays a
    /// derivation rather than two things a caller states separately.
    value: CellValue,
}

impl CellEdit {
    /// The `Qt::EditRole` datum itself — what a [`EditorForm::Selector`] or
    /// [`EditorForm::Toggle`] editor needs, and what a commit compares against.
    #[must_use]
    pub fn value(&self) -> &CellValue {
        &self.value
    }

    /// Which editor to open, and — through
    /// [`CellKind::accepts_keystroke`] / [`CellKind::parse`] — the keystroke
    /// gate and the commit parser. Qt reaches the same decision through
    /// `QItemEditorFactory::createEditor(QMetaType::Type, parent)`; here it is
    /// [`CellKind::editor_form`].
    #[must_use]
    pub fn kind(&self) -> CellKind {
        self.value.kind()
    }

    /// R1555 — which editor form this cell opens, from its own datum.
    #[must_use]
    pub fn form(&self) -> EditorForm {
        self.value.kind().editor_form()
    }

    /// The `Qt::EditRole` value in text form — what a text-buffered editor
    /// opens containing ([`CellValue::edit_text`]).
    #[must_use]
    pub fn text(&self) -> String {
        self.value.edit_text()
    }
}

impl From<CellValue> for CellEdit {
    /// The edit-role answer a [`CellValue`]-backed model gives: the datum.
    ///
    /// Every editable grid in this tree holds `CellValue`s, so a model states
    /// its edit role by handing over the value — and cannot seed an editor from
    /// a formula that has drifted from the one its commit parses back through
    /// ([`CellKind::parse`] is documented as [`CellValue::edit_text`]'s
    /// inverse).
    fn from(value: CellValue) -> Self {
        Self { value }
    }
}

impl From<&CellValue> for CellEdit {
    /// [`From<CellValue>`](CellEdit::from) for a borrowed datum — the shape a
    /// model that computes its cells reaches for.
    fn from(value: &CellValue) -> Self {
        Self {
            value: value.clone(),
        }
    }
}

/// Whether `key` is a single codepoint satisfying `pred`. Multi-codepoint
/// (named / IME) strings are not keystrokes.
fn single_char(key: &str, pred: impl Fn(char) -> bool) -> bool {
    let mut chars = key.chars();
    let Some(c) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        return false;
    }
    pred(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::assert_out_of_range_saying;

    #[test]
    fn kind_classifies_every_variant() {
        assert_eq!(CellValue::Bool(true).kind(), CellKind::Bool);
        assert_eq!(CellValue::Int(1).kind(), CellKind::Int);
        assert_eq!(CellValue::Float(1.0).kind(), CellKind::Float);
        assert_eq!(CellValue::Text(String::new()).kind(), CellKind::Text);
    }

    #[test]
    fn name_round_trips_the_wire_vocab() {
        assert_eq!(CellKind::Bool.name(), "bool");
        assert_eq!(CellKind::Int.name(), "int");
        assert_eq!(CellKind::Float.name(), "float");
        assert_eq!(CellKind::Text.name(), "text");
    }

    #[test]
    fn display_and_edit_text_render_each_kind() {
        assert_eq!(CellValue::Bool(true).display(), "On");
        assert_eq!(CellValue::Bool(false).display(), "Off");
        assert_eq!(CellValue::Bool(true).edit_text(), "true");
        assert_eq!(CellValue::Int(7).display(), "7");
        assert_eq!(CellValue::Float(12.5).display(), "12.5");
        assert_eq!(CellValue::Text("hi".to_owned()).display(), "hi");
    }

    #[test]
    fn parse_handles_int_float_text_and_rejects_garbage() {
        assert_eq!(CellKind::Int.parse(" 42 "), Some(CellValue::Int(42)));
        assert_eq!(CellKind::Int.parse("x"), None);
        assert_eq!(CellKind::Float.parse("-3.5"), Some(CellValue::Float(-3.5)));
        assert_eq!(CellKind::Float.parse("."), None);
        assert_eq!(
            CellKind::Text.parse("  hi "),
            Some(CellValue::Text("hi".to_owned()))
        );
        assert_eq!(CellKind::Bool.parse("true"), None, "bool never text-parsed");
    }

    #[test]
    fn parse_edit_text_round_trips() {
        for value in [
            CellValue::Int(-7),
            CellValue::Float(3.25),
            CellValue::Text("hello".to_owned()),
        ] {
            let kind = value.kind();
            assert_eq!(kind.parse(&value.edit_text()), Some(value));
        }
    }

    #[test]
    fn accepts_keystroke_gates_by_kind() {
        assert!(CellKind::Int.accepts_keystroke("3"));
        assert!(CellKind::Int.accepts_keystroke("-"));
        assert!(
            !CellKind::Int.accepts_keystroke("."),
            "int rejects the decimal point"
        );
        assert!(!CellKind::Int.accepts_keystroke("a"));
        assert!(CellKind::Float.accepts_keystroke("."));
        assert!(CellKind::Text.accepts_keystroke("a"));
        assert!(
            !CellKind::Text.accepts_keystroke("ab"),
            "multi-char is not a keystroke"
        );
        assert!(
            !CellKind::Bool.accepts_keystroke("1"),
            "bool accepts no keystroke"
        );
        assert!(
            !CellKind::Int.accepts_keystroke("Enter"),
            "named key rejected"
        );
    }

    #[test]
    fn coerce_is_strict_per_kind() {
        assert_eq!(
            CellKind::Int.coerce(IntrospectValue::Int(5)),
            Ok(CellValue::Int(5))
        );
        assert_eq!(
            CellKind::Int.coerce(IntrospectValue::Text("no".to_owned())),
            Err(InterveneError::TypeMismatch),
        );
        assert_eq!(
            CellKind::Bool.coerce(IntrospectValue::Bool(true)),
            Ok(CellValue::Bool(true))
        );
        assert_eq!(
            CellKind::Float.coerce(IntrospectValue::Int(3)),
            Err(InterveneError::TypeMismatch),
            "no silent int -> float coercion",
        );
    }

    #[test]
    fn to_introspect_maps_each_variant() {
        assert_eq!(
            CellValue::Bool(true).to_introspect(),
            IntrospectValue::Bool(true)
        );
        assert_eq!(CellValue::Int(9).to_introspect(), IntrospectValue::Int(9));
        assert_eq!(
            CellValue::Float(1.5).to_introspect(),
            IntrospectValue::Float(1.5)
        );
        assert_eq!(
            CellValue::Text("x".to_owned()).to_introspect(),
            IntrospectValue::Text("x".to_owned()),
        );
    }

    /// A two-option `Choice` fixture — `Normal` / `Additive`, currently
    /// `Additive`.
    fn choice_fixture() -> CellValue {
        CellValue::Choice {
            selected: 1,
            options: vec!["Normal".to_owned(), "Additive".to_owned()],
        }
    }

    #[test]
    fn choice_kind_name_and_text_gates() {
        assert_eq!(choice_fixture().kind(), CellKind::Choice);
        assert_eq!(CellKind::Choice.name(), "choice");
        assert!(
            !CellKind::Choice.is_text_editable(),
            "choice is popup-edited, not text"
        );
        assert!(
            !CellKind::Choice.accepts_keystroke("a"),
            "choice takes no keystroke"
        );
        assert_eq!(
            CellKind::Choice.parse("Normal"),
            None,
            "choice is never text-parsed"
        );
    }

    #[test]
    fn choice_display_and_edit_text_are_the_selected_label() {
        assert_eq!(choice_fixture().display(), "Additive");
        assert_eq!(choice_fixture().edit_text(), "Additive");
        // A stale index degrades to the empty label, never panics.
        let stale = CellValue::Choice {
            selected: 9,
            options: vec!["X".to_owned()],
        };
        assert_eq!(stale.display(), "");
    }

    #[test]
    fn choice_to_introspect_is_a_structured_object() {
        let IntrospectValue::Json(json) = choice_fixture().to_introspect() else {
            panic!("choice introspects as json");
        };
        assert_eq!(json["selected"], serde_json::json!(1));
        assert_eq!(json["label"], serde_json::json!("Additive"));
        assert_eq!(json["options"], serde_json::json!(["Normal", "Additive"]));
    }

    #[test]
    fn choice_with_intervene_selects_by_index_and_preserves_options() {
        let v = CellValue::Choice {
            selected: 0,
            options: vec!["A".to_owned(), "B".to_owned(), "C".to_owned()],
        };
        assert_eq!(
            v.with_intervene(IntrospectValue::Int(2)),
            Ok(CellValue::Choice {
                selected: 2,
                options: vec!["A".to_owned(), "B".to_owned(), "C".to_owned()],
            }),
        );
        // Out of range and negative both reject without mutating.
        // R1565 — the two used to be one value; an index past the end names
        // the options, a negative one cannot be an index at all.
        assert_out_of_range_saying(
            &v.with_intervene(IntrospectValue::Int(3)),
            "no option 3 on this cell",
        );
        assert_out_of_range_saying(
            &v.with_intervene(IntrospectValue::Int(-1)),
            "-1 is not an option index",
        );
        // Wrong payload variant is a type mismatch (choice sets by index).
        assert_eq!(
            v.with_intervene(IntrospectValue::Text("B".to_owned())),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn with_intervene_delegates_to_coerce_for_scalars() {
        // The scalar path is byte-identical to kind().coerce.
        assert_eq!(
            CellValue::Int(0).with_intervene(IntrospectValue::Int(5)),
            Ok(CellValue::Int(5))
        );
        assert_eq!(
            CellValue::Text(String::new()).with_intervene(IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn choice_coerce_via_kind_is_a_type_mismatch() {
        // The kind-level path can't build a Choice (no options) —
        // with_intervene is the Choice write path.
        assert_eq!(
            CellKind::Choice.coerce(IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn color_kind_name_and_gates() {
        let v = CellValue::Color(Color::rgb(255, 128, 0));
        assert_eq!(v.kind(), CellKind::Color);
        assert_eq!(CellKind::Color.name(), "color");
        assert!(
            !CellKind::Color.is_text_editable(),
            "colour is popup-edited, not inline text"
        );
        // The popup hex field accepts hex digits + the leading '#', nothing else.
        assert!(
            CellKind::Color.accepts_keystroke("a"),
            "hex field accepts hex digits"
        );
        assert!(
            CellKind::Color.accepts_keystroke("#"),
            "and the leading hash"
        );
        assert!(
            !CellKind::Color.accepts_keystroke("g"),
            "but not a non-hex letter"
        );
    }

    #[test]
    fn color_display_and_edit_text_are_the_hex() {
        let v = CellValue::Color(Color::rgb(255, 128, 0));
        assert_eq!(v.display(), "#ff8000");
        assert_eq!(v.edit_text(), "#ff8000");
        // A non-opaque colour keeps its alpha byte.
        assert_eq!(
            CellValue::Color(Color::rgba(255, 0, 0, 128)).display(),
            "#ff000080"
        );
    }

    #[test]
    fn color_parse_round_trips_the_hex() {
        // The popup's hex field commits through CellKind::parse.
        assert_eq!(
            CellKind::Color.parse(" #00ff00 "),
            Some(CellValue::Color(Color::rgb(0, 255, 0))),
        );
        assert_eq!(
            CellKind::Color.parse("not-a-colour"),
            None,
            "malformed hex rejected"
        );
    }

    #[test]
    fn color_to_introspect_is_a_structured_object() {
        let IntrospectValue::Json(json) = CellValue::Color(Color::rgb(255, 128, 0)).to_introspect()
        else {
            panic!("colour introspects as json");
        };
        assert_eq!(json["hex"], serde_json::json!("#ff8000"));
        assert_eq!(json["r"], serde_json::json!(255));
        assert_eq!(json["g"], serde_json::json!(128));
        assert_eq!(json["b"], serde_json::json!(0));
        assert_eq!(json["a"], serde_json::json!(255));
    }

    #[test]
    fn color_with_intervene_takes_a_hex_string() {
        let v = CellValue::Color(Color::rgb(0, 0, 0));
        assert_eq!(
            v.with_intervene(IntrospectValue::Text("#ff8000".to_owned())),
            Ok(CellValue::Color(Color::rgb(255, 128, 0))),
        );
        // Malformed hex is out of range; a non-Text payload is a type mismatch.
        assert_out_of_range_saying(
            &v.with_intervene(IntrospectValue::Text("xyz".to_owned())),
            r#""xyz" is not a colour"#,
        );
        assert_eq!(
            v.with_intervene(IntrospectValue::Int(5)),
            Err(InterveneError::TypeMismatch)
        );
    }

    #[test]
    fn r1253_color_intervene_round_trips_its_own_to_introspect() {
        // The R1253 wire-symmetry fix: `query value.<i>` -> `intervene value.<i>`
        // now round-trips. Before, `to_introspect` emitted JSON `{hex,r,g,b,a}`
        // that `with_intervene` rejected (`TypeMismatch`) — the §2 primary path.
        let v = CellValue::Color(Color::rgba(79, 157, 255, 200));
        assert_eq!(
            v.with_intervene(v.to_introspect()),
            Ok(v.clone()),
            "a read value (JSON) writes straight back, alpha preserved via the hex",
        );
    }

    #[test]
    fn r1253_color_intervene_accepts_json_or_trimmed_hex() {
        let v = CellValue::Color(Color::rgb(0, 0, 0));
        // The JSON `{hex}` shape `to_introspect` emits.
        assert_eq!(
            v.with_intervene(IntrospectValue::Json(
                serde_json::json!({ "hex": "#00ff00" })
            )),
            Ok(CellValue::Color(Color::rgb(0, 255, 0))),
        );
        // Bare hex `Text` with whitespace is now tolerated (matches
        // `CellKind::parse`), closing the whitespace drift between `intervene`
        // and the type-in commit.
        assert_eq!(
            v.with_intervene(IntrospectValue::Text("  #00ff00 ".to_owned())),
            Ok(CellValue::Color(Color::rgb(0, 255, 0))),
        );
        // A JSON object without a `hex` field is a type mismatch.
        assert_eq!(
            v.with_intervene(IntrospectValue::Json(serde_json::json!({ "r": 1 }))),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn r1253_choice_intervene_round_trips_and_still_takes_a_bare_int() {
        let v = CellValue::Choice {
            selected: 1,
            options: vec!["A".into(), "B".into(), "C".into()],
        };
        // Round-trips its own `to_introspect` (`{selected,label,options}`).
        assert_eq!(v.with_intervene(v.to_introspect()), Ok(v.clone()));
        // The ergonomic bare `Int` index still works.
        assert_eq!(
            v.with_intervene(IntrospectValue::Int(2)),
            Ok(CellValue::Choice {
                selected: 2,
                options: vec!["A".into(), "B".into(), "C".into()],
            }),
        );
        // An out-of-range index via EITHER form is `OutOfRange`.
        assert_out_of_range_saying(
            &v.with_intervene(IntrospectValue::Int(9)),
            "no option 9 on this cell",
        );
        assert_out_of_range_saying(
            &v.with_intervene(IntrospectValue::Json(serde_json::json!({ "selected": 9 }))),
            "no option 9 on this cell",
        );
    }

    #[test]
    fn r886_1_sort_cmp_compares_by_type_not_display() {
        use core::cmp::Ordering;
        // Bool: semantic false < true (display "Off" < "On" only by
        // label accident — the typed compare must not depend on it).
        assert_eq!(
            CellValue::Bool(false).sort_cmp(&CellValue::Bool(true)),
            Ordering::Less,
        );
        // Int: exact past 2^53 (an f64 round-trip would collapse these).
        let big = 9_007_199_254_740_993_i64; // 2^53 + 1
        assert_eq!(
            CellValue::Int(big).sort_cmp(&CellValue::Int(big - 1)),
            Ordering::Greater,
        );
        // Float: total_cmp (NaN cannot break totality).
        assert_eq!(
            CellValue::Float(f64::NAN).sort_cmp(&CellValue::Float(1.0)),
            Ordering::Greater,
        );
        // Text: the numeric-aware cell_cmp SSOT ("9" < "12").
        assert_eq!(
            CellValue::Text("9".into()).sort_cmp(&CellValue::Text("12".into())),
            Ordering::Less,
        );
    }

    #[test]
    fn r920_value_eq_is_nan_safe_unlike_derived_partial_eq() {
        // The whole point: `NaN == NaN` is `false` under the derived `PartialEq`,
        // but `value_eq` (built on `sort_cmp`'s `total_cmp`) reports equal — so a
        // no-op guard / modified check never spuriously fires on a `NaN` default.
        let nan = CellValue::Float(f64::NAN);
        assert!(
            nan != nan.clone(),
            "derived PartialEq: NaN != NaN (the trap)"
        );
        assert!(
            nan.value_eq(&nan.clone()),
            "value_eq: NaN equals NaN (NaN-safe)"
        );
        // Ordinary equality / inequality still hold for every kind.
        assert!(CellValue::Int(7).value_eq(&CellValue::Int(7)));
        assert!(!CellValue::Int(7).value_eq(&CellValue::Int(8)));
        assert!(CellValue::Float(2.5).value_eq(&CellValue::Float(2.5)));
        assert!(!CellValue::Bool(true).value_eq(&CellValue::Bool(false)));
        assert!(CellValue::Text("x".into()).value_eq(&CellValue::Text("x".into())));
    }

    #[test]
    fn r891_matches_filter_compares_by_type_not_display() {
        // Bool filters by the canonical "true"/"false" (the AI / edit_text
        // vocab), NOT the "On"/"Off" display label.
        assert!(CellValue::Bool(true).matches_filter("true"));
        assert!(CellValue::Bool(false).matches_filter("false"));
        assert!(
            !CellValue::Bool(true).matches_filter("On"),
            "not the display label"
        );
        assert!(!CellValue::Bool(true).matches_filter("false"));
        // Int compares the parsed number: whitespace + leading zeros fold.
        assert!(CellValue::Int(24).matches_filter("24"));
        assert!(
            CellValue::Int(24).matches_filter(" 024 "),
            "parsed, not literal text"
        );
        assert!(!CellValue::Int(24).matches_filter("25"));
        assert!(
            !CellValue::Int(24).matches_filter("abc"),
            "unparseable matches nothing"
        );
        // Float compares totally; trailing-zero forms fold.
        assert!(CellValue::Float(2.5).matches_filter("2.50"));
        assert!(!CellValue::Float(2.5).matches_filter("2.6"));
        // Text matches exactly (the cross-grid GridFilter contract).
        assert!(CellValue::Text("mesh".into()).matches_filter("mesh"));
        assert!(
            !CellValue::Text("mesh".into()).matches_filter("Mesh"),
            "exact, case-sensitive"
        );
        assert!(
            !CellValue::Text("mesh".into()).matches_filter("me"),
            "not a substring match"
        );
        // Choice matches the selected label; Color matches a parsed hex.
        let choice = CellValue::Choice {
            selected: 1,
            options: vec!["a".into(), "b".into()],
        };
        assert!(choice.matches_filter("b"));
        assert!(!choice.matches_filter("a"));
        let color = CellValue::Color(Color::rgb(255, 128, 0));
        assert!(color.matches_filter("#FF8000"), "hex parse folds case");
        assert!(!color.matches_filter("#000000"));
    }

    #[test]
    fn r997_matches_facet_honors_op_typed() {
        // Eq / Ne reuse the type-aware equality (so "024" still matches Int 24).
        assert!(CellValue::Int(24).matches_facet(FilterOp::Eq, " 024 "));
        assert!(CellValue::Int(24).matches_facet(FilterOp::Ne, "25"));
        assert!(!CellValue::Int(24).matches_facet(FilterOp::Ne, "24"));
        // Ordered ops compare through the TYPED sort_cmp, not lexicographic
        // text (a Float 9 is < 12, where "9" > "12" by string order).
        assert!(CellValue::Float(9.0).matches_facet(FilterOp::Lt, "12"));
        assert!(!CellValue::Float(12.0).matches_facet(FilterOp::Lt, "9"));
        assert!(CellValue::Int(20).matches_facet(FilterOp::Ge, "20"));
        assert!(CellValue::Int(30).matches_facet(FilterOp::Ge, "20"));
        assert!(!CellValue::Int(9).matches_facet(FilterOp::Ge, "20"));
        assert!(CellValue::Int(5).matches_facet(FilterOp::Le, "5"));
        assert!(CellValue::Int(30).matches_facet(FilterOp::Gt, "20"));
        // An ordered op parses the operand into the cell's kind; a Bool is
        // never text-parsed (it toggles), so `<` on a Bool matches nothing —
        // bools filter by Eq ("flag = true"), not by `<`.
        assert!(!CellValue::Bool(false).matches_facet(FilterOp::Lt, "true"));
        assert!(CellValue::Bool(false).matches_facet(FilterOp::Eq, "false"));
        // An ordered op with an unparseable numeric operand matches nothing.
        assert!(!CellValue::Int(24).matches_facet(FilterOp::Gt, "abc"));
        // Contains is a substring over the display label (the one textual op).
        assert!(CellValue::Text("mesh_lod0".into()).matches_facet(FilterOp::Contains, "lod"));
        assert!(!CellValue::Text("mesh".into()).matches_facet(FilterOp::Contains, "xyz"));
    }

    // ─────────────────────────────────────────────────────────────
    // R1555 §5.27 — the editor factory: Qt QItemEditorFactory
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r1555_every_kind_has_an_editor_and_the_census_is_complete() {
        // The census drives the loop, so a seventh kind cannot be added
        // without a form (the `match` is exhaustive) AND without a row here.
        assert_eq!(
            CellKind::ALL.len(),
            6,
            "the census enumerates every kind the factory answers for"
        );
        for kind in CellKind::ALL {
            let form = kind.editor_form();
            assert!(
                EditorForm::ALL.contains(&form),
                "{kind:?} answers with a censused form, not an ad-hoc one"
            );
            assert_eq!(
                EditorForm::from_wire(form.name()),
                Some(form),
                "{form:?} round-trips its wire token"
            );
        }
        // Distinct kinds may share a form (Int / Float are both steppers, as
        // in Qt), but every form must be reachable — an unreachable form would
        // be a paint path nothing can open.
        for form in EditorForm::ALL {
            assert!(
                CellKind::ALL.iter().any(|k| k.editor_form() == form),
                "{form:?} is reachable from some kind"
            );
        }
    }

    #[test]
    fn r1555_the_two_kinds_that_refuse_text_do_not_get_a_text_editor() {
        // The defect this round closes: `text_cell_editor` was the built-in
        // for every kind, and for these two it is an editor that accepts no
        // keystroke and whose commit can never produce a value.
        for kind in [CellKind::Bool, CellKind::Choice] {
            assert!(!kind.accepts_keystroke("a"), "{kind:?} refuses keystrokes");
            assert!(kind.parse("anything").is_none(), "{kind:?} never parses");
            assert!(
                !kind.editor_form().buffer_is_text(),
                "{kind:?} therefore cannot have a text-buffered editor"
            );
        }
    }

    #[test]
    fn r1555_is_text_editable_is_derived_from_the_form() {
        // One classification, one place. The pre-R1555 duplicate said Bool and
        // Choice were not text-edited while the seam opened a text field for
        // them anyway.
        for kind in CellKind::ALL {
            assert_eq!(
                kind.is_text_editable(),
                kind.editor_form().inline_text(),
                "{kind:?}"
            );
        }
        assert!(
            !CellKind::Color.is_text_editable(),
            "a swatch's hex field is a part of the editor, not the cell's box"
        );
        assert!(
            CellKind::Color.editor_form().buffer_is_text(),
            "but its in-flight value IS read back out of that field"
        );
    }

    #[test]
    fn r1555_a_stepper_steps_its_buffer_and_refuses_a_malformed_one() {
        assert_eq!(CellKind::Int.step_text("41", 1).as_deref(), Some("42"));
        assert_eq!(CellKind::Int.step_text("0", -1).as_deref(), Some("-1"));
        assert_eq!(CellKind::Float.step_text("1.5", 1).as_deref(), Some("2.5"));
        // Past Qt: a `QSpinBox` always holds a validated value, so `stepBy` on
        // a half-typed field steps from whatever its validator last accepted.
        // Here the user's text is left alone.
        assert_eq!(CellKind::Int.step_text("12a", 1), None);
        assert_eq!(CellKind::Int.step_text("", 1), None);
        // Saturating, so a held arrow at the type's bound cannot wrap.
        assert_eq!(
            CellKind::Int.step_text(&i64::MAX.to_string(), 1).as_deref(),
            Some(i64::MAX.to_string().as_str())
        );
        for kind in [CellKind::Bool, CellKind::Text, CellKind::Choice] {
            assert_eq!(kind.step_text("1", 1), None, "{kind:?} has no stepper");
        }
    }

    #[test]
    fn r1555_a_float_survives_an_untouched_open_and_commit() {
        // Qt's default factory hands a `QDoubleSpinBox` left at
        // `decimals() == 2`, so opening the default editor on this cell and
        // pressing Enter writes 1.23. The seed and the commit here are
        // documented inverses.
        let value = CellValue::Float(1.234);
        let edit = CellEdit::from(&value);
        assert_eq!(edit.form(), EditorForm::Stepper);
        assert_eq!(CellKind::Float.parse(&edit.text()), Some(value));
    }

    #[test]
    fn r1555_the_edit_role_carries_the_choice_domain() {
        // The reason `CellEdit` holds the datum: a `(Choice, "Bravo")` pair
        // cannot tell a selector what to select between, nor which index
        // "Bravo" is when two options share a label.
        let value = CellValue::Choice {
            selected: 1,
            options: vec!["Alpha".into(), "Bravo".into(), "Bravo".into()],
        };
        let edit = CellEdit::from(&value);
        assert_eq!(edit.form(), EditorForm::Selector);
        assert_eq!(edit.kind(), CellKind::Choice);
        assert_eq!(edit.text(), "Bravo", "the seed is still the label");
        let CellValue::Choice { selected, options } = edit.value() else {
            panic!("the datum is carried whole");
        };
        assert_eq!(*selected, 1, "and it says WHICH Bravo");
        assert_eq!(options.len(), 3);
    }

    #[test]
    fn r1555_an_edit_role_cannot_state_a_kind_its_datum_contradicts() {
        // `CellEdit::new(CellKind::Int, "not a number")` used to be a
        // representable edit-role answer. Construction now requires a datum,
        // so the kind is a derivation of it.
        let edit = CellEdit::from(CellValue::Int(7));
        assert_eq!(edit.kind(), CellKind::Int);
        assert_eq!(edit.text(), "7");
        assert_eq!(CellKind::Int.parse(&edit.text()), Some(CellValue::Int(7)));
    }
}
