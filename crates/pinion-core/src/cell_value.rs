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

/// A typed editable-cell value. `Signal<T>` requires `Serialize +
/// DeserializeOwned` (the R36 §5.31 hot-reload bound), so the model derives
/// serde — editable-grid bindings hold the value model in a `Signal`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CellValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

/// The static descriptor of a [`CellValue`] — drives editor behaviour
/// (toggle vs text-edit), the keystroke gate, and parse / format. `Copy`
/// (per-column kind arrays live in `[CellKind; N]`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CellKind {
    Bool,
    Int,
    Float,
    Text,
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
        }
    }

    /// The wire value for `scene/query` (AI-first introspection).
    #[must_use]
    pub fn to_introspect(&self) -> IntrospectValue {
        match self {
            CellValue::Bool(b) => IntrospectValue::Bool(*b),
            CellValue::Int(i) => IntrospectValue::Int(*i),
            CellValue::Float(f) => IntrospectValue::Float(*f),
            CellValue::Text(s) => IntrospectValue::Text(s.clone()),
        }
    }

    /// The value shown in a non-edited cell. Bools render as a checkbox
    /// affordance (not this text), but the spoken / AT name reuses the
    /// `On` / `Off` wording so the value reads naturally.
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            CellValue::Bool(b) => if *b { "On" } else { "Off" }.to_owned(),
            CellValue::Int(i) => i.to_string(),
            CellValue::Float(f) => format_float(*f),
            CellValue::Text(s) => s.clone(),
        }
    }

    /// The text the inline editor is seeded with when the cell enters edit
    /// mode (the round-trip inverse of [`CellKind::parse`]).
    #[must_use]
    pub fn edit_text(&self) -> String {
        match self {
            CellValue::Bool(b) => b.to_string(),
            CellValue::Int(i) => i.to_string(),
            CellValue::Float(f) => format_float(*f),
            CellValue::Text(s) => s.clone(),
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
        }
    }

    /// Whether the kind is edited as text (text / int / float) rather than
    /// toggled (bool). The begin-edit guard.
    #[must_use]
    pub fn is_text_editable(self) -> bool {
        !matches!(self, CellKind::Bool)
    }

    /// Whether a single printable keystroke is allowed into this kind's
    /// inline editor — the `<input type=number>` keystroke gate. Text
    /// accepts any single character; int accepts digits / sign; float adds
    /// the decimal point; bool accepts none (bools toggle). Caret / named
    /// keys are handled by the caller before this gate.
    #[must_use]
    pub fn accepts_keystroke(self, key: &str) -> bool {
        match self {
            CellKind::Bool => false,
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
            CellKind::Bool => None,
            CellKind::Int => trimmed.parse::<i64>().ok().map(CellValue::Int),
            CellKind::Float => trimmed.parse::<f64>().ok().map(CellValue::Float),
            CellKind::Text => Some(CellValue::Text(trimmed.to_owned())),
        }
    }

    /// Validate a programmatic `intervene` payload against this kind — the
    /// AI-first typed-set path. Strict per kind (no silent coercion) so an
    /// RPC writes exactly the type the cell holds.
    ///
    /// # Errors
    ///
    /// [`InterveneError::TypeMismatch`] when `value`'s variant does not
    /// match this kind.
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
}
