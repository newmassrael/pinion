//! Parsed representation of a `.pinion.xml` document. Per R38 §5.22 the
//! root is `<pinion xmlns="..." kind="..." name="...">` and the child set
//! is closed (`<use>` / `<signal>` / `<computed>` / `<resource>` under
//! `kind="reactive"`).
//!
//! R38.2a/b/c/d add `<signal>` / `<computed>` / `<resource>` / `<use>`
//! to the child set. R38.2a/b/c contribute a struct field and a `new`-
//! body binding each; `<use>` (R38.2d) is purely a module-level import
//! and contributes neither.

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
    /// Closed child set in declaration order. Codegen preserves the
    /// order so `<computed>` bodies authored after their `<signal>`
    /// dependencies see those identifiers in scope at emit time.
    /// `<resource>` / `<use>` land in subsequent slices.
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

/// Closed child-element enum. R38.2a/b/c/d populate all four variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinionChild {
    /// `<signal name="..." ty="...">CDATA initial</signal>`. Compiles to
    /// a `pub <name>: ::pinion_core::reactive::Signal<<ty>>` struct field
    /// plus `Signal::new(<initial>)` inside the generated `new`.
    Signal(SignalDecl),
    /// `<computed name="..." ty="...">CDATA closure body</computed>`.
    /// Compiles to a `pub <name>: ::pinion_core::reactive::Computed<<ty>>`
    /// field plus `Computed::new(move || { <body> })` with all prior
    /// child names over-cloned into the closure scope. Runtime
    /// dependency tracking (R26 push-pull) discovers the real
    /// dependency set from `<body>` at first `get()` — the codegen-side
    /// over-capture is only what Rust's borrow checker needs to make
    /// the closure type-check.
    Computed(ComputedDecl),
    /// `<resource name="..." ty="..." err="...">CDATA future body</resource>`.
    /// Compiles to a `pub <name>: ::pinion_core::reactive::Resource<<ty>, <err>>`
    /// field plus `Resource::loading()` initialization and an immediate
    /// `fetch_with(spawner, <body>)` call. The presence of any
    /// `<resource>` widens the generated `new` signature to take a
    /// `&S: LocalSpawner` second argument; documents without resources
    /// keep the simpler one-argument `new`.
    Resource(ResourceDecl),
    /// `<use path="..."/>`. Module-level `use` statement emitted above
    /// the generated struct. Carries no field, no `new`-body binding,
    /// and does not contribute to `prior_names` for over-capture — the
    /// imported names are available to every closure body via Rust's
    /// module-level scope automatically.
    Use(UseDecl),
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

/// Parsed `<computed>` child. Shares the `(name, ty, body)` shape with
/// [`SignalDecl`] at parse time — the divergence is what codegen does
/// with `body`: signals pass it straight into `Signal::new()` as an
/// initial value, computed wraps it in `move || { ... }` and feeds the
/// closure to `Computed::new()`. Prior child identifiers in the same
/// `<pinion>` document are made available inside the closure via
/// over-cloning (see [`PinionChild::Computed`] docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputedDecl {
    /// Rust identifier — struct field name and local binding name.
    pub name: String,
    /// Rust type expression substituted into `Computed<...>`. Raw author
    /// string, non-emptiness-checked only.
    pub ty: String,
    /// Closure body. Trimmed; emitted inside `move || { <body> }` so
    /// the author can write either a single expression (`a.get() + 1`)
    /// or statements followed by an expression (`let x = a.get(); x * 2`).
    pub body: String,
}

/// Parsed `<use>` child. The `path` is emitted verbatim into a
/// module-level `use <path>;` statement at the top of the generated
/// Rust file. Validation at R38.2d is intentionally shallow
/// (non-empty); `rustc` rejects malformed use paths and `syn::UseTree`-
/// based deep validation is a [carry-forward R38.2x] decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseDecl {
    /// Rust use-path. Accepts any form `rustc` accepts: simple paths
    /// (`my_crate::Foo`), groups (`my_crate::{a, b}`), renames
    /// (`my_crate::Foo as Bar`), globs (`my_crate::*`). pinion-forge
    /// does not interpret the path beyond non-emptiness.
    pub path: String,
}

/// Parsed `<resource>` child. Carries the success/error type pair and
/// the future expression that drives the initial fetch. The body must
/// evaluate to a value implementing
/// `Future<Output = Result<<ty>, <err>>>` — either an `async move { ... }`
/// block or a plain future-returning expression. pinion-forge does not
/// inspect the body shape (rustc owns the future-trait check); it does
/// over-clone every prior child identifier into the surrounding block
/// so an `async move` body can capture them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDecl {
    /// Rust identifier — struct field name and local binding name.
    pub name: String,
    /// Success type `<T>` in `Resource<T, E>`. Non-empty string,
    /// otherwise unvalidated (rustc owns soundness).
    pub ty: String,
    /// Error type `<E>` in `Resource<T, E>`. Non-empty string.
    pub err: String,
    /// Future expression. Trimmed; emitted verbatim as the second
    /// argument to `Resource::fetch_with(spawner, <body>)`. Must satisfy
    /// `Future<Output = Result<T, E>> + 'static` — checked by rustc.
    pub body: String,
}
