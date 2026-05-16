//! Parsed representation of a `.pinion.xml` document. Per R38 §5.22 the
//! root is `<pinion xmlns="..." kind="..." name="...">` and the child set
//! is closed (`<use>` / `<signal>` / `<computed>` / `<resource>` under
//! `kind="reactive"`).
//!
//! R38.2a adds `<signal>` to the child set. `<computed>` / `<resource>` /
//! `<use>` land in subsequent slices.

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
    /// Closed child set. R38.2a populates [`PinionChild::Signal`];
    /// `<computed>` / `<resource>` / `<use>` land in subsequent slices.
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

/// Closed child-element enum. R38.2a populates the `Signal` variant;
/// `<computed>` / `<resource>` / `<use>` land in subsequent slices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinionChild {
    /// `<signal name="..." ty="...">CDATA initial</signal>`. Compiles to
    /// a `pub <name>: ::pinion_core::reactive::Signal<<ty>>` struct field
    /// plus `Signal::new(<initial>)` inside the generated `new`.
    Signal(SignalDecl),
}

/// Parsed `<signal>` child. The body (CDATA or plain text) is the
/// initial-value expression that pinion-forge passes directly into
/// `Signal::new(...)` at codegen time. Surface validation is intentionally
/// shallow (non-empty name = valid Rust ident; non-empty ty; non-empty
/// initial) — `rustc` is the source of truth for type/expression
/// soundness, and surfacing a forwarded syntax error there is acceptable
/// at R38.2a. A deeper `syn`-based validation lands in a later slice if
/// the rustc message proves too distant from the `.pinion.xml` source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalDecl {
    /// Rust identifier used as both the struct field name and the local
    /// binding inside the generated `new` body.
    pub name: String,
    /// Rust type expression substituted into `Signal<...>`. Stored as the
    /// raw author string; never inspected beyond non-emptiness.
    pub ty: String,
    /// Initial-value expression. Trimmed of leading/trailing whitespace.
    pub initial: String,
}
