//! `External` primitive integration contract (§5.15, R16 slice 6).
//!
//! Eight-point contract ratified Round 7:
//!
//!   1. Backend support declaration — [`External::backends`]
//!   2. Repaint trigger ownership   — [`External::repaint_ownership`]
//!   3. Thread ownership            — [`External::thread_ownership`]
//!   4. Lifecycle event callbacks   — `on_mount` / `on_unmount` /
//!      `on_visibility_change` / `on_focus_change`
//!   5. Input forwarding policy     — [`External::handles_event`]
//!   6. DPI / resize notification   — `on_dpi_change` / `on_resize`
//!   7. Async state change channel  — [`External::poll_state`] (pull form)
//!   8. Symbolic introspection      — *opt-in*, lands as a separate
//!      sub-trait in a later slice
//!
//! Items 1-7 are mandatory; items 1-3 are required (no default), items
//! 4-7 ship sensible no-op defaults so authors only override what they
//! need.
//!
//! The trait is **dyn-safe** by construction (all methods take `&self`
//! or `&mut self`, no associated consts, no `Self`-returning methods).
//! This keeps `Box<dyn External>` available for heterogeneous storage
//! when the §5.15 scene-tree integration lands.
//!
//! `StubExternal` is a ref-impl: Gui-only, framework-driven repaint,
//! UI-thread synchronous. It exists to anchor the contract semantically
//! and to give tests/examples a baseline.

use std::borrow::Cow;
use std::rc::Rc;

use crate::Event;
use crate::input::{GesturePhase, Modifiers, PointerKind, RawPointerButton};
use crate::intent::Intent;

/// Render backends an `External` may declare support for (§5.15 item 1).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    /// GPU-backed window per §5.9 trait-based Renderer.
    Gui,
    /// Text terminal rendering per §5.9 dual backend.
    Tui,
    /// JSON-RPC symbolic surface per §5.7 §5.12.
    Rpc,
}

/// What the framework should do when a scene targets a backend the
/// `External` does not support (§5.15 item 1 fallback policy).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendFallback {
    /// Reject the scene at composition time — per §5.15 caveat,
    /// non-conforming `External` should not silently break.
    Reject,
    /// Skip the `External` (renders as an empty placeholder); useful
    /// for optional content like a video viewport when running headless.
    Skip,
}

/// Backend support declaration (§5.15 item 1).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendSupport {
    /// Backends the `External` can dispatch into. Order is not
    /// significant; uniqueness is the implementor's responsibility.
    pub supported: &'static [Backend],
    /// Policy for unsupported backends.
    pub fallback: BackendFallback,
}

impl BackendSupport {
    #[must_use]
    pub const fn new(supported: &'static [Backend], fallback: BackendFallback) -> Self {
        Self {
            supported,
            fallback,
        }
    }

    /// Returns `true` when this `External` declares support for `backend`.
    #[must_use]
    pub fn supports(&self, backend: Backend) -> bool {
        self.supported.contains(&backend)
    }
}

/// Who drives repaint scheduling for an `External` (§5.15 item 2).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepaintOwner {
    /// Framework decides when to repaint (layout-driven; default for
    /// static content like styled boxes embedding an SVG).
    Framework,
    /// `External` owns its render loop (game viewport, video player);
    /// the framework just composes the resulting surface.
    External,
}

/// Where the `External` performs its work (§5.15 item 3).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadOwnership {
    /// `External` runs synchronously on the UI thread.
    UiThreadSync,
    /// `External` owns a worker thread; framework communicates via a
    /// sync channel. State pushes use [`External::poll_state`] today;
    /// push-form variant lands when §6.3 async boundary settles.
    OwnThread,
}

/// Opaque state-update payload from an `External` to the framework
/// (§5.15 item 7). The concrete schema is settled in a later slice —
/// today this is a marker so the contract surface stays stable.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
pub struct StateUpdate;

// ---------------------------------------------------------------------------
// §5.15 item 8 — Optional symbolic introspection (opt-in sub-trait).
// ---------------------------------------------------------------------------

/// R1353 §5.12 §2 #2 — where a **parametric** path's argument is allowed to
/// come from, so a client can enumerate valid arguments instead of guessing
/// them.
///
/// The domain is expressed as a *reference to another path on the same
/// surface*, never as a literal bound. A surface's shape is live (a grid gains
/// columns, a tree collapses), so a literal `0..8` baked into a schema would be
/// stale the moment the model changed — and a schema that lies is worse than one
/// that says nothing. Pointing at `cols` instead means the bound is always read
/// fresh, from the one place that owns it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgDomain {
    /// Valid arguments are the indices `0..query(<count_path>)` — the argument
    /// is an offset into a sequence whose length the surface publishes.
    /// (`width.<col>` with `IndexOf("cols")`: read `cols`, then `width.0` …
    /// `width.<cols-1>` are exactly the answerable paths.)
    IndexOf(&'static str),
    /// Valid arguments are the values the surface lists at `<values_path>` —
    /// the argument is a key, not an offset (a panel id, a voice id).
    ValuesOf(&'static str),
    /// The surface publishes nothing a client can enumerate the argument from.
    ///
    /// **Worth suspicion at every use, and common enough to matter**: this is
    /// the majority variant today, and each one is a client still guessing. Two
    /// honest reasons to reach for it, both real in-tree:
    ///
    /// * the bound exists but is **not expressible** — `datepicker`'s
    ///   `state.<day>` runs `1..=days` (one-based, inclusive), which
    ///   [`IndexOf`](Self::IndexOf)'s `0..count` cannot state, so claiming it
    ///   would be false;
    /// * the bound exists but lives on **another surface** — `hello-tree-grid`'s
    ///   `cell_at.<pos>.<col>` bounds `pos` by a visible-row count that belongs
    ///   to the tree-state external, not to this one.
    ///
    /// What it must NOT be is a default. `Open` on a surface that publishes the
    /// count three lines up is an *affirmative false statement* — it tells an
    /// agent there is nothing to know, which is worse than the pre-R1353 silence
    /// it replaced, because now it carries a schema's authority.
    Open,
}

impl ArgDomain {
    /// The `$schema` wire form of this domain.
    ///
    /// Rendered **here**, in the crate that defines the enum, rather than in
    /// `pinion-rpc` where `$schema` is assembled. `ArgDomain` is
    /// `#[non_exhaustive]`, so a match in any other crate needs a `_` arm — and
    /// a `_` arm would render a future variant as some silent placeholder,
    /// telling clients "unconstrained" about a domain that constrains. Inside
    /// the defining crate the match is exhaustive, so adding a variant fails to
    /// compile here until its wire form is decided. The type owns its wire form;
    /// the transport only forwards it.
    #[must_use]
    pub fn to_wire(self) -> serde_json::Value {
        match self {
            Self::IndexOf(count_path) => {
                serde_json::json!({ "kind": "index_of", "count_path": count_path })
            }
            Self::ValuesOf(values_path) => {
                serde_json::json!({ "kind": "values_of", "values_path": values_path })
            }
            Self::Open => serde_json::json!({ "kind": "open" }),
        }
    }
}

/// R1353 §5.12 §2 #2 — the argument a **parametric** [`SchemaField`] takes.
///
/// See [`SchemaField::parametric`] for what parametric means and why it must be
/// declared rather than left implicit in the `query` impl.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaArg {
    /// What the argument means, for a human and for an AI's prompt: `"col"`,
    /// `"pos"`, `"id"`. Not a type — [`Self::ty`] is.
    pub name: &'static str,
    /// The argument's type tag, in the same vocabulary as [`SchemaField::ty`]
    /// (`"int"`, `"string"`).
    pub ty: &'static str,
    /// Where the answerable arguments come from.
    pub domain: ArgDomain,
}

impl SchemaArg {
    /// An index argument bounded by the sequence length at `count_path`
    /// (`ArgDomain::IndexOf`) — the overwhelmingly common shape.
    #[must_use]
    pub const fn index(name: &'static str, count_path: &'static str) -> Self {
        Self {
            name,
            ty: "int",
            domain: ArgDomain::IndexOf(count_path),
        }
    }

    /// A key argument drawn from the values listed at `values_path`
    /// ([`ArgDomain::ValuesOf`]).
    #[must_use]
    pub const fn key(name: &'static str, ty: &'static str, values_path: &'static str) -> Self {
        Self {
            name,
            ty,
            domain: ArgDomain::ValuesOf(values_path),
        }
    }

    /// An argument the surface does not constrain ([`ArgDomain::Open`]).
    #[must_use]
    pub const fn open(name: &'static str, ty: &'static str) -> Self {
        Self {
            name,
            ty,
            domain: ArgDomain::Open,
        }
    }
}

/// One declared member of an [`ExternalIntrospect`] surface: a path, the type of
/// the value it reads, and — R1353 — whether it takes an **argument**.
///
/// # Why this is a struct and not the `(path, type)` pair it replaced
///
/// (R1353 §2 #2, the PR-61 root cause.) The pair could not say that a path is
/// parametric. `("width", "int")` and `("total", "int")` rendered *identically*
/// through `$schema`, yet `total` is read as-is while `width` must be spelled
/// `width.<col>` — the argument rides the path (see
/// [`ExternalIntrospect::query`]). The arity lived only in the `query` impl's
/// `strip_prefix`, so the one surface an agent is *supposed* to discover the
/// contract from could not express it. Agents had to guess, and the guess failed
/// in both directions: `query("width")` answered `UnknownIntrospectPath` for a
/// path `$schema` had just advertised, while `query("width.999")` answered with a
/// plausible wrong number.
///
/// That is not a documentation problem. §2 #2 makes RPC the AI's *primary* path,
/// so a contract the surface cannot state is a contract that does not exist. A
/// consumer read the argument-free `query` signature, concluded a parameterized
/// read was impossible, routed it through `invoke` instead — and bought a ~30Hz
/// livelock that burned a core at idle, because `invoke` is a mutation and bumps
/// the scene revision its own `waitFor` was parked on.
///
/// A struct rather than a richer *string* (`"width.<col>"`, the URI-template
/// shape): the pair is already short of a second dimension — nothing here
/// distinguishes a readable value from an `invoke` action, so `float_policy` and
/// `set_float_policy` both render as bare `string`. A template string would fix
/// arity and have to be redone for that. Const constructors + defaulted fields
/// mean the next dimension lands additively.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaField {
    /// The declared path, as the wire **template**: a scalar spells itself
    /// (`"total"`), a parametric family spells its placeholders inline
    /// (`"width.<col>"`, `"cell.<row>.<col>"`, `"voice.<id>.gain"`).
    ///
    /// The template — rather than a bare stem plus trailing args — because an
    /// argument is not always trailing: `voice.<id>.gain` carries a literal
    /// *after* its argument, and a stem model cannot say that. It also happens
    /// to be the form the workspace's schemas were already hand-spelling before
    /// R1353 gave the arguments types; those strings were right, they were just
    /// unaccompanied.
    ///
    /// [`literal_prefix`](Self::literal_prefix) recovers the exact string a
    /// `query` impl strips.
    pub path: &'static str,
    /// Type tag of the value read at this path.
    pub ty: &'static str,
    /// The arguments the path takes, in wire order — empty for a plain scalar.
    ///
    /// A slice rather than an `Option<SchemaArg>` because a family can be keyed
    /// by more than one argument: `Table` reads a cell as `cell.<row>.<col>`.
    /// Modelling one argument would have forced that field back to a hand-spelled
    /// template string — the exact under-declaration this type exists to end.
    pub args: &'static [SchemaArg],
}

impl SchemaField {
    /// A placeholder for `const fn` schema COMPOSITION — the fill value for a
    /// fixed-size array a `const fn` builds before overwriting every slot
    /// (`hello-audio-device` concatenates the RT surface's fields with its own
    /// this way, so a field added upstream cannot silently go missing
    /// downstream).
    ///
    /// It exists because `#[non_exhaustive]` denies other crates the struct
    /// literal such a composer would otherwise use, and a composer cannot invent
    /// a placeholder from a name it does not have. Never declare it: an empty
    /// path matches no query, so a slot left un-overwritten shows up as a blank
    /// row in `$schema` rather than as something plausible.
    pub const EMPTY: Self = Self::new("", "");

    /// A plain scalar path: read it as spelled, no argument.
    #[must_use]
    pub const fn new(path: &'static str, ty: &'static str) -> Self {
        Self {
            path,
            ty,
            args: &[],
        }
    }

    /// A **parametric** path: `path` is the wire template with a `<name>`
    /// placeholder per entry of `args`, in order (`"width.<col>"`,
    /// `"cell.<row>.<col>"`, `"voice.<id>.gain"`).
    ///
    /// `args` is a slice even for the overwhelmingly common single-argument case:
    /// a `const fn` cannot build a `&'static` slice out of a by-value parameter,
    /// so a one-arg convenience constructor could not delegate here and would be
    /// a second, drifting definition of the same thing.
    ///
    /// The template's placeholders and `args` must agree in name and order. A
    /// `const fn` cannot parse the string to check that here, so it is enforced
    /// by `r1353_1_every_real_declaration_matches_its_template`, which scans the
    /// workspace's SOURCE for every real `parametric` call — a runtime test
    /// cannot see a declaration in a crate this one does not link against, and
    /// R1353's first cut cited a test that only checked its own fixtures.
    #[must_use]
    pub const fn parametric(
        path: &'static str,
        ty: &'static str,
        args: &'static [SchemaArg],
    ) -> Self {
        Self { path, ty, args }
    }

    /// The literal prefix before this field's first argument — exactly the
    /// string a `query` impl's `strip_prefix` matches (`"width.<col>"` →
    /// `"width."`, `"state:<id>"` → `"state:"`). Equal to [`path`](Self::path)
    /// for a scalar.
    ///
    /// Verbatim, with the separator: the separator is whatever the author wrote
    /// (`.` almost everywhere, `:` in `hello-input-chip`), and this type does not
    /// get to assume one. R1353's first cut trimmed `'.'` specifically, which
    /// made the same accessor answer `"width"` for a dotted template and
    /// `"state:"` for a colon one — and its doc, which claimed to return what
    /// `strip_prefix` matches, was then true only for the case it did not trim.
    #[must_use]
    pub fn literal_prefix(&self) -> &'static str {
        match self.path.find('<') {
            Some(i) => &self.path[..i],
            None => self.path,
        }
    }

    /// Does `probe` address this field? An exact hit for a scalar; for a
    /// parametric family, a probe that matches the template with a non-empty
    /// value in each placeholder.
    ///
    /// The membership question every caller actually means — a parametric family
    /// is addressed by its members, never by its template, so a bare
    /// `fields.iter().any(|f| f.path == probe)` answers "no such path" for
    /// `width.0`, a path the surface answers perfectly well. That mistake is the
    /// §2 #7 lie [`read_only_or_unknown`] exists to prevent, so it routes here.
    ///
    /// Deliberately checks that an argument is *present*, not that it is
    /// *well-formed* or in range — only the `query` impl knows that. This
    /// answers "does this field own this path", which is what an error-kind
    /// decision needs; `width.zzz` belongs to `width` and is malformed, not
    /// unknown.
    #[must_use]
    pub fn addresses(&self, probe: &str) -> bool {
        if self.args.is_empty() {
            return self.path == probe;
        }
        // Walk the template's literal segments across `probe`, requiring a
        // non-empty run wherever a placeholder sits.
        let mut rest = probe;
        let mut tmpl = self.path;
        let mut first = true;
        while let Some(open) = tmpl.find('<') {
            let literal = &tmpl[..open];
            let Some(after) = rest.strip_prefix(literal) else {
                return false;
            };
            if !first && literal.is_empty() {
                // Two placeholders with no literal between them cannot be
                // delimited; such a template is malformed, not matchable.
                return false;
            }
            rest = after;
            let Some(close) = tmpl[open..].find('>') else {
                return false;
            };
            tmpl = &tmpl[open + close + 1..];
            // The argument runs until the template's next literal (or the end).
            let next_lit_end = tmpl.find('<').unwrap_or(tmpl.len());
            let next_lit = &tmpl[..next_lit_end];
            let arg_len = if next_lit.is_empty() {
                rest.len()
            } else {
                match rest.find(next_lit) {
                    Some(i) => i,
                    None => return false,
                }
            };
            if arg_len == 0 {
                return false;
            }
            rest = &rest[arg_len..];
            first = false;
        }
        rest == tmpl
    }
}

/// Schema declaring which paths an [`ExternalIntrospect`] exposes.
///
/// R1353: a static slice of [`SchemaField`]s — a path, its type, and whether it
/// takes an argument. Future expansion (a structured `Type` enum, read-vs-action
/// kind, units of measure) lands via `#[non_exhaustive]` + defaulted const
/// constructors — additive only.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntrospectSchema {
    /// Declared fields. Authors are responsible for keeping this in sync with
    /// `query` / `intervene`; mismatches surface as test failures, not silent
    /// corruption.
    pub fields: &'static [SchemaField],
}

impl IntrospectSchema {
    #[must_use]
    pub const fn new(fields: &'static [SchemaField]) -> Self {
        Self { fields }
    }

    /// The field addressing `probe`, if any — exact for a scalar, stem-matched
    /// for a parametric family ([`SchemaField::addresses`]).
    #[must_use]
    pub fn field_for(&self, probe: &str) -> Option<&SchemaField> {
        self.fields.iter().find(|f| f.addresses(probe))
    }
}

/// Opaque value payload for `query` / `intervene`. Scalar variants
/// cover the JSON-RPC primitive surface; `Json` carries arbitrary
/// structured payloads (objects, arrays, mixed scalars) for callers
/// that round-trip through `serde_json::Value` — used by the §5.22
/// reactive bridge for `Signal<T>` where `T` is a struct or sequence
/// (R37.6 #11 extension).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum IntrospectValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Json(serde_json::Value),
}

impl IntrospectValue {
    /// R51.155 §5.15 — extract a `bool` payload. Returns `Some(b)`
    /// only when the variant is [`Self::Bool`]; every other variant
    /// (including `Json(serde_json::Value::Bool(_))`) returns `None`
    /// so the typed-extraction path stays unambiguous.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// R51.155 §5.15 — extract an `i64` payload. Returns `None` for
    /// non-[`Self::Int`] variants; numeric coercions (`Float → i64`,
    /// `Json::Number → i64`) are intentional opt-outs.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// R51.155 §5.15 — extract an `i64` payload narrowed to `i32`.
    /// Returns `None` when the variant is not [`Self::Int`] or when
    /// the stored value falls outside the `i32` range — narrowing
    /// failures are surfaced rather than silently truncated.
    /// Convenient for the common composite-widget index path
    /// (`focused_index` / `selected_index` introspect slots return
    /// non-negative `Int`).
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        self.as_i64().and_then(|v| i32::try_from(v).ok())
    }

    /// R51.155 §5.15 — extract an `i64` payload narrowed to `usize`.
    /// Returns `None` for non-[`Self::Int`] variants and for negative
    /// integers (which can't be a `usize`).
    #[must_use]
    pub fn as_usize(&self) -> Option<usize> {
        self.as_i64().and_then(|v| usize::try_from(v).ok())
    }

    /// R51.155 §5.15 — extract a `f64` payload. Returns `None` for
    /// non-[`Self::Float`] variants; integer-to-float coercion is an
    /// intentional opt-out.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// R51.155 §5.15 — extract a `f64` payload narrowed to `f32`.
    /// Returns `None` for non-[`Self::Float`] variants; the `f64 →
    /// f32` narrowing is a documented truncation (precision loss for
    /// values past f32's representable range, NaN passes through).
    /// Encapsulates the previous per-call-site
    /// `#[allow(clippy::cast_possible_truncation)]` lints that
    /// hello-slider*/hello-slider-vertical sprinkled around their
    /// `IntrospectValue::Float(v) => v as f32` matches.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn as_f32(&self) -> Option<f32> {
        self.as_f64().map(|v| v as f32)
    }

    /// R51.155 §5.15 — extract a `&str` payload. Returns `None` for
    /// non-[`Self::Text`] variants; `Json::String` is opt-out (the
    /// JSON path goes through `as_json`).
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// R51.155 §5.15 — `true` iff the variant is [`Self::Null`].
    /// Diagnostic helper paired with the typed accessors above.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// R1276 §5.15 — construct a [`Self::Json`] payload from any
    /// serializable value: the canonical way an
    /// [`ExternalIntrospect::query`] returns a structured (object / array)
    /// result. Lifts the `Json(serde_json::to_value(..).unwrap_or(Null))`
    /// idiom the narrative / place-map / audio introspection surfaces each
    /// hand-rolled (Rule-of-Three). A serialization failure — which the
    /// derived `Serialize` impls this is used with cannot produce —
    /// degrades to [`Self::Null`] rather than panicking.
    #[must_use]
    pub fn json<T: serde::Serialize>(value: &T) -> Self {
        Self::Json(serde_json::to_value(value).unwrap_or(serde_json::Value::Null))
    }
}

/// R826 §5.12 — the shared `<axis>.<pos>` introspect projection. Resolve a
/// visual position (`rest`, the part after a matched `<axis>.` prefix)
/// through `get`, then `project` the looked-up item to an
/// [`IntrospectValue`]. An out-of-range or unparseable position — or a
/// `project` that itself yields it — reports [`IntrospectValue::Null`]
/// (present-but-empty), never absence: the §5.12 convention every
/// position-indexed introspect path shares.
///
/// The single source of truth for that convention, lifted at R826 from the
/// three byte-identical copies the sort proxies
/// ([`source_at_value`](crate::widgets::order_memo)), the tree filter
/// (`id_value`), and the tree view (`row_at`) had each grown — each now a
/// thin `(get, project)` binding over this primitive, so the out-of-range
/// policy lives in exactly one place.
pub(crate) fn at_index<T>(
    rest: &str,
    get: impl Fn(usize) -> Option<T>,
    project: impl Fn(T) -> IntrospectValue,
) -> IntrospectValue {
    rest.parse::<usize>()
        .ok()
        .and_then(get)
        .map_or(IntrospectValue::Null, project)
}

/// R742 §5.51 — typed drag-and-drop payload. Produced by a drag source
/// via [`External::begin_drag`] and carried by the router's drag session
/// until the matching drop, mirroring the
/// [`Intent`] wire form (a `kind` tag plus an
/// [`IntrospectValue`]) so the in-flight drag is introspectable as
/// scene-as-data (§2 #7) and a future cross-widget drop target can match
/// on `kind` before interpreting `value`.
#[derive(Debug, Clone, PartialEq)]
pub struct DragPayload {
    /// Discriminator naming what is being dragged (e.g. `"dnd-row"`,
    /// `"dock-panel"`, `"tab"`). A drop target matches on this before
    /// reading `value`. `Cow` so a static-string source pays no
    /// allocation while a runtime-built kind is still expressible.
    pub kind: Cow<'static, str>,
    /// The dragged datum — typically the source item's stable id or
    /// index, addressed the same way an [`Intent`] payload is.
    pub value: IntrospectValue,
}

/// R742 §5.51 — the live drop location the router resolves under the
/// cursor during a drag and feeds back to the drag source via
/// [`External::drag_to`] / [`External::drag_release`].
///
/// `tag` is the full paint tag directly under the cursor — a composite
/// `widget#sub` when the hovered region is a sub-element (the reorder
/// row / dock panel / tab the cursor is over). `x_rel` / `y_rel` are the
/// cursor position normalised over that tag's post-layout rect, in
/// `0.0`..`1.0` because `tag` is the region the cursor is over — the
/// normalisation is NOT itself clamped, so a value outside that range
/// would mean the cursor had left the rect. The source coordinator
/// classifies before / after / centre from these without re-reading
/// layout — the generalisation of the dock resolver's edge-vs-centre
/// zone test.
#[derive(Debug, Clone, PartialEq)]
pub struct DropPoint {
    /// Full paint tag under the cursor (possibly composite `widget#sub`).
    pub tag: String,
    /// Cursor X normalised over `tag`'s rect (`0.0` left .. `1.0` right).
    pub x_rel: f32,
    /// Cursor Y normalised over `tag`'s rect (`0.0` top .. `1.0` bottom).
    pub y_rel: f32,
}

/// (R1156 §5.51) Reserved [`DropPoint::tag`] the cross-window drop resolution
/// returns when the cursor lands in the OUTER PERIMETER band of the drop surface
/// (within [`OUTER_DOCK_MARGIN`] of the window content's edge) instead of over an
/// inner panel. A dock consumer reads it as a FULL-SPAN outer dock at the edge
/// the `x_rel` / `y_rel` (normalised over the WHOLE surface here, not a panel) is
/// nearest — the container-edge / "outer dock guide" gesture (VS Code edge zones,
/// Qt ADS outer dock areas). The leading `NUL` makes it a sentinel no real paint
/// tag can collide with.
pub const OUTER_DOCK_ZONE_TAG: &str = "\u{0}outer-dock-zone";

/// (R1205 §5.51 §5.39) Tag the dock walker
/// ([`view_dock_surface`](../../pinion_widget_paint/dock/fn.view_dock_surface.html))
/// stamps on the container wrapping its whole workspace subtree, so the laid-out
/// rect of that wrapper IS the DOCK AREA — the region the reorganizer manages,
/// wherever the composing view places it (below a client-side chrome strip, below
/// a fixed toolbar / menu, inside a split, …). The one SSOT for "where is the
/// dock area" ([`Scene::dock_surface_rect`](crate::scene::Scene::dock_surface_rect)):
/// the same-window OUTER dock band (`InputRouter::resolve_own_outer_dock`) and the
/// cross-window redock preview both read this rect, so they agree on the dock area
/// with ZERO wiring — no per-window chrome-height scalar to stamp (R1202/R1203's
/// `dock_area_top_inset` / `inset_below_chrome`, a top-only approximation blind to a
/// toolbar, were retired for this rect). The leading `NUL` makes it a sentinel no
/// real user paint tag can collide with, and it is a structural ANCESTOR of the
/// tagged splitter / panel that fills it, so `resolve_hover_tag`'s deepest-first
/// walk never resolves to it.
pub const DOCK_SURFACE_TAG: &str = "\u{0}dock-surface";

/// (R1156 §5.51) How far INSIDE / outside the drop surface's perimeter the cursor
/// may sit and still classify as an OUTER full-span dock (logical px). The
/// outermost band of this width maps to the container edge; the interior past it
/// maps to per-panel inner zones — so a drop at the very top of the dock area is a
/// full-width row, while a drop between two panels splits just those panels.
///
/// ★LIVE-TUNE (R1167, HW-gated): this is a fixed absolute band. The user found it
/// "too thin" cross-window; the R1167 same-window outer dock
/// (`InputRouter::resolve_own_outer_dock`) reaches it from inside, so it is now
/// reachable at this width, but the FEEL of the width is the user's `:0` call. A
/// fixed widen is NOT scale-safe (a band wider than a small floater's half makes
/// its whole area outer), so a future tune is likely a fraction-of-dimension band
/// (`min(MARGIN, frac * dim)`), not a bigger constant — deferred to live feedback.
pub const OUTER_DOCK_MARGIN: f64 = 32.0;

/// (R1081 §5.51; R1167 SSOT-lift to core) The [`DragPayload::kind`] discriminator a
/// dock-panel drag (a panel header OR a tab) carries. Lives in core so BOTH the
/// producing widget (`pinion_widget_paint::dock`, which re-exports it) AND the
/// consuming runtime router (which gates the same-window OUTER-dock override on it
/// — a non-dock drag like the outliner tree reparent must NOT get the dock
/// sentinel) name the one string. The drag-kind is wire vocabulary shared across
/// the producer/consumer boundary, so its canon home is the shared crate
/// (the [[wire-vocab-canon-pin-not-fold]] pattern), like [`OUTER_DOCK_ZONE_TAG`].
pub const DOCK_PANEL_DRAG_KIND: &str = "dock-panel";

/// Failure modes for [`ExternalIntrospect::intervene`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterveneError {
    /// Path is not declared in the schema.
    UnknownPath,
    /// Path exists but the value variant does not match the slot type.
    /// Use this for "the JSON was a String when an Int was expected",
    /// not for "the Int was outside the slot's accepted range" — the
    /// latter is [`OutOfRange`](Self::OutOfRange).
    TypeMismatch,
    /// Path exists and the type matches but the slot is read-only.
    ReadOnly,
    /// R51.91 §5.40 — path exists and the value variant matches the
    /// slot type, but the value itself falls outside the accepted
    /// range. Composite widgets that address sub-elements by index
    /// (`RadioGroup::selected_index` / `focused_index`,
    /// future `ListBox::selected_index` / `TabBar::active_index`)
    /// raise this for negative integers and indices `>= count`. Slot
    /// types with continuous-value clamping (`Slider::value`) prefer
    /// internal clamping over rejection and do not raise this.
    OutOfRange,
}

/// Failure modes for [`ExternalIntrospect::invoke`] (R17 bidirectional
/// RPC spec round — symbolic action channel, third leg of the
/// query / intervene / invoke triad).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvokeError {
    /// Path is not declared as an action in the schema.
    UnknownPath,
    /// Args variant does not match the action's declared argument
    /// type.
    TypeMismatch,
    /// Path exists and args type matches, but the action refused to
    /// fire (preconditions unmet, statechart in a forbidding state,
    /// etc.). Distinct from `TypeMismatch` because retrying with
    /// different args may succeed.
    Rejected,
}

/// Opt-in symbolic introspection (§5.15 item 8). An `External` exposes
/// this sub-trait by overriding [`External::introspect`] /
/// [`External::introspect_mut`] to return `Some(self)`.
///
/// The triad of operations (R17 bidirectional RPC spec round):
///   * [`schema`](Self::schema): declare which paths exist.
///   * [`query`](Self::query): read a value at a path (`&self`).
///   * [`intervene`](Self::intervene): write a value to a slot
///     (`&mut self`, returns `()`).
///   * [`invoke`](Self::invoke): trigger an action with args
///     (`&mut self`, returns `IntrospectValue`).
///
/// The split: `intervene` writes a *slot* (idempotent assignment),
/// `invoke` calls an *action* (event-shaped, may return a computed
/// value such as the resulting state). Schemas may declare a path as
/// either a state slot (write via intervene) or an action (call via
/// invoke); §5.3 DSL settles whether the schema distinguishes them
/// explicitly.
///
/// Designed dyn-safe (all methods take `&self` or `&mut self`,
/// no associated items, no `Self`-returning methods) so the framework
/// can hold `&dyn ExternalIntrospect` for path-driven dispatch under
/// the §5.12 `query` / `snapshot` / `rewind` / `invoke` RPC methods.
pub trait ExternalIntrospect {
    /// Schema of introspectable state.
    fn schema(&self) -> IntrospectSchema;

    /// Read the value at `path`. `None` when `path` is not in the
    /// schema.
    ///
    /// # A read CAN take an argument: encode it in the path
    ///
    /// (R1352 §5.12 §2 #2 PR-61) There is no `args` parameter here, and that
    /// reads at a glance like "a parameterized read is impossible" — a
    /// consumer who reached exactly that conclusion from this signature
    /// routed their offset-bearing read through [`invoke`](Self::invoke)
    /// instead, and paid for it (see below). It is not impossible. The
    /// **argument rides the path**, and the workspace does this widely:
    ///
    /// * `width.<col>` — [`ColumnWidthExternal`](crate::widgets::column_widths)
    /// * `id_at.<pos>` / `level_at.<pos>` — [`tree_nav`](crate::widgets::tree_nav)
    /// * `name.<idx>` / `is_dir.<idx>` — [`file_browser`](crate::widgets::file_browser)
    /// * `state.<idx>` / `expanded.<idx>` — [`disclosure_group`](crate::widgets::disclosure_group)
    ///
    /// An impl serves one by matching the prefix before its exact-path arm:
    ///
    /// ```ignore
    /// fn query(&self, path: &str) -> Option<IntrospectValue> {
    ///     if let Some(rest) = path.strip_prefix("width.") {
    ///         let col: usize = rest.parse().ok()?;
    ///         return Some(IntrospectValue::Int(self.width(col).into()));
    ///     }
    ///     match path { /* argument-free paths */ _ => None }
    /// }
    /// ```
    ///
    /// Declare the family in [`schema`](Self::schema) with
    /// [`SchemaField::parametric`], whose path is the wire TEMPLATE
    /// (`"width.<col>"`) — see the next section. `query("width")` with no index
    /// correctly resolves to `None`, and a snapshot skips parametric families by
    /// declaration, so a family never pollutes one.
    ///
    /// ## The schema SAYS a path is parametric — do not make a client guess
    ///
    /// (R1352 found this hole; R1353 closed it.) A parametric family is declared
    /// with [`SchemaField::parametric`], which carries the wire template and a
    /// typed [`SchemaArg`] per placeholder — so `$schema` renders
    /// `{"path": "width.<col>", "type": "int", "args": [{"name": "col",
    /// "type": "int", "domain": {"kind": "index_of", "count_path": "cols"}}]}`
    /// where a scalar renders a bare `{"path": "total", "type": "int"}`.
    /// A client reads the arity, the argument's type, and where its valid values
    /// come from, instead of inferring any of it.
    ///
    /// It briefly did not. `("width", "int")` and `("total", "int")` rendered
    /// identically, so an agent had to guess that one of them took an argument —
    /// and the guess failed quietly, because an out-of-range `width.999` answered
    /// with the min clamp rather than an error. That hole is why a consumer
    /// reading this signature concluded a parameterized read was impossible at
    /// all: the convention was undiscoverable from the surface built to reveal
    /// it. Both halves are addressed — the declaration states the contract, and
    /// an out-of-range read no longer fabricates a value.
    ///
    /// The fix went into [`IntrospectSchema`], **not** into an `args` parameter
    /// here: that would fork the read surface in two and orphan every family
    /// listed above.
    ///
    /// ## Two things it does NOT settle
    ///
    /// **The separator has no escape.** An argument is delimited by `.`, and
    /// nothing escapes a `.` *inside* an argument. So a key that contains one is
    /// not addressable: `voice.<id>.gain` cannot express the id `"x.gain"`
    /// (matching takes the first trailing literal, so `voice.x.gain.gain` matches
    /// nothing), and a template's final argument swallows the rest, so
    /// `cell.<row>.<col>` reads `cell.1.2.3` as `col = "2.3"` — owned by the
    /// field, malformed, `None` from `query`. This is safe today only because
    /// every argument in the workspace is an integer index or a dot-free id. A
    /// family keyed by a filename or a dotted path needs an escaping rule first;
    /// declaring one without it would be a promise the wire cannot keep.
    ///
    /// **Out-of-range has two spellings**, both honest and both in the tree:
    /// `None` — "no such path" — from the surfaces that guard the index
    /// explicitly (`column_widths`, `listbox`, `radio_group`,
    /// `disclosure_group`, `table`, `file_browser`, `row_style`, and any other
    /// that bounds before reading), and `Some(Null)` — "that position holds
    /// nothing" — from everything routed through `at_index`, which `map_or`s a
    /// missing element to `Null` (`tree_nav`, `tree_filter`, `grid_sort`,
    /// `view_order`, `group_order`, `row_search`, …). Treat neither list as
    /// exhaustive; the rule, not the roster, is what holds: **neither
    /// fabricates**, and that is the property
    /// `r1353_declared_domains_hold_on_real_widgets` enforces. Unifying the two
    /// spellings is a separate call nobody has needed to make.
    ///
    /// # Why this matters more than it looks
    ///
    /// `scene/query` is classified `MethodOcc::Read`, so it does **not** bump
    /// the scene revision. `scene/invoke` is `Mutate` and does. A read
    /// disguised as an invoke therefore *broadcasts a state change on every
    /// read* — and a client that also parks on `scene/waitFor` (which waits on
    /// that revision) wakes its own waiter with its own read. That loop was
    /// measured at ~30Hz: a core burnt at idle, ~4 CPU-hours on a day-old
    /// instance, and a wedged socket. Splitting the concept into extra
    /// argument-free paths to dodge it (one slot for "live", one for
    /// "historical") only moves the damage into the binding's wire vocabulary
    /// — one concept, two addresses, forever.
    ///
    /// So: a read that needs an argument is a `query` with the argument in the
    /// path. It is a read, and it stays free.
    fn query(&self, path: &str) -> Option<IntrospectValue>;

    /// Write `value` to `path`. Errors when the path is unknown, the
    /// value does not match the slot type, or the slot is read-only.
    ///
    /// # Errors
    ///
    /// Returns [`InterveneError`] per the variants above.
    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError>;

    /// Trigger the action at `path` with `args`, returning a typed
    /// result value (e.g. the new state after a state-machine
    /// transition). Default impl returns `Err(InvokeError::UnknownPath)`
    /// so existing `External` impls remain valid without opting in to
    /// the action channel.
    ///
    /// # Errors
    ///
    /// Returns [`InvokeError`] per the variants above.
    fn invoke(
        &mut self,
        _path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        Err(InvokeError::UnknownPath)
    }
}

/// R1407 §5.35 §5.22 — the payload a `Ctrl`/`Cmd`+C copy chord would write for
/// an introspectable external, or `None` when the key is not the copy chord OR
/// the external has nothing to copy (an empty or non-`Text` `query_field`). The
/// caller performs the OS side effect with its own clipboard tag:
///
/// ```ignore
/// if let Some(payload) = selection_copy_payload(intro, key, modifiers, FIELD) {
///     use_app_clipboard(TAG).copy(payload);
///     return true;
/// }
/// ```
///
/// `query_field` is the **AI-first peer**: the SAME serialization an
/// out-of-process `query <field>` reads, so a keyboard copy and an AI client
/// share one path. Copy is a *read* plus an OS side effect (CQS): this queries
/// the payload through the `&self` [`query`](ExternalIntrospect::query) — it
/// never mutates the external — and returns the string; the OS write (a binding's
/// `use_app_clipboard(TAG).copy(payload)`) stays at the call site.
///
/// Returning the payload rather than writing it here is forced by layering, not
/// preference: the platform clipboard (`pinion-platform-clipboard`) depends on
/// `pinion-core`, so a core helper that performed the write would invert that
/// edge into a dependency cycle. The split also keeps the decision unit-testable
/// **without** a clipboard, so the real OS clipboard is never raced from a test.
///
/// The `Ctrl`-OR-`Cmd`-not-`Alt` gate mirrors the `text_field` chord decode: on
/// layouts where `AltGr` is `Ctrl`+`Alt`,
/// [`command_key`](Modifiers::command_key) is `true` while the keypress composes
/// a character, so without the `!alt_key` guard `AltGr`+C would misfire the copy
/// and swallow the char (R1223). The key match is case-insensitive so
/// `Shift`+`Ctrl`+`C` (platforms deliver `"C"`) still copies.
///
/// This lifts the byte-identical copy chord+query its three consumers had grown
/// (`hello-cell-select` R1222, `hello-data-grid` R1372, `hello-hex-dump` R1407).
/// Each consumer's divergent part — *what* to serialize — stays in its own
/// `query` (the field name it passes), not here. The `text_field` widget's own
/// `Ctrl`+C is a distinct consumer class (an attached clipboard +
/// `TextEditState::selection_text`, not an introspect query) and deliberately
/// does NOT route through this helper.
#[must_use]
pub fn selection_copy_payload(
    intro: &dyn ExternalIntrospect,
    key: &str,
    modifiers: Modifiers,
    query_field: &str,
) -> Option<String> {
    if !(modifiers.command_key() && !modifiers.alt_key() && key.eq_ignore_ascii_case("c")) {
        return None;
    }
    match intro.query(query_field) {
        Some(IntrospectValue::Text(payload)) => Some(payload),
        _ => None,
    }
}

/// R738 §5.35 / R786 §5.35 — the rect a captured widget's cursor is
/// normalized against (returned by [`External::capture_normalize`]). One
/// exhaustive decision rather than the bool + `Option` pair it replaced, so a
/// widget cannot simultaneously request its primary *and* a named tag (an
/// illegal state the precedence rule used to resolve silently).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureNormalize<'a> {
    /// The grabbed (sub-)tag's own rect — the default. Correct for single-tag
    /// capture widgets and composites whose drag value is sub-region-relative
    /// (a dock panel tear-off measured against the grabbed header).
    Target,
    /// The primary half of the composite tag (`primary#sub` → `primary`). The
    /// range slider grabs a thumb sub-tag but its value spans the track.
    Primary,
    /// An explicitly named element's rect — for a drag whose reference is
    /// neither the grabbed tag nor its primary (column resize → grid viewport).
    Tag(&'a str),
}

/// R1101 §5.15 §5.51 — the live-drag context the router forwards to a drag
/// SOURCE on every move ([`External::drag_to_at`]) and the matching release
/// ([`External::drag_release_at`]). One struct rather than the widening
/// positional-argument ladder the `_at` pair had grown into (3 → 4 → … args):
/// the [`CaptureNormalize`] precedent applied to forwarding data. A new
/// dimension of drag context is a new field here, NOT a new positional arg
/// every `External` impl and call site must re-thread — so adding one leaves
/// the `drag_to_at` / `drag_release_at` SIGNATURE (and thus every drag source's
/// impl) untouched; only the router and the test builders that *construct* a
/// `DragUpdate` change.
///
/// Every field is data the **router alone** holds — a source `External` only
/// ever sees rect-relative coordinates — so the router is the single producer
/// and the source consumes. In particular [`became_drag`](Self::became_drag) is
/// the router's click-vs-drag verdict: a drag source MUST read it rather than
/// re-derive "is this press a drag yet" from its own distance tracking. A
/// private re-derivation drifts from the router's SSOT because it cannot see
/// the real press point (only the router's `press_gestures` does); it can only
/// approximate the origin with the first post-move sample, so it latches later
/// and at a different distance (the R1097 → R1101 `detach_latch` clearance).
///
/// Not `#[non_exhaustive]`: the router (pinion-runtime) and the drag-source
/// tests construct it with a struct literal across crate boundaries, exactly
/// as they construct [`DropPoint`] — the established precedent for a
/// cross-crate-constructed drag data carrier. `#[non_exhaustive]` would forbid
/// that literal and force a positional constructor, relocating the very
/// arg-ladder this struct removes.
#[derive(Debug, Clone, PartialEq)]
pub struct DragUpdate<'a> {
    /// The drop location currently under the cursor (the full tag + cursor
    /// normalised over its rect), or `None` over no tagged region — the
    /// rect-relative signal [`drag_to`](External::drag_to) already carried.
    pub over: Option<DropPoint>,
    /// The absolute **window-logical** cursor `(x, y)` the router holds (the
    /// same frame `scene/layout` reports). The only live pointer signal once
    /// `over` goes `None` (the cursor escaped every tag — the tear-off case).
    pub cursor: (f64, f64),
    /// The spec id of the window whose drop target `over` resolved in, when the
    /// cursor escaped THIS (the source's) window into ANOTHER and the shell
    /// resolved the drop there (`pinion_runtime::resolve_cross_window_drop`).
    /// `None` = the own / source window (every same-window drag). The shell
    /// composes this; the per-window router stays cross-window-blind, so the
    /// window dimension rides the `_at` path only.
    pub over_window: Option<&'a str>,
    /// (R1107 §5.16 §5.41 §5.51) The spec id of the window the drag is happening
    /// IN — the window whose router is driving this gesture, so `cursor` is in
    /// THIS window's logical frame. The symmetric peer of `over_window` (the
    /// TARGET window) at the DATA layer: `over_window` names where the drop
    /// lands, `source_window` names where the cursor is measured. (Their
    /// POPULATION is asymmetric: `over_window` is per-gesture transient session
    /// state the shell composes and pushes onto the router; `source_window` is
    /// the router's OWN permanent window identity, stamped once at the
    /// per-window dispatch seam — the router stays cross-window-blind for
    /// resolution but knows which window it IS.) A tear-off follow needs it to convert
    /// the window-logical cursor to a DESKTOP position via the SOURCE window's
    /// outer origin — re-dragging an already-floating panel's header reports a
    /// cursor in that floating window's frame, not the main window's, so a
    /// binding that assumed `"main"` placed the follower wrong. `None` = the
    /// router did not record its window id (a single-window / pre-R1107 path);
    /// the binding falls back to its primary window. The router fills it from
    /// its own window id, which the shell sets at the per-window dispatch choke.
    pub source_window: Option<&'a str>,
    /// The router's click-vs-drag verdict for the in-flight press: `true` once
    /// it strayed past `DRAG_CLICK_THRESHOLD_PX` *from the real press point*.
    /// The single SSOT the router owns (`press_gestures`) — a drag source reads
    /// it to decide "is this a drag yet" (a dock panel tearing off) instead of
    /// opening its own latch, which would drift.
    pub became_drag: bool,
    /// (R1117 §5.15 §5.51) The window-logical cursor at the PRESS that opened
    /// this drag session — constant for the gesture. `cursor` is the LIVE
    /// position; `press_cursor` is where the grab started. A grab-offset drag
    /// (relocate a borderless floater by its title bar, keeping the grabbed
    /// point under the cursor) computes `cursor - press_cursor` — the
    /// displacement since the grab. Without it a source could only anchor on the
    /// first MOVE sample, which already lags the press by the first motion delta
    /// (a constant mis-grab, ~1px for a real mouse but systematically wrong). The
    /// router fills it from the cursor it held when `begin_drag` opened the
    /// session, so it is exact regardless of how far the first move strays.
    pub press_cursor: (f64, f64),
}

/// The 8-point integration contract (§5.15). Items 1-3 are required;
/// items 4-7 have no-op defaults so authors override selectively.
///
/// `Debug` is a super-trait so `Box<dyn External>` participates in the
/// scene tree's `#[derive(Debug)]` machinery (§5.2 `ExternalNode`).
pub trait External: core::fmt::Debug {
    // --- 1. Backend support declaration ---

    /// Which backends this `External` dispatches into, and the policy
    /// for unsupported ones.
    fn backends(&self) -> BackendSupport;

    // --- 2. Repaint trigger ownership ---

    /// Whether the framework drives repaints (layout cadence) or the
    /// `External` owns its own render loop.
    fn repaint_ownership(&self) -> RepaintOwner;

    // --- 3. Thread ownership ---

    /// UI-thread synchronous, or `External`-owned worker thread.
    fn thread_ownership(&self) -> ThreadOwnership;

    // --- 4. Lifecycle event callbacks ---

    fn on_mount(&mut self) {}
    fn on_unmount(&mut self) {}
    fn on_visibility_change(&mut self, _visible: bool) {}
    fn on_focus_change(&mut self, _focused: bool) {}

    /// R1185 §5.16 §5.35 — re-project declarative policy from a freshly
    /// rebuilt descriptor onto this preserved live handle during an
    /// external-set reconcile.
    ///
    /// [`CoreShell::reconcile_externals`](../../pinion_runtime/core_shell/struct.CoreShell.html#method.reconcile_externals)
    /// keeps the LIVE `External` node for every surviving tag — preserving
    /// the in-flight gesture / session state (a drag capture, an id-minting
    /// counter, a lifecycle chart) that a blind rebuild would reset — and
    /// drops the freshly built handle. That is right for in-flight state,
    /// but it also freezes any **declarative policy** the factory computes
    /// as a reactive projection of binding state: a dock panel's
    /// `movable` / `floatable` lock that must flip when the last docked
    /// pane may no longer tear off is recomputed by the factory on every
    /// reconcile, yet the fresh value never reaches the live node, so the
    /// flag stays frozen at first construction. The reconcile hands the
    /// live handle the `fresh` descriptor here so a widget can copy its
    /// reactive-but-declarative fields across while retaining its own
    /// in-flight state.
    ///
    /// `fresh` is the just-built handle for the SAME tag (the same concrete
    /// type as this `&mut self`). Read its declared policy through the
    /// introspection channel already provided for exactly this cross-`dyn`
    /// read ([`introspect`](Self::introspect) +
    /// [`ExternalIntrospect::query`]) — no downcast required. Called on
    /// every reconcile pass for every surviving surface, so keep it to a
    /// few field copies, and do NOT touch in-flight state (that is the
    /// state the reconcile preserved this live node to keep).
    ///
    /// Default: ignore `fresh`, keep every field as-is — the pre-R1185
    /// behaviour, so an `External` with no reactive policy (and every
    /// existing impl) is unaffected.
    fn reconcile_from(&mut self, _fresh: &dyn External) {}

    // --- 5. Input forwarding policy ---

    /// Return `true` to claim the event (framework does *not* forward
    /// it further); `false` lets the framework process normally. Default
    /// is `false` — the framework forwards every event.
    fn handles_event(&self, _event: &Event) -> bool {
        false
    }

    /// R51.34 §5.15 + §5.35 — pointer-capture opt-in. When `true`, the
    /// framework's [`InputRouter`](crate#) keeps the cursor lock on
    /// this widget across the `pointer_down` → `pointer_up` span
    /// even when the cursor strays outside the widget rect (Material
    /// / `SwiftUI` / Qt gesture-recognizer convention) — `cursor_moved`
    /// forwards the cursor to the widget and **suppresses the
    /// `PointerLeave` that hover re-resolution would otherwise fire** for
    /// any stray, so a small jitter during the press cannot cancel it.
    ///
    /// R741 §5.35: button-like widgets (Button / Toggle / Checkbox /
    /// Radio) override this to `true` so a real-mouse click is robust to
    /// the sub-pixel jitter between press and release (before R741 they
    /// defaulted `false`, so a 1px stray fired `PointerLeave → Idle` and
    /// the click silently cancelled — the canonical toolkits all capture
    /// to avoid exactly this). They pair it with
    /// [`cancel_on_release_off_target`](Self::cancel_on_release_off_target)
    /// `= true` so a *deliberate* slide-off-and-release still cancels.
    ///
    /// Drag-aware widgets (Slider in R51.35, drag-to-resize, range
    /// pickers) override to `true` with the default
    /// `cancel_on_release_off_target = false` (release commits the value
    /// wherever the cursor ended). The router still dispatches
    /// `PointerDown` / `PointerUp` symbolic events to the widget via
    /// `ExternalIntrospect::invoke("send", ...)`; the difference is
    /// purely in the cursor-leave handling.
    fn wants_pointer_capture(&self) -> bool {
        false
    }

    /// R1405 §5.35 — opt into receiving [`pointer_move`](Self::pointer_move)
    /// on **plain hover** (no button held), not only under a capture-lock.
    ///
    /// By default the router forwards `pointer_move` to a widget only while it
    /// holds pointer capture (a drag); a free hover delivers only the
    /// `Enter` / `Leave` boundary events, never the intra-widget position. A
    /// widget that must react to *where inside it* the pointer is on hover —
    /// a `TextGrid` lighting the OSC-8 link cell under the cursor, a chart
    /// crosshair, a map tooltip — returns `true`, and the router then also
    /// forwards each hover move's position (the same rel-coord
    /// `pointer_move(x_rel, y_rel)` the capture path uses). Independent of
    /// [`wants_pointer_capture`](Self::wants_pointer_capture): a widget can
    /// want hover positions without capturing the press.
    fn wants_hover_move(&self) -> bool {
        false
    }

    /// R1416 §5.35 §5.15 — opt into the **raw multi-button pointer stream**:
    /// receive [`raw_pointer_button`](Self::raw_pointer_button) for EVERY mouse
    /// button (left / middle / right) on BOTH the press and release edge, each
    /// carrying the held modifiers, with the button identified.
    ///
    /// The default pipeline routes only the LEFT button to a widget (as the
    /// `send`-wire `PointerDown` / `PointerUp`), and its right / middle presses
    /// drive GUI *default actions* instead — a right press opens the
    /// own-renderer context menu, a middle press-release pastes the PRIMARY
    /// selection, a middle drag pans. A widget that IS the pointer authority
    /// for its region — a terminal pane forwarding xterm mouse reports, a game
    /// viewport, a remote-desktop surface — needs the raw edges, not those
    /// GUI interpretations. Returning `true` makes the router deliver each
    /// button edge to this widget through
    /// [`raw_pointer_button`](Self::raw_pointer_button) and **suppress the GUI
    /// default** for it (no context menu, no paste, no pan, no legacy
    /// `PointerDown` / `PointerUp` send wire).
    ///
    /// The suppression is scoped to THIS widget: every other widget keeps the
    /// standard button semantics (left = focus / select, middle = PRIMARY
    /// paste, right = context menu). Only the widget that owns the raw stream
    /// trades them for the raw edges — the W3C model, where a listener that
    /// handles `mousedown` / `contextmenu` opts out of the browser's default.
    ///
    /// Independent of, and usually paired with,
    /// [`wants_hover_move`](Self::wants_hover_move) (or
    /// [`wants_pointer_capture`](Self::wants_pointer_capture)): those forward
    /// the cursor POSITION a raw sink correlates each button edge against;
    /// this forwards the button EDGES. A raw sink wants both.
    fn wants_raw_pointer_buttons(&self) -> bool {
        false
    }

    /// R1416 §5.35 §5.15 — deliver one raw mouse-button edge to a widget that
    /// opted into the multi-button stream via
    /// [`wants_raw_pointer_buttons`](Self::wants_raw_pointer_buttons).
    ///
    /// Called for each left / middle / right press and release while this
    /// widget is the pointer target (the hover target under the cursor, or its
    /// captured target while it holds capture). The [`RawPointerButton`] carries
    /// the button, the press/release edge, and the modifiers held at that edge;
    /// the cursor POSITION arrives separately via
    /// [`pointer_move`](Self::pointer_move) (see the type docs). Default no-op,
    /// so a widget that does not opt in never sees this.
    fn raw_pointer_button(&mut self, _event: RawPointerButton) {}

    /// R741 §5.35 — release-position policy for a captured widget.
    /// Consulted only when [`wants_pointer_capture`](Self::wants_pointer_capture)
    /// is `true`. On `pointer_up`, the router checks whether the cursor
    /// is still over this widget:
    ///
    /// * `false` (default, drag widgets) — release always dispatches
    ///   `PointerUp` (the drag commits its value wherever the cursor
    ///   ended; a Slider released past the track edge still commits the
    ///   clamped value).
    /// * `true` (button-like widgets) — release **over** the widget
    ///   dispatches `PointerUp` (activate); release **off** the widget
    ///   dispatches `PointerLeave` (cancel). This is the standard
    ///   button "press, slide off to abort, release off = no-op"
    ///   gesture, made reachable now that capture suppresses the
    ///   mid-press leave.
    fn cancel_on_release_off_target(&self) -> bool {
        false
    }

    /// R880 §5.35 §5.49 — opt-in for the **bare** (non-composite) send wire
    /// to carry the R781 held-modifier token. When `true`, a background
    /// dispatch with a non-empty modifier state reaches `invoke("send", ...)`
    /// as the three-segment wire `":<EventName>:<token>"` — the key segment
    /// is *empty* (a background press has no sub-target), so
    /// [`split_send_payload`](crate::composite_tag::split_send_payload)
    /// decodes it through the exact same `:` grammar SSOT as a composite
    /// send, with `""` as the key. An empty modifier state always emits the
    /// plain `"<EventName>"` back-compat wire.
    ///
    /// Default `false`, because the bare payload doubles as the SCXML event
    /// name for the entire statechart-driven catalogue — an un-gated wire
    /// change would turn every `Ctrl`+click on a plain widget into an
    /// unmatchable `":PointerUp:c"` statechart event. Only a coordinator
    /// that decodes its own send wire (and needs background-release
    /// modifiers — e.g. a `Ctrl`/`Shift` marquee) should opt in, exactly as
    /// [`wants_pointer_capture`](Self::wants_pointer_capture) gates the
    /// capture machinery.
    fn wants_bare_send_modifiers(&self) -> bool {
        false
    }

    /// R738 §5.35 / R786 §5.35 — which post-layout rect the framework's
    /// [`InputRouter`](crate#) normalizes the dragged cursor against while this
    /// widget holds capture. One exhaustive [`CaptureNormalize`] decision —
    /// `Target` (default), `Primary`, or `Tag(name)` — so a widget cannot ask
    /// for two rects at once (the bool + `Option` pair this replaced could).
    ///
    /// - [`CaptureNormalize::Target`] (default): the grabbed (sub-)tag's own
    ///   rect — correct for a single-tag capture widget and for a composite
    ///   whose value is sub-region-relative (a dock panel's tear-off fraction).
    /// - [`CaptureNormalize::Primary`]: the primary half of the composite tag —
    ///   the dual-thumb range slider tags thumbs `range#low` / `range#high` but
    ///   the value maps across the whole track, so it normalizes against the
    ///   primary (track) rect instead of the ~18px thumb.
    /// - [`CaptureNormalize::Tag`]: an explicitly named element's rect — the
    ///   column-resize handle's drag is a **pixel** delta needing a rect whose
    ///   width is **stable across the drag**; the grabbed cell resizes under it,
    ///   so the handle names the grid viewport (which does not resize when a
    ///   column does), exactly as the splitter normalizes against its stable
    ///   pane container, not the moving handle.
    fn capture_normalize(&self) -> CaptureNormalize<'_> {
        CaptureNormalize::Target
    }

    /// R51.34 §5.15 + §5.35 — pointer-move forward during drag. The
    /// framework's [`InputRouter`](crate#) calls this whenever the
    /// cursor moves while this widget holds capture (i.e. after a
    /// `pointer_down` on a `wants_pointer_capture` = true widget and
    /// before the matching `pointer_up`). `x_rel` / `y_rel` are
    /// normalised over the widget's post-layout rect: `0.0` is the
    /// left / top edge, `1.0` is the right / bottom edge.
    ///
    /// Coordinates may exceed `[0.0, 1.0]` (or be negative) when the
    /// cursor strays outside the rect under capture lock — the
    /// implementor decides whether to clamp (Slider does, since its
    /// value is `0.0..=1.0` normalised) or extrapolate (a future
    /// fling / overscroll gesture might not).
    ///
    /// Default no-op; widgets that need cursor-position state
    /// override. Non-drag widgets must not override — capture-lock
    /// without `pointer_move` is a valid stance (e.g. a future
    /// long-press widget that only cares about the dwell time, not
    /// the cursor X).
    fn pointer_move(&mut self, _x_rel: f32, _y_rel: f32) {}

    /// R1423 §5.35 §5.15 — the current pointer PRESSURE for this widget, the W3C
    /// `PointerEvent.pressure` / Qt `QTabletEvent::pressure()` peer: a normalised
    /// `0.0..=1.0` force, `0.0` when no pressure is reported (a plain mouse, or a
    /// lifted pen). Forwarded alongside each [`pointer_move`](Self::pointer_move)
    /// (pressure travels WITH position, the W3C `pointermove` model) AND on a
    /// standalone pressure change (a pen pressing harder in place), so a
    /// pressure-aware surface — an ink brush whose width tracks force, a DCC
    /// viewport, a velocity-sensitive control — reads the live force without a
    /// separate device query.
    ///
    /// The native source is the platform pen / touch force (winit
    /// `Touch::force`, normalised); the AI-first source is the `scene/pointer_pressure`
    /// RPC (§2 #2), so the value is drivable and introspectable headless — a
    /// tablet is not required to exercise a pressure-reactive widget.
    ///
    /// Default no-op; only a widget that reacts to force overrides. A mouse
    /// reports `0.0` (Qt gives a mouse no `QTabletEvent` either — pressure is a
    /// pen/touch axis, not a synthesised mouse-button level).
    fn pointer_pressure(&mut self, _pressure: f32) {}

    /// R1429 §5.35 §5.15 — the current pointer TILT for this widget, the W3C
    /// `PointerEvent.tiltX` / `tiltY` / Qt `QTabletEvent::xTilt()` / `yTilt()`
    /// peer: the pen's lean off the surface normal, in DEGREES, each axis
    /// `-90.0..=90.0`. `tilt_x` is the lean in the device X-Z plane (positive =
    /// the pen top tilts toward +X / screen right); `tilt_y` in the Y-Z plane
    /// (positive = the pen top tilts toward +Y / screen bottom). `(0.0, 0.0)` is
    /// a pen held perpendicular, and what a plain mouse reports (a mouse has no
    /// tilt, exactly as it has no pressure). Forwarded alongside each
    /// [`pointer_move`](Self::pointer_move) (tilt travels WITH position, the W3C
    /// `pointermove` model) AND on a standalone tilt change (a pen leaning in
    /// place), so a tilt-aware surface — a calligraphy nib whose stroke shape
    /// follows the lean, a DCC viewport — reads the live angle without a separate
    /// device query.
    ///
    /// winit 0.30 exposes no tablet-tilt axis, so the sole driver is the
    /// `scene/pointer_tilt` RPC (§2 #2, the AI-first primary path): the value is
    /// drivable and introspectable headless, no tablet required. A future winit
    /// tablet API, or a platform bridge, would populate it natively — the same
    /// place [`pointer_pressure`](Self::pointer_pressure) reads winit
    /// `Touch::force` today.
    ///
    /// Default no-op; only a widget that reacts to lean overrides.
    fn pointer_tilt(&mut self, _tilt_x: f32, _tilt_y: f32) {}

    /// R1430 §5.35 §5.15 — the current pointer TWIST for this widget, the W3C
    /// `PointerEvent.twist` / Qt `QTabletEvent::rotation()` peer: the barrel
    /// rotation of an art pen about its own axis, in DEGREES clockwise,
    /// normalised `0.0..=360.0` (`0.0` = a plain pen / mouse, which has no barrel
    /// to turn). Forwarded WITH position like the tilt / pressure axes, so a
    /// twist-aware surface — a calligraphic nib whose broad edge follows the
    /// barrel, a pattern brush whose stamp rotates — reads the live angle.
    ///
    /// The sole driver is the `scene/pointer_twist` RPC (§2 #2): winit 0.30
    /// exposes no barrel-rotation axis, so the value is drivable / introspectable
    /// headless, no art pen required. Default no-op.
    fn pointer_twist(&mut self, _twist: f32) {}

    /// R1430 §5.35 §5.15 — the current pointer TANGENTIAL PRESSURE for this
    /// widget, the W3C `PointerEvent.tangentialPressure` / Qt
    /// `QTabletEvent::tangentialPressure()` peer: the airbrush finger-wheel
    /// position, normalised `-1.0..=1.0` (`0.0` = the wheel's neutral rest, and
    /// what a plain pen / mouse reports — it has no wheel). Forwarded WITH
    /// position like the other axes, so an airbrush-aware surface reads the live
    /// wheel without a device query.
    ///
    /// The sole driver is the `scene/pointer_tangential_pressure` RPC (§2 #2):
    /// winit 0.30 exposes no finger-wheel axis. Default no-op.
    fn pointer_tangential_pressure(&mut self, _tangential: f32) {}

    /// R1430 §5.35 §5.15 — the current pointer HEIGHT for this widget, the Qt
    /// `QTabletEvent::z()` peer: the pen's distance ABOVE the tablet surface
    /// while it hovers, `0.0` at contact and rising as the pen lifts (device
    /// units, non-negative — there is no W3C `PointerEvent` equivalent, so this
    /// is the Qt-parity axis). Forwarded WITH position like the other axes, so a
    /// hover-height-aware surface — a preview that fades as the pen lifts, a
    /// depth-cued brush cursor — reads the live distance.
    ///
    /// The sole driver is the `scene/pointer_height` RPC (§2 #2): winit 0.30
    /// exposes no hover-distance axis. Default no-op.
    fn pointer_height(&mut self, _height: f32) {}

    /// R1431 §5.35 §5.15 — the DEVICE that produced the current pointer stream
    /// for this widget, the W3C `PointerEvent.pointerType` / Qt
    /// `QTabletEvent::pointerType()` peer: [`PointerKind::Mouse`] / `Pen` /
    /// `Eraser` / `Touch`. `Mouse` is the default — what a plain pointer reports.
    /// The `Eraser` variant is the stylus's eraser end (a Qt distinction W3C folds
    /// into `"pen"`), so an eraser-aware surface — a paint canvas that flips to
    /// erase when the pen is inverted — reads the device without a query.
    /// Forwarded WITH position like the scalar axes.
    ///
    /// The sole driver is the `scene/pointer_type` RPC (§2 #2): winit 0.30 does
    /// not classify the pointer device. Default no-op.
    fn pointer_kind(&mut self, _kind: PointerKind) {}

    /// R1432 §5.35 §5.15 — a native PINCH (magnify) gesture over this widget,
    /// the Qt `QNativeGestureEvent` `ZoomNativeGesture` / macOS `magnify:` /
    /// W3C wheel-with-`Ctrl` peer: a two-finger trackpad pinch a viewport reads
    /// to zoom without a wheel or a button chord.
    ///
    /// * `x_rel` / `y_rel` — the cursor normalised over the SAME rect the
    ///   [`wheel`](Self::wheel) / [`pointer_move`](Self::pointer_move) hooks use
    ///   (a pinch has no position of its own; it targets whatever the cursor
    ///   hovers, so a zoom anchored at the cursor and a drag share one basis).
    /// * `magnification` — the INCREMENTAL scale delta for this event (winit's
    ///   `PinchGesture::delta` / the macOS `magnification`): positive zooms in,
    ///   negative zooms out, `0.0` at rest. It is a per-event increment, not an
    ///   absolute factor, so a surface accumulates it across the
    ///   [`Begin`](GesturePhase::Begin)`..`[`End`](GesturePhase::End) arc
    ///   (each event does `scale *= 1.0 + magnification`) and drops the
    ///   accumulator on [`Cancel`](GesturePhase::Cancel).
    /// * `phase` — the gesture lifecycle ([`GesturePhase`]) bracketing the arc.
    /// * `modifiers` — the held keyboard modifiers (the same out-of-band cache
    ///   the wheel reads), so a `Shift`-constrained or `Alt`-fine zoom is one
    ///   hook.
    ///
    /// Return `true` to consume the gesture, `false` (default) to decline — the
    /// same consume contract as [`wheel`](Self::wheel), though a pinch has no
    /// `Scene::Scroll` default action to fall through to (Qt delivers a native
    /// gesture only to the widget under the cursor, with no scroll fallback), so
    /// declining is simply a no-op.
    ///
    /// The native source is the platform trackpad (winit
    /// `WindowEvent::PinchGesture`, macOS / iOS only); the AI-first source is
    /// the `scene/pinch_gesture` RPC (§2 #2), so a zoom-reactive viewport is
    /// drivable and introspectable headless with no trackpad. Default no-op;
    /// only a widget that zooms overrides.
    fn pinch_gesture(
        &mut self,
        _x_rel: f32,
        _y_rel: f32,
        _magnification: f64,
        _phase: GesturePhase,
        _modifiers: crate::input::Modifiers,
    ) -> bool {
        false
    }

    /// R877 §5.15 §5.49 — wheel-input forward (the §5.15 item-5 input-
    /// forwarding leg the pointer hooks left open). The framework's
    /// [`InputRouter`](crate#) offers a wheel event to the `External`
    /// resolved from the tag under the cursor *before* falling back to
    /// the `Scene::Scroll` dispatch — the W3C model where a wheel
    /// listener on the event path may consume (`preventDefault`) ahead
    /// of the scroll default action; the offered widget can be an
    /// ancestor of a deeper `Scroll` (the canvas-hijack pattern), and
    /// declining preserves the inner scroll chain.
    ///
    /// * `x_rel` / `y_rel` — the cursor normalised over the SAME rect
    ///   [`capture_normalize`](Self::capture_normalize) selects for
    ///   [`pointer_move`](Self::pointer_move), so wheel-anchor math (a
    ///   canvas zoom anchored at the cursor) and drag math share one
    ///   coordinate basis.
    /// * `dx` / `dy` — the wheel delta in logical pixels (lines already
    ///   scaled by the framework's line height), W3C sign convention:
    ///   positive `dy` scrolls content downward.
    /// * `modifiers` — the held keyboard modifiers, so one hook covers
    ///   the canonical wheel vocabulary (plain = pan / scroll,
    ///   `Shift` = horizontal, `Ctrl` = zoom).
    ///
    /// Return `true` to consume the event (the router stops — no scroll
    /// dispatch); `false` to decline (default), letting the wheel fall
    /// through to the nearest [`Scene::Scroll`] ancestor exactly as
    /// before this hook existed. First consumer: the node-editor canvas
    /// (pan / `Ctrl`-zoom); the same shape serves a spin-box / slider
    /// wheel-step without another trait change.
    ///
    /// [`Scene::Scroll`]: crate::Scene::Scroll
    fn wheel(
        &mut self,
        _x_rel: f32,
        _y_rel: f32,
        _dx: f32,
        _dy: f32,
        _modifiers: crate::input::Modifiers,
    ) -> bool {
        false
    }

    // --- 5b. Drag-and-drop source / coordinator (R742 §5.51) ---

    /// R742 §5.51 — drag-source hook. The framework's
    /// [`InputRouter`](crate#) calls this immediately after it dispatches
    /// `PointerDown` to this widget. Returning `Some(payload)` **starts a
    /// drag session**: the router pins this pointer's hover (so the
    /// statechart sees no spurious `PointerLeave` mid-drag, exactly like
    /// capture) and, on every subsequent cursor move, resolves the drop
    /// location under the *absolute* cursor and forwards it back to this
    /// widget via [`drag_to_at`](Self::drag_to_at) (the rect-relative
    /// [`DropPoint`] **plus** the absolute window-logical cursor; its default
    /// delegates to the cursor-less [`drag_to`](Self::drag_to)), then once
    /// via [`drag_release_at`](Self::drag_release_at) on the matching
    /// `pointer_up`.
    ///
    /// Why the *source* receives the updates, not the hovered target: an
    /// `External` only ever sees rect-relative coordinates and the router
    /// routes the whole press → release gesture to the pressed widget, so
    /// no widget can resolve "what is under the cursor" on its own — only
    /// the router holds the absolute cursor plus the full paint layout.
    /// The router does the hit-test and hands the resolved [`DropPoint`]
    /// to the coordinator that started the drag. Every drop candidate for
    /// the in-tree reorder consumers (reorder list, tab bar, dock, tree)
    /// belongs to that one coordinator, so the source *is* the resolver —
    /// the pointer-driven generalisation of the invoke-driven dock
    /// `resolve_dock_drop`. A future cross-widget drop (palette → canvas)
    /// adds a target-side hook without changing this source contract.
    ///
    /// Called on `&self`: arming is observation of state the matching
    /// `PointerDown` already recorded (which sub-region was pressed), not
    /// a mutation. Default `None` — the widget is not a drag source and
    /// no session starts, so every pre-R742 `External` is unaffected.
    fn begin_drag(&self) -> Option<DragPayload> {
        None
    }

    /// (R1348 §5.51 PR-57) Does this drag source ACCEPT the synthetic
    /// [`OUTER_DOCK_ZONE_TAG`] perimeter zone the router is about to CLAIM at
    /// `point`? `false` ⇒ the router does not claim it and the plain inner
    /// hit-test applies instead, exactly as in the band's interior — so the
    /// widget UNDER the perimeter keeps its own drop bands. Default `true`
    /// (claim as before), so every pre-R1348 `External` is unaffected: the
    /// router only ASKS on the same-window override, which is gated on
    /// [`DOCK_PANEL_DRAG_KIND`]. (That gate is not a claim that a non-dock drag
    /// never meets the sentinel — the CROSS-window perimeter resolver is
    /// drag-kind-BLIND and can put [`OUTER_DOCK_ZONE_TAG`] in a non-dock drag's
    /// `over`. It is unvetoable today; see the carry on
    /// `InputRouter::resolve_drag_own_over`.)
    ///
    /// **Why the SOURCE answers.** Whether a perimeter zone is worth claiming is
    /// a question about the source's own model (for a dock: "does an outer band
    /// at that edge reach any arrangement an inner split does not?"), which the
    /// router cannot answer — it holds geometry, not topology, and the crate that
    /// owns the topology is its SIBLING, not its dependency. This is the
    /// [`begin_drag`](Self::begin_drag) contract carried one step further: the
    /// source *is* the resolver, so the source also decides which synthetic
    /// targets are offered to it. The router asks BEFORE stealing the hit-test,
    /// so a zone the source would only reject cannot be claimed in the first
    /// place.
    ///
    /// **The invariant this closes.** R1201 declared the VS Code / Qt ADS rule —
    /// *an outer drop indicator is offered only when the outcome differs* — but
    /// enforced it one layer too LATE, at RESOLVE (`resolve_drop_checked` mapped a
    /// redundant perimeter drop to a stay-put `SnapBack`). The claim still
    /// happened, so the outcome died while the CLAIM survived: the band previewed
    /// nothing, did nothing, and masked the split bands of the panel beneath it —
    /// a dead strip. A source that answers this the same way it resolves makes
    /// "claimed but inert" unrepresentable, rather than merely unwanted.
    /// Implementors MUST therefore answer with the SAME predicate their
    /// `drag_release` resolves with, so claim and outcome cannot drift.
    fn accepts_outer_dock(&self, _payload: &DragPayload, _point: &DropPoint) -> bool {
        true
    }

    /// R742 §5.51 — live drag update. Called on every cursor move while a
    /// session this widget started via [`begin_drag`](Self::begin_drag)
    /// is in flight. `over` is the drop location currently under the
    /// cursor, or `None` when the cursor is over no tagged region. The
    /// widget updates its drop-preview state (e.g. the insertion index a
    /// reorder list highlights) — typically by writing a shared
    /// `Rc<Signal<_>>` the view fn also reads, so the highlight
    /// re-renders reactively. Default no-op.
    fn drag_to(&mut self, _payload: &DragPayload, _over: Option<DropPoint>) {}

    /// R742 §5.51 — drop commit. Called once on `pointer_up` with the
    /// final drop location (`None` when released over no tagged region).
    /// The widget applies the move / reorder and clears its drop-preview
    /// state. The router *also* dispatches the normal `PointerUp` to the
    /// source afterwards, so a press-release-in-place (no real drag) still
    /// reaches the statechart as a click. Default no-op.
    fn drag_release(&mut self, _payload: &DragPayload, _over: Option<DropPoint>) {}

    /// R1093 §5.15 §5.51 — live drag update WITH the full [`DragUpdate`]
    /// context. The §5.15 input-forwarding ENRICHMENT (not a break):
    /// [`drag_to`](Self::drag_to) forwards only the rect-relative [`DropPoint`]
    /// — which is `None` the moment the cursor escapes every tagged region,
    /// exactly the dock tear-off case — so a coordinator that must place
    /// something **at the cursor** (a floating window that follows the pointer)
    /// had no way to read where the cursor actually is, whether the press has
    /// become a drag, or which window the cursor escaped into. The router calls
    /// THIS method on every move with all of that ([`DragUpdate::cursor`] /
    /// [`became_drag`](DragUpdate::became_drag) / [`over_window`](DragUpdate::over_window));
    /// its default delegates to [`drag_to`](Self::drag_to) with just the
    /// rect-relative `over`, so every pre-R1093 drag source is bit-identical and
    /// no widget receives both calls. Override this **instead of** `drag_to` to
    /// receive the context. (R1101 collapsed the former positional `cursor` /
    /// `over_window` args into [`DragUpdate`]; see that struct for why a drag
    /// source consumes [`became_drag`](DragUpdate::became_drag) rather than
    /// re-deriving it.)
    fn drag_to_at(&mut self, payload: &DragPayload, update: &DragUpdate) {
        self.drag_to(payload, update.over.clone());
    }

    /// R1093 §5.15 §5.51 — drop commit WITH the full [`DragUpdate`] context,
    /// the release sibling of [`drag_to_at`](Self::drag_to_at). The router calls
    /// this once on `pointer_up`; its default delegates to
    /// [`drag_release`](Self::drag_release) with just `over` so pre-R1093
    /// sources are unaffected. A coordinator that opens a floating window where
    /// the drag was released reads [`DragUpdate::cursor`] here (in the SOURCE
    /// window's logical frame; converting to a desktop position additionally
    /// needs the source window's outer position, which the shell owns) and
    /// [`over_window`](DragUpdate::over_window) to redock into another window.
    fn drag_release_at(&mut self, payload: &DragPayload, update: &DragUpdate) {
        self.drag_release(payload, update.over.clone());
    }

    /// R937.1 §5.51 — drag ABORT. Called once when the OS revokes an
    /// in-flight drag this widget started (winit `TouchPhase::Cancelled` —
    /// a system gesture, phone call, app-switcher, edge-swipe), the
    /// drag-session counterpart of [`pointer_cancel`]. Unlike
    /// [`drag_release`](Self::drag_release), the widget must **discard** the
    /// gesture (NO move / reorder applied) and clear its drop-preview state —
    /// a cancel is "the drag never happened", so committing the last preview
    /// would be wrong. Default no-op (a widget with no preview / arm state to
    /// clear needs nothing). The router clears the drag session regardless,
    /// so even a no-op impl can no longer leave a dangling session.
    ///
    /// [`pointer_cancel`]: ../../pinion_runtime/input/struct.InputRouter.html#method.pointer_cancel
    fn drag_cancel(&mut self, _payload: &DragPayload) {}

    // --- 6. DPI / resize notification ---

    fn on_dpi_change(&mut self, _scale: f32) {}
    fn on_resize(&mut self, _width: u32, _height: u32) {}

    // --- 7. Async state change channel (pull form) ---

    /// Poll for a state change pushed by the `External`. Default
    /// returns `None`. Push-form (channel-based) lands when §6.3 async
    /// boundary is settled at the runtime crate edge.
    fn poll_state(&mut self) -> Option<StateUpdate> {
        None
    }

    // --- §5.20 intent channel (R18; complements item 7's state-update
    //     poll with a symbolic event stream). ---

    /// Drain any pending [`Intent`]s into `sink`. Default no-op so
    /// existing `External` authors are unaffected. Implementors that
    /// emit intents (e.g. a button whose state machine just clicked)
    /// override this to flush their internal queue.
    fn drain_intents(&mut self, _sink: &mut dyn FnMut(Intent)) {}

    /// Return `true` when this `External` has pending intents the
    /// runtime should drain on the current frame. Default `false`.
    /// Used to skip the [`drain_intents`](Self::drain_intents) virtual
    /// call when there is nothing to harvest.
    fn is_dirty(&self) -> bool {
        false
    }

    // --- 8. Optional symbolic introspection (opt-in per §5.15 caveat) ---

    /// Surface the [`ExternalIntrospect`] view of this `External`, when
    /// the author opts in. Default returns `None`; override with
    /// `Some(self)` after `impl ExternalIntrospect for YourType`.
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        None
    }

    /// Mutable counterpart to [`introspect`](Self::introspect), used by
    /// the §5.12 `rewind` and `dry_run` paths.
    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        None
    }
}

/// R810.1 §5.38 §5.12 — a reactive state holder that exposes a
/// **read-only** introspection view of itself: its declared schema and
/// the live value at each path. Implement this on a widget's reactive
/// holder (e.g. `ModalState`, `SnackbarTimer`) and wrap it in a
/// [`QueryOnlyIntrospect`] to get a query-only RPC node — no hand-rolled
/// `External` boilerplate, no second source of truth.
///
/// The "read-only" contract is enforced by [`QueryOnlyIntrospect`], not
/// here: any path in `introspect_schema` is
/// refused on `intervene` with [`InterveneError::ReadOnly`]. This is the
/// right shape when the state is *driver-coupled* — a modal's open flag
/// moves with its focus-trap, a snackbar's countdown is advanced by the
/// animation driver — so a raw rewind would desync it. Mutations go
/// through the holder's own methods (a reducer / action), never the wire.
/// R834 §5.12 — widen a `usize` count to the `i64` an
/// [`IntrospectValue::Int`] slot carries, saturating at [`i64::MAX`]
/// rather than wrapping or panicking. The single SSOT for the
/// introspect-count-widening decision (was hand-rolled in `widgets::table`
/// + two example bindings).
#[must_use]
pub fn int_of(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

pub trait QuerySource {
    /// The declared query paths and their type-name tags. Usually a
    /// `'static` slice independent of `self` (the schema is fixed per
    /// type), kept in lockstep with [`introspect_query`](Self::introspect_query).
    fn introspect_schema(&self) -> IntrospectSchema;

    /// The live value at `path`, or `None` for an undeclared path.
    fn introspect_query(&self, path: &str) -> Option<IntrospectValue>;
}

/// R847 §5.15 §5.40 — emit the [`External`] skeleton shared by every
/// **display-config proxy coordinator** (the sort/filter proxy
/// [`ViewSortFilterExternal`](crate::widgets::view_order::ViewSortFilterExternal),
/// the grid-sort proxy
/// [`GridSortExternal`](crate::widgets::grid_sort::GridSortExternal), the
/// group-by proxy
/// [`GroupOrderExternal`](crate::widgets::group_order::GroupOrderExternal)): a
/// `Gui`+`Rpc` backend, framework-owned repaint, UI-thread external that emits
/// **no §5.20 intent** (its state is a shared reactive holder whose `Signal`
/// writes already repaint every subscriber, so it is never independently dirty)
/// and surfaces that state through [`ExternalIntrospect`] only.
///
/// These five methods were byte-identical across all three proxies (the R847
/// audit's Rule-of-Three); this macro is their SSOT, so a change to (say)
/// [`thread_ownership`](External::thread_ownership) cannot silently diverge
/// between them. The type must implement [`ExternalIntrospect`] (the macro's
/// `introspect` / `introspect_mut` return `Some(self)`); each proxy keeps its
/// own `ExternalIntrospect` impl (the part that genuinely differs).
///
/// R858 — exported (`pub use` below) so a `Gui`+`Rpc` config-holder External in
/// an *example* (e.g. `hello-tree-filter`'s `TreeSortExternal`) reaches the same
/// SSOT instead of hand-rolling the skeleton:
/// `pinion_core::external::query_proxy_external_impl!(MyExternal);`.
#[macro_export]
macro_rules! query_proxy_external_impl {
    ($t:ty) => {
        impl $crate::external::External for $t {
            fn backends(&self) -> $crate::external::BackendSupport {
                $crate::external::BackendSupport::new(
                    &[
                        $crate::external::Backend::Gui,
                        $crate::external::Backend::Rpc,
                    ],
                    $crate::external::BackendFallback::Skip,
                )
            }
            fn repaint_ownership(&self) -> $crate::external::RepaintOwner {
                $crate::external::RepaintOwner::Framework
            }
            fn thread_ownership(&self) -> $crate::external::ThreadOwnership {
                $crate::external::ThreadOwnership::UiThreadSync
            }
            fn introspect(&self) -> Option<&dyn $crate::external::ExternalIntrospect> {
                Some(self)
            }
            fn introspect_mut(&mut self) -> Option<&mut dyn $crate::external::ExternalIntrospect> {
                Some(self)
            }
        }
    };
}
pub use query_proxy_external_impl;

/// The `intervene` error for a path with **no writable slot**: `ReadOnly` when
/// `schema` declares it (a real field, just not writable), `UnknownPath` when it
/// does not.
///
/// The §2 #7 distinction between "you cannot write this" and "this does not
/// exist" is worth keeping honest — an agent that gets `UnknownPath` for a field
/// it can plainly `query` learns something false about the surface. Every
/// read-only External needs exactly this rule, so it lives here rather than being
/// re-typed: `QueryOnlyIntrospect` below and `pinion-audio`'s RT External (whose
/// own copy admitted, in its docstring, to mirroring this one) both route through
/// it.
/// (R1353) Parametric members resolve through [`SchemaField::addresses`], so
/// `width.0` reports `ReadOnly` like the family it belongs to. Matching the
/// declared path exactly would have answered `UnknownPath` for it — telling an
/// agent that a path it can plainly `query` does not exist, which is the precise
/// lie this function was written to prevent.
#[must_use]
pub fn read_only_or_unknown(schema: &IntrospectSchema, path: &str) -> InterveneError {
    if schema.field_for(path).is_some() {
        InterveneError::ReadOnly
    } else {
        InterveneError::UnknownPath
    }
}

/// Move an inner `External`'s pending §5.20 [`Intent`]s into `sink`.
///
/// For the **wrapper** shape: an External that delegates to an inner one has to
/// forward the intents the inner queued, or they are simply lost. Written inline
/// that is not one line but three, because `src` and `sink` are usually two fields
/// of the same wrapper and the borrow checker needs them destructured apart:
///
/// ```ignore
/// let result = self.inner.invoke(path, args);
/// let Self { inner, pending_intents, .. } = &mut *self;   // <- only to split the borrow
/// inner.drain_intents(&mut |intent| pending_intents.push(intent));
/// ```
///
/// Taking the two as separate arguments splits the borrow at the call site, so
/// the dance disappears:
///
/// ```ignore
/// let result = self.inner.invoke(path, args);
/// forward_intents(&mut self.inner, &mut self.pending_intents);
/// ```
///
/// Lifted at the fourth byte-identical copy (three example bindings).
pub fn forward_intents(src: &mut impl External, sink: &mut Vec<Intent>) {
    src.drain_intents(&mut |intent| sink.push(intent));
}

/// R1276 §5.15 — the `External` skeleton for a **read-write introspection**
/// node: RPC-only (paints nothing — the binding's `view` is the paint
/// scene), framework repaint, UI-thread sync, and it drains a §5.20 intent
/// channel. The sibling of [`query_proxy_external_impl`] for nodes that also
/// `invoke` / `intervene` and emit intents (a cursor walk, an audio
/// controller), rather than being purely query-only.
///
/// The type must have a `pending_intents: Vec<Intent>` field (the macro's
/// `drain_intents` / `is_dirty` read it) and its own [`ExternalIntrospect`]
/// impl (the part that genuinely differs). This is the SSOT for the
/// otherwise byte-identical skeleton that `pinion-narrative`'s
/// `NarrativeExternal` and `pinion-audio`'s `AudioEngineExternal` had each
/// hand-rolled (the Rule-of-Three lift; the read-only case has
/// [`QueryOnlyIntrospect`]).
///
/// # Declaring thread ownership (§5.15 item 3)
///
/// The one-argument form declares [`ThreadOwnership::UiThreadSync`], which is
/// right for the common case — an External the framework calls straight from the
/// UI thread. An External that **owns a real OS thread** and is spoken to over a
/// channel (an audio device callback, say) must say so, or it lies about a
/// mandatory §5.15 item:
///
/// ```ignore
/// pinion_core::intent_query_external_impl!(DeviceAudioExternal, OwnThread);
/// ```
///
/// The name is any [`ThreadOwnership`] variant.
#[macro_export]
macro_rules! intent_query_external_impl {
    ($t:ty) => {
        $crate::intent_query_external_impl!($t, UiThreadSync);
    };
    ($t:ty, $thread_ownership:ident) => {
        impl $crate::external::External for $t {
            fn backends(&self) -> $crate::external::BackendSupport {
                $crate::external::BackendSupport::new(
                    &[$crate::external::Backend::Rpc],
                    $crate::external::BackendFallback::Skip,
                )
            }
            fn repaint_ownership(&self) -> $crate::external::RepaintOwner {
                $crate::external::RepaintOwner::Framework
            }
            fn thread_ownership(&self) -> $crate::external::ThreadOwnership {
                $crate::external::ThreadOwnership::$thread_ownership
            }
            fn introspect(&self) -> Option<&dyn $crate::external::ExternalIntrospect> {
                Some(self)
            }
            fn introspect_mut(&mut self) -> Option<&mut dyn $crate::external::ExternalIntrospect> {
                Some(self)
            }
            fn drain_intents(&mut self, sink: &mut dyn FnMut($crate::intent::Intent)) {
                for intent in self.pending_intents.drain(..) {
                    sink(intent);
                }
            }
            fn is_dirty(&self) -> bool {
                !self.pending_intents.is_empty()
            }
        }
    };
}
pub use intent_query_external_impl;

/// R810.1 §5.38 §5.12 — the generic **query-only** introspection
/// `External`: a node that paints nothing (RPC backend only), handles no
/// events, and forwards `schema` / `query` to its [`QuerySource`] while
/// refusing every `intervene` (read-only). It lifts the byte-identical
/// `External` boilerplate that `ModalIntrospect` (R795) and
/// `SnackbarIntrospect` (R810) had each hand-rolled — the
/// [[abstraction-needs-second-consumer]] payoff, made now rather than at
/// the 3rd consumer because pinion's AI-introspection thesis guarantees
/// every transient widget grows one of these. Bindings register it via a
/// thin `*_introspection_extra(tag, state)` helper
/// (`ExtraExternal::new(tag, Box::new(QueryOnlyIntrospect::new(state)))`).
#[derive(Debug)]
pub struct QueryOnlyIntrospect<S> {
    source: Rc<S>,
}

impl<S> QueryOnlyIntrospect<S> {
    /// Wrap a shared [`QuerySource`] as its query-only introspection
    /// node. The `Rc` is cloned, so the view / driver / this node all
    /// report the same live state.
    #[must_use]
    pub fn new(source: Rc<S>) -> Self {
        Self { source }
    }
}

impl<S: QuerySource + core::fmt::Debug + 'static> External for QueryOnlyIntrospect<S> {
    /// RPC-only: the node carries no pixels (the binding paints the real
    /// surface), so the visual backends skip it while §5.12 `query` still
    /// routes through it.
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn handles_event(&self, _event: &Event) -> bool {
        false
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl<S: QuerySource + core::fmt::Debug + 'static> ExternalIntrospect for QueryOnlyIntrospect<S> {
    fn schema(&self) -> IntrospectSchema {
        self.source.introspect_schema()
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        self.source.introspect_query(path)
    }

    /// Every declared slot is read-only; an undeclared path is
    /// `UnknownPath`. The schema is the single source of "which paths
    /// exist", so a slot can never drift between `query` and `intervene`.
    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(read_only_or_unknown(&self.source.introspect_schema(), path))
    }
}

/// Reference no-op `External`: Gui only, framework-driven repaint,
/// UI-thread synchronous. Useful as a baseline for tests and as a
/// minimal example for new `External` authors.
#[derive(Debug, Clone, Copy, Default)]
pub struct StubExternal;

impl StubExternal {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl External for StubExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
}

/// Reference `External` opting *in* to symbolic introspection (§5.15
/// item 8). Exposes a single `count: int` slot via the [`ExternalIntrospect`]
/// trait; useful as a worked example and as a fixture for the §5.12
/// `query` / `rewind` RPC methods once they wire up.
///
/// Additionally demonstrates the §5.20 intent channel: every successful
/// `intervene` write enqueues a `"counted.changed"` intent carrying the
/// new value, which `drain_intents` / `is_dirty` flush into the runtime
/// queue. Keeps the existing fixture role intact (no `Copy` removal
/// breaks any test using `Box<dyn External>` storage).
#[derive(Debug, Clone, Default)]
pub struct CountedExternal {
    pub count: i64,
    pending_intents: Vec<Intent>,
}

impl CountedExternal {
    #[must_use]
    pub const fn new(count: i64) -> Self {
        Self {
            count,
            pending_intents: Vec::new(),
        }
    }
}

impl External for CountedExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        for intent in self.pending_intents.drain(..) {
            sink(intent);
        }
    }

    fn is_dirty(&self) -> bool {
        !self.pending_intents.is_empty()
    }
}

impl ExternalIntrospect for CountedExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(const { &[SchemaField::new("count", "int")] })
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "count" => Some(IntrospectValue::Int(self.count)),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "count" => match value {
                IntrospectValue::Int(n) => {
                    self.count = n;
                    self.pending_intents.push(Intent::new_static(
                        "counted.changed",
                        IntrospectValue::Int(n),
                    ));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Action: add the integer arg to the running count and
            // return the new total. Demos the §5.15 invoke triad with
            // a minimal mutating action that returns a computed value.
            "increment" => match args {
                IntrospectValue::Int(delta) => {
                    self.count = self.count.saturating_add(delta);
                    Ok(IntrospectValue::Int(self.count))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::WindowEvent;

    /// R1353 §2 #2 — a declared arity that does not match the `query` impl is
    /// worse than no declaration: it is a confident lie on the one surface an
    /// agent is told to trust. These pin the two directions the declaration can
    /// be wrong, on a real widget.
    #[test]
    fn r1353_declared_arity_matches_the_real_query_impl() {
        use crate::widgets::column_widths::{ColumnWidthExternal, ColumnWidths};
        use std::rc::Rc;

        let ext = ColumnWidthExternal::new(Rc::new(ColumnWidths::new(vec![100, 200, 300])));
        let schema = ext.schema();

        // Direction 1: a PARAMETRIC declaration must not be answerable bare, and
        // must be answerable with an argument. (`width` declared parametric but
        // secretly readable as a scalar would make the arg a lie.)
        let width = schema
            .fields
            .iter()
            .find(|f| f.path == "width.<col>")
            .expect("width is declared");
        let [arg] = width.args else {
            panic!("width declares exactly one argument");
        };
        assert_eq!(width.path, "width.<col>", "the declared wire template");
        assert!(
            ext.query("width").is_none(),
            "a parametric stem must not answer bare",
        );
        assert!(ext.query("width.0").is_some(), "…but answers with an arg");

        // Direction 2: the declared DOMAIN must be true. `IndexOf("cols")`
        // promises that `cols` is readable and that exactly `0..cols` answer —
        // the promise a client plans against instead of probing.
        let ArgDomain::IndexOf(count_path) = arg.domain else {
            panic!("width's domain is IndexOf");
        };
        let Some(IntrospectValue::Int(cols)) = ext.query(count_path) else {
            panic!("the declared count_path {count_path:?} must itself be readable");
        };
        assert_eq!(cols, 3);
        for col in 0..cols {
            assert!(
                ext.query(&format!("width.{col}")).is_some(),
                "every index below the declared count answers ({col})",
            );
        }
        assert!(
            ext.query(&format!("width.{cols}")).is_none(),
            "and the first index at/above it does NOT — an out-of-range read that \
             answered would be the fabricated value R1353 removed",
        );

        // Direction 3: a SCALAR declaration must answer bare. (`total` declared
        // scalar but secretly needing an arg is the original defect inverted.)
        assert!(
            schema
                .fields
                .iter()
                .find(|f| f.path == "total")
                .expect("total is declared")
                .args
                .is_empty(),
        );
        assert!(ext.query("total").is_some(), "a scalar answers bare");
    }

    /// R1353.1 §2 #2 — run a parametric widget's DECLARED arity against its real
    /// `query` impl.
    ///
    /// **Coverage is the widgets listed at the bottom, not all of them.** 32
    /// files declare a `parametric` field; this audits the ones `pinion-core` can
    /// construct, and a widget in another crate (`pinion-widget-paint`,
    /// `pinion-audio`, every example) cannot be reached from here at all. R1353's
    /// first cut claimed "every parametric widget" while auditing six — a test
    /// that names a coverage it does not have is worse than no test, because it
    /// answers the question "is this checked?" with a lie.
    ///
    /// What IS workspace-wide is the static half:
    /// `r1353_1_every_real_declaration_matches_its_template` scans every real
    /// declaration's source. This is the dynamic half — it needs a live widget,
    /// so it goes as far as linkage allows. Adding widget N+1 here is a manual
    /// line; the honest fix is a per-crate audit each crate runs over its own
    /// externals, which nothing has forced yet.
    #[test]
    fn r1353_declared_domains_hold_on_real_widgets() {
        use crate::widgets::column_widths::{ColumnWidthExternal, ColumnWidths};
        use crate::widgets::disclosure_group::DisclosureGroupExternal;
        use crate::widgets::listbox::ListBoxExternal;
        use crate::widgets::pagination::PaginationExternal;
        use crate::widgets::radio_group::RadioGroupExternal;
        use crate::widgets::row_search::{RowSearchExternal, RowSearchState};
        use crate::widgets::row_style::{RowStyleExternal, RowStyleState};
        use crate::widgets::table::TableExternal;
        use crate::widgets::toolbar::{ToolItem, ToolbarExternal};
        use std::rc::Rc;

        /// Assert every declared field of `ext` tells the truth about itself.
        fn audit(label: &str, ext: &dyn ExternalIntrospect) {
            for f in ext.schema().fields {
                if f.args.is_empty() {
                    // A scalar promises it reads as spelled. (`send` / action
                    // slots legitimately read `None` — they are write channels —
                    // so absence alone is not a defect here; the claim under test
                    // is only that a scalar never needs an argument.)
                    assert!(
                        !f.path.contains('<'),
                        "{label}: {:?} spells a placeholder but declares no args",
                        f.path,
                    );
                    continue;
                }
                // NOTE: R1353's first cut also asserted "a parametric field's
                // stem is not readable". It never caught anything: `Table`
                // legitimately declares BOTH a scalar `selected` ("is anything
                // selected") and a family `selected.<row>`, so the check needed a
                // special-case to pass at all, and against a verbatim
                // `literal_prefix` ("selected.") it is vacuous — that never reads.
                // Removed rather than kept as reassurance. The three checks below
                // are the ones that pin a real promise.
                for a in f.args {
                    let ArgDomain::IndexOf(count_path) = a.domain else {
                        // `ValuesOf` / `Open` are audited by their own surfaces;
                        // only `IndexOf` makes a bound claim this test can check.
                        continue;
                    };
                    // The promise: `count_path` is itself readable, and it is an
                    // int. A domain pointing at a path that does not exist is a
                    // dead end a client cannot follow.
                    let Some(IntrospectValue::Int(n)) = ext.query(count_path) else {
                        panic!(
                            "{label}: {:?} declares domain IndexOf({count_path:?}), \
                             but that path does not read as an int",
                            f.path,
                        );
                    };
                    // A single-arg family is fully checkable: every index below
                    // the count answers, and the first one at it does not.
                    if f.args.len() == 1 && n > 0 {
                        let inside = f.path.replace(&format!("<{}>", a.name), "0");
                        assert!(
                            ext.query(&inside).is_some(),
                            "{label}: {inside:?} is inside the declared domain but \
                             does not answer",
                        );
                        // Outside the declared domain, a read must not produce a
                        // VALUE. Two spellings of that are already in the tree and
                        // both are honest: `None` from the surfaces that guard the
                        // index explicitly, and `Some(Null)` from everything routed
                        // through `at_index`. (See `ExternalIntrospect::query`; the
                        // rosters there are examples, not an exhaustive list.) The
                        // invariant that matters is neither of those; it is that
                        // nothing plausible comes back. `width.999` answering `40`
                        // — a real-looking width for a column that does not exist —
                        // is what R1353 removed, and it is what this catches.
                        let outside = f.path.replace(&format!("<{}>", a.name), &n.to_string());
                        let answer = ext.query(&outside);
                        assert!(
                            matches!(answer, None | Some(IntrospectValue::Null)),
                            "{label}: {outside:?} is OUTSIDE the declared domain \
                             (count={n}) yet answered {answer:?} — a client that \
                             trusts the declaration cannot tell that apart from a \
                             real value",
                        );
                    }
                }
            }
        }

        audit(
            "column_widths",
            &ColumnWidthExternal::new(Rc::new(ColumnWidths::new(vec![100, 200, 300]))),
        );
        audit("listbox", &ListBoxExternal::new(4));
        audit("radio_group", &RadioGroupExternal::new(3));
        audit("disclosure_group", &DisclosureGroupExternal::new(3));
        audit(
            "row_search",
            &RowSearchExternal::new(Rc::new(RowSearchState::new(
                2,
                vec![vec!["a".into(), "b".into()], vec!["c".into(), "d".into()]],
            ))),
        );
        audit(
            "toolbar",
            &ToolbarExternal::new(vec![ToolItem::Command, ToolItem::Toggle]),
        );
        audit("pagination", &PaginationExternal::new(4, 0));
        // `with_source` is what makes `match.<row>` / `tint.<row>` answerable at
        // all, so the audit must use it — a bare `RowStyleExternal` has
        // `row_count = 0` and the domain check would skip.
        audit(
            "row_style",
            &RowStyleExternal::new(Rc::new(RowStyleState::new()))
                .with_source(3, |_row| vec!["a".to_string()]),
        );
        audit(
            "table",
            &TableExternal::new(
                vec!["a".into(), "b".into()],
                vec![vec!["1".into(), "2".into()], vec!["3".into(), "4".into()]],
            ),
        );
    }

    /// R1353.1 — the template's `<placeholders>` must match `args` in name and
    /// order **at every real declaration in the workspace**, not just the
    /// fixtures below.
    ///
    /// A source-text scan, for the same reason
    /// `widgets::commit`'s literal guard is one: no runtime assertion can see a
    /// declaration in a crate this one does not depend on, and a `const fn`
    /// cannot parse its own path string. R1353's first cut asserted this about
    /// four literals it wrote itself while `SchemaField::parametric`'s doc
    /// claimed the invariant was "enforced" — a green test named for a coverage
    /// it did not have, which is worse than no test. The invariant the whole
    /// model rests on (that `"cell.<row>.<col>"` really does take `row` then
    /// `col`) now gets checked where it is actually written.
    #[test]
    fn r1353_1_every_real_declaration_matches_its_template() {
        fn placeholders(t: &str) -> Vec<String> {
            let mut out = Vec::new();
            let mut rest = t;
            while let Some(o) = rest.find('<') {
                rest = &rest[o + 1..];
                let Some(c) = rest.find('>') else { break };
                out.push(rest[..c].to_string());
                rest = &rest[c + 1..];
            }
            out
        }
        // `parametric("<template>", "<ty>", const { &[SchemaArg::<k>("<name>", ..), ..] })`
        // — matched textually across the workspace, including crates and examples
        // that pinion-core cannot link against.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root is two levels above this crate");
        let mut checked = 0usize;
        let mut offenders: Vec<String> = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    let name = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if !matches!(name.as_str(), "target" | "vendor" | ".git") {
                        stack.push(p);
                    }
                    continue;
                }
                if p.extension().is_none_or(|x| x != "rs") {
                    continue;
                }
                let Ok(src) = std::fs::read_to_string(&p) else {
                    continue;
                };
                for (i, _) in src.match_indices("SchemaField::parametric(") {
                    let tail = &src[i..];
                    // The declaration ends at its closing `)`, which the nested
                    // `const { &[..] }` makes the LAST one before the next
                    // declaration — bound the window at the next `SchemaField::`
                    // to stay inside this literal.
                    let end = tail[1..]
                        .find("SchemaField::")
                        .map_or(tail.len(), |n| n + 1);
                    let win = &tail[..end];
                    let Some(t0) = win.find('"') else { continue };
                    let Some(t1) = win[t0 + 1..].find('"') else {
                        continue;
                    };
                    let template = &win[t0 + 1..t0 + 1 + t1];
                    let names: Vec<String> = win
                        .match_indices("SchemaArg::")
                        .filter_map(|(j, _)| {
                            let a = &win[j..];
                            let s = a.find('"')?;
                            let e = a[s + 1..].find('"')?;
                            Some(a[s + 1..s + 1 + e].to_string())
                        })
                        .collect();
                    checked += 1;
                    if placeholders(template) != names {
                        offenders.push(format!(
                            "{}: template {template:?} declares {:?} but its args are {names:?}",
                            p.file_name().unwrap_or_default().to_string_lossy(),
                            placeholders(template),
                        ));
                    }
                }
            }
        }
        assert!(
            checked > 100,
            "the scan must actually reach the workspace's declarations; saw {checked}",
        );
        assert!(
            offenders.is_empty(),
            "{} declaration(s) whose template and args disagree:\n{}",
            offenders.len(),
            offenders.join("\n"),
        );
    }

    /// R1353 — the template's `<placeholders>` must match `args` in name and
    /// order. Fixture-level companion to
    /// `r1353_1_every_real_declaration_matches_its_template`: this one pins the
    /// SHAPES the rule must handle (scalar, one arg, two args, an infix arg),
    /// which a scan over today's declarations cannot guarantee stay represented.
    #[test]
    fn r1353_every_declared_template_matches_its_args() {
        fn placeholders(template: &str) -> Vec<&str> {
            let mut out = Vec::new();
            let mut rest = template;
            while let Some(open) = rest.find('<') {
                rest = &rest[open + 1..];
                let close = rest.find('>').expect("an unclosed <placeholder>");
                out.push(&rest[..close]);
                rest = &rest[close + 1..];
            }
            out
        }
        let check = |f: &SchemaField| {
            let names: Vec<&str> = f.args.iter().map(|a| a.name).collect();
            assert_eq!(
                placeholders(f.path),
                names,
                "template {:?} and its declared args disagree",
                f.path,
            );
        };
        check(&SchemaField::new("total", "int"));
        check(&SchemaField::parametric(
            "width.<col>",
            "int",
            const { &[SchemaArg::index("col", "cols")] },
        ));
        check(&SchemaField::parametric(
            "cell.<row>.<col>",
            "string",
            const {
                &[
                    SchemaArg::index("row", "rows"),
                    SchemaArg::index("col", "cols"),
                ]
            },
        ));
        // The infix shape: a literal AFTER the argument. This is why `path` holds
        // the template rather than a stem plus trailing args.
        check(&SchemaField::parametric(
            "voice.<id>.gain",
            "float",
            const { &[SchemaArg::key("id", "int", "voices")] },
        ));
    }

    /// R1353.1 — `literal_prefix` returns exactly what a `query` impl strips,
    /// for every template shape and **every separator**.
    ///
    /// The separator is the author's choice, not this type's: almost everything
    /// uses `.`, `hello-input-chip` uses `:`. R1353's first cut trimmed `'.'`
    /// specifically, so the same accessor answered `"width"` for one template and
    /// `"state:"` for the other — and claimed, in its doc, to return the string
    /// `strip_prefix` matches, which was then true only where it had not
    /// trimmed. Verbatim is the only answer that is true for both.
    #[test]
    fn r1353_1_literal_prefix_is_what_a_query_impl_strips() {
        // A scalar is its own prefix.
        assert_eq!(SchemaField::new("total", "int").literal_prefix(), "total");
        // `column_widths::query` does `strip_prefix("width.")`.
        assert_eq!(
            SchemaField::parametric(
                "width.<col>",
                "int",
                const { &[SchemaArg::open("col", "int")] }
            )
            .literal_prefix(),
            "width.",
        );
        // `hello-input-chip::query` does `strip_prefix("state:")` — a different
        // separator, and the accessor must not assume one.
        assert_eq!(
            SchemaField::parametric(
                "state:<id>",
                "string",
                const { &[SchemaArg::open("id", "int")] }
            )
            .literal_prefix(),
            "state:",
        );
        // An infix template's prefix stops at its FIRST argument.
        assert_eq!(
            SchemaField::parametric(
                "voice.<id>.gain",
                "float",
                const { &[SchemaArg::open("id", "int")] }
            )
            .literal_prefix(),
            "voice.",
        );
    }

    /// R1353 — an INFIX argument (a literal after the placeholder) matches only
    /// when the trailing literal is present. `voice.3` is not `voice.3.gain`.
    #[test]
    fn r1353_addresses_handles_an_infix_argument() {
        let f = SchemaField::parametric(
            "voice.<id>.gain",
            "float",
            const { &[SchemaArg::key("id", "int", "voices")] },
        );
        assert!(f.addresses("voice.3.gain"));
        assert!(
            f.addresses("voice.abc.gain"),
            "well-formedness is query's job"
        );
        assert!(!f.addresses("voice.3"), "the trailing literal is required");
        assert!(
            !f.addresses("voice..gain"),
            "an empty argument is not a member"
        );
        assert!(!f.addresses("voice.3.pan"), "a DIFFERENT field's template");
    }

    /// R1353 — the separator has no escape, pinned as MEASURED behaviour rather
    /// than left as a sentence in a doc.
    ///
    /// An argument is delimited by `.` and nothing escapes a `.` inside one, so
    /// a key containing the separator is not addressable. These cases are the
    /// boundary of what the convention can express; if someone later adds an
    /// escaping rule, this test is the list of answers that must change (and
    /// `ExternalIntrospect::query`'s "does NOT settle" section is the prose to
    /// update with it).
    #[test]
    fn r1353_an_argument_cannot_contain_the_separator() {
        let infix = SchemaField::parametric(
            "voice.<id>.gain",
            "float",
            const { &[SchemaArg::key("id", "int", "voices")] },
        );
        // A dot-free id is fine — including one that spells the trailing literal.
        assert!(infix.addresses("voice.3.gain"));
        assert!(infix.addresses("voice.gain.gain"), "id = \"gain\"");
        // …but the id "x.gain" is UNREACHABLE: matching binds the argument at the
        // FIRST trailing literal, so this addresses nothing rather than silently
        // binding a different id than the caller meant.
        assert!(
            !infix.addresses("voice.x.gain.gain"),
            "an id containing the separator is not addressable",
        );

        // A template's FINAL argument has no trailing literal, so it takes the
        // rest — `col` binds to "2.3". The field owns the path (that is what
        // `addresses` answers); `query` then rejects it as malformed. Owned and
        // malformed is deliberately distinct from unknown: the agent asked this
        // field a bad question, it did not ask a question of nothing.
        let two = SchemaField::parametric(
            "cell.<row>.<col>",
            "string",
            const {
                &[
                    SchemaArg::index("row", "rows"),
                    SchemaArg::index("col", "cols"),
                ]
            },
        );
        assert!(two.addresses("cell.1.2"));
        assert!(
            two.addresses("cell.1.2.3"),
            "the final argument takes the rest"
        );
        assert!(
            !two.addresses("cell.1"),
            "a missing argument is not a member"
        );
        assert!(
            !two.addresses("cell.1."),
            "an empty final argument is not a member"
        );
        assert!(
            !two.addresses("cell..2"),
            "an empty leading argument is not a member"
        );
    }

    /// R1353 — `addresses` is the membership question, so a parametric family is
    /// addressed by its MEMBERS. Pinned because `read_only_or_unknown` routes
    /// through it: matching the stem exactly would tell an agent that `width.0`
    /// — a path it can plainly read — does not exist.
    #[test]
    fn r1353_addresses_matches_members_not_the_bare_stem() {
        let scalar = SchemaField::new("total", "int");
        assert!(scalar.addresses("total"));
        assert!(!scalar.addresses("total.0"), "a scalar has no members");

        let param = SchemaField::parametric(
            "width.<col>",
            "int",
            const { &[SchemaArg::index("col", "cols")] },
        );
        assert!(param.addresses("width.0"));
        assert!(param.addresses("width.12"));
        assert!(
            !param.addresses("width"),
            "the bare stem addresses nothing — it is not a readable path",
        );
        assert!(
            !param.addresses("width."),
            "an empty argument is not a member",
        );
        assert!(
            !param.addresses("widths"),
            "a longer path that merely shares the prefix is a DIFFERENT field \
             (column_widths declares both `width` and `widths`)",
        );
    }

    #[test]
    fn r1353_read_only_or_unknown_sees_parametric_members() {
        let schema = IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("total", "int"),
                    SchemaField::parametric(
                        "width.<col>",
                        "int",
                        const { &[SchemaArg::index("col", "cols")] },
                    ),
                ]
            },
        );
        assert_eq!(
            read_only_or_unknown(&schema, "width.0"),
            InterveneError::ReadOnly,
            "a real, readable member is read-only — not 'no such path'",
        );
        assert_eq!(
            read_only_or_unknown(&schema, "total"),
            InterveneError::ReadOnly,
        );
        assert_eq!(
            read_only_or_unknown(&schema, "nope"),
            InterveneError::UnknownPath,
        );
    }

    #[test]
    fn stub_declares_gui_only_with_skip_fallback() {
        let stub = StubExternal::new();
        let support = stub.backends();
        assert!(support.supports(Backend::Gui));
        assert!(!support.supports(Backend::Tui));
        assert!(!support.supports(Backend::Rpc));
        assert_eq!(support.fallback, BackendFallback::Skip);
    }

    #[test]
    fn stub_uses_framework_repaint_and_ui_thread() {
        let stub = StubExternal::new();
        assert_eq!(stub.repaint_ownership(), RepaintOwner::Framework);
        assert_eq!(stub.thread_ownership(), ThreadOwnership::UiThreadSync);
    }

    #[test]
    fn stub_does_not_claim_any_event() {
        let stub = StubExternal::new();
        let event = Event::Window(WindowEvent::Close);
        assert!(!stub.handles_event(&event));
    }

    #[test]
    fn stub_lifecycle_and_dpi_callbacks_are_noop() {
        let mut stub = StubExternal::new();
        stub.on_mount();
        stub.on_visibility_change(true);
        stub.on_focus_change(false);
        stub.on_dpi_change(2.0);
        stub.on_resize(800, 600);
        stub.on_unmount();
    }

    #[test]
    fn stub_poll_state_is_none() {
        let mut stub = StubExternal::new();
        assert!(stub.poll_state().is_none());
    }

    #[test]
    fn drag_at_methods_default_delegate_to_cursorless() {
        use std::cell::Cell;
        // R1093 — a drag source that overrides ONLY the pre-R1093
        // cursor-less hooks. The additive `drag_to_at`/`drag_release_at`
        // defaults must route into them, so every existing source stays
        // bit-identical (the cursor is simply dropped) and no source ever
        // receives both the `_at` call AND the cursor-less one.
        #[derive(Debug)]
        struct RecordingSource {
            to_calls: Cell<u32>,
            release_calls: Cell<u32>,
        }
        impl External for RecordingSource {
            fn backends(&self) -> BackendSupport {
                BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
            }
            fn repaint_ownership(&self) -> RepaintOwner {
                RepaintOwner::Framework
            }
            fn thread_ownership(&self) -> ThreadOwnership {
                ThreadOwnership::UiThreadSync
            }
            fn drag_to(&mut self, _p: &DragPayload, _o: Option<DropPoint>) {
                self.to_calls.set(self.to_calls.get() + 1);
            }
            fn drag_release(&mut self, _p: &DragPayload, _o: Option<DropPoint>) {
                self.release_calls.set(self.release_calls.get() + 1);
            }
        }
        let mut src = RecordingSource {
            to_calls: Cell::new(0),
            release_calls: Cell::new(0),
        };
        let payload = DragPayload {
            kind: Cow::Borrowed("dock-panel"),
            value: IntrospectValue::Text("inspector".to_owned()),
        };
        let to_update = DragUpdate {
            over: None,
            cursor: (12.0, 34.0),
            over_window: None,
            source_window: None,
            became_drag: false,
            press_cursor: (12.0, 34.0),
        };
        let release_update = DragUpdate {
            over: None,
            cursor: (56.0, 78.0),
            over_window: None,
            source_window: None,
            became_drag: false,
            press_cursor: (56.0, 78.0),
        };
        src.drag_to_at(&payload, &to_update);
        src.drag_release_at(&payload, &release_update);
        assert_eq!(
            src.to_calls.get(),
            1,
            "drag_to_at default must delegate to drag_to"
        );
        assert_eq!(
            src.release_calls.get(),
            1,
            "drag_release_at default must delegate to drag_release"
        );
    }

    #[test]
    fn stub_does_not_want_pointer_capture() {
        let stub = StubExternal::new();
        assert!(!stub.wants_pointer_capture());
    }

    #[test]
    fn stub_pointer_move_default_is_noop() {
        let mut stub = StubExternal::new();
        // Default impl drops both coords — exercising it is the
        // assertion that the trait signature remains dyn-safe and
        // the no-op body compiles for the StubExternal baseline.
        stub.pointer_move(0.5, 0.5);
        stub.pointer_move(-0.1, 1.3);
    }

    #[test]
    fn stub_is_not_a_drag_source() {
        // R742 §5.51 — the default `begin_drag` returns `None`, so the
        // router never starts a session for a non-DnD widget. `drag_to`
        // / `drag_release` are no-op defaults exercised here so the
        // additive trait surface compiles for the StubExternal baseline
        // and stays dyn-safe.
        let mut stub = StubExternal::new();
        assert!(stub.begin_drag().is_none());
        let payload = DragPayload {
            kind: Cow::Borrowed("dnd-row"),
            value: IntrospectValue::Int(0),
        };
        let over = DropPoint {
            tag: "dnd#1".to_string(),
            x_rel: 0.5,
            y_rel: 0.25,
        };
        stub.drag_to(&payload, Some(over.clone()));
        stub.drag_to(&payload, None);
        stub.drag_release(&payload, Some(over));
        stub.drag_release(&payload, None);
    }

    #[test]
    fn trait_is_dyn_safe() {
        // Compile-time guard: any future change that loses dyn-safety
        // (associated consts, Self-returning methods, etc.) breaks this.
        let _: Box<dyn External> = Box::new(StubExternal::new());
    }

    #[test]
    fn stub_opts_out_of_introspection() {
        let stub = StubExternal::new();
        assert!(stub.introspect().is_none());
        let mut stub_mut = StubExternal::new();
        assert!(stub_mut.introspect_mut().is_none());
    }

    #[test]
    fn counted_opts_in_to_introspection() {
        let counted = CountedExternal::new(7);
        let introspect = counted.introspect().expect("opt-in declared");
        assert_eq!(introspect.query("count"), Some(IntrospectValue::Int(7)),);
        assert!(introspect.query("missing").is_none());
    }

    #[test]
    fn counted_schema_lists_count_field() {
        let counted = CountedExternal::new(0);
        let schema = counted.schema();
        assert_eq!(schema.fields, &[SchemaField::new("count", "int")]);
    }

    #[test]
    fn intervene_updates_value() {
        let mut counted = CountedExternal::new(0);
        let introspect = counted.introspect_mut().expect("opt-in declared");
        introspect
            .intervene("count", IntrospectValue::Int(42))
            .expect("matching type");
        assert_eq!(counted.count, 42);
    }

    #[test]
    fn intervene_rejects_type_mismatch() {
        let mut counted = CountedExternal::new(0);
        let err = counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Bool(true))
            .unwrap_err();
        assert_eq!(err, InterveneError::TypeMismatch);
    }

    #[test]
    fn intervene_rejects_unknown_path() {
        let mut counted = CountedExternal::new(0);
        let err = counted
            .introspect_mut()
            .unwrap()
            .intervene("ghost", IntrospectValue::Int(1))
            .unwrap_err();
        assert_eq!(err, InterveneError::UnknownPath);
    }

    #[test]
    fn introspect_sub_trait_is_dyn_safe() {
        let counted = CountedExternal::new(0);
        let _: &dyn ExternalIntrospect = &counted;
    }

    #[test]
    fn counted_invoke_increment_returns_new_total() {
        let mut counted = CountedExternal::new(10);
        let out = counted
            .invoke("increment", IntrospectValue::Int(5))
            .unwrap();
        assert_eq!(out, IntrospectValue::Int(15));
        assert_eq!(counted.count, 15);
    }

    #[test]
    fn counted_invoke_increment_with_wrong_type_is_type_mismatch() {
        let mut counted = CountedExternal::new(0);
        let err = counted
            .invoke("increment", IntrospectValue::Text("nope".to_string()))
            .unwrap_err();
        assert_eq!(err, InvokeError::TypeMismatch);
    }

    #[test]
    fn counted_invoke_unknown_path_is_unknown_path() {
        let mut counted = CountedExternal::new(0);
        let err = counted
            .invoke("ghost", IntrospectValue::Int(1))
            .unwrap_err();
        assert_eq!(err, InvokeError::UnknownPath);
    }

    #[test]
    fn stub_is_dirty_default_is_false() {
        // §5.20 default contract: an External that doesn't opt in to
        // the intent channel reports clean — `walk_scene_and_drain`
        // can skip the drain virtual call.
        let stub = StubExternal::new();
        assert!(!stub.is_dirty());
    }

    #[test]
    fn stub_drain_intents_default_is_noop() {
        // Default `drain_intents` must not emit even when the runtime
        // calls it anyway. Guards against accidental drain-through-
        // unrelated-state-changes.
        let mut stub = StubExternal::new();
        let mut harvested = Vec::new();
        stub.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }

    #[test]
    fn counted_intervene_marks_dirty_and_drains_intent() {
        let mut counted = CountedExternal::new(0);
        assert!(!counted.is_dirty());
        counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Int(7))
            .unwrap();
        assert!(counted.is_dirty());
        let mut harvested: Vec<Intent> = Vec::new();
        counted.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "counted.changed");
        assert_eq!(harvested[0].payload, IntrospectValue::Int(7));
        assert!(!counted.is_dirty());
    }

    #[test]
    fn counted_multiple_intervenes_accumulate_intents() {
        // Each successful intervene pushes one intent; drain flushes
        // them in insertion order so subscribers observe the same
        // sequence the state actually traversed.
        let mut counted = CountedExternal::new(0);
        counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Int(1))
            .unwrap();
        counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Int(2))
            .unwrap();
        let mut harvested: Vec<Intent> = Vec::new();
        counted.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 2);
        assert_eq!(harvested[0].payload, IntrospectValue::Int(1));
        assert_eq!(harvested[1].payload, IntrospectValue::Int(2));
    }

    #[test]
    fn counted_failed_intervene_does_not_mark_dirty() {
        let mut counted = CountedExternal::new(0);
        let _ = counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Bool(true));
        assert!(!counted.is_dirty());
    }

    #[test]
    fn stub_invoke_default_is_unknown_path() {
        // `StubExternal` does not opt in to invoke beyond the default
        // impl — assertion guards against accidental future override
        // that would change the contract.
        //
        // StubExternal doesn't implement ExternalIntrospect; reach the
        // default via the trait-bound dispatch path by constructing an
        // ad-hoc impl-of-the-trait. Item definitions are hoisted before
        // any `let` to keep clippy::items_after_statements clean.
        struct NullIntrospect;
        impl ExternalIntrospect for NullIntrospect {
            fn schema(&self) -> IntrospectSchema {
                IntrospectSchema::new(const { &[] })
            }
            fn query(&self, _: &str) -> Option<IntrospectValue> {
                None
            }
            fn intervene(&mut self, _: &str, _: IntrospectValue) -> Result<(), InterveneError> {
                Err(InterveneError::UnknownPath)
            }
            // invoke uses default impl
        }
        let mut stub = StubExternal::new();
        let mut null = NullIntrospect;
        let err = null.invoke("anything", IntrospectValue::Null).unwrap_err();
        assert_eq!(err, InvokeError::UnknownPath);
        // Silence the unused stub binding.
        let _ = &mut stub;
    }

    // ───────────────────────────────────────────────────────────────
    // R51.155 §5.15 — IntrospectValue typed accessors.
    // ───────────────────────────────────────────────────────────────

    #[test]
    fn as_bool_extracts_only_bool_variant() {
        assert_eq!(IntrospectValue::Bool(true).as_bool(), Some(true));
        assert_eq!(IntrospectValue::Bool(false).as_bool(), Some(false));
        assert_eq!(IntrospectValue::Null.as_bool(), None);
        assert_eq!(IntrospectValue::Int(1).as_bool(), None);
        assert_eq!(IntrospectValue::Float(1.0).as_bool(), None);
        assert_eq!(IntrospectValue::Text("true".into()).as_bool(), None);
    }

    #[test]
    fn as_i64_extracts_only_int_variant() {
        assert_eq!(IntrospectValue::Int(42).as_i64(), Some(42));
        assert_eq!(IntrospectValue::Int(-1).as_i64(), Some(-1));
        assert_eq!(IntrospectValue::Float(1.0).as_i64(), None);
        assert_eq!(IntrospectValue::Null.as_i64(), None);
        assert_eq!(IntrospectValue::Bool(true).as_i64(), None);
    }

    #[test]
    fn as_i32_narrows_in_range_int() {
        assert_eq!(IntrospectValue::Int(42).as_i32(), Some(42));
        assert_eq!(
            IntrospectValue::Int(i64::from(i32::MAX)).as_i32(),
            Some(i32::MAX),
        );
        assert_eq!(
            IntrospectValue::Int(i64::from(i32::MIN)).as_i32(),
            Some(i32::MIN),
        );
    }

    #[test]
    fn as_i32_rejects_out_of_range_int() {
        assert_eq!(
            IntrospectValue::Int(i64::from(i32::MAX) + 1).as_i32(),
            None,
            "narrowing failure surfaces as None, not silent truncation",
        );
        assert_eq!(IntrospectValue::Int(i64::from(i32::MIN) - 1).as_i32(), None,);
    }

    #[test]
    fn as_usize_rejects_negative() {
        assert_eq!(IntrospectValue::Int(0).as_usize(), Some(0));
        assert_eq!(IntrospectValue::Int(42).as_usize(), Some(42));
        assert_eq!(
            IntrospectValue::Int(-1).as_usize(),
            None,
            "negative ints can't be usize",
        );
    }

    #[test]
    fn as_f64_extracts_only_float_variant() {
        assert_eq!(IntrospectValue::Float(1.5).as_f64(), Some(1.5));
        assert_eq!(IntrospectValue::Float(0.0).as_f64(), Some(0.0));
        assert_eq!(IntrospectValue::Int(1).as_f64(), None);
        assert_eq!(IntrospectValue::Null.as_f64(), None);
    }

    #[test]
    fn as_f32_narrows_with_documented_truncation() {
        assert_eq!(
            IntrospectValue::Float(0.5).as_f32().map(f32::to_bits),
            Some(0.5_f32.to_bits()),
        );
        // f64 → f32 truncation: pi in f64 vs f32.
        let pi = IntrospectValue::Float(std::f64::consts::PI).as_f32();
        assert!(pi.is_some());
        // Round-trip precision lost — but the truncation is documented.
        let pi_f32 = pi.unwrap();
        let diff = (pi_f32 - std::f32::consts::PI).abs();
        assert!(diff < 1e-6, "as_f32 truncation lands close to f32 const");
    }

    #[test]
    fn as_f32_returns_none_for_non_float_variants() {
        assert_eq!(IntrospectValue::Int(1).as_f32(), None);
        assert_eq!(IntrospectValue::Null.as_f32(), None);
        assert_eq!(IntrospectValue::Bool(true).as_f32(), None);
    }

    #[test]
    fn as_str_extracts_only_text_variant() {
        assert_eq!(
            IntrospectValue::Text("hello".to_string()).as_str(),
            Some("hello"),
        );
        assert_eq!(IntrospectValue::Null.as_str(), None);
        assert_eq!(IntrospectValue::Int(1).as_str(), None);
    }

    #[test]
    fn is_null_distinguishes_null_only() {
        assert!(IntrospectValue::Null.is_null());
        assert!(!IntrospectValue::Bool(false).is_null());
        assert!(!IntrospectValue::Int(0).is_null());
        assert!(!IntrospectValue::Float(0.0).is_null());
        assert!(!IntrospectValue::Text(String::new()).is_null());
    }
}

#[cfg(test)]
mod selection_copy_payload_tests {
    //! R1407 §5.35 §5.22 — the lifted `Ctrl`/`Cmd`+C copy chord + query. Pure:
    //! it returns the string a copy would write (or `None`) and never touches a
    //! clipboard, so every path — including `AltGr` safety — is asserted without
    //! racing the real OS clipboard.
    use super::{
        ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue,
        selection_copy_payload,
    };
    use crate::input::Modifiers;

    /// A minimal introspectable whose `"sel"` field returns a fixed payload (or
    /// `None` when the range is empty), a non-`Text` `"count"`, and no other
    /// path — the two shapes the copy must accept and reject.
    struct FakeSelection(Option<&'static str>);

    impl ExternalIntrospect for FakeSelection {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(&[])
        }
        fn query(&self, path: &str) -> Option<IntrospectValue> {
            match path {
                "sel" => self.0.map(|s| IntrospectValue::Text(s.to_owned())),
                "count" => Some(IntrospectValue::Int(3)),
                _ => None,
            }
        }
        fn intervene(&mut self, _: &str, _: IntrospectValue) -> Result<(), InterveneError> {
            Err(InterveneError::UnknownPath)
        }
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        }
    }

    #[test]
    fn ctrl_c_yields_the_queried_payload() {
        let ext = FakeSelection(Some("50494e01"));
        assert_eq!(
            selection_copy_payload(&ext, "c", ctrl(), "sel"),
            Some("50494e01".to_owned()),
            "the chord returns the field's serialization",
        );
    }

    #[test]
    fn cmd_shift_c_still_matches_the_uppercase_key() {
        // The platform delivers "C" when Shift is held, and Cmd (meta) is the
        // macOS command key; the case-insensitive match must still fire.
        let ext = FakeSelection(Some("ab"));
        let mods = Modifiers {
            meta: true,
            shift: true,
            ..Modifiers::default()
        };
        assert_eq!(
            selection_copy_payload(&ext, "C", mods, "sel"),
            Some("ab".to_owned()),
        );
    }

    #[test]
    fn a_non_chord_key_yields_nothing() {
        let ext = FakeSelection(Some("x"));
        // A bare "c" (no modifier) is a literal, not a copy.
        assert_eq!(
            selection_copy_payload(&ext, "c", Modifiers::default(), "sel"),
            None,
        );
        // Ctrl held but the wrong letter.
        assert_eq!(selection_copy_payload(&ext, "v", ctrl(), "sel"), None);
    }

    #[test]
    fn altgr_c_does_not_misfire_the_copy() {
        // AltGr = Ctrl+Alt: command_key() is true but the key is composing a
        // character, so the copy must NOT fire and swallow it (R1223).
        let ext = FakeSelection(Some("x"));
        let altgr = Modifiers {
            ctrl: true,
            alt: true,
            ..Modifiers::default()
        };
        assert_eq!(selection_copy_payload(&ext, "c", altgr, "sel"), None);
    }

    #[test]
    fn the_chord_with_nothing_selected_yields_nothing() {
        // A copy chord whose field is empty returns None, so the caller leaves
        // the key unhandled and it can bubble to an application shortcut.
        let ext = FakeSelection(None);
        assert_eq!(selection_copy_payload(&ext, "c", ctrl(), "sel"), None);
    }

    #[test]
    fn a_non_text_payload_yields_nothing() {
        // A field resolving to a non-Text value is skipped (a copy needs a
        // string) and the key falls through.
        let ext = FakeSelection(Some("x"));
        assert_eq!(selection_copy_payload(&ext, "c", ctrl(), "count"), None);
    }
}
