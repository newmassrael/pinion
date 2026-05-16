//! Parsed representation of a `.pinion.xml` document. Per R38 §5.22 the
//! root is `<pinion xmlns="..." kind="..." name="...">` and the child set
//! is closed (`<use>` / `<signal>` / `<computed>` / `<resource>` under
//! `kind="reactive"`).
//!
//! At R38.1 the child set is empty (the variants land in R38.2+) — the
//! AST shape is set now so adding variants is purely additive and not a
//! breaking change for downstream codegen consumers.

/// Top-level `<pinion>` document. One file = one document = one emitted
/// Rust struct (R38 ratify: "one file = one struct").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinionDoc {
    /// Name of the emitted Rust struct. Validated as a Rust identifier by
    /// the parser (no keywords, ASCII `[A-Za-z_][A-Za-z0-9_]*`).
    pub name: String,
    /// DSL category — pins the codegen template chosen by
    /// [`crate::codegen`]. R38.1 supports only [`PinionKind::Reactive`].
    pub kind: PinionKind,
    /// Closed child set. Always empty at R38.1; populated in R38.2+ when
    /// `<signal>` / `<computed>` / `<resource>` / `<use>` land.
    pub children: Vec<PinionChild>,
}

/// DSL category attribute (`<pinion kind="...">`). Closed enum — extending
/// requires a spec round (the codegen template tree is keyed off this).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinionKind {
    /// Fine-grained reactive primitives (Signal / Computed / Resource).
    Reactive,
}

impl PinionKind {
    /// Parse the `kind` attribute literal. Returns `None` for any value
    /// not in the closed set — the caller raises
    /// `PinionForgeDiagnostic::UnknownKind`.
    #[must_use]
    pub fn from_attr(literal: &str) -> Option<Self> {
        match literal {
            "reactive" => Some(Self::Reactive),
            _ => None,
        }
    }

    /// Inverse of [`Self::from_attr`]. Stable wire identity per R38 ratify.
    #[must_use]
    pub fn as_attr(self) -> &'static str {
        match self {
            Self::Reactive => "reactive",
        }
    }
}

/// Closed child-element enum. Uninhabited at R38.1 by design: the parser
/// rejects every `<pinion>` child with
/// `PinionForgeDiagnostic::UnsupportedElement` until R38.2 lands the real
/// variants. The shape exists so adding `Signal { .. }`, `Computed { .. }`
/// etc. in R38.2 is additive and downstream consumers can already
/// `match` on the (empty) set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinionChild {}
