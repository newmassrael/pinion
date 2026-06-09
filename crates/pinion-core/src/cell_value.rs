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
    Choice { selected: usize, options: Vec<String> },
    /// An sRGB colour (the property-grid colour cell, R869). Edited by a
    /// popup — a preset swatch palette plus a hex field for an arbitrary
    /// value — not inline text or a toggle. The `intervene` write takes a
    /// `#RRGGBB[AA]` hex string ([`CellValue::with_intervene`]); the standalone
    /// 2-D HSV picker stays the dedicated `hello-color-picker` widget (its
    /// model-owning contract does not fit a cell whose model is this `Color`).
    Color(Color),
}

/// The static descriptor of a [`CellValue`] — drives editor behaviour
/// (toggle vs text-edit vs popup), the keystroke gate, and parse / format.
/// `Copy` (per-column kind arrays live in `[CellKind; N]`); the option list a
/// [`CellKind::Choice`] needs lives on the *value*, not the kind, so `Copy`
/// is preserved.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
    /// Scalar kinds delegate to `coerce`; a `Choice` takes an
    /// [`IntrospectValue::Int`] option **index** and preserves its options.
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
                let IntrospectValue::Int(i) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let idx = usize::try_from(i).map_err(|_| InterveneError::OutOfRange)?;
                if idx >= options.len() {
                    return Err(InterveneError::OutOfRange);
                }
                Ok(CellValue::Choice { selected: idx, options: options.clone() })
            }
            CellValue::Color(_) => {
                let IntrospectValue::Text(hex) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                Color::from_hex(&hex).map(CellValue::Color).ok_or(InterveneError::OutOfRange)
            }
            _ => self.kind().coerce(value),
        }
    }
}

impl CellKind {
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

    /// Whether the kind is edited as text (text / int / float) rather than
    /// toggled (bool) or popup-edited (choice / colour). The begin-edit guard
    /// for the inline text field.
    #[must_use]
    pub fn is_text_editable(self) -> bool {
        matches!(self, CellKind::Int | CellKind::Float | CellKind::Text)
    }

    /// Whether a single printable keystroke is allowed into this kind's
    /// inline editor — the `<input type=number>` keystroke gate. Text
    /// accepts any single character; int accepts digits / sign; float adds
    /// the decimal point; bool accepts none (bools toggle). Caret / named
    /// keys are handled by the caller before this gate.
    #[must_use]
    pub fn accepts_keystroke(self, key: &str) -> bool {
        match self {
            CellKind::Bool | CellKind::Choice | CellKind::Color => false,
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
        assert_eq!(CellKind::Text.parse("  hi "), Some(CellValue::Text("hi".to_owned())));
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
        assert!(!CellKind::Int.accepts_keystroke("."), "int rejects the decimal point");
        assert!(!CellKind::Int.accepts_keystroke("a"));
        assert!(CellKind::Float.accepts_keystroke("."));
        assert!(CellKind::Text.accepts_keystroke("a"));
        assert!(!CellKind::Text.accepts_keystroke("ab"), "multi-char is not a keystroke");
        assert!(!CellKind::Bool.accepts_keystroke("1"), "bool accepts no keystroke");
        assert!(!CellKind::Int.accepts_keystroke("Enter"), "named key rejected");
    }

    #[test]
    fn coerce_is_strict_per_kind() {
        assert_eq!(CellKind::Int.coerce(IntrospectValue::Int(5)), Ok(CellValue::Int(5)));
        assert_eq!(
            CellKind::Int.coerce(IntrospectValue::Text("no".to_owned())),
            Err(InterveneError::TypeMismatch),
        );
        assert_eq!(CellKind::Bool.coerce(IntrospectValue::Bool(true)), Ok(CellValue::Bool(true)));
        assert_eq!(
            CellKind::Float.coerce(IntrospectValue::Int(3)),
            Err(InterveneError::TypeMismatch),
            "no silent int -> float coercion",
        );
    }

    #[test]
    fn to_introspect_maps_each_variant() {
        assert_eq!(CellValue::Bool(true).to_introspect(), IntrospectValue::Bool(true));
        assert_eq!(CellValue::Int(9).to_introspect(), IntrospectValue::Int(9));
        assert_eq!(CellValue::Float(1.5).to_introspect(), IntrospectValue::Float(1.5));
        assert_eq!(
            CellValue::Text("x".to_owned()).to_introspect(),
            IntrospectValue::Text("x".to_owned()),
        );
    }

    /// A two-option `Choice` fixture — `Normal` / `Additive`, currently
    /// `Additive`.
    fn choice_fixture() -> CellValue {
        CellValue::Choice { selected: 1, options: vec!["Normal".to_owned(), "Additive".to_owned()] }
    }

    #[test]
    fn choice_kind_name_and_text_gates() {
        assert_eq!(choice_fixture().kind(), CellKind::Choice);
        assert_eq!(CellKind::Choice.name(), "choice");
        assert!(!CellKind::Choice.is_text_editable(), "choice is popup-edited, not text");
        assert!(!CellKind::Choice.accepts_keystroke("a"), "choice takes no keystroke");
        assert_eq!(CellKind::Choice.parse("Normal"), None, "choice is never text-parsed");
    }

    #[test]
    fn choice_display_and_edit_text_are_the_selected_label() {
        assert_eq!(choice_fixture().display(), "Additive");
        assert_eq!(choice_fixture().edit_text(), "Additive");
        // A stale index degrades to the empty label, never panics.
        let stale = CellValue::Choice { selected: 9, options: vec!["X".to_owned()] };
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
        assert_eq!(v.with_intervene(IntrospectValue::Int(3)), Err(InterveneError::OutOfRange));
        assert_eq!(v.with_intervene(IntrospectValue::Int(-1)), Err(InterveneError::OutOfRange));
        // Wrong payload variant is a type mismatch (choice sets by index).
        assert_eq!(
            v.with_intervene(IntrospectValue::Text("B".to_owned())),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn with_intervene_delegates_to_coerce_for_scalars() {
        // The scalar path is byte-identical to kind().coerce.
        assert_eq!(CellValue::Int(0).with_intervene(IntrospectValue::Int(5)), Ok(CellValue::Int(5)));
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
        assert!(!CellKind::Color.is_text_editable(), "colour is popup-edited, not inline text");
        assert!(!CellKind::Color.accepts_keystroke("a"), "colour takes no inline keystroke");
    }

    #[test]
    fn color_display_and_edit_text_are_the_hex() {
        let v = CellValue::Color(Color::rgb(255, 128, 0));
        assert_eq!(v.display(), "#ff8000");
        assert_eq!(v.edit_text(), "#ff8000");
        // A non-opaque colour keeps its alpha byte.
        assert_eq!(CellValue::Color(Color::rgba(255, 0, 0, 128)).display(), "#ff000080");
    }

    #[test]
    fn color_parse_round_trips_the_hex() {
        // The popup's hex field commits through CellKind::parse.
        assert_eq!(
            CellKind::Color.parse(" #00ff00 "),
            Some(CellValue::Color(Color::rgb(0, 255, 0))),
        );
        assert_eq!(CellKind::Color.parse("not-a-colour"), None, "malformed hex rejected");
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
        assert_eq!(
            v.with_intervene(IntrospectValue::Text("xyz".to_owned())),
            Err(InterveneError::OutOfRange),
        );
        assert_eq!(v.with_intervene(IntrospectValue::Int(5)), Err(InterveneError::TypeMismatch));
    }
}
