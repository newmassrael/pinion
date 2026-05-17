//! Parsed representation of a `.pinion.xml` document. Per R38 §5.22 the
//! root is `<pinion xmlns="..." kind="..." name="...">`; the kind-specific
//! payload (child set for `reactive`, attributes for `renderer`, etc.) is
//! captured in [`PinionSpec`] so illegal combinations
//! (e.g. `kind="renderer"` carrying reactive children) are unrepresentable
//! at the type level — make illegal states unrepresentable (Minsky, RWOC
//! ch.6 "Variants"; Effective Rust Item 1).
//!
//! R38.2a/b/c/d added `<signal>` / `<computed>` / `<resource>` / `<use>`
//! under `kind="reactive"`. R46 §5.16 build slice 1 introduces the
//! `kind="renderer"` variant whose payload is a single `backend` attribute
//! (no children). Future kinds (effect / animation / view-fn) attach as
//! additional [`PinionSpec`] variants; pattern-match exhaustiveness on
//! [`PinionSpec`] ensures every codegen dispatch site picks up the new
//! kind at compile time.

/// Top-level `<pinion>` document. One file = one document = one emitted
/// Rust artifact (R38 ratify: "one file = one struct" for the reactive
/// kind; the renderer kind emits a manifest entry consumed by R46
/// commit 2 codegen).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinionDoc {
    /// Name of the emitted Rust identifier. Validated as a Rust ident by
    /// the parser (no keywords, ASCII `[A-Za-z_][A-Za-z0-9_]*`). The same
    /// name is the struct name for `kind="reactive"` and the manifest key
    /// for `kind="renderer"`.
    pub name: String,
    /// Kind-specific payload. The variant is fixed by the `<pinion
    /// kind="...">` attribute and carries every attribute / child set
    /// the kind requires. Pattern-match this directly in codegen; do not
    /// route through a separate [`PinionKind`] dispatch (that enum is
    /// for wire / hash identity only).
    pub spec: PinionSpec,
}

/// Kind-specific document payload. Each variant carries the exact field
/// set the kind needs at codegen time — no `Option<...>` placeholders, no
/// "ignore this field for this kind" rules. Adding a new kind is a single
/// `enum` variant addition; every existing `match` site flags up at
/// compile time as a missing arm (textbook ADT pattern, mirrors
/// `syn::Expr` / `serde_json::Value` / `proc_macro2::TokenTree`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinionSpec {
    /// `<pinion kind="reactive" name="...">` — fine-grained reactive
    /// primitives (Signal / Computed / Resource) plus module-level
    /// `<use>` imports. The child set is closed (R38.2a/b/c/d). Codegen
    /// emits one `pub struct <name>` with one field per binding child.
    Reactive {
        /// Closed child set in declaration order. Codegen preserves the
        /// order so `<computed>` bodies authored after their `<signal>`
        /// dependencies see those identifiers in scope at emit time.
        children: Vec<PinionChild>,
    },
    /// `<pinion kind="renderer" name="..." backend="..."/>` — R46 §5.16
    /// `SceneRenderer` manifest entry. The element is self-closing (no
    /// children); the `backend` attribute selects the codegen template
    /// chosen at build time. R46 commit 2 lands the Vello emit template;
    /// future emit templates (thin-RHI, custom-pass, B3) attach as
    /// additional [`RendererBackend`] variants per R41 phasing.
    Renderer {
        /// Render backend selected at build time. Compile-time per target
        /// — runtime virtual dispatch is 0 (R11 zero-overhead, §2#6
        /// scene-as-data invariant).
        backend: RendererBackend,
    },
}

impl PinionSpec {
    /// Wire / hash identity for the kind. Pattern-match [`PinionSpec`]
    /// directly when dispatching codegen; use this accessor only for
    /// diagnostic messages, wire serialization, or the unknown-kind
    /// error path (which has no `PinionSpec` to match on).
    #[must_use]
    pub fn kind(&self) -> PinionKind {
        match self {
            Self::Reactive { .. } => PinionKind::Reactive,
            Self::Renderer { .. } => PinionKind::Renderer,
        }
    }
}

/// Wire / hash identity for [`PinionSpec`] variants. Closed enum, stable
/// string form via [`Self::as_attr`]. Codegen does not dispatch on this
/// — that role belongs to [`PinionSpec`] pattern matching. This enum
/// exists for: (a) the `dsl/unknown-kind` diagnostic (raised before any
/// `PinionSpec` is constructed) and (b) optional wire surfaces that
/// echo the kind as a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinionKind {
    /// Fine-grained reactive primitives (Signal / Computed / Resource).
    Reactive,
    /// R46 §5.16 `SceneRenderer` manifest entry (renderer kind).
    Renderer,
}

impl PinionKind {
    /// Parse the `kind` attribute literal. Returns `None` for any value
    /// not in the closed set — the caller raises
    /// [`crate::diagnostic::PinionForgeDiagnostic::UnknownKind`].
    #[must_use]
    pub fn from_attr(literal: &str) -> Option<Self> {
        match literal {
            "reactive" => Some(Self::Reactive),
            "renderer" => Some(Self::Renderer),
            _ => None,
        }
    }

    /// Inverse of [`Self::from_attr`]. Stable wire identity per R38
    /// ratify (reactive) and R46 §5.16 (renderer).
    #[must_use]
    pub fn as_attr(self) -> &'static str {
        match self {
            Self::Reactive => "reactive",
            Self::Renderer => "renderer",
        }
    }
}

/// Render backend selector for `kind="renderer"`. Closed sum type — each
/// variant carries the backend-specific options it needs at codegen
/// time. Adding a new backend (R41 Phase 2/3/4: thin-RHI / custom-pass /
/// B3) is a single variant addition; existing `match` sites flag at
/// compile time. R46.1 introduced the `Vello` variant; R46.2.1 added
/// the `aa` payload (build-time AA mode selection per §5.16 R45
/// "compile-time per target", forward-compat with §2#4 mode toggle —
/// UI mode = Area / game mode candidates = Msaa8 / Msaa16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererBackend {
    /// Vello-backed renderer (R41 Phase 1, hybrid path C). Carries the
    /// antialiasing mode selected at build time — Vello compiles only
    /// the shaders for the requested mode (smaller binary, faster
    /// init). Runtime AA toggle (e.g. quality-slider UX) is a separate
    /// axis — would require all three modes compiled in.
    Vello {
        /// Compile-time antialiasing selector. Determines both the
        /// emitted [`vello::AaSupport`] struct (which shaders compile)
        /// and the [`vello::AaConfig`] passed at each
        /// `render_to_texture` call (which shader runs per frame).
        aa: VelloAaMode,
    },
}

/// Wire / hash identity for [`RendererBackend`] variants. Tag-only
/// twin — parallels [`PinionKind`] ↔ [`PinionSpec`]. The parser uses
/// this enum to decode the `backend` attribute literal; the actual
/// [`RendererBackend`] is then assembled with the kind-specific payload
/// (e.g. `aa` for `Vello`). Codegen does not dispatch on this — that
/// role belongs to [`RendererBackend`] pattern matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererBackendKind {
    /// Vello (R41 hybrid path C, Phase 1).
    Vello,
}

impl RendererBackendKind {
    /// Parse the `backend` attribute literal. Returns `None` for any
    /// value outside the closed set — the caller raises
    /// [`crate::diagnostic::PinionForgeDiagnostic::UnknownBackend`].
    #[must_use]
    pub fn from_attr(literal: &str) -> Option<Self> {
        match literal {
            "vello" => Some(Self::Vello),
            _ => None,
        }
    }

    /// Inverse of [`Self::from_attr`]. Stable wire identity.
    #[must_use]
    pub fn as_attr(self) -> &'static str {
        match self {
            Self::Vello => "vello",
        }
    }
}

impl RendererBackend {
    /// Variant tag identity — drops the payload, returns the
    /// [`RendererBackendKind`] matching the wire `backend=...` literal.
    /// Use for diagnostic / wire surfaces; codegen pattern-matches the
    /// full variant directly.
    #[must_use]
    pub fn kind(self) -> RendererBackendKind {
        match self {
            Self::Vello { .. } => RendererBackendKind::Vello,
        }
    }
}

/// Vello antialiasing mode. Closed enum mapping to
/// [`vello::AaSupport`] + [`vello::AaConfig`] pairs. R46.2.1 introduces
/// all three Vello 0.6 modes; future Vello releases adding new modes
/// (e.g. distance-field shading) attach as additional variants.
///
/// Default policy: `Area` — the cheapest analytic mode, canonical for
/// UI workloads (Vello / Xilem documented default). The manifest's
/// `aa` attribute is optional; absence resolves to [`Self::Area`] so
/// R46.1-vintage manifests (with only `backend="vello"`) keep working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VelloAaMode {
    /// `aa="area"` — Vello's analytic-area antialiasing. UI mode
    /// canonical (cheapest, single-pass, no MSAA buffer cost).
    Area,
    /// `aa="msaa8"` — 8× multisample antialiasing. Intermediate
    /// quality / cost; suitable for mixed UI + light 3D scenes.
    Msaa8,
    /// `aa="msaa16"` — 16× multisample antialiasing. Highest quality;
    /// game-mode candidate per §2#4 mode toggle, R41 Phase 2+ path.
    Msaa16,
}

impl VelloAaMode {
    /// Parse the `aa` attribute literal. Returns `None` for any value
    /// outside the closed set — the caller raises
    /// [`crate::diagnostic::PinionForgeDiagnostic::UnknownAa`].
    #[must_use]
    pub fn from_attr(literal: &str) -> Option<Self> {
        match literal {
            "area" => Some(Self::Area),
            "msaa8" => Some(Self::Msaa8),
            "msaa16" => Some(Self::Msaa16),
            _ => None,
        }
    }

    /// Inverse of [`Self::from_attr`]. Stable wire identity.
    #[must_use]
    pub fn as_attr(self) -> &'static str {
        match self {
            Self::Area => "area",
            Self::Msaa8 => "msaa8",
            Self::Msaa16 => "msaa16",
        }
    }
}

/// Closed child-element enum for the reactive kind. R38.2a/b/c/d
/// populate all four variants. The renderer kind has no children — its
/// payload lives entirely in root attributes — so this enum is only
/// referenced from [`PinionSpec::Reactive`].
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
