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

/// Schema declaring which paths an [`ExternalIntrospect`] exposes.
///
/// Minimal skeleton today: a static slice of `(path, type_name)` pairs.
/// Future expansion (structured `Type` enum, nested paths, units of
/// measure) lands via `#[non_exhaustive]` — additive only.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntrospectSchema {
    /// Declared paths and their type-name tags. Authors are responsible
    /// for keeping this in sync with `query` / `intervene`; mismatches
    /// surface as test failures, not silent corruption.
    pub fields: &'static [(&'static str, &'static str)],
}

impl IntrospectSchema {
    #[must_use]
    pub const fn new(fields: &'static [(&'static str, &'static str)]) -> Self {
        Self { fields }
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
/// [`Intent`](crate::intent::Intent) wire form (a `kind` tag plus an
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
/// here: any path in [`introspect_schema`](Self::introspect_schema) is
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
        if self
            .source
            .introspect_schema()
            .fields
            .iter()
            .any(|(name, _)| *name == path)
        {
            Err(InterveneError::ReadOnly)
        } else {
            Err(InterveneError::UnknownPath)
        }
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
        IntrospectSchema::new(&[("count", "int")])
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
        };
        let release_update = DragUpdate {
            over: None,
            cursor: (56.0, 78.0),
            over_window: None,
            source_window: None,
            became_drag: false,
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
        assert_eq!(schema.fields, &[("count", "int")]);
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
                IntrospectSchema::new(&[])
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
