//! JSON-RPC 2.0 envelope and method-dispatch entry (§5.7, R16 slice 10).
//!
//! Realizes the wire shape ratified in §5.7: parse a JSON-RPC 2.0
//! request envelope, route to a typed handler (§5.12), and emit a
//! response envelope. The registered-method SSOT is the
//! [`dispatch_parsed`] match itself — a prose enumeration here was a
//! second encoding of that match and had drifted ~25 methods stale
//! by R888.1, so it was removed rather than maintained (each
//! handler's own doc carries its method name and wire shape). The
//! preview-lifecycle methods take the `&PreviewLedger` and
//! `&SceneRevision` arguments the dispatcher receives from its
//! caller alongside the scene.
//!
//! Notifications (requests without `id`) elicit no response per the spec
//! — [`dispatch`] returns `None` in that case. Errors map to the
//! standard JSON-RPC error codes:
//!
//!   * -32700 Parse error      — invalid JSON
//!   * -32600 Invalid Request  — wrong `jsonrpc` version / missing fields
//!   * -32601 Method not found — unknown `method`
//!   * -32602 Invalid params   — params shape or domain failure
//!   * -32603 Internal error   — handler panic / unexpected
//!
//! Domain errors from [`crate::query`] map onto -32602 with `data`
//! carrying the typed [`QueryError`] variant name so AI clients can
//! pattern-match without parsing prose.

use pinion_a11y::{AccessFocus, AccessNode};
use pinion_core::event::WheelDelta;
use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::style::{Border, BoxStyle, Color};
use pinion_core::{Owner, Scene, SceneRevision};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::animation_state::{AnimationStateError, AnimationStateOutcome, animation_state};
use crate::cache_stats::{CacheStatsError, CacheStatsOutcome, cache_stats};
use crate::caret_state::{CaretStateOutcome, caret_state};
use crate::commands::{CommandsError, list_pending_commands};
use crate::dry_run::{DryRunError, dry_run};
use crate::export_pdf::{ExportPdfError, ExportPdfParams, export_pdf};
use crate::font::{self, FontError, FontRegistry};
use crate::frame_timings::{FrameTimingsError, FrameTimingsOutcome, frame_timings};
use crate::intents::{IntentsError, drain_intents};
use crate::intervene::{InterveneError, intervene};
use crate::invoke::{InvokeError, invoke};
use crate::layout_query::{LayoutQueryError, LayoutQueryParams, layout_query};
use crate::locate::{
    BboxError, LocateError, LocateOutcome, LocateRegionOutcome, bbox, locate, locate_region,
};
use crate::methods::rpc_methods;
use crate::preview::{
    ApplyError, ApplyOutcome, PreviewId, PreviewLedger, PreviewView, ProposeError, ProposeOutcome,
    TypedProposal, ViewBlueprint, apply_preview, cancel_preview, list_previews, propose_change,
};
use crate::query::{QueryError, query};
use crate::render_fidelity::{RenderFidelityError, render_fidelity};
use crate::resize::{ResizeError, ResizeParams, resize};
use crate::rewind::{RewindError, rewind};
use crate::screenshot::{Screenshot, ScreenshotError};
use crate::scroll_state::{
    ScrollStateOutcome, SetScrollOffsetParams, scroll_state, set_scroll_offset,
};
use crate::simulate::{SimulateError, SimulateStep, simulate, simulate_with_owner};
use crate::snapshot::{
    GridCursorSnapshot, GridRowSnapshot, GridStyleRun, SnapshotError, SnapshotNode,
    TermColorSnapshot, TextGridSnapshot, snapshot,
};
use crate::substrate_introspect::{SubstrateIntrospectError, introspect_error_to_data};
use crate::text::{NormalizeForm, NormalizeOutcome, text_normalize};
use crate::text_state::{
    SetCaretParams, SetSelectionParams, SetTextParams, TextStateOutcome, set_caret, set_selection,
    set_text, text_state,
};
use crate::theme::{
    PaletteParseError, SetThemeModeError, SetThemeModeOutcome, SetThemeModeParams,
    SetThemePalettesError, SetThemePalettesOutcome, SetThemePalettesParams, ThemeTokensError,
    ThemeTokensOutcome, parse_palette_value, set_theme_mode, set_theme_palettes, theme_tokens,
};
use crate::wait_for::{WaitForError, WaitOutcome, wait_for};
use crate::window_move::{WindowMoveError, WindowMoveParams, window_move};

/// JSON-RPC 2.0 request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
}

impl Request {
    /// R890.1 §5.49 — THE one extraction home for the out-of-band
    /// `{window: "<id>"}` scope param. Every consumer — the backend
    /// entries deriving their dispatch scope, the
    /// [`unknown_window_verdict`] judgment, and [`dispatch_parsed`]'s
    /// own type validation — reads the param through here, so the
    /// extraction cannot drift between sites (pre-R890.1 the GUI
    /// shell hand-rolled a byte-identical extraction to compute its
    /// out-of-band scope, and the two had to agree forever for the
    /// R889 gate to hold in production).
    ///
    /// # Errors
    ///
    /// `Ok(None)` — param absent (or no params object): the caller
    /// applies its entry's default scope. `Ok(Some(id))` — a string
    /// scope. `Err(_)` — the param is present but not a string;
    /// [`dispatch_parsed`] rejects the frame with `-32602` (pre-R890.1
    /// a non-string scope was silently dropped and the request acted
    /// on the primary — the alias smell class R889 set out to kill,
    /// surviving in the type-error corner).
    pub fn window_scope(&self) -> Result<Option<&str>, RpcError> {
        match self.params.as_ref().and_then(|p| p.get("window")) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(other) => Err(RpcError::invalid_params(format!(
                "params.window must be a string, got {other}"
            ))),
        }
    }
}

/// JSON-RPC 2.0 request id (number, string, or null).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Num(i64),
    Str(String),
    Null,
}

/// JSON-RPC 2.0 response envelope. Exactly one of `result` / `error` is
/// `Some` per the spec.
///
/// `result` distinguishes the three wire shapes:
///
/// * **success with a non-null value** — `"result": <json>` → `Some(<json>)`
/// * **success with a null value** — `"result": null` → `Some(Value::Null)`
/// * **error response** — `result` field absent → `None`
///
/// Plain `Option<Value>` would conflate the second and third because
/// serde's default `Option` deserializer collapses both `null` and
/// missing field to `None`. R51.25 — replaced with a `deserialize_with`
/// helper + `default` attribute so `result: null` survives the round
/// trip as `Some(Value::Null)` and only an absent field decays to
/// `None`. Cleans up R51.16's `selected_index None` carry that had to
/// `assert!(raw.contains("\"result\":null"))` against the wire form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    #[serde(
        default,
        deserialize_with = "deserialize_nullable_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    pub id: Option<RequestId>,
}

/// Deserialize the `result` field so an explicit JSON `null` becomes
/// `Some(Value::Null)` rather than `None`. Paired with
/// `#[serde(default)]` on the field — when the field is absent the
/// deserializer never runs and serde supplies `None` via the default.
fn deserialize_nullable_present<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// R51.89 §5.40 — base constructor for the JSON-RPC error object.
    /// All other constructors / helpers in this crate route through
    /// here so the `(code, message, data)` triple is built in one
    /// canonical place. Chain [`Self::with_data`] /
    /// [`Self::with_data_string`] to attach the optional `data`
    /// payload.
    #[must_use]
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// R51.89 §5.40 — chainable builder that attaches an arbitrary
    /// `serde_json::Value` as the error's `data` payload.
    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// R51.89 §5.40 — convenience wrapper for the most common shape:
    /// `data = Some(Value::String(_))`. Most error-to-RPC converters
    /// in this crate (focus/scene/font/...) attach a short variant
    /// or detail string here, so the explicit `Value::String(...)`
    /// wrapper is collapsed into a single call.
    #[must_use]
    pub fn with_data_string(self, detail: impl Into<String>) -> Self {
        self.with_data(Value::String(detail.into()))
    }

    /// R51.89 §5.40 — JSON-RPC `-32602 Invalid params` with a
    /// `Display` detail. The dispatcher and the `focus/*` adapter
    /// share this builder so code + message stay in lockstep.
    ///
    /// R51.101 §5.40 — pre-R51.101 callers reached this through a
    /// crate-local `fn invalid_params(&str)` wrapper kept for
    /// R51.89.1's incremental 30-caller sweep. The wrapper is now
    /// retired (all sites call `RpcError::invalid_params` directly).
    #[must_use]
    pub fn invalid_params(detail: impl std::fmt::Display) -> Self {
        Self::new(-32602, "Invalid params").with_data_string(detail.to_string())
    }

    /// R51.89 §5.40 — JSON-RPC `-32603 Internal error` with a
    /// `Display` detail. Used by serializer-side and other
    /// "shouldn't normally happen" lifts where the detail is a
    /// programmer-facing message.
    #[must_use]
    pub fn internal_error(detail: impl std::fmt::Display) -> Self {
        Self::new(-32603, "Internal error").with_data_string(detail.to_string())
    }
}

const JSONRPC_V2: &str = "2.0";

/// Bundle of all the runtime state a dispatch call needs (§5.34 R40.7).
///
/// Introduced once the dispatcher's parameter list reached four
/// (`&mut Scene`, `&PreviewLedger`, `&SceneRevision`, `&str`) to
/// prevent the further bloat that R40.7+ variants and any post-R40
/// stateful primitive (event history ring, effect ledger, …) would
/// inevitably bring. New runtime handles slot in as additional
/// `DispatchContext` fields; the public [`dispatch`] entry point's
/// signature stays stable.
///
/// Construct fresh per request — the struct holds only borrows, so
/// the embedder retains ownership of its scene / ledger / revision.
pub struct DispatchContext<'a> {
    /// Live scene the handlers read or mutate.
    pub scene: &'a mut Scene,
    /// §5.34 preview lifecycle ledger.
    pub previews: &'a PreviewLedger,
    /// §5.34 R40.4 OCC revision token.
    pub revision: &'a SceneRevision,
    /// R47.7.1 §5.12 — application-supplied paint scene producer.
    /// Invoked by `scene/layout` with the request's hypothetical
    /// viewport `(width, height)` to obtain a freshly-laid paint
    /// scene. `None` causes `scene/layout` to fail with
    /// `LayoutQueryError::PaintProducerUnavailable`; non-layout
    /// methods ignore the field. The application is expected to run
    /// `compute_layout` inside the closure so the returned `Scene`
    /// carries measured rects.
    pub paint_producer: Option<&'a mut (dyn FnMut(u32, u32) -> Scene + 'a)>,
    /// R979 §5.40 §2 #7 — application-supplied accessibility-tree
    /// producer. Invoked by `scene/access` to obtain the enriched,
    /// bounds-resolved [`AccessNode`] list (plus the [`AccessFocus`]
    /// target) the platform AccessKit adapter would receive — the same
    /// build the shell runs for a live screen reader
    /// (`V::access_node_for_window` → `enrich_names_from_scene` →
    /// `resolve_access_bounds`). `None` causes `scene/access` to fail
    /// with an `AccessTreeUnavailable` error; other methods ignore the
    /// field. Mirrors [`Self::paint_producer`]: the embedder threads a
    /// fresh closure before each dispatch.
    pub access_producer:
        Option<&'a mut (dyn FnMut() -> (Vec<AccessNode>, Option<AccessFocus>) + 'a)>,
    /// R47.7.4 §5.12 — application-supplied resize request hook.
    /// Invoked by `scene/resize` with the requested logical
    /// `(width, height)`. The application typically calls
    /// `winit::window::Window::request_inner_size` inside the
    /// closure so winit emits a `Resized` event on the next loop
    /// iteration. Asynchronous — AI clients pair with
    /// `scene/wait_for_frame` for stable observation.
    pub resize_request: Option<&'a mut (dyn FnMut(u32, u32) + 'a)>,
    /// (R1088 §5.16 §5.41 §2 #7 PR-31) The WRITE peer of the
    /// `scene/windows` read: `scene/window_move` invokes this closure
    /// with `(window_id, x, y)` and the application writes the new
    /// logical-pixel position into the binding's `Signal<Vec<WindowSpec>>`
    /// (the reconcile move pass then drives the live OS window). Returns
    /// `true` when a declared window matched the id; `false` →
    /// `WindowMoveError::UnknownWindow`. Absent on single-window / TUI
    /// bindings (no `windows_signal`).
    #[allow(
        clippy::type_complexity,
        reason = "the resize_request sibling above carries the same boxed-FnMut shape un-aliased; a one-field type alias for the dispatch context's reposition hook would obscure the parallel more than it clarifies"
    )]
    pub reposition_request: Option<&'a mut (dyn FnMut(&str, i32, i32) -> bool + 'a)>,
    /// (R705 §5.12 §2 #7) The named window's most recently painted
    /// scene — the exact tree that produced the pixels on screen.
    /// `scene/snapshot from: paint` serializes THIS borrow when present
    /// so introspection equals the displayed frame by construction,
    /// rather than re-running the paint producer at query time (the
    /// §2 #7 violation R705 closes: a re-render reflects current state
    /// while the screen may still show a pre-mutation frame —
    /// [[introspection-from-paint-not-screen]]). `None` falls back to
    /// the [`Self::paint_producer`] (headless bootstrap, never-painted
    /// window). Non-snapshot methods ignore the field.
    pub last_paint_scene: Option<&'a Scene>,
    /// R50.X.1 §5.37.2 — text engine font handle store. AI agents
    /// register OpenType binaries through `font/parse` and address
    /// them by `font_id` in follow-up `font/*` calls. `None` causes
    /// every `font/*` method to fail with
    /// `FontRpcError::RegistryUnavailable`; non-text methods ignore
    /// the field. Lifetime is server-scoped; the embedder owns the
    /// registry and `DispatchContext` only borrows.
    pub font_registry: Option<&'a FontRegistry>,
    /// R51.73 §5.40 — application's focus manager handle. Consumed
    /// by `focus/set` (mutable) + `focus/get` (read-only) so AI
    /// clients can drive focus programmatically as a dual to the
    /// `AccessKit::Action::Focus` AT path. `None` causes both
    /// methods to error with `focus manager unavailable`; the
    /// embedder may opt out of focus surfacing for headless test
    /// fixtures that have no `FocusManager`.
    pub focus_manager: Option<&'a mut pinion_runtime::FocusManager>,

    /// Substrate's root [`Owner`] handle — the reactive scope that
    /// holds every [`Owner::cache`](pinion_core::reactive::Owner::cache)
    /// slot the application has bound during paint (theme provider,
    /// scroll state, caret blink, …) and the pending
    /// [`Command`](pinion_core::Command) queue. Read-only borrow:
    /// every consumer takes a non-draining snapshot so the framework
    /// pump's drain on the next dispatch cycle stays correct.
    ///
    /// First wired in R51.161 §5.23 for the `scene/commands` pending
    /// queue introspection; R597 §5.50 renamed the slot from
    /// `commands_owner` to `runtime_owner` ahead of the second
    /// consumer (`scene/theme_tokens` reading the
    /// [`ThemeProvider`](pinion_core::theme::ThemeProvider) cached
    /// under this owner) so the name reflects the broader semantics.
    ///
    /// `None` causes every consumer to surface its own
    /// `*Unavailable` variant; consumer methods that do not read
    /// reactive substrate state simply ignore the field.
    pub runtime_owner: Option<&'a Owner>,

    /// R51.162 §5.23 — substrate's [`CommandExecutor`](pinion_runtime::CommandExecutor)
    /// handle the §5.23 `scene/commands` RPC method peeks for
    /// in-flight [`Command`](pinion_core::Command) introspection.
    ///
    /// Optional: when `None`, `scene/commands` still works against
    /// the pending-side [`Self::runtime_owner`] but the
    /// `result.in_flight` array is empty. Backends inject this
    /// alongside [`Self::runtime_owner`] for full pending +
    /// in-flight symmetry.
    pub commands_executor: Option<&'a pinion_runtime::CommandExecutor>,

    /// R51.195 §5.49 §5.45 — deferred input inbox.
    ///
    /// `scene/wheel` (and future `scene/key` / `scene/cursor_move`)
    /// cannot mutate the scene from inside [`dispatch`] because the
    /// shell holds `&mut scene` for the whole call and the input
    /// router lives on the surrounding [`ShellCore`]. The dispatcher
    /// instead **enqueues** a [`DeferredInput`] entry on this inbox
    /// per accepted request; the embedder drains the inbox after the
    /// call returns and calls the matching `ShellCore::wheel` (etc.)
    /// methods so the [`InputRouter`](pinion_runtime::InputRouter)
    /// fires under its normal post-frame redraw rules.
    ///
    /// `None` causes every input-injection method to fail with
    /// `InputInjectionUnavailable`. Backends inject `Some(&mut Vec)`
    /// at the start of each dispatch and consume the queue after
    /// `dispatch` returns.
    pub deferred_inputs: Option<&'a mut Vec<DeferredInput>>,

    /// R670.B §5.16 — multi-window scope hint. RPC frames carrying
    /// `{window: "<id>"}` resolve here to the AI-supplied window id
    /// from the JSON-RPC params; the embedder reads it (after
    /// `dispatch` returns) to decide which window's paint scene the
    /// dispatch should observe / mutate. `None` means the frame
    /// omitted the field — embedder defaults to the primary spec.
    ///
    /// First wired for `scene/snapshot` / `scene/layout` / paint-
    /// producer scope; subsequent extensions (`scene/click` /
    /// `scene/key` per-window) ride the same wire shape.
    ///
    /// `None` is the steady-state for single-window bindings —
    /// every existing dispatch path defaults to the primary window
    /// (R670.A `WindowSpec::main`, id = "main") so legacy bindings see
    /// bit-identical behaviour.
    pub window_id: Option<&'a str>,

    /// R682.B §5.16 — per-window paint-fragment cache observability
    /// snapshot. Resolved by the embedder before dispatch
    /// (`pinion-shell::ShellCore::dispatch_rpc_for_window` reads the
    /// `window_id` slot and looks up
    /// `ShellCore::fragment_cache_stats_for_window`). Consumed by
    /// `scene/cache_stats` to surface hit/miss counters + damage
    /// region without giving RPC clients access to the
    /// `vello::Scene`-bearing `paint_adapter::FragmentCache` itself.
    ///
    /// `None` indicates either (a) the window has not painted yet
    /// (bootstrap frame), or (b) the embedder opted out of cache
    /// observability for this dispatch (headless test fixtures).
    /// `scene/cache_stats` surfaces `CacheStatsUnavailable` in both
    /// cases so the AI client can distinguish "no data yet" from
    /// "all zeros".
    pub fragment_cache_stats: Option<pinion_runtime::FragmentCacheStats>,

    /// R907 §5.16 §5.7 — per-window frame-timing profiler snapshot.
    /// Resolved by the embedder before dispatch (the
    /// [`Self::fragment_cache_stats`] pattern:
    /// `pinion-shell::ShellCore::window_scoped_rpc_reads` reads
    /// `ShellCore::frame_timings_for_window`). Consumed by
    /// `scene/frame_timings` to surface per-phase build/encode/render
    /// timings + rolling-window aggregates. `None` →
    /// `FrameTimingsUnavailable` (window has not painted yet, or
    /// headless embedder opt-out), distinct from an all-zero snapshot.
    pub frame_timings: Option<pinion_runtime::FrameTimingsSnapshot>,

    /// R1036 §5.16 §5.7 §2 #7 — per-window render-fidelity record (PR-17),
    /// resolved by the embedder before dispatch from
    /// `ShellCore::render_fidelity_for_window` (the [`Self::frame_timings`]
    /// pattern). Consumed by `scene/render_fidelity` to project the last
    /// PRESENTED frame's per-`TextGrid` fingerprint and compare it against a
    /// fresh recompute of current producer state — an AI-first, pixel-free
    /// divergence verdict that the contaminated `scene/snapshot from: paint`
    /// cannot give. `None` → `RenderFidelityUnavailable` (window has not painted
    /// yet, or headless embedder opt-out).
    pub render_fidelity: Option<pinion_runtime::RenderFidelity>,

    /// R1060 §5.12 §5.16 — pre-captured live-surface screenshot, resolved
    /// by the embedder before dispatch ONLY when the method is
    /// `scene/screenshot` (the [`Self::render_fidelity`] snapshot pattern,
    /// gated to avoid a GPU readback on every dispatch). The `AppShell`
    /// windowed entry renders the addressed window through
    /// `VelloRenderer::capture_rgba8` (which reads back the presented
    /// swapchain texture) and hands the pixels here. `None` →
    /// `RenderBackendUnavailable` (headless / single-window entry with no
    /// live surface, or a non-screenshot method).
    pub screenshot: Option<Screenshot>,

    /// R885 §5.49 — out-of-band input state snapshot, resolved by the
    /// embedder before dispatch (the [`Self::fragment_cache_stats`]
    /// pattern: by-value, per the dispatch's window scope). Consumed
    /// by `scene/input_state` — the READ peer of the
    /// `scene/modifiers` / `scene/key state:"down"/"up"` / cursor-
    /// positioning writes, closing the write-only-wire introspection
    /// gap (§2 #2: every state an input write mutates must be
    /// AI-readable). `None` surfaces `InputStateUnavailable`
    /// (headless fixture / embedder opt-out), distinct from an
    /// all-empty snapshot.
    pub input_state: Option<pinion_core::InputStateSnapshot>,
    /// R888 §5.49 §5.28 — the dispatch-scoped window's frame-pacing
    /// target, resolved by the embedder before dispatch (the
    /// [`Self::input_state`] pattern). Consumed by
    /// `scene/pacing_state` — the READ peer of `scene/set_fps`.
    /// `None` surfaces `PacingStateUnavailable` (backend keeps no
    /// pacing clock — the TUI — or unknown window id), distinct from
    /// [`PacingState::DefaultPolicy`] ("no override installed", a
    /// real value of the axis).
    pub pacing_state: Option<PacingState>,
    /// R1087 §5.16 §5.41 §2 #7 PR-31 — the windows the binding currently
    /// DECLARES (id + title + position), resolved by the embedder before
    /// dispatch from [`pinion_shell::WidgetView::windows_signal`] (the
    /// [`Self::input_state`] by-value pattern). Consumed by
    /// `scene/windows` — the scene-as-data READ that makes a torn-off
    /// panel's floating-window placement observable, not just its
    /// existence. `None` surfaces `DeclaredWindowsUnavailable` (a backend
    /// or fixture that resolved no declared set), distinct from an empty
    /// list ("the binding declares no windows"). Resolved only for the
    /// `scene/windows` method, so every other dispatch pays nothing.
    pub declared_windows: Option<Vec<DeclaredWindow>>,
    /// R1099 §5.51 §2 #7 PR-33 — the cross-window drop the embedder resolved
    /// for THIS request's absolute cursor (the `scene/cross_window_drop`
    /// READ). [`pinion_core::scene::Scene`] is not `Clone`, so the shell
    /// resolves in place — borrowing every window's stored paint scene before
    /// the dispatch borrow split — and threads the small owned result here;
    /// the handler validates the request carried a cursor and serialises it.
    /// `None` either when the method is not `scene/cross_window_drop` or when
    /// the cursor landed on no window's drop target (the handler distinguishes
    /// the two via the request params: a missing cursor is an error, a valid
    /// cursor with no target is a `null` drop).
    pub cross_window_drop: Option<pinion_runtime::CrossWindowDrop>,
    /// R889 §5.49 — the embedder's unknown-window verdict: `Some(id)`
    /// when the request's out-of-band `{window: "<id>"}` scope names a
    /// window the binding does not know
    /// (`pinion_runtime::CoreShell::is_window_known` is the one
    /// predicate; the embedder threads the verdict as data because the
    /// dispatch borrow split precludes a live closure into the
    /// substrate). [`dispatch_parsed`] rejects the whole request with
    /// `-32602 unknown_window` before method routing — READ and WRITE
    /// axes share the gate, so a bogus window id can neither read
    /// another window's state nor mutate it (pre-R889 the GUI shell
    /// silently aliased unknown ids onto the primary: `scene/set_fps
    /// {window: "bogus", fps: 0}` froze the primary's game loop).
    /// `None` (the default) means the scope named a known window or
    /// the request carried no window param.
    pub unknown_window: Option<String>,
}

/// R881 §5.35 §5.49 — which mouse button a `scene/drag` injection
/// holds for the duration of the gesture. The wire vocabulary is the
/// W3C `PointerEvent.button` *name* set (lower-case, human/AI-readable
/// per the RPC string-vocab convention): `"left"` (the default — every
/// pre-R881 `scene/drag`) and `"middle"` (drag-to-pan / the middle
/// gesture pair). `"right"` is deliberately absent until a right-drag
/// gesture exists to expand to — `scene/click`-class context-menu
/// injection is a different arc.
///
/// Encode ([`as_wire_name`](Self::as_wire_name)) and decode
/// ([`from_wire_name`](Self::from_wire_name)) live as an adjacent pair —
/// `decode == inverse(encode)`, the R773 wire-vocabulary SSOT class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DragButton {
    /// The primary button — the pre-R881 `scene/drag` arc
    /// (press / capture-lock march / release).
    #[default]
    Left,
    /// The middle (W3C "auxiliary") button — expands through the
    /// R881 middle-gesture pair: a moved drag pans the pinned
    /// scrollable / canvas, an in-place press-release pastes.
    Middle,
}

impl DragButton {
    /// Canonical wire name — the single source the docs / errors quote.
    /// Inverse of [`from_wire_name`](Self::from_wire_name).
    #[must_use]
    pub fn as_wire_name(self) -> &'static str {
        match self {
            DragButton::Left => "left",
            DragButton::Middle => "middle",
        }
    }

    /// Decode a wire button name; `None` for anything outside the
    /// vocabulary (the dispatcher rejects with `invalid_params` so a
    /// typo surfaces at the call site, not as a silent left-drag).
    /// Inverse of [`as_wire_name`](Self::as_wire_name).
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "left" => Some(DragButton::Left),
            "middle" => Some(DragButton::Middle),
            _ => None,
        }
    }
}

/// R1138 §5.49 §2 #2 — which slice of the press / march / release arc a
/// `scene/drag` injection runs, so an AI can hold a drag mid-gesture and
/// snapshot the held state the atomic [`DragPhase::Full`] arc can never
/// expose (the press-and-hold RPC peer the R1114 drag-image + R1137 redock
/// hint need to be observable — every input a human makes must have an RPC
/// peer, including a held one).
///
///   * `full` (the default — every pre-R1138 `scene/drag`) presses at
///     `from`, marches to `to`, then releases: one self-contained gesture.
///   * `begin` presses at `from` and marches to `to` but does NOT release —
///     the drag session stays open in the router across RPC calls, so a
///     follow-up `scene/snapshot from: paint` sees the held mid-drag scene
///     (the drag-image follower, the redock hint over the hovered zone).
///   * `move` neither presses nor releases — it only marches the held
///     cursor from `from` to `to`, re-aiming the in-flight drag at a new
///     zone between snapshots.
///   * `end` marches from `from` to `to`, then releases — settling the held
///     drag (the redock / drop fires on this release).
///
/// `begin` → (`move` → snapshot)\* → `end` is the AI peer of a human press,
/// hold-and-look, then let-go. Encode / decode are an adjacent inverse pair,
/// the R773 wire-vocabulary SSOT class (`decode == inverse(encode)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DragPhase {
    /// The whole arc in one call — press, march, release (the pre-R1138
    /// `scene/drag` behaviour, so an omitted `phase` is byte-for-byte the
    /// legacy gesture).
    #[default]
    Full,
    /// Press at `from` + march to `to`, then HOLD (no release): opens a drag
    /// session that persists across RPC calls for mid-drag snapshotting.
    Begin,
    /// March the held cursor `from` → `to` only (no press, no release):
    /// re-aims an in-flight `Begin` drag without ending it.
    Move,
    /// March `from` → `to` then release: settles a held drag (the drop /
    /// redock fires here).
    End,
}

impl DragPhase {
    /// Canonical wire name — the single source the docs / errors quote.
    /// Inverse of [`from_wire_name`](Self::from_wire_name).
    #[must_use]
    pub fn as_wire_name(self) -> &'static str {
        match self {
            DragPhase::Full => "full",
            DragPhase::Begin => "begin",
            DragPhase::Move => "move",
            DragPhase::End => "end",
        }
    }

    /// Decode a wire phase name; `None` for anything outside the vocabulary
    /// (the dispatcher rejects with `invalid_params` so a typo surfaces at
    /// the call site, not as a silent full-arc drag).
    /// Inverse of [`as_wire_name`](Self::as_wire_name).
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "full" => Some(DragPhase::Full),
            "begin" => Some(DragPhase::Begin),
            "move" => Some(DragPhase::Move),
            "end" => Some(DragPhase::End),
            _ => None,
        }
    }

    /// Whether this phase opens the gesture with a button press at `from`
    /// (`full` / `begin`). The embedder drain gates `mouse_pressed` on this.
    #[must_use]
    pub fn presses(self) -> bool {
        matches!(self, DragPhase::Full | DragPhase::Begin)
    }

    /// Whether this phase closes the gesture with a button release
    /// (`full` / `end`). The embedder drain gates `mouse_released` on this.
    #[must_use]
    pub fn releases(self) -> bool {
        matches!(self, DragPhase::Full | DragPhase::End)
    }
}

/// R887 §5.49 §5.53 — which mouse button a `scene/click` press-release
/// cycle uses (`params.button`, default [`ClickButton::Left`]).
///
/// A deliberately *different* vocabulary from [`DragButton`]
/// (`left | middle`), not a fold of it: each wire names exactly the
/// buttons whose physical arc that method can mirror.
///
///   * `left` — the press/release activation pair
///     ([`DeferredInput::Click`]).
///   * `right` — the secondary-button press
///     ([`DeferredInput::SecondaryClick`]): a one-shot press-edge
///     dispatch (`apply_secondary_click`, the W3C `contextmenu`
///     convention), no release half, no capture arc — which is why
///     `right` has no place on `scene/drag`.
///   * `middle` is *rejected here with a pointer*: a middle
///     press-release is a gesture whose meaning (pan vs paste) is
///     decided by whether the pointer moved, so only `scene/drag
///     {button: "middle"}` can express it (`from == to` for the
///     in-place paste click).
///
/// The shared `"left"` token is pinned byte-identical across both
/// vocabularies by `click_and_drag_button_vocabularies_share_left`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClickButton {
    /// The primary button — the pre-R887 `scene/click` arc.
    #[default]
    Left,
    /// The secondary (right) button — the R772 context-menu press.
    Right,
}

impl ClickButton {
    /// Canonical wire name — the single source the docs / errors quote.
    /// Inverse of [`from_wire_name`](Self::from_wire_name).
    #[must_use]
    pub fn as_wire_name(self) -> &'static str {
        match self {
            ClickButton::Left => "left",
            ClickButton::Right => "right",
        }
    }

    /// Decode a wire button name; `None` for anything outside the
    /// vocabulary (the dispatcher rejects with `invalid_params`, and
    /// `"middle"` with a redirect to `scene/drag`, so a typo or a
    /// wrong-method button surfaces at the call site, not as a silent
    /// left-click). Inverse of [`as_wire_name`](Self::as_wire_name).
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "left" => Some(ClickButton::Left),
            "right" => Some(ClickButton::Right),
            _ => None,
        }
    }
}

/// R882 §5.49 §5.39 — which keyboard edge a `scene/key` injection
/// mirrors. The winit `KeyboardInput` `ElementState` RPC peer: a
/// physical key delivers a `Pressed` edge (dispatch + held-state
/// cache update) and a `Released` edge (held-state cache update
/// only); the pre-R882 `scene/key` wire had no edge concept and stays
/// the default — an *atomic* logical keypress that dispatches without
/// touching any held-key cache, so a legacy `scene/key {key:"Space"}`
/// can never strand the shell's Space pan chord in the held state.
///
/// Wire form: the optional `state` param — absent ⇒ [`Self::Press`],
/// `"down"` ⇒ [`Self::Down`], `"up"` ⇒ [`Self::Up`], anything else
/// rejects with `invalid_params`. Encode
/// ([`as_wire_param`](Self::as_wire_param)) and decode
/// ([`from_wire_param`](Self::from_wire_param)) live as an adjacent
/// pair — `decode == inverse(encode)` including the absence case, the
/// R773 wire-vocabulary SSOT class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyWireState {
    /// The legacy atomic keypress (param absent) — dispatch the key,
    /// leave every held-key cache untouched.
    #[default]
    Press,
    /// `"down"` — the winit `Pressed` mirror: update the held-key
    /// absolute-state cache, then dispatch the key exactly as
    /// [`Self::Press`] would (a real key-down does both).
    Down,
    /// `"up"` — the winit `Released` mirror: update the held-key
    /// cache; dispatch nothing (the native shell drops release edges
    /// at the dispatch tier too).
    Up,
}

impl KeyWireState {
    /// Canonical wire form — `None` means the param is omitted (the
    /// [`Self::Press`] default). Inverse of
    /// [`from_wire_param`](Self::from_wire_param).
    #[must_use]
    pub fn as_wire_param(self) -> Option<&'static str> {
        match self {
            KeyWireState::Press => None,
            KeyWireState::Down => Some("down"),
            KeyWireState::Up => Some("up"),
        }
    }

    /// Decode the optional `state` wire param; `None` for an
    /// out-of-vocabulary value (the dispatcher rejects with
    /// `invalid_params` so a typo surfaces at the call site, not as a
    /// silent atomic press). Inverse of
    /// [`as_wire_param`](Self::as_wire_param).
    #[must_use]
    pub fn from_wire_param(param: Option<&str>) -> Option<Self> {
        match param {
            None => Some(KeyWireState::Press),
            Some("down") => Some(KeyWireState::Down),
            Some("up") => Some(KeyWireState::Up),
            Some(_) => None,
        }
    }

    /// R882 — the held-key cache half of the drain policy: `Some(held)`
    /// when this edge updates the shell's held-key absolute state
    /// (`Down` ⇒ held, `Up` ⇒ released), `None` for the legacy atomic
    /// [`Press`](Self::Press) (which must never touch the cache). One
    /// home for the edge → cache decision so the GUI and TUI drains
    /// cannot diverge.
    #[must_use]
    pub fn held_edge(self) -> Option<bool> {
        match self {
            KeyWireState::Press => None,
            KeyWireState::Down => Some(true),
            KeyWireState::Up => Some(false),
        }
    }

    /// R882 — the dispatch half of the drain policy: every edge except
    /// [`Up`](Self::Up) dispatches the key (a real key-down both
    /// updates the cache and types; a release dispatches nothing — the
    /// native winit arm drops release edges at the dispatch tier too).
    #[must_use]
    pub fn dispatches(self) -> bool {
        !matches!(self, KeyWireState::Up)
    }
}

/// R51.195 §5.49 §5.45 — single deferred-input entry. One per
/// AI-injected event; the embedder drains the inbox once `dispatch`
/// returns and feeds each entry into the matching shell substrate
/// method.
///
/// `pinion-shell` and `pinion-tui` share this enum so RPC clients see
/// the same wire shape regardless of which backend is hosting the
/// dispatcher.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum DeferredInput {
    /// `scene/wheel` injection. The embedder applies
    /// `cursor_moved(MOUSE, x, y)` and then `wheel(MOUSE, delta)` so
    /// the router has a fresh cursor before the wheel arm fires
    /// (mirrors the winit / web / iOS flow that re-uses the last
    /// pointer position).
    ///
    /// R877 §5.15 §5.49 — held modifiers ride the R763 out-of-band
    /// [`Self::SetModifiers`] absolute-state channel (`scene/modifiers`,
    /// the `ModifiersChanged` mirror), NOT a per-wheel field: the
    /// embedder's drain reads its modifier cache when forwarding, so
    /// `scene/modifiers {ctrl} → scene/wheel` zooms a canvas exactly as
    /// a held physical `Ctrl` would.
    Wheel { x: f64, y: f64, delta: WheelDelta },
    /// R51.196 §5.49 — `scene/click` v1 injection. Synthesises one
    /// complete press / release cycle (down → up) at `(x, y)`. The
    /// embedder applies `cursor_moved(MOUSE, x, y)`, then
    /// `pointer_down(MOUSE)`, then `pointer_up(MOUSE)`, so the
    /// `InputRouter` fires the same activation arc winit's
    /// `WindowEvent::MouseInput` triggers from a real mouse click.
    /// Replaces the R51.193-era probe-only path that only consulted
    /// `External::handles_event` policy.
    Click { x: f64, y: f64 },
    /// R887 §5.49 §5.53 — `scene/click {button: "right"}` injection:
    /// the secondary-button (right-click) press. The embedder applies
    /// `cursor_moved(MOUSE, x, y)` then routes the press-edge one-shot
    /// through `apply_secondary_click` — the exact arc winit's
    /// `MouseInput { button: Right, state: Pressed }` takes (the W3C
    /// `contextmenu` convention fires on press, no release half), so a
    /// context menu opens for an AI client on *any* binding that
    /// implements [`WidgetCore::apply_secondary_click`], not only one
    /// that hand-exposed an `invoke("open_at", …)` action (§2
    /// invariant #2 — every input a human makes has an RPC peer).
    /// Closes the R881.1 right-click wire-gap carry.
    ///
    /// [`WidgetCore::apply_secondary_click`]: pinion_core::WidgetCore::apply_secondary_click
    SecondaryClick { x: f64, y: f64 },
    /// R51.197 §5.49 §5.45 — `scene/key` named-key injection. The
    /// embedder applies `cursor_moved(MOUSE, x, y)` then
    /// `handle_named_key(key)` so the substrate first hands the W3C
    /// `KeyboardEvent.key` string to `V::apply_key` (focused widget
    /// shortcuts: Slider arrows, Toggle Space, Button Enter, …); on
    /// `None` return the R51.187 scroll-key fallback fires for
    /// `ArrowUp/Down/Left/Right`, `PageUp/Down`, `Home`, `End`
    /// against the `ScrollNode` under the cursor. Mirrors the winit
    /// `WindowEvent::KeyboardInput` arc with `Key::Named` — `Escape`
    /// / `Tab` stay shell-reserved and are not injectable.
    ///
    /// R666 §5.37 — `scene/key` auto-discriminates by
    /// `key.chars().count()`: single-codepoint strings (`"a"`,
    /// `" "`, `"漢"`) route as [`CharacterKey`](Self::CharacterKey)
    /// (the `Key::Character` arc); multi-char W3C named strings
    /// (`"Enter"`, `"ArrowUp"`, `"PageDown"`) land here.
    ///
    /// R882 §5.49 §5.39 — `state` carries the keyboard edge
    /// ([`KeyWireState`]): the default `Press` is the legacy atomic
    /// dispatch; `Down` / `Up` mirror the winit `Pressed` / `Released`
    /// edges so an AI can hold a key (`note_key_state` — the Space
    /// pan chord) exactly as a physical keyboard would.
    Key {
        x: f64,
        y: f64,
        key: String,
        state: KeyWireState,
    },
    /// R666 §5.37 §5.49 — `scene/key` character-key injection. The
    /// embedder applies `cursor_moved(MOUSE, x, y)` then
    /// `handle_character_key(character)` so the substrate first
    /// consults `WidgetView::keybinding` (the typed-event channel —
    /// hello-counter `+ / -`, hello-button menu mnemonics, vim-style
    /// `j / k`); on `None` falls through to `V::apply_key` with the
    /// same character string (`TextField` printable-insert, listbox
    /// typeahead). Mirrors the winit `WindowEvent::KeyboardInput` arc
    /// with `Key::Character`.
    ///
    /// Pre-R666 `scene/key` collapsed all input through `Key` →
    /// `handle_named_key` so single-character `V::keybinding`
    /// intercepts were invisible to RPC drivers
    /// (`[[scene-key-character-named-gap]]`). R666 closes the gap.
    ///
    /// R882 §5.49 §5.39 — `state` carries the keyboard edge; see
    /// [`Key`](Self::Key). Held-key tracking keys on the canonical
    /// string vocabulary, so the chord key is the *named* `"Space"`
    /// (the winit boundary string), not the `" "` character.
    CharacterKey {
        x: f64,
        y: f64,
        character: String,
        state: KeyWireState,
    },
    /// R663 §5.49 — `scene/double_click` injection. Emits the W3C
    /// `UIEvent` `detail: 2` convention via two complete press/release
    /// cycles at `(x, y)` without an intervening cursor move so the
    /// receiving `InputRouter` arc fires identically to a real-mouse
    /// double-click. Mirrors [`Click`] for the longer-arc axis the
    /// `TasteJS` `TodoMVC` "double-click row to edit" UX requires; the
    /// substrate-canonical entry point for any future widget that
    /// distinguishes single-click activation from double-click drill-in.
    DoubleClick { x: f64, y: f64 },
    /// R660 §5.49 — `scene/drag` injection. The embedder applies
    /// `cursor_moved(MOUSE, from_x, from_y)`, then `mouse_pressed(MOUSE)`
    /// (gated on [`DragPhase::presses`]), then `steps` interpolated
    /// `cursor_moved` frames marching linearly toward `(to_x, to_y)`, then
    /// `mouse_released(MOUSE)` (gated on [`DragPhase::releases`]). Mirrors the
    /// real-mouse drag arc winit emits for a `MouseInput::Pressed`
    /// followed by a sequence of `CursorMoved` and the matching
    /// `Released`, exercising the `InputRouter`'s R51.34 capture lock
    /// plus the receiving widget's `pointer_move` fractional dispatch
    /// — R55.D.3 `ScrollBar` drag math today; future `Slider` drag
    /// rides the same primitive.
    ///
    /// R881 §5.35 §5.49 — `button` selects which mouse button holds the
    /// drag (`params.button`, default [`DragButton::Left`]). A
    /// [`DragButton::Middle`] drag expands through the middle-gesture
    /// pair (`middle_pressed` / `middle_released`) instead, so an AI
    /// client drives drag-to-pan over a scrollable / canvas through the
    /// exact arc a physical middle-button drag takes (§2 invariant #2 —
    /// every input a human makes must have an RPC peer).
    ///
    /// R1138 §5.49 §2 #2 — `phase` selects which slice of the press / march
    /// / release arc this injection runs ([`DragPhase`], default
    /// [`DragPhase::Full`] = the legacy whole-gesture arc). A
    /// [`DragPhase::Begin`] presses + marches but HOLDS, leaving the drag
    /// session open so a follow-up `scene/snapshot` sees the held mid-drag;
    /// [`DragPhase::Move`] re-aims it; [`DragPhase::End`] releases. This is
    /// the press-and-hold RPC peer a human's held drag needs (R1114).
    Drag {
        from_x: f64,
        from_y: f64,
        to_x: f64,
        to_y: f64,
        steps: u32,
        button: DragButton,
        phase: DragPhase,
    },
    /// R770 §5.49 §5.15 — `scene/hover_file` injection: the winit
    /// `WindowEvent::HoveredFile(PathBuf)` RPC peer. A file is being
    /// dragged *over* the window (the OS reports the path, not a drop
    /// position — winit's file-DnD is window-scoped, like
    /// [`Self::PointerLeave`]). The embedder runs
    /// [`WidgetView::on_file_hover`] so a drop-zone can light up its
    /// "release to drop" affordance before the user lets go.
    FileHover { path: String },
    /// R770 §5.49 §5.15 — `scene/hover_file_cancel` injection: the winit
    /// `WindowEvent::HoveredFileCancelled` peer. The drag left the window
    /// (or was cancelled) without a drop; the embedder runs
    /// [`WidgetView::on_file_hover_cancel`] so the drop-zone clears its
    /// affordance. Positionless + path-less, like a file-DnD
    /// [`Self::PointerLeave`].
    FileHoverCancel,
    /// R770 §5.49 §5.15 — `scene/drop_file` injection: the winit
    /// `WindowEvent::DroppedFile(PathBuf)` peer. A file was dropped on the
    /// window; the embedder runs [`WidgetView::on_file_drop`] with the
    /// path. winit delivers one event per file (a multi-file drop arrives
    /// as several `DroppedFile`s), so each injection carries one path —
    /// the canonical OS "drag a file from the file manager into the app"
    /// path (§5.15 input-forwarding contract, item 5).
    FileDrop { path: String },
    /// R695 §5.49 §5.35 — `scene/hover` injection. The embedder applies
    /// a single `cursor_moved(MOUSE, x, y)` and nothing else, so the
    /// `InputRouter` re-resolves its hover target and fires the
    /// synthetic `PointerEnter` / `PointerLeave` arc on a tag
    /// transition — exactly the winit `WindowEvent::CursorMoved` flow,
    /// minus any press. The pointer-position-only peer to [`Click`]
    /// (which adds press/release) for the hover-driven widgets a real
    /// mouse cursor exercises incidentally on every button but which a
    /// `Tooltip` (R695) makes its **primary** trigger. Previously the
    /// AI client could observe hover state only as a side effect of
    /// `scene/click`'s leading `cursor_moved` (which then pressed);
    /// `scene/hover` exposes the bare hover transition (§2 invariant #2
    /// — every input a human makes must have an RPC peer).
    Hover { x: f64, y: f64 },
    /// R719 §5.49 §5.35 — `scene/pointer_leave` injection. The embedder
    /// applies a single `cursor_left(MOUSE)` (winit's
    /// `WindowEvent::CursorLeft`): the pointer exits the window
    /// entirely, so the [`InputRouter`](pinion_runtime::InputRouter)
    /// drops the cursor and rolls back any in-flight `Hover` —
    /// re-running the synthetic `PointerLeave` arc on whatever widget
    /// was hovered. Window-scoped (no coordinate): the per-frame
    /// `window` field already routes it to the addressed window's
    /// router, exactly like every other [`DeferredInput`].
    ///
    /// This is the missing RPC peer to [`Self::Hover`]: `scene/hover`
    /// mirrors winit `CursorMoved`, but until R719 winit `CursorLeft`
    /// had no peer, violating §2 invariant #2 ("every input a human
    /// makes must have an RPC peer"). A human can move the pointer
    /// *off* the surface; an AI client now can too. It also gives the
    /// headless harness a portable way to establish a deterministic
    /// "no pointer over the window" baseline, independent of where the
    /// host's physical desktop cursor happens to sit when a real X /
    /// Wayland server maps the test window under it.
    PointerLeave,
    /// R724 §5.28 — `scene/tick` injection. Advances the addressed
    /// window's animation clock by `dt` seconds
    /// (`CoreShell::tick_animations_for_window`), so time-driven state
    /// — `§5.28` springs, the R57.X theme-fade, caret blink, and (R724
    /// onward) timed widget dismissal — is *deterministically*
    /// drivable by an AI client. Until R724 a headless client could
    /// read animation state but never advance the clock on demand
    /// (real-frame ticks are non-deterministic between RPC calls), so
    /// settled-value assertions had to be time-tolerant (R723's
    /// theme-fade demo). The caller-injected `dt` keeps the §2 #3
    /// dry-run guarantee intact (same as `Tween::tick` / `Frame::dt`).
    Tick { dt: f32 },
    /// R763 §5.49 §5.39 — `scene/modifiers` injection: the winit
    /// `WindowEvent::ModifiersChanged` RPC peer. Sets the embedder's
    /// absolute modifier cache (`ShellCore::set_modifiers`) so a
    /// subsequent `scene/click` (Shift-click selection-extend),
    /// `scene/drag`, or `scene/key` press reads the held modifiers
    /// exactly as a real key-down would. Modifiers are tracked
    /// out-of-band — they have their own winit event, not a per-click
    /// field — so the mirror is a standalone absolute-state setter that
    /// persists until the next `scene/modifiers` (a real key-up sends
    /// the empty state). Closes the R742.2 RPC-modifier-channel gap for
    /// every input path (`§2` invariant #2: every input a human makes
    /// has an RPC peer — a human can hold Shift, an AI client now can
    /// too).
    SetModifiers {
        shift: bool,
        ctrl: bool,
        alt: bool,
        meta: bool,
    },
    /// R829 §2 #4 §5.28 — `scene/set_fps` injection: the AI-facing peer
    /// of [`pinion_shell::ShellCore::set_target_fps_for_window`]. Sets
    /// the addressed window's target frame rate, the §2 #4 game-loop
    /// pacing policy: `0` *pauses* the per-window paint clock (the
    /// continuous immediate-mode loop stops auto-painting — the window
    /// only repaints on an explicit redraw such as a [`Self::Tick`]
    /// step), and `N` polls it at N fps. Pausing then stepping via
    /// `scene/tick` lets an AI client frame-step the immediate-mode
    /// game loop *deterministically* (the §2 #2 "every input a human
    /// makes has an RPC peer" invariant — a developer can pause/throttle
    /// a render loop in a debugger; an AI client now can too), which a
    /// continuous wall-clock loop cannot offer.
    ///
    /// R888 §5.49 §5.28 — `fps` is an `Option`: `Some(n)` installs the
    /// override (`0` = paused), `None` (`{"fps": null}` on the wire)
    /// *clears* it, restoring the adaptive default policy. Pre-R888 the
    /// boot state (no override — 60fps while immediate-mode content is
    /// active, idle otherwise) was unreachable once any set landed,
    /// which the [`PacingState`] READ peer made visible
    /// ([[wire-form-read-write-symmetry]]: every readable state of an
    /// axis must be writable when the write wire claims the axis).
    SetTargetFps { fps: Option<u32> },
}

// R888.1 §5.28 — `PacingState` is homed in
// `pinion_runtime::frame_pacing` next to `WindowFramePolicy` (the
// READ-payload precedent: `InputStateSnapshot` → pinion-core,
// `FragmentCacheStats` → pinion-runtime; vocabulary lives with its
// domain, [[helper-crate-home-ssot-axis]]). Re-exported here so the
// wire API surface stays `pinion_rpc::PacingState`.
pub use pinion_runtime::PacingState;

/// R1087 §5.16 §5.41 §2 #7 PR-31 — one entry in the `scene/windows`
/// read: a window the binding currently DECLARES, projected to the wire.
///
/// The shell maps each [`pinion_shell::WindowSpec`] it reconciles into one
/// of these (`pinion-shell` depends on `pinion-rpc`, so the domain → wire
/// map runs shell-side; this crate owns only the wire shape, the
/// `InputStateSnapshot`/`PacingState` read-payload precedent). `position`
/// reports the binding's **declared** logical-pixel geometry — `position`
/// (`[x, y]`, R1087) and `declared_size` (`[w, h]`, R1092), each `null` when
/// that axis is system-determined rather than declared — scene-as-data
/// observability for the floating-panel-as-positioned-window model so an AI
/// can read **where each torn-off panel's window is declared to sit and how
/// big it is declared to be**, not merely that it exists.
///
/// **Declared, not a live OS read-back:** this is the geometry the binding
/// wrote and the shell drives the OS window toward, NOT a query of the
/// window's current OS rect. R1088 (`note_window_moved`) DOES feed a user
/// `WindowEvent::Moved` back into the signal so the declared *position*
/// converges on actual — but only for an ALREADY-positioned window; a `null`
/// WM-placed window is deliberately left WM-managed (one user drag must not
/// pin it), so its actual position is never reflected here. And that
/// feedback's live delivery is HW-gated (unverified headlessly). `declared_size`
/// likewise tracks the binding's declared open size (`SizeStrategy`), never a
/// live `WindowEvent::Resized` read-back. So either axis can lag a user
/// drag/resize, and the `Declared*` naming keeps the read honest as declared
/// intent, not a live read-back. For shell/RPC-driven moves declared ==
/// eventual-actual.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DeclaredWindow {
    /// AI-facing window handle (the `{window: "<id>"}` scope key).
    pub id: String,
    /// OS window title (the platform decoration string).
    pub title: String,
    /// Declared outer position in logical pixels, or `null` when
    /// placement is left to the window manager (every pre-R1087 window).
    pub position: Option<(i32, i32)>,
    /// R1092 §5.16 §5.41 §2 #7 — declared open size in logical pixels, or
    /// `null` when the size is content-determined rather than declared
    /// up-front. `Some` mirrors a `SizeStrategy::Fixed`/`OpenResizable` open
    /// size; `null` is an `IntrinsicAfterFirstPaint` window, whose final size
    /// is the post-first-paint content bbox (the shell knows only a transient
    /// `min` floor at declare time, not the eventual geometry) — the same
    /// `null`-means-system-determined honesty [`position`](Self::position)
    /// uses for a WM-placed window. The SSOT is
    /// `pinion_shell::SizeStrategy::declared_size`.
    pub declared_size: Option<(u32, u32)>,
    /// R1115 §5.16 §5.51 §2 #7 PR-38 — does the OS draw this window's chrome
    /// (title bar + border)? `true` is an OS-decorated window (the default);
    /// `false` is a binding-chromed window — a torn-off dock panel floated
    /// into a borderless window that paints its own header. Unlike
    /// [`position`](Self::position) / [`declared_size`](Self::declared_size),
    /// this is never `null`: every window has a known declared chrome state
    /// (the binding's `WindowSpec::decorations`, defaulting `true`). Scene-as-data
    /// observability so an AI can read whether a floating panel is borderless,
    /// not merely where it sits. Declared intent (read at create), not a live
    /// OS read-back. The SSOT is `pinion_shell::WindowSpec::decorations`.
    pub decorations: bool,
}

impl<'a> DispatchContext<'a> {
    /// Build a context from the three borrowed runtime handles.
    /// `paint_producer` starts unset — callers that want
    /// `scene/layout` to succeed register one via
    /// [`Self::with_paint_producer`].
    #[must_use]
    pub fn new(
        scene: &'a mut Scene,
        previews: &'a PreviewLedger,
        revision: &'a SceneRevision,
    ) -> Self {
        Self {
            scene,
            previews,
            revision,
            paint_producer: None,
            access_producer: None,
            resize_request: None,
            reposition_request: None,
            last_paint_scene: None,
            font_registry: None,
            focus_manager: None,
            runtime_owner: None,
            commands_executor: None,
            deferred_inputs: None,
            window_id: None,
            fragment_cache_stats: None,
            frame_timings: None,
            render_fidelity: None,
            screenshot: None,
            input_state: None,
            pacing_state: None,
            declared_windows: None,
            cross_window_drop: None,
            unknown_window: None,
        }
    }

    /// Builder: attach the paint scene producer closure (R47.7.1
    /// §5.12). The closure is invoked by `scene/layout` with the
    /// request's hypothetical `(viewport_w, viewport_h)`; it must
    /// return a `Scene` whose nodes already carry measured `rect`
    /// values (i.e. it should call `compute_layout` internally).
    #[must_use]
    pub fn with_paint_producer(
        mut self,
        producer: &'a mut (dyn FnMut(u32, u32) -> Scene + 'a),
    ) -> Self {
        self.paint_producer = Some(producer);
        self
    }

    /// Builder: attach the accessibility-tree producer closure (R979
    /// §5.40 §2 #7). The closure is invoked by `scene/access` and must
    /// return the enriched, bounds-resolved [`AccessNode`] list (plus the
    /// [`AccessFocus`] target) the AccessKit adapter would receive — the
    /// embedder runs the same `V::access_node_for_window` →
    /// `enrich_names_from_scene` → `resolve_access_bounds` build the live
    /// AT path uses, against the addressed window's last paint scene.
    #[must_use]
    pub fn with_access_producer(
        mut self,
        producer: &'a mut (dyn FnMut() -> (Vec<AccessNode>, Option<AccessFocus>) + 'a),
    ) -> Self {
        self.access_producer = Some(producer);
        self
    }

    /// Builder: attach the window resize request closure (R47.7.4
    /// §5.12). The closure is invoked by `scene/resize` with the
    /// requested logical `(width, height)`; it typically calls
    /// `winit::window::Window::request_inner_size` so winit emits a
    /// `Resized` event on the next loop iteration.
    #[must_use]
    pub fn with_resize_request(mut self, request: &'a mut (dyn FnMut(u32, u32) + 'a)) -> Self {
        self.resize_request = Some(request);
        self
    }

    /// Builder: attach the window reposition request closure (R1088
    /// §5.16 §5.41 §2 #7 PR-31 — the `scene/window_move` WRITE peer of
    /// the `scene/windows` read). The closure is invoked with
    /// `(window_id, x, y)`; it writes the new logical-pixel position into
    /// the binding's `Signal<Vec<WindowSpec>>` and returns whether a
    /// declared window matched.
    #[must_use]
    pub fn with_reposition_request(
        mut self,
        request: &'a mut (dyn FnMut(&str, i32, i32) -> bool + 'a),
    ) -> Self {
        self.reposition_request = Some(request);
        self
    }

    /// Builder: attach the named window's most recently painted scene
    /// (R705 §5.12 §2 #7). `scene/snapshot from: paint` serializes this
    /// borrow — the exact tree on screen — instead of re-rendering at
    /// query time, so introspection equals the displayed frame by
    /// construction. The embedder threads the borrow from
    /// [`pinion_runtime::CoreShell::scene_mut_and_last_paint_for_window`]
    /// (disjoint from the `&mut scene` the dispatcher holds). Omit it
    /// (or pass `None`) for a never-painted window so the snapshot
    /// handler falls back to the paint producer.
    #[must_use]
    pub fn with_last_paint_scene(mut self, paint_scene: &'a Scene) -> Self {
        self.last_paint_scene = Some(paint_scene);
        self
    }

    /// Builder: attach the text engine font registry (R50.X.1
    /// §5.37.2). `font/*` methods resolve handles through the
    /// registry; non-text methods ignore the field. Lifetime is
    /// server-scoped — the embedder constructs the registry once and
    /// reattaches the borrow on each dispatch.
    #[must_use]
    pub fn with_font_registry(mut self, registry: &'a FontRegistry) -> Self {
        self.font_registry = Some(registry);
        self
    }

    /// Builder: attach the application's focus manager (R51.73
    /// §5.40). `focus/set` mutates it; `focus/get` reads it. Both
    /// methods error with `focus manager unavailable` when this is
    /// not registered.
    #[must_use]
    pub fn with_focus_manager(mut self, focus: &'a mut pinion_runtime::FocusManager) -> Self {
        self.focus_manager = Some(focus);
        self
    }

    /// Builder: attach the substrate's root [`Owner`] handle so RPC
    /// methods that need reactive-substrate state can read it.
    /// First wired in R51.161 §5.23 for `scene/commands` (pending
    /// [`Command`](pinion_core::Command) queue introspection); also
    /// consumed by `scene/theme_tokens` (R598 §5.50) reading the
    /// [`ThemeProvider`](pinion_core::theme::ThemeProvider) cached
    /// under this owner. Read-only borrow — every consumer
    /// snapshots without draining so the framework pump on the next
    /// dispatch cycle still observes the original state.
    #[must_use]
    pub fn with_runtime_owner(mut self, owner: &'a Owner) -> Self {
        self.runtime_owner = Some(owner);
        self
    }

    /// R51.162 §5.23 — builder: attach the substrate's
    /// [`CommandExecutor`](pinion_runtime::CommandExecutor) handle so
    /// `scene/commands` can include the `result.in_flight` array
    /// (non-draining snapshot of every executor-tracked task). Pair
    /// with [`Self::with_runtime_owner`] for full pending +
    /// in-flight symmetry.
    #[must_use]
    pub fn with_commands_executor(mut self, executor: &'a pinion_runtime::CommandExecutor) -> Self {
        self.commands_executor = Some(executor);
        self
    }

    /// R51.195 §5.49 §5.45 — builder: attach the deferred-input
    /// inbox so `scene/wheel` (and future input-injection methods)
    /// can enqueue events for post-dispatch drain. `None` (the
    /// default) makes those methods fail with
    /// `InputInjectionUnavailable`.
    #[must_use]
    pub fn with_deferred_inputs(mut self, inbox: &'a mut Vec<DeferredInput>) -> Self {
        self.deferred_inputs = Some(inbox);
        self
    }

    /// R670.B §5.16 — builder: attach the multi-window scope hint.
    /// `id` is the `&'static str` (or `&'a str` from a JSON params
    /// borrow) the AI client supplied as `{window: "<id>"}` on the
    /// RPC frame. Single-window bindings (or RPC frames that omit
    /// the field) pass `None` and dispatchers default to the primary
    /// spec.
    ///
    /// The dispatcher proper does not consume `window_id` today —
    /// it's surfaced for the embedder to read after `dispatch`
    /// returns + to thread into the per-window paint producer
    /// inside `DispatchContext::with_paint_producer` (the embedder
    /// closes over `window_id` to call
    /// `ShellCore::compute_paint_scene_for_window` instead of the
    /// single-window variant). The field is `pub` for direct read
    /// access; the builder mirrors the rest of the typed surface.
    #[must_use]
    pub fn with_window(mut self, id: &'a str) -> Self {
        self.window_id = Some(id);
        self
    }

    /// R682.B §5.16 — builder: attach the per-window paint-fragment
    /// cache observability snapshot the embedder pre-resolved from
    /// `pinion-shell::ShellCore::fragment_cache_stats_for_window`.
    /// `scene/cache_stats` reads the slot; non-cache methods ignore
    /// it. `None` (the default) causes `scene/cache_stats` to surface
    /// `CacheStatsUnavailable`.
    #[must_use]
    pub fn with_fragment_cache_stats(mut self, stats: pinion_runtime::FragmentCacheStats) -> Self {
        self.fragment_cache_stats = Some(stats);
        self
    }

    /// R907 §5.16 §5.7 — builder: attach the per-window frame-timing
    /// profiler snapshot the embedder pre-resolved from
    /// `pinion-shell::ShellCore::frame_timings_for_window`.
    /// `scene/frame_timings` reads the slot; every other arm ignores
    /// it. `None` (the default) causes `scene/frame_timings` to
    /// surface `FrameTimingsUnavailable`.
    #[must_use]
    pub fn with_frame_timings(mut self, snapshot: pinion_runtime::FrameTimingsSnapshot) -> Self {
        self.frame_timings = Some(snapshot);
        self
    }

    /// R1036 §5.16 §5.7 §2 #7 — builder: attach the per-window render-fidelity
    /// record the embedder pre-resolved from
    /// `pinion-shell::ShellCore::render_fidelity_for_window`.
    /// `scene/render_fidelity` reads the slot; every other arm ignores it.
    /// `None` (the default) causes `scene/render_fidelity` to surface
    /// `RenderFidelityUnavailable`.
    #[must_use]
    pub fn with_render_fidelity(mut self, record: pinion_runtime::RenderFidelity) -> Self {
        self.render_fidelity = Some(record);
        self
    }

    /// R1060 §5.12 §5.16 — builder: attach the live-surface screenshot the
    /// embedder pre-captured for a `scene/screenshot` dispatch. The
    /// handler returns these pixels; `None` (the default) keeps the
    /// `RenderBackendUnavailable` stub for headless / non-screenshot
    /// paths.
    #[must_use]
    pub fn with_screenshot(mut self, shot: Screenshot) -> Self {
        self.screenshot = Some(shot);
        self
    }

    /// Builder: attach the embedder-resolved input-state snapshot
    /// (R885 §5.49). Consumed by `scene/input_state`; absent →
    /// `InputStateUnavailable`.
    #[must_use]
    pub fn with_input_state(mut self, snapshot: pinion_core::InputStateSnapshot) -> Self {
        self.input_state = Some(snapshot);
        self
    }

    /// Builder: install the pre-resolved frame-pacing target for the
    /// dispatch-scoped window (R888 §5.49 §5.28). Consumed by
    /// `scene/pacing_state`; absent -> `PacingStateUnavailable`.
    #[must_use]
    pub fn with_pacing_state(mut self, state: PacingState) -> Self {
        self.pacing_state = Some(state);
        self
    }

    /// Builder: install the binding's declared-window set for the
    /// dispatch (R1087 §5.16 §5.41 §2 #7 PR-31). Consumed by
    /// `scene/windows`; absent -> `DeclaredWindowsUnavailable`.
    #[must_use]
    pub fn with_declared_windows(mut self, windows: Vec<DeclaredWindow>) -> Self {
        self.declared_windows = Some(windows);
        self
    }

    /// Builder: install the embedder's pre-resolved cross-window drop for the
    /// dispatch (R1099 §5.51 §2 #7 PR-33). Consumed by
    /// `scene/cross_window_drop`. Absent on bindings the shell did not resolve
    /// it for (a non-cross-window method, or a cursor over no drop target).
    #[must_use]
    pub fn with_cross_window_drop(mut self, drop: pinion_runtime::CrossWindowDrop) -> Self {
        self.cross_window_drop = Some(drop);
        self
    }

    /// Builder: record the embedder's unknown-window verdict (R889
    /// §5.49). [`dispatch_parsed`] rejects the request with `-32602
    /// unknown_window` before method routing; see
    /// [`Self::unknown_window`].
    #[must_use]
    pub fn with_unknown_window(mut self, supplied: String) -> Self {
        self.unknown_window = Some(supplied);
        self
    }
}

/// Dispatch one JSON-RPC 2.0 frame against `ctx`.
///
/// `ctx.scene` is mutably borrowed because some methods
/// (e.g. `scene/rewind`) mutate External state through introspection.
/// Read-only methods accept a reborrowed `&Scene` internally.
///
/// `ctx.previews` is the §5.34 preview lifecycle ledger; the
/// lifecycle methods (`propose_change` / `cancel_preview` /
/// `list_previews` / `apply_preview`) read or mutate it through
/// interior mutability. Methods that do not interact with the
/// lifecycle simply ignore the field.
///
/// `ctx.revision` is the §5.34 R40.4 OCC token. Mutating handlers
/// (`scene/click`, `scene/rewind`, `scene/invoke`) bump it on
/// success so an in-flight preview's `base_revision` can detect
/// concurrent mutation at apply time. `scene/apply_preview` bumps
/// internally. Callers that mutate the scene through channels
/// other than the dispatcher (e.g. winit input forwarded straight
/// to `External::invoke`) are responsible for calling
/// [`SceneRevision::bump`] themselves.
///
/// Returns `Some(json)` for call requests (any with an `id`), `None`
/// for notifications. Parse errors return a `Some(json)` carrying
/// id=null per the spec.
#[must_use]
pub fn dispatch(ctx: &mut DispatchContext<'_>, request_json: &str) -> Option<String> {
    // R671 §5.7 — `dispatch` parses the JSON-RPC envelope then forwards
    // to [`dispatch_parsed`]. Pre-R671 the body was inline + every
    // caller paid one parse cost; R671 splits the parse step out so
    // surface-side AppShell code can parse once + extract
    // out-of-band params (`{window: "<id>"}` per-window scope) +
    // hand the same [`Request`] to the dispatcher (no double-parse).
    // [`pinion_tui::ShellCoreTui::dispatch_rpc`] + tests + any other
    // caller without an out-of-band parse keep going through this
    // entry; the substrate refactor is opt-in.
    let request = match parse_request(request_json) {
        Ok(r) => r,
        Err(resp) => return Some(resp),
    };
    dispatch_parsed(ctx, request)
}

/// R671 §5.7 — parse a JSON-RPC 2.0 envelope into a [`Request`].
///
/// Returns the parsed envelope on success. On parse failure returns
/// the canonical JSON-RPC 2.0 `Parse error` (-32700) response body
/// (id=null per the spec) ready to write back to the AI client. The
/// `Err` payload is the serialized response string — callers either
/// forward it as-is (the dispatcher path) or wrap it as the
/// transport-level frame.
///
/// Sole public extract from [`dispatch`] for the R671 single-parse
/// refactor: `AppShell` now parses the envelope once + extracts
/// out-of-band `{window: "<id>"}` per-window scope from the parsed
/// `Request.params` + hands the same `Request` to
/// [`dispatch_parsed`].
///
/// # Errors
///
/// Returns `Err(serialized_response)` carrying the canonical
/// JSON-RPC 2.0 `Parse error` (-32700) frame when `request_json` is
/// not valid JSON or does not match the [`Request`] schema. The
/// frame is ready to write back to the AI client.
pub fn parse_request(request_json: &str) -> Result<Request, String> {
    let parsed: Result<Request, _> = serde_json::from_str(request_json);
    match parsed {
        Ok(r) => Ok(r),
        Err(e) => Err(serialize(&error_response(
            None,
            -32700,
            "Parse error",
            Some(Value::String(e.to_string())),
        ))),
    }
}

/// R889 §5.49 — resolve a request's unknown-window verdict: reads the
/// IN-BAND `{window: "<id>"}` scope param off the parsed [`Request`]
/// and judges it through the embedder-supplied window-known predicate
/// (`pinion_runtime::CoreShell::is_window_known` behind a closure —
/// the substrate borrow split at the call sites precludes passing the
/// core itself). Returns `Some(id)` for a supplied-but-unknown window,
/// to thread into [`DispatchContext::with_unknown_window`]; `None`
/// when the param is absent / not a string / names a known window.
///
/// One home for the extraction + judgment glue so the GUI
/// (`pinion-shell::ShellCore::dispatch_rpc_inner`) and the TUI
/// (`pinion-tui::ShellCoreTui::dispatch_rpc`) ingresses cannot drift
/// (the R886.1 `input_state_snapshot` 2-copy-glue lesson).
pub fn unknown_window_verdict(
    request: &Request,
    is_window_known: impl Fn(&str) -> bool,
) -> Option<String> {
    request
        .window_scope()
        .ok()
        .flatten()
        .filter(|wid| !is_window_known(wid))
        .map(str::to_owned)
}

/// R671 §5.7 — dispatch a pre-parsed [`Request`] against the live
/// context. Identical method routing as [`dispatch`] but skips the
/// envelope parse step so callers that have already parsed the
/// request (typically to extract out-of-band scope params like
/// `{window: "<id>"}`) hand the same object through without paying
/// a second parse.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "the routing match is the single source of truth for method names — growing with each method addition is the textbook canonical evolution path (currently 23 scene/* + 11 font/* + 1 text/* + 4 focus/* + 1 rpc/*; mirrored by the rpc/methods RPC_METHODS catalog, kept in sync by the catalog_matches_dispatch_match_arms test)"
)]
pub fn dispatch_parsed(ctx: &mut DispatchContext<'_>, request: Request) -> Option<String> {
    let scene: &mut Scene = &mut *ctx.scene;
    let previews: &PreviewLedger = ctx.previews;
    let revision: &SceneRevision = ctx.revision;
    // R47.7.1 §5.12 — `scene/layout` consumes the paint producer
    // closure exactly once per dispatch. Take it out of the context so
    // the (split-borrow) main match below can dispatch other methods
    // against `&mut Scene` without colliding on the producer slot. The
    // caller registers a fresh producer before each dispatch.
    let mut paint_producer = ctx.paint_producer.take();
    // R979 §5.40 §2 #7 — same one-shot take for the access-tree producer:
    // `scene/access` invokes it once to dump the AccessKit projection.
    let mut access_producer = ctx.access_producer.take();
    let mut resize_request = ctx.resize_request.take();
    let mut reposition_request = ctx.reposition_request.take();
    // R51.73 §5.40 — same split-borrow pattern for the focus manager:
    // `focus/set` mutates, `focus/get` reads; both need exclusive
    // access during the route arm.
    let mut focus_manager = ctx.focus_manager.take();
    // R705 §5.12 §2 #7 — the stored paint scene (displayed frame).
    // `scene/snapshot from: paint` serializes this instead of
    // re-rendering at query time, and (R890.1) `scene/layout
    // {viewport: null}` + the path→coordinate resolvers project from
    // the SAME borrow — one channel, so the layout READ and the
    // pixels-on-screen introspection cannot disagree about which
    // frame they describe. Copied out for the dispatch lifetime.
    let last_paint_scene = ctx.last_paint_scene;
    // R51.161 §5.23 — substrate's root Owner. Read-only borrow:
    // `scene/commands` snapshots the pending queue;
    // `scene/theme_tokens` (R598 §5.50) reads the cached
    // ThemeProvider.
    let runtime_owner = ctx.runtime_owner;
    // R51.162 §5.23 — executor borrow for in-flight projection.
    let commands_executor = ctx.commands_executor;
    // R682.B §5.16 — per-window paint-fragment cache observability
    // snapshot the embedder pre-resolved from
    // `ShellCore::fragment_cache_stats_for_window`. Copy out of the
    // context for the dispatch lifetime; `scene/cache_stats` reads
    // the value, every other arm ignores it.
    let fragment_cache_stats = ctx.fragment_cache_stats;
    // R907 §5.16 — per-window frame-timing profiler snapshot the
    // embedder pre-resolved from `ShellCore::frame_timings_for_window`.
    // Copy out for the dispatch lifetime; `scene/frame_timings` reads
    // it, every other arm ignores it.
    let frame_timings = ctx.frame_timings;
    // R1036 §5.16 §5.7 §2 #7 — per-window render-fidelity record; taken out for
    // the dispatch lifetime (owned, non-`Copy` — carries a `Vec` of per-grid
    // fingerprints). `scene/render_fidelity` is the only consumer.
    let render_fidelity = ctx.render_fidelity.take();
    // R1060 §5.12 §5.16 — pre-captured live-surface screenshot; taken out
    // for the dispatch lifetime (owned, non-`Copy` — carries the RGBA8
    // `Vec`). `scene/screenshot` is the only consumer.
    let screenshot = ctx.screenshot.take();
    // R885 §5.49 — embedder-resolved input-state snapshot; taken out
    // for the dispatch lifetime (the slot is per-dispatch like the
    // inbox), `scene/input_state` is the only consumer.
    let input_state = ctx.input_state.take();
    // R888 §5.49 — same single-consumer take as `input_state`.
    let pacing_state = ctx.pacing_state.take();
    // R1087 §5.16 §5.41 §2 #7 — declared-window set; same single-consumer
    // take. `scene/windows` is the only reader; owned (carries a `Vec`).
    let declared_windows = ctx.declared_windows.take();
    let cross_window_drop = ctx.cross_window_drop.take();
    // R50.X.1 §5.37.2 — font registry is shared (read-only borrow);
    // copy the optional reference out for the dispatch lifetime so
    // the registry slot itself is not consumed.
    let font_registry = ctx.font_registry;
    // R51.195 §5.49 §5.45 — deferred-input inbox, taken once per
    // dispatch. `scene/wheel` enqueues into it; the surrounding
    // shell drains after the call returns so the InputRouter fires
    // outside the dispatcher's `&mut scene` borrow.
    let mut deferred_inputs = ctx.deferred_inputs.take();

    if request.jsonrpc != JSONRPC_V2 {
        return Some(serialize(&error_response(
            request.id,
            -32600,
            "Invalid Request",
            Some(Value::String(format!(
                "expected jsonrpc=\"2.0\", got \"{}\"",
                request.jsonrpc
            ))),
        )));
    }

    let is_notification = request.id.is_none();
    let id = request.id.clone();

    // R890.1 §5.49 — window-scope TYPE validation, before method
    // routing: a present-but-non-string `{window: ...}` is `-32602`,
    // not a silent drop (pre-R890.1 `{"window": 42}` fell through the
    // extraction and the request acted on the primary — the R889
    // alias smell class surviving in the type-error corner).
    if let Err(err) = request.window_scope() {
        if is_notification {
            return None;
        }
        return Some(serialize(&error_response(
            id,
            err.code,
            &err.message,
            err.data,
        )));
    }

    // R889 §5.49 — unknown-window gate, before method routing: the
    // embedder resolved the request's `{window: "<id>"}` scope against
    // the window-known registry (`CoreShell::is_window_known`) and
    // threaded the verdict; a request scoped to a window the binding
    // does not know is rejected wholesale. One gate for every method —
    // READ and WRITE share the availability signal, replacing the
    // pre-R889 GUI silent-alias-to-primary (wrong target for writes,
    // wrong data for reads) and the per-axis `*Unavailable` ambiguity
    // ("no such window" vs "no data yet"). Post-R889 the per-axis
    // `*Unavailable` answers mean "known window, axis has no data /
    // backend lacks the axis" only.
    //
    // R890.1 — notifications stay silent (JSON-RPC 2.0: the server
    // MUST NOT reply to a notification; the file's method-routing
    // tail honors the same rule), and the gate sits after the
    // `is_notification` sample for exactly that reason.
    if let Some(supplied) = ctx.unknown_window.take() {
        if is_notification {
            return None;
        }
        return Some(serialize(&error_response(
            id,
            -32602,
            "unknown_window",
            Some(Value::String(supplied)),
        )));
    }

    // R620 §5.7 — every match arm returns `(handler_outcome, kind)`;
    // the kind is the OCC bump contract right at the arm so the
    // compiler enforces the pairing (missing kind = tuple shape
    // mismatch). See `HandlerKind` docstring for the read-vs-mutate
    // taxonomy.
    let (outcome, kind) = match request.method.as_str() {
        "scene/query" => (
            handle_scene_query(scene, last_paint_scene, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/click" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let producer = paint_producer.as_mut().map(|p| &mut **p);
            (
                handle_scene_click(inbox, producer, last_paint_scene, request.params.as_ref()),
                HandlerKind::Mutate,
            )
        }
        "scene/hover" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let producer = paint_producer.as_mut().map(|p| &mut **p);
            (
                handle_scene_hover(inbox, producer, last_paint_scene, request.params.as_ref()),
                HandlerKind::Mutate,
            )
        }
        "scene/pointer_leave" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            (handle_scene_pointer_leave(inbox), HandlerKind::Mutate)
        }
        "scene/hover_file" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            (
                handle_scene_file_event(inbox, request.params.as_ref(), FileEventKind::Hover),
                HandlerKind::Mutate,
            )
        }
        "scene/hover_file_cancel" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            (
                handle_scene_file_event(inbox, request.params.as_ref(), FileEventKind::Cancel),
                HandlerKind::Mutate,
            )
        }
        "scene/drop_file" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            (
                handle_scene_file_event(inbox, request.params.as_ref(), FileEventKind::Drop),
                HandlerKind::Mutate,
            )
        }
        "scene/tick" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            (
                handle_scene_tick(inbox, request.params.as_ref()),
                HandlerKind::Mutate,
            )
        }
        "scene/set_fps" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            (
                handle_scene_set_fps(inbox, request.params.as_ref(), pacing_state.is_some()),
                HandlerKind::Mutate,
            )
        }
        "scene/modifiers" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            (
                handle_scene_modifiers(inbox, request.params.as_ref()),
                HandlerKind::Mutate,
            )
        }
        "scene/double_click" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let producer = paint_producer.as_mut().map(|p| &mut **p);
            (
                handle_scene_double_click(
                    inbox,
                    producer,
                    last_paint_scene,
                    request.params.as_ref(),
                ),
                HandlerKind::Mutate,
            )
        }
        "scene/drag" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let producer = paint_producer.as_mut().map(|p| &mut **p);
            (
                handle_scene_drag(inbox, producer, last_paint_scene, request.params.as_ref()),
                HandlerKind::Mutate,
            )
        }
        "scene/rewind" => (
            handle_scene_rewind(scene, request.params.as_ref()),
            HandlerKind::Mutate,
        ),
        "scene/snapshot" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let producer = paint_producer.as_mut().map(|p| &mut **p);
            (
                handle_scene_snapshot(scene, producer, last_paint_scene, request.params.as_ref()),
                HandlerKind::Read,
            )
        }
        "scene/access" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let producer = access_producer.as_mut().map(|p| &mut **p);
            (handle_scene_access(producer), HandlerKind::Read)
        }
        "scene/dry_run" => (
            handle_scene_dry_run(scene, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/simulate" => (
            handle_scene_simulate(scene, runtime_owner, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/waitFor" => (
            handle_scene_wait_for(scene, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/screenshot" => (
            handle_scene_screenshot(screenshot, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/invoke" => (
            handle_scene_invoke(scene, request.params.as_ref()),
            HandlerKind::Mutate,
        ),
        "scene/intervene" => (
            handle_scene_intervene(scene, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/intents" => (handle_scene_intents(scene), HandlerKind::Read),
        "scene/commands" => (
            handle_scene_commands(runtime_owner, commands_executor),
            HandlerKind::Read,
        ),
        "scene/theme_tokens" => (
            handle_scene_theme_tokens(runtime_owner, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/set_theme_mode" => (
            handle_scene_set_theme_mode(runtime_owner, request.params.as_ref()),
            HandlerKind::Mutate,
        ),
        "scene/set_theme_palettes" => (
            handle_scene_set_theme_palettes(runtime_owner, request.params.as_ref()),
            HandlerKind::Mutate,
        ),
        "scene/animation_state" => (
            handle_scene_animation_state(runtime_owner, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/cache_stats" => (
            handle_scene_cache_stats(fragment_cache_stats),
            HandlerKind::Read,
        ),
        "scene/frame_timings" => (handle_scene_frame_timings(frame_timings), HandlerKind::Read),
        "scene/render_fidelity" => {
            let producer = paint_producer.as_deref_mut();
            (
                handle_scene_render_fidelity(render_fidelity.as_ref(), producer),
                HandlerKind::Read,
            )
        }
        "scene/export_pdf" => (
            handle_scene_export_pdf(last_paint_scene, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/pacing_state" => (handle_scene_pacing_state(pacing_state), HandlerKind::Read),
        "rpc/methods" => (handle_rpc_methods(), HandlerKind::Read),
        "scene/windows" => (handle_scene_windows(declared_windows), HandlerKind::Read),
        "scene/cross_window_drop" => (
            handle_scene_cross_window_drop(cross_window_drop, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/window_move" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let req = reposition_request.as_mut().map(|p| &mut **p);
            (
                handle_scene_window_move(req, request.params.as_ref()),
                // Async — the signal write fires reconcile + the actual OS
                // move lands when the shell next reconciles. No OCC bump
                // here (mirrors `scene/resize`).
                HandlerKind::Read,
            )
        }
        "scene/input_state" => (handle_scene_input_state(input_state), HandlerKind::Read),
        "scene/animate_settle" => (
            handle_scene_animate_settle(runtime_owner),
            HandlerKind::Mutate,
        ),
        "scene/animate_cancel" => (
            handle_scene_animate_cancel(runtime_owner),
            HandlerKind::Mutate,
        ),
        "scene/scroll_state" => (
            handle_scene_scroll_state(runtime_owner, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/set_scroll_offset" => (
            handle_scene_set_scroll_offset(runtime_owner, request.params.as_ref()),
            HandlerKind::Mutate,
        ),
        "scene/text_state" => (
            handle_scene_text_state(runtime_owner, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/set_text" => (
            handle_scene_set_text(runtime_owner, request.params.as_ref()),
            HandlerKind::Mutate,
        ),
        "scene/set_selection" => (
            handle_scene_set_selection(runtime_owner, request.params.as_ref()),
            HandlerKind::Mutate,
        ),
        "scene/set_caret" => (
            handle_scene_set_caret(runtime_owner, request.params.as_ref()),
            HandlerKind::Mutate,
        ),
        "scene/caret_state" => (
            handle_scene_caret_state(runtime_owner, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/locate" => (
            handle_scene_locate(scene, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/locate_region" => (
            handle_scene_locate_region(scene, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/bbox" => (
            handle_scene_bbox(scene, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/resize" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let req = resize_request.as_mut().map(|p| &mut **p);
            (
                handle_scene_resize(req, request.params.as_ref()),
                // Async — actual scene mutation lands when the
                // embedder repaints. No immediate OCC bump.
                HandlerKind::Read,
            )
        }
        "scene/layout" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let producer = paint_producer.as_mut().map(|p| &mut **p);
            (
                handle_scene_layout(producer, last_paint_scene, request.params.as_ref()),
                HandlerKind::Read,
            )
        }
        "scene/key" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let producer = paint_producer.as_mut().map(|p| &mut **p);
            (
                handle_scene_key(inbox, producer, last_paint_scene, request.params.as_ref()),
                // Input enqueue — mutation deferred to next dispatch
                // cycle; no immediate OCC bump.
                HandlerKind::Read,
            )
        }
        "scene/type" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let producer = paint_producer.as_mut().map(|p| &mut **p);
            (
                handle_scene_type(inbox, producer, last_paint_scene, request.params.as_ref()),
                // Batch text injection — one deferred CharacterKey per
                // codepoint; the mutation lands next dispatch cycle, no
                // immediate OCC bump (mirrors scene/key).
                HandlerKind::Read,
            )
        }
        "scene/wheel" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "Vec is not DerefMut; manual reborrow required"
            )]
            let inbox = deferred_inputs.as_mut().map(|p| &mut **p);
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let producer = paint_producer.as_mut().map(|p| &mut **p);
            (
                handle_scene_wheel(inbox, producer, last_paint_scene, request.params.as_ref()),
                HandlerKind::Read,
            )
        }
        "scene/scroll" => {
            #[allow(
                clippy::option_as_ref_deref,
                reason = "dyn FnMut is not DerefMut; manual reborrow required"
            )]
            let producer = paint_producer.as_mut().map(|p| &mut **p);
            (
                handle_scene_scroll(producer, last_paint_scene, request.params.as_ref()),
                HandlerKind::Read,
            )
        }
        "scene/cancel_preview" => (
            handle_scene_cancel_preview(previews, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/list_previews" => (handle_scene_list_previews(previews), HandlerKind::Read),
        "scene/propose_change" => (
            handle_scene_propose_change(previews, revision, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/apply_preview" => (
            handle_scene_apply_preview(scene, revision, previews, request.params.as_ref()),
            // apply_preview bumps SceneRevision INTERNALLY (via
            // crate::preview::apply_preview); a dispatcher-side bump
            // would double-count. HandlerKind::Read signals "do not
            // bump from here".
            HandlerKind::Read,
        ),
        "font/parse" => (
            handle_font_parse(font_registry, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "font/family_name" => (
            handle_font_family_name(font_registry, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "font/glyph_id_for" => (
            handle_font_glyph_id_for(font_registry, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "font/glyph_outline" => (
            handle_font_glyph_outline(font_registry, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "font/cmap_subtables" => (
            handle_font_cmap_subtables(font_registry, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "font/metrics" => (
            handle_font_metrics(font_registry, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "font/subfamily_name" => (
            handle_font_subfamily_name(font_registry, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "font/full_name" => (
            handle_font_full_name(font_registry, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "font/postscript_name" => (
            handle_font_postscript_name(font_registry, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "font/dispose" => (
            handle_font_dispose(font_registry, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "font/list" => (handle_font_list(font_registry), HandlerKind::Read),
        "text/normalize" => (
            handle_text_normalize(request.params.as_ref()),
            HandlerKind::Read,
        ),
        "focus/set" => (
            crate::focus::handle_focus_set(focus_manager.as_deref_mut(), request.params.as_ref()),
            // Focus state tracked independently of SceneRevision.
            HandlerKind::Read,
        ),
        "focus/get" => (
            crate::focus::handle_focus_get(focus_manager.as_deref()),
            HandlerKind::Read,
        ),
        "focus/next" => (
            crate::focus::handle_focus_next(focus_manager.as_deref_mut()),
            HandlerKind::Read,
        ),
        "focus/prev" => (
            crate::focus::handle_focus_prev(focus_manager),
            HandlerKind::Read,
        ),
        _ => (
            Err(RpcError::new(-32601, "Method not found").with_data_string(request.method.clone())),
            HandlerKind::Read,
        ),
    };

    // §5.34 R40.4 + R620 §5.7 — bump the OCC token after any
    // mutating handler succeeds. Pre-R620 the decision was an
    // external `mutates_scene_on_success(method: &str)` matches!
    // gate that the dispatch match arm had to mirror. R620 moves
    // the kind tag into the match arm itself (tuple shape forces
    // every arm to declare a kind), so the bump decision now reads
    // straight off `kind` with no separate cross-reference.
    if outcome.is_ok() && matches!(kind, HandlerKind::Mutate) {
        revision.bump();
    }

    if is_notification {
        return None;
    }

    let resp = match outcome {
        Ok(value) => Response {
            jsonrpc: JSONRPC_V2.to_string(),
            result: Some(value),
            error: None,
            id,
        },
        Err(err) => Response {
            jsonrpc: JSONRPC_V2.to_string(),
            result: None,
            error: Some(err),
            id,
        },
    };

    Some(serialize(&resp))
}

/// R620 §5.7 — typed flag every match arm in [`dispatch`] tags onto
/// its handler outcome so the dispatcher can decide whether to bump
/// the [`SceneRevision`] OCC token after the handler returns `Ok`.
///
/// Pre-R620 the equivalent decision lived in a separate
/// `mutates_scene_on_success(method: &str)` function that did a
/// `matches!` against the method name. Adding a new mutating method
/// required updating *two* sites — the dispatch match arm + this
/// helper — and the compiler did not enforce the pairing. R620 moves
/// the kind tag to the match arm itself: every arm now returns
/// `(Result<Value, RpcError>, HandlerKind)`, so adding a new arm
/// without choosing a kind fails to compile (tuple shape mismatch).
/// The two-site fragility is eliminated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandlerKind {
    /// Pure read OR side-effect that does NOT change
    /// [`SceneRevision`]-tracked state:
    ///
    /// - Pure introspection (`scene/query`, `scene/snapshot`,
    ///   `scene/dry_run`, `scene/locate`, every `*_state` reader,
    ///   every `font/*`).
    /// - Deferred input enqueue (`scene/key` / `scene/wheel` /
    ///   `scene/scroll`) — the mutation happens later, not at
    ///   handler return.
    /// - Self-bumping handlers (`scene/apply_preview`) — the
    ///   handler itself calls `revision.bump()` so a dispatcher-side
    ///   bump would double-count.
    /// - Out-of-OCC-scope state (`focus/*`) — focus is tracked
    ///   independently of [`SceneRevision`].
    Read,
    /// Synchronous mutation of [`SceneRevision`]-tracked scene
    /// state. The dispatcher bumps the OCC token after the handler
    /// returns `Ok`. Adding a new mutating method = add a match arm
    /// returning `HandlerKind::Mutate` — the kind is visible at the
    /// arm itself, so a code reader can see the OCC contract in
    /// place instead of cross-referencing a separate helper.
    Mutate,
}

fn handle_scene_query(
    scene: &Scene,
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.path missing or not a string",
        ));
    };

    match query(scene, path) {
        Ok(value) => Ok(introspect_value_to_json(value)),
        // R828 §2 #4 §5.12 — paint-scene fallback for immediate-mode
        // drivers. `Scene::ImmediateModeNode`s live only in the per-frame
        // paint scene (the view fn emits them; they are absent from the
        // boot-frozen state scene the query above walked — see
        // [[state-scene-vs-paint-scene-introspect]]). When the
        // state-scene walk finds no node at the path, retry against the
        // last painted scene, whose `ImmediateModeNode.handle` is the
        // same `Owner::cache` driver `Rc` the live game loop ticks, so
        // the query reads current simulation state. State-scene authority
        // is preserved: paint is consulted ONLY on `NoExternalAtPath`, so
        // a retained widget that opted out (`IntrospectionOptedOut`) or
        // any other resolution outcome is returned verbatim.
        Err(QueryError::NoExternalAtPath) => match last_paint_scene {
            Some(paint) => match query(paint, path) {
                Ok(value) => Ok(introspect_value_to_json(value)),
                Err(err) => Err(query_error_to_rpc(err)),
            },
            None => Err(query_error_to_rpc(QueryError::NoExternalAtPath)),
        },
        Err(err) => Err(query_error_to_rpc(err)),
    }
}

/// R888.1 §5.49 — the one prose source of `scene/click`'s button
/// vocabulary error (two reject sites in [`handle_scene_click`]; a
/// vocabulary change edits one string).
const CLICK_BUTTON_VOCAB_ERR: &str = "params.button must be \"left\" or \"right\"";

/// R51.196 / R51.201 §5.49 — `scene/click` typed dispatcher.
///
/// Two mutually-exclusive parameter shapes:
///   * `{at: {x, y}}` — click at the given logical-pixel coordinate
///     (R51.196 v1 form).
///   * `{path: "<tag>"}` — R51.201 path-based form: the dispatcher
///     walks the paint scene for the first node carrying `tag`, takes
///     the rect centre as the click target, and enqueues the click.
///     Needs a paint producer wired into the [`DispatchContext`];
///     otherwise returns `PaintProducerUnavailable`. The tag walk
///     descends through `Container.children` and `Scroll.content`
///     but does NOT translate scroll-local coordinates back to
///     window-absolute, so a tag *inside* a `Scene::Scroll` content
///     resolves to a content-local rect — the R51.200 carry
///     `absolute_rect_of` will lift that constraint.
///
/// On success enqueues a [`DeferredInput::Click`] that the embedder
/// drains into `cursor_moved + pointer_down + pointer_up`, firing
/// the `InputRouter` along the same activation arc winit's
/// `WindowEvent::MouseInput` triggers. Returns `null` on success;
/// the AI client follows up with `scene/snapshot` (or `scene/query`)
/// to observe the resulting state transition.
///
/// R887 §5.49 §5.53 — `params.button` ([`ClickButton`], optional,
/// default `"left"`) selects the mouse button: `"right"` enqueues a
/// [`DeferredInput::SecondaryClick`] (press-edge one-shot, the
/// context-menu arc) instead of the press/release pair. `"middle"` is
/// rejected with a redirect — a middle press-release is a gesture
/// (pan vs paste decided by movement), expressible only via
/// `scene/drag {button: "middle"}`.
fn handle_scene_click<F>(
    inbox: Option<&mut Vec<DeferredInput>>,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let params = require_params(params)?;
    let button = match params.get("button") {
        None => ClickButton::default(),
        Some(v) => {
            let name = v
                .as_str()
                .ok_or_else(|| RpcError::invalid_params(CLICK_BUTTON_VOCAB_ERR))?;
            // R888.1 — detect the wrong-method button STRUCTURALLY
            // through DragButton's own decoder (not a re-hardcoded
            // token), so the redirect tracks that vocabulary; the
            // shared-token pin test guards the prose.
            if DragButton::from_wire_name(name) == Some(DragButton::Middle) {
                return Err(RpcError::invalid_params(format!(
                    "params.button \"{}\" is a gesture, not a click — use \
                     scene/drag {{button: \"{}\"}} (from == to for the in-place \
                     paste click)",
                    DragButton::Middle.as_wire_name(),
                    DragButton::Middle.as_wire_name(),
                )));
            }
            ClickButton::from_wire_name(name)
                .ok_or_else(|| RpcError::invalid_params(CLICK_BUTTON_VOCAB_ERR))?
        }
    };
    let (x, y) = resolve_at_or_path(params, paint_producer, last_paint_scene)?;
    inbox.push(match button {
        ClickButton::Left => DeferredInput::Click { x, y },
        ClickButton::Right => DeferredInput::SecondaryClick { x, y },
    });
    Ok(Value::Null)
}

/// R695 §5.49 §5.35 — `scene/hover` handler: mirror of
/// [`handle_scene_click`]'s `at` / `path` selector taxonomy but
/// enqueues a single [`DeferredInput::Hover`] (cursor move, no press).
/// Drives the hover-resolution arc the `Tooltip` (R695) uses as its
/// primary trigger; returns `null` on success, after which the AI
/// client reads the resulting state via `scene/query` /
/// `scene/snapshot`.
fn handle_scene_hover<F>(
    inbox: Option<&mut Vec<DeferredInput>>,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let params = require_params(params)?;
    let (x, y) = resolve_at_or_path(params, paint_producer, last_paint_scene)?;
    inbox.push(DeferredInput::Hover { x, y });
    Ok(Value::Null)
}

/// R719 §5.49 §5.35 — `scene/pointer_leave` handler: enqueues a single
/// [`DeferredInput::PointerLeave`] (the pointer exits the window; no
/// coordinate, no press). Unlike [`handle_scene_hover`] it takes no
/// `at` / `path` selector — winit `CursorLeft` carries no position, so
/// the variant is window-scoped and the per-frame `window` field
/// already routes the drain to the addressed window's router. The RPC
/// peer to [`handle_scene_hover`] for the cursor-exit half of the
/// hover arc (§2 invariant #2); returns `null` on success, after which
/// the AI client reads the resulting (un-hovered) state via
/// `scene/query` / `scene/snapshot`.
fn handle_scene_pointer_leave(inbox: Option<&mut Vec<DeferredInput>>) -> Result<Value, RpcError> {
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    inbox.push(DeferredInput::PointerLeave);
    Ok(Value::Null)
}

/// R770 §5.49 §5.15 — which winit file-DnD event a `scene/*_file` method
/// mirrors. `Hover` / `Drop` carry a `path`; `Cancel` is positionless +
/// path-less.
#[derive(Clone, Copy)]
enum FileEventKind {
    Hover,
    Cancel,
    Drop,
}

/// R770 §5.49 §5.15 — shared handler for the three OS file-drag-drop RPC
/// peers (`scene/hover_file` / `scene/hover_file_cancel` /
/// `scene/drop_file`). `Hover` / `Drop` require a `params.path` string
/// (the dragged file); `Cancel` ignores params. Enqueues the matching
/// [`DeferredInput`] for the embedder to drain into the
/// [`WidgetView`](pinion-shell) file hooks — the AI-first peers of a
/// human dragging a file from the OS file manager onto the window.
fn handle_scene_file_event(
    inbox: Option<&mut Vec<DeferredInput>>,
    params: Option<&Value>,
    kind: FileEventKind,
) -> Result<Value, RpcError> {
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let event = match kind {
        FileEventKind::Cancel => DeferredInput::FileHoverCancel,
        FileEventKind::Hover | FileEventKind::Drop => {
            let params = require_params(params)?;
            let Some(path) = params.get("path").and_then(Value::as_str) else {
                return Err(RpcError::invalid_params(
                    "params.path missing or not a string",
                ));
            };
            let path = path.to_string();
            match kind {
                FileEventKind::Drop => DeferredInput::FileDrop { path },
                _ => DeferredInput::FileHover { path },
            }
        }
    };
    inbox.push(event);
    Ok(Value::Null)
}

/// R724 §5.28 — `scene/tick` handler: enqueue a [`DeferredInput::Tick`]
/// advancing the addressed window's animation clock by `params.dt`
/// seconds. `dt` must be a finite, non-negative number (a negative or
/// NaN delta is rejected — the clock only moves forward). Window-scoped
/// like [`handle_scene_pointer_leave`] (the per-frame `window` field
/// routes the drain). Returns `null` on success; the AI client then
/// reads the advanced state via `scene/snapshot` / `scene/query`,
/// making time-driven widgets deterministically verifiable.
fn handle_scene_tick(
    inbox: Option<&mut Vec<DeferredInput>>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let dt = params
        .and_then(|p| p.get("dt"))
        .and_then(Value::as_f64)
        .ok_or_else(|| RpcError::invalid_params("params.dt missing or not a number"))?;
    if !dt.is_finite() || dt < 0.0 {
        return Err(RpcError::invalid_params(
            "params.dt must be finite and >= 0",
        ));
    }
    #[allow(clippy::cast_possible_truncation)]
    inbox.push(DeferredInput::Tick { dt: dt as f32 });
    Ok(Value::Null)
}

/// R829 §2 #4 §5.28 — `scene/set_fps` handler: enqueue a
/// [`DeferredInput::SetTargetFps`] setting the addressed window's target
/// frame rate (the §2 #4 game-loop pacing policy). `fps` must be a
/// non-negative integer; `0` pauses the per-window paint clock so the
/// AI client can frame-step the immediate-mode loop deterministically
/// via `scene/tick`. Window-scoped like [`handle_scene_tick`] (the
/// per-frame `window` field routes the drain). Returns `null` on
/// success.
fn handle_scene_set_fps(
    inbox: Option<&mut Vec<DeferredInput>>,
    params: Option<&Value>,
    pacing_available: bool,
) -> Result<Value, RpcError> {
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    // R888.1 §5.49 — write/read agree on ONE availability signal: a
    // backend that cannot answer `scene/pacing_state` (no pacing
    // clock — the TUI) must not silently accept-and-drop the write
    // either (the R884 silent-no-op class). Same wire token as the
    // READ peer's error.
    if !pacing_available {
        return Err(
            RpcError::invalid_params("frame pacing unavailable for this dispatch")
                .with_data_string("PacingStateUnavailable"),
        );
    }
    // R888 §5.49 — `{"fps": null}` clears the override (restores the
    // adaptive default policy); a MISSING `fps` stays an error so a
    // typo'd param name cannot silently clear a frame-step pause.
    let fps = match params.and_then(|p| p.get("fps")) {
        None => {
            return Err(RpcError::invalid_params(
                "params.fps missing — pass a non-negative integer, or null to \
                 clear the override (restore the default pacing policy)",
            ));
        }
        Some(Value::Null) => None,
        Some(v) => {
            let n = v.as_u64().ok_or_else(|| {
                RpcError::invalid_params("params.fps must be a non-negative integer or null")
            })?;
            Some(u32::try_from(n).map_err(|_| {
                RpcError::invalid_params("params.fps exceeds the u32 frame-rate range")
            })?)
        }
    };
    inbox.push(DeferredInput::SetTargetFps { fps });
    Ok(Value::Null)
}

/// R888 §5.49 §5.28 — `scene/pacing_state` typed handler: the READ
/// peer of `scene/set_fps` ([[wire-form-read-write-symmetry]]).
/// Serializes the embedder pre-resolved [`PacingState`] for the
/// dispatch-scoped window:
///
/// * `{"fps": n}` — an override is installed (`0` = paused /
///   frame-step mode).
/// * `{"fps": null}` — no override; the adaptive default policy
///   applies (and `scene/set_fps {"fps": null}` is the matching
///   write).
///
/// Absent snapshot (backend keeps no pacing clock — the TUI's
/// terminal repaints are event-driven — or an unknown window id) →
/// `PacingStateUnavailable`, the `CacheStatsUnavailable` /
/// `InputStateUnavailable` honesty parity: a bogus window must not
/// alias onto "default policy". Read-only — `HandlerKind::Read`
/// upstream skips the [`SceneRevision`] bump.
fn handle_scene_pacing_state(state: Option<PacingState>) -> Result<Value, RpcError> {
    let Some(state) = state else {
        return Err(
            RpcError::invalid_params("pacing state unavailable for this dispatch")
                .with_data_string("PacingStateUnavailable"),
        );
    };
    let fps = match state {
        PacingState::DefaultPolicy => Value::Null,
        PacingState::Override(n) => Value::Number(n.into()),
    };
    Ok(serde_json::json!({ "fps": fps }))
}

/// R1087 §5.16 §5.41 §2 #7 PR-31 — `scene/windows` handler: list the
/// windows the binding currently DECLARES, each with its title and
/// declared logical-pixel position (`[x, y]` or `null` for
/// window-manager placement).
///
/// Read-only — `HandlerKind::Read` upstream skips the [`SceneRevision`]
/// bump. `None` (no declared set resolved — a backend/fixture that does
/// not thread one) errors with `DeclaredWindowsUnavailable`, the
/// `PacingStateUnavailable` honesty parity; an empty `Vec` is a real
/// value ("the binding declares no windows", e.g. a single-window binding
/// that opted out of `windows_signal`). This is the scene-as-data READ
/// for the floating-panel-as-positioned-window model: an AI reads where
/// each torn-off panel's window is **declared to sit** (see
/// [`DeclaredWindow`] — declared, not a live OS-position read-back),
/// satisfying §2 #7 for the position state the binding's tear-off reducer
/// produces.
fn handle_scene_windows(windows: Option<Vec<DeclaredWindow>>) -> Result<Value, RpcError> {
    let Some(windows) = windows else {
        return Err(
            RpcError::invalid_params("declared windows unavailable for this dispatch")
                .with_data_string("DeclaredWindowsUnavailable"),
        );
    };
    Ok(serde_json::json!({ "windows": windows }))
}

/// R1089 §5.7 §5.12 §2 #7 — `rpc/methods` handler: serialize the
/// [`crate::RPC_METHODS`] catalog so an AI can DISCOVER the wire surface
/// instead of needing every method's literal string. Context-free (the
/// catalog is a const), so it never errors except on a serialize fault.
fn handle_rpc_methods() -> Result<Value, RpcError> {
    serde_json::to_value(rpc_methods())
        .map_err(|e| RpcError::invalid_params(format!("serialize: {e}")))
}

/// R979 §5.40 §2 #7 — `scene/access` handler: dump the accessibility tree.
///
/// Invokes the embedder's [`access_producer`](DispatchContext::access_producer)
/// once to obtain the enriched, bounds-resolved [`AccessNode`] list (plus the
/// [`AccessFocus`] target) the platform AccessKit adapter would receive, and
/// serializes it via [`crate::access::access_to_json`]. `None` (no producer
/// wired — a headless fixture without the shell's a11y build) errors with
/// `AccessTreeUnavailable`, the `PacingStateUnavailable` honesty parity.
/// Read-only — `HandlerKind::Read` upstream skips the [`SceneRevision`] bump.
///
/// R984.1 — that read-only-ness is load-bearing: both shells build the producer
/// over the **entry** focus sample (`focus_before`), while the live AccessKit
/// emit samples focus at emit time. They agree only because no focus mutation
/// can occur within a read dispatch; were this handler ever reachable from a
/// focus-mutating method, the dump would report stale (entry) focus while the
/// live emit reports the mutated focus — a dump that lies about focus.
fn handle_scene_access(
    producer: Option<&mut (dyn FnMut() -> (Vec<AccessNode>, Option<AccessFocus>) + '_)>,
) -> Result<Value, RpcError> {
    let Some(producer) = producer else {
        return Err(
            RpcError::invalid_params("access tree unavailable for this dispatch")
                .with_data_string("AccessTreeUnavailable"),
        );
    };
    let (nodes, focus) = producer();
    Ok(crate::access::access_to_json(&nodes, focus.as_ref()))
}

/// R763 §5.49 §5.39 — `scene/modifiers` handler: enqueue a
/// [`DeferredInput::SetModifiers`] the embedder drains into
/// `ShellCore::set_modifiers`, the winit `ModifiersChanged` RPC peer.
///
/// Params shape — each key optional, absent = released (`false`),
/// mirroring `winit::keyboard::ModifiersState` (an absolute snapshot,
/// not a delta):
///
/// ```json
/// { "shift": <bool>, "ctrl": <bool>, "alt": <bool>, "meta": <bool> }
/// ```
///
/// `{}` (or no params) clears every modifier — the canonical "all keys
/// up" reset a demo issues after a Shift-click. A non-boolean value for
/// any present key is rejected so a malformed call fails loudly rather
/// than silently dropping the held state.
fn handle_scene_modifiers(
    inbox: Option<&mut Vec<DeferredInput>>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    // Each key defaults to `false` (released) when absent; a present
    // non-boolean is a malformed call.
    let read_bit = |name: &str| -> Result<bool, RpcError> {
        match params.and_then(|p| p.get(name)) {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(RpcError::invalid_params(
                "params.<modifier> must be a boolean",
            )),
        }
    };
    let shift = read_bit("shift")?;
    let ctrl = read_bit("ctrl")?;
    let alt = read_bit("alt")?;
    let meta = read_bit("meta")?;
    inbox.push(DeferredInput::SetModifiers {
        shift,
        ctrl,
        alt,
        meta,
    });
    Ok(Value::Null)
}

/// R663 §5.49 — `scene/double_click` handler: mirror of
/// [`handle_scene_click`] but enqueues [`DeferredInput::DoubleClick`]
/// for the W3C `detail:2` `UIEvent` convention. Same `at` / `path`
/// selector taxonomy (single coordinate; the second click lands at
/// the same point).
fn handle_scene_double_click<F>(
    inbox: Option<&mut Vec<DeferredInput>>,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let params = require_params(params)?;
    let (x, y) = resolve_at_or_path(params, paint_producer, last_paint_scene)?;
    inbox.push(DeferredInput::DoubleClick { x, y });
    Ok(Value::Null)
}

/// R660 §5.49 — `scene/drag` handler: enqueue a [`DeferredInput::Drag`]
/// the embedder drains into the full `cursor_moved + mouse_pressed +
/// N interpolated cursor_moved + mouse_released` arc. Mirrors the
/// AI-first introspection contract (§2 #2 invariant) for the
/// previously-untestable drag axis.
///
/// Params shape:
///
/// ```json
/// {
///   "from": {"x": <f64>, "y": <f64>},        // or "from_path": "<tag>"
///   "to":   {"x": <f64>, "y": <f64>},        // or "to_path":   "<tag>"
///   "steps": <u32>,                           // optional, default 8
///   "button": "left" | "middle",              // optional, default "left" (R881)
///   "phase": "full"|"begin"|"move"|"end"      // optional, default "full" (R1138)
/// }
/// ```
///
/// R1138 §2 #2 — `phase` runs only a slice of the press / march / release
/// arc ([`DragPhase`]): `begin` HOLDS the drag open (no release) so a
/// follow-up `scene/snapshot` sees the held mid-drag, `move` re-aims it,
/// `end` releases. The default `full` is the legacy self-contained gesture.
///
/// `from` / `from_path` are mutually exclusive (mirror of
/// `scene/click`'s `at` / `path` selector); same for `to` / `to_path`.
/// `steps` controls how many intermediate `cursor_moved` frames the
/// substrate generates — 0 collapses to a press / release at `from`
/// (degenerate but well-defined); the default 8 is a good compromise
/// for visible drag-axis demos (enough frames for the receiving
/// widget's state machine to observe the mid-drag values, few enough
/// to keep the drain cheap).
fn handle_scene_drag<F>(
    inbox: Option<&mut Vec<DeferredInput>>,
    mut paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let params = require_params(params)?;

    #[allow(
        clippy::option_as_ref_deref,
        reason = "dyn FnMut is not DerefMut; manual reborrow required so both endpoint resolves \
                  can share the same producer"
    )]
    let producer_for_from = paint_producer.as_mut().map(|p| &mut **p);
    let from = resolve_drag_endpoint(
        params,
        "from",
        "from_path",
        producer_for_from,
        last_paint_scene,
    )?;
    #[allow(
        clippy::option_as_ref_deref,
        reason = "manual reborrow for the 2nd resolve (same rationale as the 1st)"
    )]
    let producer_for_to = paint_producer.as_mut().map(|p| &mut **p);
    let to = resolve_drag_endpoint(params, "to", "to_path", producer_for_to, last_paint_scene)?;
    let steps = match params.get("steps") {
        Some(v) => v
            .as_u64()
            .ok_or_else(|| RpcError::invalid_params("params.steps must be a non-negative integer"))
            .and_then(|n| {
                u32::try_from(n)
                    .map_err(|_| RpcError::invalid_params("params.steps does not fit in u32"))
            })?,
        None => 8,
    };
    // R881 §5.35 §5.49 — optional held-button selector. Decode through
    // the DragButton wire pair so an out-of-vocabulary name rejects
    // loudly instead of silently degrading to a left drag.
    let button = match params.get("button") {
        None => DragButton::default(),
        Some(v) => {
            let name = v.as_str().ok_or_else(|| {
                RpcError::invalid_params("params.button must be \"left\" or \"middle\"")
            })?;
            // R888.1 — wrong-method redirect, the mirror of
            // `scene/click`'s "middle" arm: a right press is a
            // press-edge one-shot with no capture arc, so only
            // `scene/click {button: "right"}` can express it.
            if ClickButton::from_wire_name(name) == Some(ClickButton::Right) {
                return Err(RpcError::invalid_params(format!(
                    "params.button \"{n}\" has no drag arc (a secondary press is a \
                     press-edge one-shot) — use scene/click {{button: \"{n}\"}}",
                    n = ClickButton::Right.as_wire_name(),
                )));
            }
            DragButton::from_wire_name(name).ok_or_else(|| {
                RpcError::invalid_params("params.button must be \"left\" or \"middle\"")
            })?
        }
    };
    // R1138 §5.49 §2 #2 — optional gesture-slice selector. Decode through
    // the DragPhase wire pair so an out-of-vocabulary name rejects loudly
    // instead of silently degrading to a full atomic drag.
    let phase = match params.get("phase") {
        None => DragPhase::default(),
        Some(v) => {
            let name = v.as_str().ok_or_else(|| {
                RpcError::invalid_params(
                    "params.phase must be \"full\", \"begin\", \"move\", or \"end\"",
                )
            })?;
            DragPhase::from_wire_name(name).ok_or_else(|| {
                RpcError::invalid_params(
                    "params.phase must be \"full\", \"begin\", \"move\", or \"end\"",
                )
            })?
        }
    };
    inbox.push(DeferredInput::Drag {
        from_x: from.0,
        from_y: from.1,
        to_x: to.0,
        to_y: to.1,
        steps,
        button,
        phase,
    });
    Ok(Value::Null)
}

/// R660 §5.49 — shared endpoint resolver for `scene/drag`. Reads one
/// of `params.<at_key>` (object with `x`/`y`) or `params.<path_key>`
/// (paint-scene tag) and returns the resolved `(x, y)` coordinate.
/// Mirrors [`resolve_at_or_path`] but parameterised over the key
/// names so a single `drag` call carries both `from` and `to` without
/// shadowing.
fn resolve_drag_endpoint<F>(
    params: &Value,
    at_key: &str,
    path_key: &str,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
) -> Result<(f64, f64), RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let at_param = params.get(at_key);
    let path_param = params.get(path_key).and_then(Value::as_str);
    match (at_param, path_param) {
        (Some(_), Some(_)) => Err(RpcError::invalid_params(format!(
            "params.{at_key} and params.{path_key} are mutually exclusive — pick one"
        ))),
        (None, None) => Err(RpcError::invalid_params(format!(
            "params requires either `{at_key}: {{x, y}}` or `{path_key}: \"<tag>\"`"
        ))),
        (Some(at_value), None) => parse_at_coords(at_value),
        (None, Some(tag)) => resolve_path_to_center(tag, paint_producer, last_paint_scene),
    }
}

/// R51.201 / R51.202 §5.49 — shared `at` vs `path` resolution for
/// every deferred-input dispatcher (`scene/click`, `scene/wheel`,
/// `scene/key`). Returns the `(x, y)` cursor coordinate from either
/// the literal `{at: {x, y}}` shape or the `{path: "<tag>"}` paint-
/// scene lookup. Exactly one of `at` / `path` must be present —
/// supplying neither or both is `invalid_params`.
fn resolve_at_or_path<F>(
    params: &Value,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
) -> Result<(f64, f64), RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let at_param = params.get("at");
    let path_param = params.get("path").and_then(Value::as_str);
    match (at_param, path_param) {
        (Some(_), Some(_)) => Err(RpcError::invalid_params(
            "params.at and params.path are mutually exclusive — pick one",
        )),
        (None, None) => Err(RpcError::invalid_params(
            "params requires either `at: {x, y}` or `path: \"<tag>\"`",
        )),
        (Some(at_value), None) => parse_at_coords(at_value),
        (None, Some(tag)) => resolve_path_to_center(tag, paint_producer, last_paint_scene),
    }
}

fn parse_at_coords(at_value: &Value) -> Result<(f64, f64), RpcError> {
    let at = at_value
        .as_object()
        .ok_or_else(|| RpcError::invalid_params("params.at missing or not an object"))?;
    let x = at
        .get("x")
        .and_then(Value::as_f64)
        .ok_or_else(|| RpcError::invalid_params("params.at.x missing or not a number"))?;
    let y = at
        .get("y")
        .and_then(Value::as_f64)
        .ok_or_else(|| RpcError::invalid_params("params.at.y missing or not a number"))?;
    Ok((x, y))
}

/// R51.201 §5.49 — runs the paint producer at a default viewport
/// (matching `scene/snapshot`'s default), walks the resulting scene
/// for the first node tagged `target_tag`, and returns the rect
/// centre as the click coordinate.
fn resolve_path_to_center<F>(
    target_tag: &str,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
) -> Result<(f64, f64), RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(producer) = paint_producer else {
        return Err(RpcError::invalid_params("PaintProducerUnavailable"));
    };
    // The tag's rect only matches the live window's hit-test if the
    // paint pass runs at the same viewport size as the window. Pull
    // it from the addressed window's stored paint scene (its root
    // rect IS that window's live geometry — R890.1: the same borrow
    // `scene/snapshot from: paint` serializes, so hit-tests and
    // pixel introspection agree by construction); fall back to a
    // 720×480 default for headless / no-frame-yet callers.
    let (vw, vh) = last_paint_scene.map_or((720, 480), |s| {
        let r = s.rect();
        (r.w.max(1), r.h.max(1))
    });
    let scene = producer(vw, vh);
    let Some(rect) = find_rect_by_tag(&scene, target_tag) else {
        return Err(RpcError::invalid_params(format!(
            "tag {target_tag:?} not found in paint scene"
        )));
    };
    #[allow(clippy::cast_precision_loss)]
    let cx = f64::from(rect.x) + f64::from(rect.w) / 2.0;
    #[allow(clippy::cast_precision_loss)]
    let cy = f64::from(rect.y) + f64::from(rect.h) / 2.0;
    Ok((cx, cy))
}

/// R51.201 / R51.200 / R55.G.7 §5.49 — window-absolute, viewport-clipped
/// rect of the node tagged `target_tag`.
///
/// R705.1 §5.45 — delegates to the single coordinate-translation
/// authority [`pinion_core::scene::Scene::rect_for_tag_absolute`].
/// Pre-R705.1 this scroll-offset / viewport-clip walk lived here as
/// `find_rect_by_tag_with_offset` + `translate_rect_into_clip`; R705.1
/// lifted it into `pinion-core` so the §5.39 focus-ring overlay
/// (`pinion_overlay`) draws its ring at the same window-absolute
/// position this resolver lands clicks on — one resolver, no drift
/// between where input goes and where the ring paints
/// ([[introspection-from-paint-not-screen]]).
fn find_rect_by_tag(scene: &Scene, target_tag: &str) -> Option<pinion_core::scene::Rect> {
    scene.rect_for_tag_absolute(target_tag)
}

/// R55.F §5.45 — `scene/scroll` programmatic scroll mutation.
///
/// Params:
///   * Target locator (exactly one): `path: "<scroll_tag>"` walks the
///     paint scene for a `Scene::Scroll` whose tag matches; or
///     `at: {x, y}` resolves the innermost Scroll whose clipped
///     viewport contains the integer coordinate, reusing the same
///     `Scene::scroll_state_at` substrate R55.C.2 wired for wheel /
///     arrow dispatch (R55.G.15 §5.49 consistency with
///     `scene/click` / `scene/wheel` / `scene/key`).
///   * Action (exactly one): `to: {x, y}` sets the absolute offset, or
///     `by: {dx, dy}` adds the delta. Both saturate against the
///     content `[0, max_{x,y}]` bounds.
///
/// Calls `ScrollState::scroll_to` / `scroll_by` on the resolved
/// state. The reactive `Signal::set` inside those methods drives the
/// next view re-run, so the AI client follows up with
/// `scene/snapshot` to observe the new offset.
///
/// Bypasses the `InputRouter` wheel/key activation arc — useful for
/// "jump to row N" patterns where simulating ten `PageDown`
/// injections would be noisy. The bound clamping is identical to the
/// input-route path (`scroll_to` / `scroll_by` both saturate against
/// `[0, max_{x,y}]`).
fn handle_scene_scroll<F>(
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let params = require_params(params)?;
    let to = params.get("to");
    let by = params.get("by");
    let action = match (to, by) {
        (Some(_), Some(_)) => {
            return Err(RpcError::invalid_params(
                "params.to and params.by are mutually exclusive — pick one",
            ));
        }
        (None, None) => {
            return Err(RpcError::invalid_params(
                "params requires either `to: {x, y}` or `by: {dx, dy}`",
            ));
        }
        (Some(to_value), None) => ScrollAction::To(parse_xy(to_value, "to")?),
        (None, Some(by_value)) => ScrollAction::By(parse_xy(by_value, "by")?),
    };
    let state = resolve_scroll_target_at_or_path(params, paint_producer, last_paint_scene)?;
    match action {
        ScrollAction::To((x, y)) => state.scroll_to(x, y),
        ScrollAction::By((dx, dy)) => state.scroll_by(dx, dy),
    }
    Ok(Value::Null)
}

/// R55.G.15 §5.49 §5.45 — resolve the `scene/scroll` target via
/// `path` (tag lookup) or `at` (coordinate lookup) against a fresh
/// paint scene. Mirrors the click / wheel / key locator shape
/// (`resolve_at_or_path` R51.202) but returns the attached
/// `ScrollState` directly instead of a cursor coordinate, since
/// `scene/scroll` mutates the state without going through the
/// `InputRouter` arc.
///
/// Exactly one of `path` / `at` must be supplied; neither or both is
/// `invalid_params`. The paint producer must be wired (the target
/// `Scene::Scroll` lives in the `V::view` output, not the state
/// scene) — `PaintProducerUnavailable` otherwise. The coord-based
/// arm reuses `Scene::scroll_state_at(u32, u32)`, the same
/// substrate `InputRouter` walks for wheel / arrow dispatch in
/// R55.C.2, so `at`-based mutation and a real user wheel event
/// converge on the same Scroll.
fn resolve_scroll_target_at_or_path<F>(
    params: &Value,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
) -> Result<std::rc::Rc<pinion_core::widgets::scroll::ScrollState>, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    enum Target<'a> {
        Path(&'a str),
        At(&'a Value),
    }
    let path = params.get("path").and_then(Value::as_str);
    let at = params.get("at");
    let target = match (path, at) {
        (Some(_), Some(_)) => {
            return Err(RpcError::invalid_params(
                "params.path and params.at are mutually exclusive — pick one",
            ));
        }
        (None, None) => {
            return Err(RpcError::invalid_params(
                "params requires either `path: \"<tag>\"` or `at: {x, y}`",
            ));
        }
        (Some(tag), None) => Target::Path(tag),
        (None, Some(at_value)) => Target::At(at_value),
    };
    let Some(producer) = paint_producer else {
        return Err(RpcError::invalid_params("PaintProducerUnavailable"));
    };
    let (vw, vh) = last_paint_scene.map_or((720, 480), |s| {
        let r = s.rect();
        (r.w.max(1), r.h.max(1))
    });
    let painted = producer(vw, vh);
    match target {
        Target::Path(tag) => find_scroll_state_by_tag(&painted, tag).ok_or_else(|| {
            RpcError::invalid_params(format!(
                "scroll tag {tag:?} not found or has no attached ScrollState"
            ))
        }),
        Target::At(at_value) => {
            let (xu, yu) = parse_at_coords_u32(at_value)?;
            painted.scroll_state_at(xu, yu).ok_or_else(|| {
                RpcError::invalid_params(format!(
                    "no Scroll with attached ScrollState at ({xu}, {yu})"
                ))
            })
        }
    }
}

/// R55.G.15 §5.49 — parse an `at: {x, y}` object into the `u32`
/// shape `Scene::scroll_state_at` expects. Mirrors `parse_at_coords`
/// (the click / wheel / key f64 variant) but rejects non-integer or
/// negative coords up front so the coord-to-Scroll lookup never sees
/// a wrapped value.
fn parse_at_coords_u32(at_value: &Value) -> Result<(u32, u32), RpcError> {
    let at = at_value
        .as_object()
        .ok_or_else(|| RpcError::invalid_params("params.at missing or not an object"))?;
    let raw_x = at
        .get("x")
        .and_then(Value::as_i64)
        .ok_or_else(|| RpcError::invalid_params("params.at.x missing or not an integer"))?;
    let raw_y = at
        .get("y")
        .and_then(Value::as_i64)
        .ok_or_else(|| RpcError::invalid_params("params.at.y missing or not an integer"))?;
    let coord_x = u32::try_from(raw_x)
        .map_err(|_| RpcError::invalid_params(format!("params.at.x out of u32 range: {raw_x}")))?;
    let coord_y = u32::try_from(raw_y)
        .map_err(|_| RpcError::invalid_params(format!("params.at.y out of u32 range: {raw_y}")))?;
    Ok((coord_x, coord_y))
}

enum ScrollAction {
    To((i32, i32)),
    By((i32, i32)),
}

fn parse_xy(value: &Value, label: &str) -> Result<(i32, i32), RpcError> {
    let obj = value.as_object().ok_or_else(|| {
        RpcError::invalid_params(format!("params.{label} missing or not an object"))
    })?;
    let x = obj
        .get("x")
        .or_else(|| obj.get("dx"))
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            RpcError::invalid_params(format!("params.{label}.x/dx missing or not a number"))
        })?;
    let y = obj
        .get("y")
        .or_else(|| obj.get("dy"))
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            RpcError::invalid_params(format!("params.{label}.y/dy missing or not a number"))
        })?;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "JSON i64 → ScrollState i32 wire boundary; out-of-range saturates at clamp"
    )]
    Ok((x as i32, y as i32))
}

/// R55.F §5.45 — depth-first walk of the scene for the first
/// `Scene::Scroll` whose `tag` matches `target_tag` AND which has
/// an attached `ScrollState`. Returns `None` if no such node
/// exists.
fn find_scroll_state_by_tag(
    scene: &Scene,
    target_tag: &str,
) -> Option<std::rc::Rc<pinion_core::widgets::scroll::ScrollState>> {
    match scene {
        Scene::Container(c) => c
            .children
            .iter()
            .find_map(|child| find_scroll_state_by_tag(child, target_tag)),
        Scene::Scroll(s) => {
            if s.tag.as_deref() == Some(target_tag) {
                s.state.clone()
            } else {
                find_scroll_state_by_tag(s.content.as_ref(), target_tag)
            }
        }
        _ => None,
    }
}

fn handle_scene_rewind(scene: &mut Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.path missing or not a string",
        ));
    };
    let Some(value_json) = params.get("value") else {
        return Err(RpcError::invalid_params("params.value missing"));
    };
    let Some(value) = json_to_introspect_value(value_json) else {
        return Err(RpcError::invalid_params(
            "params.value unsupported (v0: null/bool/number/string only)",
        ));
    };

    match rewind(scene, path, value) {
        Ok(()) => Ok(Value::Null),
        Err(err) => Err(rewind_error_to_rpc(err)),
    }
}

fn rewind_error_to_rpc(err: RewindError) -> RpcError {
    let variant = match err {
        RewindError::Path(_) => "Path",
        RewindError::UnsupportedPath => "UnsupportedPath",
        RewindError::NoExternalAtPath => "NoExternalAtPath",
        RewindError::IntrospectionOptedOut => "IntrospectionOptedOut",
        RewindError::Intervene(_) => "Intervene",
    };
    RpcError::invalid_params(variant)
}

/// R51.194 §5.49 §5.45 — `scene/snapshot` typed dispatcher.
///
/// Two scene sources are addressable through the `from` param:
///   * `"state"` (default) — dump the application's state scene (root
///     `External`), preserving the v0 wire shape every existing test
///     and demo depends on.
///   * `"paint"` — dump the paint scene produced by `V::view(state)` at
///     the supplied (or default) viewport, so AI demos can walk the
///     `Scene::Container` / `Scene::Scroll` hierarchy the shell
///     actually renders. Requires the dispatcher's `paint_producer`
///     wire (`DispatchContext::with_paint_producer`); fails with
///     `PaintProducerUnavailable` when absent.
///
/// R705 §5.12 §2 #7 — paint-mode source resolution. When the embedder
/// threaded the window's stored paint scene
/// ([`DispatchContext::with_last_paint_scene`]), `from: paint`
/// serializes THAT — the exact tree on screen — so introspection
/// equals the displayed frame by construction. Pre-R705 this re-ran
/// the paint producer at query time, which reflects current state even
/// when the screen still shows a pre-mutation frame; the resulting
/// drift between `scene/snapshot`, the live window, and the state value
/// was a §2 #7 violation ([[introspection-from-paint-not-screen]]).
///
/// The `viewport` param is therefore IGNORED when a stored frame
/// exists (the displayed frame has the window's real size, not a
/// hypothetical one). It is honoured only in the producer fallback
/// below — a never-painted window (headless bootstrap with no winit
/// paint loop) has no displayed frame to mirror, so a fresh render at
/// the requested (or default 720×480) viewport is the only available
/// truth. Bindings driven by a real window observe the parity fix; the
/// in-crate headless test harness keeps re-rendering exactly as before.
fn handle_scene_snapshot<F>(
    scene: &Scene,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.path missing or not a string",
        ));
    };
    let from = params
        .get("from")
        .and_then(Value::as_str)
        .unwrap_or("state");

    let node = match from {
        "state" => snapshot(scene, path).map_err(snapshot_error_to_rpc)?,
        "paint" => {
            // Prefer the displayed frame (§2 #7 parity); fall back to a
            // query-time render only when no frame has been stored yet.
            if let Some(painted) = last_paint_scene {
                snapshot(painted, path).map_err(snapshot_error_to_rpc)?
            } else if let Some(producer) = paint_producer {
                let (w, h) = parse_snapshot_viewport(params)?;
                let paint_scene = (producer)(w, h);
                snapshot(&paint_scene, path).map_err(snapshot_error_to_rpc)?
            } else {
                return Err(RpcError::invalid_params("PaintProducerUnavailable"));
            }
        }
        other => {
            return Err(RpcError::invalid_params(format!(
                "params.from must be \"state\" or \"paint\", got {other:?}",
            )));
        }
    };

    Ok(snapshot_node_to_json(node))
}

/// R51.197 §5.49 §5.45 — `scene/key` typed dispatcher.
///
/// Params: `{at: {x: f64, y: f64}, key: <W3C KeyboardEvent.key string>,
/// state?: "down" | "up"}`.
///
/// R882 §5.49 §5.39 — the optional `state` param is the winit
/// `KeyboardInput` edge peer ([`KeyWireState`]): `"down"` dispatches
/// the key AND records it held (the shell's held-key absolute-state
/// cache — `"Space"` arms the left-drag pan chord); `"up"` releases
/// it without dispatching. Absent = the legacy atomic press, which
/// never touches the held cache (an atomic press cannot strand the
/// chord).
///
/// Enqueues a [`DeferredInput`] entry on the dispatcher's inbox. The
/// embedder drains the inbox after `dispatch` returns and applies
/// `cursor_moved(x, y)` followed by the appropriate substrate key
/// dispatch.
///
/// R666 §5.37 — auto-discriminates between W3C `Key::Named`
/// (multi-char strings: `"Enter"`, `"ArrowDown"`, `"PageUp"`, …) and
/// `Key::Character` (single-codepoint strings: `"a"`, `" "`, `"$"`,
/// `"漢"`) by `key.chars().count()`. Named keys flow through
/// [`DeferredInput::Key`] → `handle_named_key` (focused-widget
/// shortcuts then scroll fallback); character keys flow through
/// [`DeferredInput::CharacterKey`] → `handle_character_key`
/// (`V::keybinding` typed-event intercept then `apply_key` fallback).
///
/// Pre-R666 every `scene/key` request collapsed to
/// `handle_named_key`, so single-character `V::keybinding` overrides
/// were invisible to RPC drivers
/// (`[[scene-key-character-named-gap]]`). The single-codepoint
/// disambiguator matches the W3C convention — every named key in the
/// pinion shell maps to a ≥ 2-char string per
/// `pinion-shell::named_key_str`, so the boundary is unambiguous.
fn handle_scene_key<F>(
    inbox: Option<&mut Vec<DeferredInput>>,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let params = require_params(params)?;
    let key = params
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("params.key missing or not a string"))?;
    if key.is_empty() {
        return Err(RpcError::invalid_params("params.key must not be empty"));
    }
    // R882 §5.49 §5.39 — the optional keyboard edge: absent = the
    // legacy atomic press; "down" / "up" mirror the winit Pressed /
    // Released edges (held-key absolute state — the Space pan chord).
    // Out-of-vocabulary values reject loudly (no silent atomic press).
    let state_param = params.get("state");
    let state = match state_param {
        None => KeyWireState::Press,
        Some(value) => {
            let name = value.as_str().ok_or_else(|| {
                RpcError::invalid_params("params.state must be a string (\"down\" | \"up\")")
            })?;
            KeyWireState::from_wire_param(Some(name)).ok_or_else(|| {
                RpcError::invalid_params("params.state must be \"down\" or \"up\"")
            })?
        }
    };
    // R51.202 §5.49 — key location is either an explicit cursor
    // coordinate or a tag lookup via the paint scene, mirroring
    // `scene/click`'s shape. R882.1 — a `state:"up"` edge is
    // positionless (the winit `Released` mirror carries no cursor and
    // the drain dispatches nothing for it), so `at`/`path` is optional
    // there; the placeholder coordinate is never consumed (the drain
    // gates its cursor move on `dispatches()`).
    let (x, y) = if state == KeyWireState::Up
        && params.get("at").is_none()
        && params.get("path").is_none()
    {
        (0.0, 0.0)
    } else {
        resolve_at_or_path(params, paint_producer, last_paint_scene)?
    };
    // R666 §5.37 — single-codepoint vs multi-codepoint discriminator.
    // `chars().count()` is the Unicode-scalar-value count, so
    // pre-composed CJK syllables like `"안"` (one codepoint) still
    // route as Character; multi-syllable IME composition output
    // like `"안녕"` (two codepoints) routes as Named and gets
    // rejected by `apply_key`'s `is_printable_key` predicate, which
    // is the correct fall-through to the R56.1.g preedit substrate.
    let mut chars = key.chars();
    let first = chars.next();
    let single_codepoint = first.is_some() && chars.next().is_none();
    if single_codepoint {
        inbox.push(DeferredInput::CharacterKey {
            x,
            y,
            character: key.to_owned(),
            state,
        });
    } else {
        inbox.push(DeferredInput::Key {
            x,
            y,
            key: key.to_owned(),
            state,
        });
    }
    Ok(Value::Null)
}

/// §5.49 §5.39 — `scene/type` batch text-injection handler: the keyboard
/// sibling of [`handle_scene_drag`]'s batched pointer gesture. Fans the
/// `text` string out into one [`DeferredInput::CharacterKey`] per Unicode
/// scalar, in order, so an AI drives a whole string in a single RPC the
/// way a human types it — instead of one `scene/key` per codepoint
/// (`"claude"` was 6 calls). Every codepoint takes the exact character-key
/// path `scene/key` uses (drain → `handle_character_key` → `V::keybinding`
/// typed-event then `apply_key` fallback), so the synthesised keystrokes
/// are indistinguishable from real ones at the input boundary: terminals,
/// text fields, and code editors all receive the text "as typed".
///
/// Pure text only (scope decision): `text` is injected verbatim,
/// codepoint by codepoint. Control characters are NOT mapped to named-key
/// edges — a `"\n"` in `text` is the literal newline scalar, not an Enter
/// key. A client that wants to submit composes an explicit
/// `scene/key {key: "Enter"}` after the `scene/type`, keeping the text
/// vocabulary and the named-key vocabulary separate on the wire (the same
/// separation `scene/drag` keeps from `scene/click`).
///
/// `at` / `path` resolve the focus target exactly as [`handle_scene_key`]
/// does (exactly one required); every enqueued codepoint carries the same
/// resolved coordinate, so the drain's leading `cursor_moved` re-targets
/// the same focused point idempotently. All N codepoints enqueue within
/// this one call, so the embedder drains them in order in a single cycle.
/// Returns `null` on success (mirrors every deferred-input sibling —
/// `scene/key` / `scene/drag` / `scene/wheel`); the client follows up with
/// `scene/snapshot` to observe the typed result.
fn handle_scene_type<F>(
    inbox: Option<&mut Vec<DeferredInput>>,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let params = require_params(params)?;
    let text = params
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("params.text missing or not a string"))?;
    if text.is_empty() {
        return Err(RpcError::invalid_params("params.text must not be empty"));
    }
    // Resolve the target once — the same focused point re-targets every
    // codepoint (the drain's `cursor_moved` to an unchanged point is a
    // no-op). Exactly one of `at` / `path` is required, as with `scene/key`.
    let (x, y) = resolve_at_or_path(params, paint_producer, last_paint_scene)?;
    // Fan out: each Unicode scalar becomes one atomic character keypress
    // (`KeyWireState::Press` — the same edge `scene/key` uses when no
    // `state` is given). CharacterKey routes through `V::keybinding` then
    // `apply_key`, so a printable scalar is deposited as typed text and a
    // non-printable one falls through the same predicate `scene/key` does.
    for character in text.chars() {
        inbox.push(DeferredInput::CharacterKey {
            x,
            y,
            character: character.to_string(),
            state: KeyWireState::Press,
        });
    }
    Ok(Value::Null)
}

/// R51.195 §5.49 §5.45 — `scene/wheel` typed dispatcher.
///
/// Params: `{at: {x: f64, y: f64}, delta: {lines: {dx, dy}} | {pixels: {dx, dy}}}`.
///
/// Enqueues a single [`DeferredInput::Wheel`] entry on the
/// dispatcher's inbox. The embedder drains the inbox after `dispatch`
/// returns and applies `cursor_moved(x, y)` followed by
/// `wheel(delta)` so the [`InputRouter`](pinion_runtime::InputRouter)
/// fires under its normal post-frame redraw rules — the dispatcher
/// holds `&mut scene` for the whole call, which prevents direct
/// `ShellCore::wheel` access from inside it. Returns `null` on
/// success; the AI client follows up with `scene/snapshot` to observe
/// the post-wheel offset change.
fn handle_scene_wheel<F>(
    inbox: Option<&mut Vec<DeferredInput>>,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let params = require_params(params)?;
    let delta_obj = params
        .get("delta")
        .and_then(Value::as_object)
        .ok_or_else(|| RpcError::invalid_params("params.delta missing or not an object"))?;
    let delta = parse_wheel_delta(delta_obj)?;
    // R51.202 §5.49 — wheel target is either an explicit cursor
    // coordinate or a tag lookup via the paint scene.
    let (x, y) = resolve_at_or_path(params, paint_producer, last_paint_scene)?;
    inbox.push(DeferredInput::Wheel { x, y, delta });
    Ok(Value::Null)
}

fn parse_wheel_delta(obj: &serde_json::Map<String, Value>) -> Result<WheelDelta, RpcError> {
    let extract =
        |key: &str| -> Result<Option<(f32, f32)>, RpcError> {
            let Some(inner) = obj.get(key).and_then(Value::as_object) else {
                return Ok(None);
            };
            let dx = inner.get("dx").and_then(Value::as_f64).ok_or_else(|| {
                RpcError::invalid_params(format!("params.delta.{key}.dx missing"))
            })?;
            let dy = inner.get("dy").and_then(Value::as_f64).ok_or_else(|| {
                RpcError::invalid_params(format!("params.delta.{key}.dy missing"))
            })?;
            // JSON Number is f64 wire-side; WheelDelta stores f32. The
            // downcast is the wire-format boundary — application-level
            // wheel deltas never exceed f32 precision in practice
            // (winit / web / iOS all originate the value as f32).
            #[allow(
                clippy::cast_possible_truncation,
                reason = "JSON f64 → WheelDelta f32 wire boundary; loss is intentional"
            )]
            let pair = (dx as f32, dy as f32);
            Ok(Some(pair))
        };
    match (extract("lines")?, extract("pixels")?) {
        (Some(_), Some(_)) => Err(RpcError::invalid_params(
            "params.delta carries both \"lines\" and \"pixels\"; pick one",
        )),
        (Some((dx, dy)), None) => Ok(WheelDelta::Lines { dx, dy }),
        (None, Some((dx, dy))) => Ok(WheelDelta::Pixels { dx, dy }),
        (None, None) => Err(RpcError::invalid_params(
            "params.delta requires either \"lines\" or \"pixels\"",
        )),
    }
}

fn parse_snapshot_viewport(params: &Value) -> Result<(u32, u32), RpcError> {
    let Some(vp) = params.get("viewport") else {
        return Ok((720, 480));
    };
    let Some(obj) = vp.as_object() else {
        return Err(RpcError::invalid_params(
            "params.viewport must be an object {w, h}",
        ));
    };
    let w = obj
        .get("w")
        .and_then(Value::as_u64)
        .ok_or_else(|| RpcError::invalid_params("params.viewport.w missing or not a u64"))?;
    let h = obj
        .get("h")
        .and_then(Value::as_u64)
        .ok_or_else(|| RpcError::invalid_params("params.viewport.h missing or not a u64"))?;
    let w = u32::try_from(w)
        .map_err(|_| RpcError::invalid_params("params.viewport.w out of u32 range"))?;
    let h = u32::try_from(h)
        .map_err(|_| RpcError::invalid_params("params.viewport.h out of u32 range"))?;
    Ok((w, h))
}

fn snapshot_error_to_rpc(err: SnapshotError) -> RpcError {
    let variant = match err {
        SnapshotError::Path(_) => "Path",
        SnapshotError::UnsupportedPath => "UnsupportedPath",
    };
    RpcError::invalid_params(variant)
}

fn snapshot_node_to_json(node: SnapshotNode) -> Value {
    let mut obj = serde_json::Map::new();
    let type_tag = match &node {
        SnapshotNode::Box(_) => "Box",
        SnapshotNode::Text(_) => "Text",
        SnapshotNode::Path(_) => "Path",
        SnapshotNode::Image(_) => "Image",
        SnapshotNode::Container(_) => "Container",
        SnapshotNode::Effect => "Effect",
        SnapshotNode::External(_) => "External",
        SnapshotNode::Scroll(_) => "Scroll",
        // R681 §2 #4 — `ImmediateModeNode` payload wire shape.
        SnapshotNode::ImmediateModeNode(_) => "ImmediateModeNode",
        // R972 §5.41 — cell-native text-grid geometry wire shape.
        SnapshotNode::TextGrid(_) => "TextGrid",
        // `SnapshotNode::Unknown` and future non_exhaustive additions
        // collapse to "Unknown".
        _ => "Unknown",
    };
    obj.insert("type".to_string(), Value::String(type_tag.to_string()));

    match node {
        SnapshotNode::Box(snap) => {
            obj.insert("rect".to_string(), snapshot_rect_to_json(snap.rect));
            obj.insert("tag".to_string(), snapshot_tag_to_json(snap.tag.as_deref()));
            obj.insert("style".to_string(), box_style_to_json(&snap.style));
        }
        SnapshotNode::Text(snap) => {
            obj.insert("rect".to_string(), snapshot_rect_to_json(snap.rect));
            obj.insert("tag".to_string(), snapshot_tag_to_json(snap.tag.as_deref()));
            obj.insert("content".to_string(), Value::String(snap.content));
            obj.insert("style".to_string(), text_style_to_json(&snap.style));
            // R713 §5.36 — styled-run spans (empty for single-style
            // text). Each run reports its byte range + resolved style
            // so AI clients read `RichText` structure as data.
            let runs: Vec<Value> = snap.runs.iter().map(style_run_to_json).collect();
            obj.insert("runs".to_string(), Value::Array(runs));
        }
        SnapshotNode::Path(snap) => {
            obj.insert("rect".to_string(), snapshot_rect_to_json(snap.rect));
            obj.insert("tag".to_string(), snapshot_tag_to_json(snap.tag.as_deref()));
            let cmds: Vec<Value> = snap.commands.iter().map(path_command_to_json).collect();
            obj.insert("commands".to_string(), Value::Array(cmds));
            obj.insert("style".to_string(), path_style_to_json(&snap.style));
        }
        SnapshotNode::Image(snap) => {
            obj.insert("rect".to_string(), snapshot_rect_to_json(snap.rect));
            obj.insert("tag".to_string(), snapshot_tag_to_json(snap.tag.as_deref()));
            obj.insert("source".to_string(), Value::String(snap.source));
            obj.insert("style".to_string(), image_style_to_json(snap.style));
        }
        SnapshotNode::External(snap) => {
            obj.insert("rect".to_string(), snapshot_rect_to_json(snap.rect));
            obj.insert("tag".to_string(), snapshot_tag_to_json(snap.tag.as_deref()));
            match snap.introspect {
                Some(fields) => {
                    let mut intro = serde_json::Map::new();
                    for (name, value) in fields {
                        intro.insert(name, introspect_value_to_json(value));
                    }
                    obj.insert("introspect".to_string(), Value::Object(intro));
                }
                None => {
                    obj.insert("introspect".to_string(), Value::Null);
                }
            }
        }
        SnapshotNode::Container(snap) => {
            obj.insert("rect".to_string(), snapshot_rect_to_json(snap.rect));
            obj.insert("tag".to_string(), snapshot_tag_to_json(snap.tag.as_deref()));
            obj.insert("style".to_string(), box_style_to_json(&snap.style));
            let children = snap
                .children
                .into_iter()
                .map(snapshot_node_to_json)
                .collect();
            obj.insert("children".to_string(), Value::Array(children));
        }
        SnapshotNode::Scroll(snap) => {
            obj.insert("tag".to_string(), snapshot_tag_to_json(snap.tag.as_deref()));
            obj.insert("viewport".to_string(), snapshot_rect_to_json(snap.viewport));
            obj.insert("offset_x".to_string(), Value::Number(snap.offset_x.into()));
            obj.insert("offset_y".to_string(), Value::Number(snap.offset_y.into()));
            // R784 — expose the scroll axis (AI distinguishes h/v scroller).
            obj.insert("axis".to_string(), Value::String(snap.axis.to_string()));
            obj.insert("content".to_string(), snapshot_node_to_json(*snap.content));
        }
        // R681 §2 #4 — wire serialisation for `ImmediateModeNode`.
        // Exposes the §5.20 tag (so `find_by_tag` walks resolve),
        // the post-layout viewport (so `node_center` works the
        // same as for any other primitive), and the per-paint
        // `last_dt_micros` sidecar (substrate-published delta the
        // AI client can verify the game-loop pacing against).
        SnapshotNode::ImmediateModeNode(snap) => {
            obj.insert("tag".to_string(), snapshot_tag_to_json(snap.tag.as_deref()));
            obj.insert("viewport".to_string(), snapshot_rect_to_json(snap.viewport));
            // Also expose the viewport under `rect` so the generic
            // `node_center` / `rect_of` helpers in the test harness
            // resolve uniformly (they probe `rect` first; only
            // `Scroll` falls back to `viewport`).
            obj.insert("rect".to_string(), snapshot_rect_to_json(snap.viewport));
            obj.insert(
                "last_dt_micros".to_string(),
                Value::Number(snap.last_dt_micros.into()),
            );
        }
        // R972 §5.41 — text-grid geometry: pixel `rect` + node-local
        // cell metric + derived winsize `(cols, rows)`. Extracted to a
        // helper so this dispatcher stays under the pedantic line budget.
        SnapshotNode::TextGrid(snap) => text_grid_snapshot_fields(&mut obj, &snap),
        _ => {}
    }

    Value::Object(obj)
}

/// R972 §5.41 — wire fields for a [`SnapshotNode::TextGrid`]. An AI
/// client reconstructs the whole cell↔pixel mapping from `rect` + the
/// node-local metric (`cell_w` / `cell_h`) + the derived winsize
/// `(cols, rows)` — the §2 #7 scene-as-data contract, no OCR.
fn text_grid_snapshot_fields(obj: &mut serde_json::Map<String, Value>, snap: &TextGridSnapshot) {
    obj.insert("rect".to_string(), snapshot_rect_to_json(snap.rect));
    obj.insert("tag".to_string(), snapshot_tag_to_json(snap.tag.as_deref()));
    obj.insert("cell_w".to_string(), Value::Number(snap.cell_w.into()));
    obj.insert("cell_h".to_string(), Value::Number(snap.cell_h.into()));
    obj.insert("cols".to_string(), Value::Number(snap.cols.into()));
    obj.insert("rows".to_string(), Value::Number(snap.rows.into()));
    // R974.1 §5.41 — the projection's OWN dims (size the producer last
    // sent), distinct from the layout-derived winsize `(cols, rows)` it is
    // told to size to; an AI compares the two to detect a resize-lag
    // divergence directly. `0×0` for a geometry-only grid.
    obj.insert(
        "buffer_cols".to_string(),
        Value::Number(snap.buffer_cols.into()),
    );
    obj.insert(
        "buffer_rows".to_string(),
        Value::Number(snap.buffer_rows.into()),
    );
    // R973 §5.41 — the cell-content projection: one entry per row, each
    // with the row text and its palette-resolved style runs.
    obj.insert(
        "grid_rows".to_string(),
        Value::Array(snap.grid_rows.iter().map(grid_row_to_json).collect()),
    );
    // R975 §5.41 — the grid's single cursor (position / shape / visible).
    obj.insert("cursor".to_string(), grid_cursor_to_json(&snap.cursor));
    // R977 §5.41 — which screen this projection is (main / alternate).
    obj.insert("screen".to_string(), Value::String(snap.screen.to_string()));
}

/// R975 §5.41 — wire form for a [`GridCursorSnapshot`]: `{col, row, shape,
/// visible}`. `shape` is the wire string `"block"` / `"bar"` /
/// `"underline"`; a client tests `(col, row)` against the grid's `(cols,
/// rows)` to know whether the cursor is in bounds.
fn grid_cursor_to_json(cursor: &GridCursorSnapshot) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("col".to_string(), Value::Number(cursor.col.into()));
    obj.insert("row".to_string(), Value::Number(cursor.row.into()));
    obj.insert("shape".to_string(), Value::String(cursor.shape.to_string()));
    obj.insert("visible".to_string(), Value::Bool(cursor.visible));
    Value::Object(obj)
}

/// R973 §5.41 — wire form for one [`GridRowSnapshot`]: `{text, runs,
/// generation}` (R978 added the per-row damage `generation`).
fn grid_row_to_json(row: &GridRowSnapshot) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("text".to_string(), Value::String(row.text.clone()));
    obj.insert(
        "runs".to_string(),
        Value::Array(row.runs.iter().map(grid_style_run_to_json).collect()),
    );
    obj.insert(
        "generation".to_string(),
        Value::Number(row.generation.into()),
    );
    Value::Object(obj)
}

/// R973 §5.41 — wire form for one [`GridStyleRun`]: `{start, len, fg, bg,
/// attrs, width}` (R974 added the SGR `attrs` object; R976 added the
/// `width` role `"narrow"` / `"wide"` / `"trailer"`).
fn grid_style_run_to_json(run: &GridStyleRun) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("start".to_string(), Value::Number(run.start.into()));
    obj.insert("len".to_string(), Value::Number(run.len.into()));
    obj.insert("fg".to_string(), term_color_snapshot_to_json(&run.fg));
    obj.insert("bg".to_string(), term_color_snapshot_to_json(&run.bg));
    obj.insert("attrs".to_string(), cell_attrs_to_json(run.attrs));
    obj.insert("width".to_string(), Value::String(run.width.to_string()));
    Value::Object(obj)
}

/// R974 §5.41 — wire form for [`CellAttrs`]: the SGR flags as named
/// booleans. `reverse` is the cell's stored flag; a renderer swaps the
/// effective fg / bg for it at paint time.
///
/// [`CellAttrs`]: pinion_core::CellAttrs
fn cell_attrs_to_json(attrs: pinion_core::CellAttrs) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("bold".to_string(), Value::Bool(attrs.bold));
    obj.insert("dim".to_string(), Value::Bool(attrs.dim));
    obj.insert("italic".to_string(), Value::Bool(attrs.italic));
    obj.insert("underline".to_string(), Value::Bool(attrs.underline));
    obj.insert("blink".to_string(), Value::Bool(attrs.blink));
    obj.insert("reverse".to_string(), Value::Bool(attrs.reverse));
    obj.insert("hidden".to_string(), Value::Bool(attrs.hidden));
    obj.insert(
        "strikethrough".to_string(),
        Value::Bool(attrs.strikethrough),
    );
    Value::Object(obj)
}

/// R973 §5.41 — wire form for one [`TermColorSnapshot`]: the stored
/// `kind` / `index` plus the palette-resolved `rgb` hex.
fn term_color_snapshot_to_json(color: &TermColorSnapshot) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("kind".to_string(), Value::String(color.kind.to_string()));
    obj.insert(
        "index".to_string(),
        color.index.map_or(Value::Null, |i| Value::Number(i.into())),
    );
    obj.insert("rgb".to_string(), Value::String(color.rgb.clone()));
    Value::Object(obj)
}

fn snapshot_tag_to_json(tag: Option<&str>) -> Value {
    match tag {
        Some(t) => Value::String(t.to_string()),
        None => Value::Null,
    }
}

/// R51.198 carry §5.49 — wire serialization for `PathCommand`. Each
/// command becomes a JSON object with a `type` discriminator plus the
/// variant's payload (`point` for `MoveTo`/`LineTo`; `c1`/`c2`/`end`
/// for `CurveTo`; no payload for `Close`). The wildcard arm collapses
/// future `non_exhaustive` additions to `"Unknown"` so the wire stays
/// forward-compatible.
fn path_command_to_json(cmd: &pinion_core::scene::PathCommand) -> Value {
    use pinion_core::scene::PathCommand;
    let point_to_json = |p: &pinion_core::scene::PathPoint| -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "x".to_string(),
            serde_json::Number::from_f64(f64::from(p.x)).map_or(Value::Null, Value::Number),
        );
        obj.insert(
            "y".to_string(),
            serde_json::Number::from_f64(f64::from(p.y)).map_or(Value::Null, Value::Number),
        );
        Value::Object(obj)
    };
    let mut obj = serde_json::Map::new();
    match cmd {
        PathCommand::MoveTo(p) => {
            obj.insert("type".to_string(), Value::String("MoveTo".to_string()));
            obj.insert("point".to_string(), point_to_json(p));
        }
        PathCommand::LineTo(p) => {
            obj.insert("type".to_string(), Value::String("LineTo".to_string()));
            obj.insert("point".to_string(), point_to_json(p));
        }
        PathCommand::CurveTo { c1, c2, end } => {
            obj.insert("type".to_string(), Value::String("CurveTo".to_string()));
            obj.insert("c1".to_string(), point_to_json(c1));
            obj.insert("c2".to_string(), point_to_json(c2));
            obj.insert("end".to_string(), point_to_json(end));
        }
        PathCommand::Close => {
            obj.insert("type".to_string(), Value::String("Close".to_string()));
        }
        // R51.198 carry §5.49 — `PathCommand` is `non_exhaustive`;
        // surface future variants as `"Unknown"` markers so the wire
        // stays forward-compatible.
        _ => {
            obj.insert("type".to_string(), Value::String("Unknown".to_string()));
        }
    }
    Value::Object(obj)
}

fn snapshot_rect_to_json(rect: pinion_core::scene::Rect) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("x".to_string(), Value::Number(rect.x.into()));
    obj.insert("y".to_string(), Value::Number(rect.y.into()));
    obj.insert("w".to_string(), Value::Number(rect.w.into()));
    obj.insert("h".to_string(), Value::Number(rect.h.into()));
    Value::Object(obj)
}

/// R55.G.8 §5.49 — wire serialization for `pinion_core::Color`. Emits
/// `{r, g, b, a}` 4-tuple with `u8` channel values. AI clients can
/// compare against literal channel values or compute derived shapes
/// (luminance / contrast) without parsing CSS hex syntax.
fn color_to_json(color: pinion_core::Color) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("r".to_string(), Value::Number(color.r.into()));
    obj.insert("g".to_string(), Value::Number(color.g.into()));
    obj.insert("b".to_string(), Value::Number(color.b.into()));
    obj.insert("a".to_string(), Value::Number(color.a.into()));
    Value::Object(obj)
}

/// R55.G.8 §5.49 — wire serialization for `BorderPlacement`. The enum
/// is `non_exhaustive`, so a wildcard arm collapses future variants to
/// `"Unknown"` for forward-compat (mirrors the `PathCommand` shape).
fn border_placement_to_json(p: pinion_core::style::BorderPlacement) -> Value {
    use pinion_core::style::BorderPlacement;
    let name = match p {
        BorderPlacement::Inside => "Inside",
        BorderPlacement::Center => "Center",
        BorderPlacement::Outside => "Outside",
        _ => "Unknown",
    };
    Value::String(name.to_string())
}

/// R55.G.8 §5.49 — wire serialization for `Border`. Surfaces colour /
/// width / placement so AI clients can verify a widget's outline
/// chrome without inspecting pixels.
fn border_to_json(border: pinion_core::style::Border) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("color".to_string(), color_to_json(border.color));
    obj.insert("width".to_string(), Value::Number(border.width.into()));
    obj.insert(
        "placement".to_string(),
        border_placement_to_json(border.placement),
    );
    Value::Object(obj)
}

/// R708 §5.50 — wire serialization for `Extend`. The enum is
/// `non_exhaustive`, so a wildcard arm collapses future variants to
/// `"Unknown"` for forward-compat (mirrors the `BorderPlacement` shape).
fn extend_to_json(extend: pinion_core::style::Extend) -> Value {
    use pinion_core::style::Extend;
    let name = match extend {
        Extend::Pad => "Pad",
        Extend::Repeat => "Repeat",
        Extend::Reflect => "Reflect",
        _ => "Unknown",
    };
    Value::String(name.to_string())
}

/// R708 §5.50 — wire serialization for a `Gradient`. Surfaces the
/// geometry kind (`linear` start/end or `radial` center/radius, all in
/// box-relative UV), the ordered `stops` (`offset` + `color`), and the
/// `extend` mode so AI clients can read back the exact gradient ramp a
/// box paints without inspecting pixels (§2 #7 scene-as-data).
fn gradient_to_json(gradient: &pinion_core::style::Gradient) -> Value {
    use pinion_core::style::GradientKind;
    let mut obj = serde_json::Map::new();
    let mut kind = serde_json::Map::new();
    match gradient.kind {
        GradientKind::Linear { start, end } => {
            kind.insert("kind".to_string(), Value::String("linear".to_string()));
            kind.insert("start".to_string(), uv_to_json(start));
            kind.insert("end".to_string(), uv_to_json(end));
        }
        GradientKind::Radial { center, radius } => {
            kind.insert("kind".to_string(), Value::String("radial".to_string()));
            kind.insert("center".to_string(), uv_to_json(center));
            kind.insert("radius".to_string(), f32_to_json(radius));
        }
    }
    obj.insert("geometry".to_string(), Value::Object(kind));
    let stops: Vec<Value> = gradient
        .stops
        .iter()
        .map(|stop| {
            let mut s = serde_json::Map::new();
            s.insert("offset".to_string(), f32_to_json(stop.offset));
            s.insert("color".to_string(), color_to_json(stop.color));
            Value::Object(s)
        })
        .collect();
    obj.insert("stops".to_string(), Value::Array(stops));
    obj.insert("extend".to_string(), extend_to_json(gradient.extend));
    Value::Object(obj)
}

/// R708 §5.50 — serialize a box-relative UV point `(u, v)` as
/// `{"u": .., "v": ..}`.
fn uv_to_json(uv: (f32, f32)) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("u".to_string(), f32_to_json(uv.0));
    obj.insert("v".to_string(), f32_to_json(uv.1));
    Value::Object(obj)
}

/// R708 §5.50 — serialize an `f32` as a JSON number, falling back to
/// `null` for any non-finite value (`serde_json` rejects NaN/inf).
fn f32_to_json(x: f32) -> Value {
    serde_json::Number::from_f64(f64::from(x)).map_or(Value::Null, Value::Number)
}

/// R710 §5.50 — wire serialization for a [`BoxShadow`]. Surfaces the
/// cast colour, the `(offset.x, offset.y)` translation, the gaussian
/// `blur` radius and the `spread`, so AI clients can read back a box's
/// elevation without sampling pixels (§2 #7 scene-as-data).
///
/// [`BoxShadow`]: pinion_core::style::BoxShadow
fn shadow_to_json(shadow: &pinion_core::style::BoxShadow) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("color".to_string(), color_to_json(shadow.color));
    let mut offset = serde_json::Map::new();
    offset.insert("x".to_string(), f32_to_json(shadow.offset_x));
    offset.insert("y".to_string(), f32_to_json(shadow.offset_y));
    obj.insert("offset".to_string(), Value::Object(offset));
    obj.insert("blur".to_string(), f32_to_json(shadow.blur));
    obj.insert("spread".to_string(), f32_to_json(shadow.spread));
    Value::Object(obj)
}

/// R55.G.8 §5.49 — wire serialization for `BoxStyle`. Surfaces fill,
/// optional border (null when absent), `corner_radius`, the R708
/// optional `gradient` overlay (null when absent), and the R710
/// `shadows` list (empty array when none) so AI clients can introspect
/// the rendered look of any `BoxNode` or `ContainerNode` without OCR
/// (§2 #7 scene-as-data).
fn box_style_to_json(style: &pinion_core::style::BoxStyle) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("fill".to_string(), color_to_json(style.fill));
    obj.insert(
        "border".to_string(),
        style.border.map_or(Value::Null, border_to_json),
    );
    obj.insert(
        "corner_radius".to_string(),
        Value::Number(style.corner_radius.into()),
    );
    obj.insert(
        "gradient".to_string(),
        style
            .gradient
            .as_ref()
            .map_or(Value::Null, gradient_to_json),
    );
    obj.insert(
        "shadows".to_string(),
        Value::Array(style.shadows.iter().map(shadow_to_json).collect()),
    );
    Value::Object(obj)
}

/// R55.G.8 §5.49 — wire serialization for `FontStyle`. The
/// upright / italic variants serialize as their bare name; `Oblique`
/// emits a `{kind, angle}` object so the optional CCW degrees survive
/// the wire. Wildcard arm collapses future variants to `"Unknown"`.
fn font_style_to_json(style: pinion_core::style::FontStyle) -> Value {
    use pinion_core::style::FontStyle;
    match style {
        FontStyle::Normal => Value::String("Normal".to_string()),
        FontStyle::Italic => Value::String("Italic".to_string()),
        FontStyle::Oblique(angle) => {
            let mut obj = serde_json::Map::new();
            obj.insert("kind".to_string(), Value::String("Oblique".to_string()));
            obj.insert(
                "angle".to_string(),
                angle.map_or(Value::Null, |deg| Value::Number(deg.into())),
            );
            Value::Object(obj)
        }
        _ => Value::String("Unknown".to_string()),
    }
}

/// R55.G.11 §5.49 — wire serialization for `StrokeCap`. Bare string
/// per variant; wildcard arm collapses future variants to `"Unknown"`.
fn stroke_cap_to_json(cap: pinion_core::style::StrokeCap) -> Value {
    use pinion_core::style::StrokeCap;
    let name = match cap {
        StrokeCap::Butt => "Butt",
        StrokeCap::Round => "Round",
        StrokeCap::Square => "Square",
        _ => "Unknown",
    };
    Value::String(name.to_string())
}

/// R55.G.11 §5.49 — wire serialization for `Stroke`. Surfaces colour,
/// width, and cap policy so AI clients can verify a path's ink stroke
/// without inspecting pixels.
fn stroke_to_json(stroke: pinion_core::style::Stroke) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("color".to_string(), color_to_json(stroke.color));
    obj.insert("width".to_string(), Value::Number(stroke.width.into()));
    obj.insert("cap".to_string(), stroke_cap_to_json(stroke.cap));
    Value::Object(obj)
}

/// R55.G.11 §5.49 — wire serialization for `PathStyle`. Both arms are
/// optional (a Path may stroke without filling or vice versa), so the
/// wire keeps them as `null`-able fields.
fn path_style_to_json(style: &pinion_core::style::PathStyle) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "stroke".to_string(),
        style.stroke.map_or(Value::Null, stroke_to_json),
    );
    obj.insert(
        "fill".to_string(),
        style.fill.map_or(Value::Null, color_to_json),
    );
    // R722 §5.50 — gradient fill (reuses the BoxStyle gradient wire).
    obj.insert(
        "gradient".to_string(),
        style
            .gradient
            .as_ref()
            .map_or(Value::Null, gradient_to_json),
    );
    Value::Object(obj)
}

/// R55.G.11 §5.49 — wire serialization for `Fit`. Bare string per
/// variant (CSS `object-fit` vocabulary); wildcard arm for future
/// additions.
fn fit_to_json(fit: pinion_core::style::Fit) -> Value {
    use pinion_core::style::Fit;
    let name = match fit {
        Fit::Fill => "Fill",
        Fit::Contain => "Contain",
        Fit::Cover => "Cover",
        Fit::Tile => "Tile",
        _ => "Unknown",
    };
    Value::String(name.to_string())
}

/// R55.G.11 §5.49 — wire serialization for `ImageStyle`. `tint` is
/// optional (a `None` tint paints the source as-is) so the wire emits
/// `null` when absent.
fn image_style_to_json(style: pinion_core::style::ImageStyle) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("fit".to_string(), fit_to_json(style.fit));
    obj.insert(
        "tint".to_string(),
        style.tint.map_or(Value::Null, color_to_json),
    );
    Value::Object(obj)
}

/// R55.G.10 §5.49 — wire serialization for `LineHeight`. `Normal`
/// serializes as a bare string; the data-bearing variants emit a
/// `{kind, value}` object (matching the `FontStyle::Oblique` shape).
/// Wildcard arm collapses future variants to `"Unknown"`.
fn line_height_to_json(lh: pinion_core::style::LineHeight) -> Value {
    use pinion_core::style::LineHeight;
    match lh {
        LineHeight::Normal => Value::String("Normal".to_string()),
        LineHeight::Px(px) => {
            let mut obj = serde_json::Map::new();
            obj.insert("kind".to_string(), Value::String("Px".to_string()));
            obj.insert("value".to_string(), Value::Number(px.into()));
            Value::Object(obj)
        }
        LineHeight::MultiplierX100(m) => {
            let mut obj = serde_json::Map::new();
            obj.insert(
                "kind".to_string(),
                Value::String("MultiplierX100".to_string()),
            );
            obj.insert("value".to_string(), Value::Number(m.into()));
            Value::Object(obj)
        }
        _ => Value::String("Unknown".to_string()),
    }
}

/// R55.G.10 §5.49 — wire serialization for `TextAlign`. Bare string
/// for each variant; wildcard catches future additions.
fn text_align_to_json(a: pinion_core::style::TextAlign) -> Value {
    use pinion_core::style::TextAlign;
    let name = match a {
        TextAlign::Start => "Start",
        TextAlign::Center => "Center",
        TextAlign::End => "End",
        TextAlign::Justify => "Justify",
        _ => "Unknown",
    };
    Value::String(name.to_string())
}

/// R55.G.10 §5.49 — wire serialization for `TextDecoration`. Both
/// flags may be `true` simultaneously (Figma allows underline +
/// strikethrough combo), so the wire keeps them as independent bools.
fn text_decoration_to_json(d: pinion_core::style::TextDecoration) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("underline".to_string(), Value::Bool(d.underline));
    obj.insert("strikethrough".to_string(), Value::Bool(d.strikethrough));
    Value::Object(obj)
}

/// R55.G.10 §5.49 — wire serialization for `TextOverflow`. Bare
/// string per variant; wildcard arm for future additions.
fn text_overflow_to_json(o: pinion_core::style::TextOverflow) -> Value {
    use pinion_core::style::TextOverflow;
    let name = match o {
        TextOverflow::Visible => "Visible",
        TextOverflow::Clip => "Clip",
        TextOverflow::Ellipsis => "Ellipsis",
        _ => "Unknown",
    };
    Value::String(name.to_string())
}

/// R55.G.8 + R55.G.10 §5.49 — wire serialization for `TextStyle`.
/// G.8 landed the visual axis (family / size / colour / weight /
/// style); G.10 extended the wire to the layout axis (line-height,
/// letter-spacing, text-align, decoration, overflow) so AI clients
/// can introspect every rendered typography knob without OCR
/// (§2 #7 scene-as-data completeness).
/// R713 §5.36 — serialize a [`StyleRun`] to wire JSON: its UTF-8 byte
/// range plus the fully-resolved per-span [`TextStyle`]. Mirrors
/// `text_style_to_json` for the `style` field so a styled-run span is
/// introspected with the same shape as the base style.
///
/// [`StyleRun`]: pinion_core::scene::StyleRun
fn style_run_to_json(run: &pinion_core::scene::StyleRun) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("start".to_string(), Value::Number(run.start.into()));
    obj.insert("end".to_string(), Value::Number(run.end.into()));
    obj.insert("style".to_string(), text_style_to_json(&run.style));
    Value::Object(obj)
}

fn text_style_to_json(style: &pinion_core::style::TextStyle) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "font_family".to_string(),
        style
            .font_family
            .as_ref()
            .map_or(Value::Null, |f| Value::String(f.as_wire().into_owned())),
    );
    obj.insert(
        "font_size_px".to_string(),
        Value::Number(style.font_size_px.into()),
    );
    obj.insert("fg_color".to_string(), color_to_json(style.fg_color));
    obj.insert(
        "font_weight".to_string(),
        Value::Number(style.font_weight.0.into()),
    );
    obj.insert(
        "font_style".to_string(),
        font_style_to_json(style.font_style),
    );
    obj.insert(
        "line_height".to_string(),
        line_height_to_json(style.line_height),
    );
    obj.insert(
        "letter_spacing".to_string(),
        Value::Number(style.letter_spacing.into()),
    );
    obj.insert(
        "text_align".to_string(),
        text_align_to_json(style.text_align),
    );
    obj.insert(
        "decoration".to_string(),
        text_decoration_to_json(style.decoration),
    );
    obj.insert(
        "overflow".to_string(),
        text_overflow_to_json(style.overflow),
    );
    Value::Object(obj)
}

fn handle_scene_dry_run(scene: &mut Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.path missing or not a string",
        ));
    };
    let Some(value_json) = params.get("value") else {
        return Err(RpcError::invalid_params("params.value missing"));
    };
    let Some(value) = json_to_introspect_value(value_json) else {
        return Err(RpcError::invalid_params(
            "params.value unsupported (v0: null/bool/number/string only)",
        ));
    };

    match dry_run(scene, path, value) {
        Ok(snap) => Ok(snapshot_node_to_json(snap)),
        Err(err) => Err(dry_run_error_to_rpc(err)),
    }
}

fn dry_run_error_to_rpc(err: DryRunError) -> RpcError {
    let variant = match err {
        DryRunError::Path(_) => "Path",
        DryRunError::UnsupportedPath => "UnsupportedPath",
        DryRunError::NoExternalAtPath => "NoExternalAtPath",
        DryRunError::IntrospectionOptedOut => "IntrospectionOptedOut",
        DryRunError::InitialQueryFailed => "InitialQueryFailed",
        DryRunError::Intervene(_) => "Intervene",
        DryRunError::RollbackFailed => "RollbackFailed",
        DryRunError::SnapshotFailed => "SnapshotFailed",
    };
    RpcError::invalid_params(variant)
}

/// R646 §5.12 — `scene/simulate` handler. Accepts
/// `{ steps: [{path, value}, ...] }` and dispatches into
/// [`crate::simulate::simulate`] for multi-event scenario
/// exploration. Returns the [`SnapshotNode`](crate::snapshot::SnapshotNode)
/// reflecting the compound hypothetical state; rollback is performed
/// before return so the live scene is unchanged.
///
/// R647 §5.22 R26 — when [`DispatchContext::runtime_owner`] is set,
/// route through [`simulate_with_owner`] so Signal graph state is
/// also snapshotted + restored. Without the owner the call falls
/// back to External-only rollback (R646 behaviour) — bindings that
/// have not registered a substrate owner still get the multi-event
/// composition shape.
fn handle_scene_simulate(
    scene: &mut Scene,
    runtime_owner: Option<&pinion_core::Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(steps_json) = params.get("steps").and_then(Value::as_array) else {
        return Err(RpcError::invalid_params(
            "params.steps missing or not an array",
        ));
    };
    if steps_json.is_empty() {
        return Err(RpcError::invalid_params(
            "params.steps empty (use scene/snapshot for the no-op case)",
        ));
    }
    let mut steps: Vec<SimulateStep> = Vec::with_capacity(steps_json.len());
    for (i, step_json) in steps_json.iter().enumerate() {
        let Some(obj) = step_json.as_object() else {
            return Err(RpcError::invalid_params(format!(
                "params.steps[{i}] not an object",
            )));
        };
        let Some(path) = obj.get("path").and_then(Value::as_str) else {
            return Err(RpcError::invalid_params(format!(
                "params.steps[{i}].path missing or not a string",
            )));
        };
        let Some(value_json) = obj.get("value") else {
            return Err(RpcError::invalid_params(format!(
                "params.steps[{i}].value missing",
            )));
        };
        let Some(value) = json_to_introspect_value(value_json) else {
            return Err(RpcError::invalid_params(format!(
                "params.steps[{i}].value unsupported (v0: null/bool/number/string only)",
            )));
        };
        steps.push(SimulateStep {
            path: path.to_string(),
            value,
        });
    }

    let outcome = match runtime_owner {
        Some(owner) => simulate_with_owner(scene, owner, &steps),
        None => simulate(scene, &steps),
    };
    match outcome {
        Ok(snap) => Ok(snapshot_node_to_json(snap)),
        Err(err) => Err(simulate_error_to_rpc(&err)),
    }
}

fn simulate_error_to_rpc(err: &SimulateError) -> RpcError {
    let variant = match err {
        SimulateError::Path { .. } => "Path",
        SimulateError::UnsupportedPath { .. } => "UnsupportedPath",
        SimulateError::NoExternalAtPath => "NoExternalAtPath",
        SimulateError::IntrospectionOptedOut => "IntrospectionOptedOut",
        SimulateError::InitialQueryFailed { .. } => "InitialQueryFailed",
        SimulateError::Intervene { .. } => "Intervene",
        SimulateError::RollbackFailed => "RollbackFailed",
        SimulateError::SnapshotFailed => "SnapshotFailed",
        SimulateError::EmptySteps => "EmptySteps",
    };
    RpcError::invalid_params(variant)
}

fn handle_scene_wait_for(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.path missing or not a string",
        ));
    };
    let Some(target_json) = params.get("target") else {
        return Err(RpcError::invalid_params("params.target missing"));
    };
    let Some(target) = json_to_introspect_value(target_json) else {
        return Err(RpcError::invalid_params(
            "params.target unsupported (v0: null/bool/number/string only)",
        ));
    };
    let Some(max_attempts) = params.get("max_attempts").and_then(Value::as_u64) else {
        return Err(RpcError::invalid_params(
            "params.max_attempts missing or not u64",
        ));
    };
    let max_attempts = u32::try_from(max_attempts)
        .map_err(|_| RpcError::invalid_params("params.max_attempts exceeds u32 range"))?;

    match wait_for(scene, path, &target, max_attempts) {
        Ok(outcome) => Ok(wait_outcome_to_json(&outcome)),
        Err(err) => Err(wait_for_error_to_rpc(err)),
    }
}

fn wait_outcome_to_json(outcome: &WaitOutcome) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("matched".to_string(), Value::Bool(outcome.matched));
    obj.insert(
        "attempts".to_string(),
        Value::Number(outcome.attempts.into()),
    );
    obj.insert(
        "final_value".to_string(),
        introspect_value_to_json(outcome.final_value.clone()),
    );
    Value::Object(obj)
}

fn wait_for_error_to_rpc(err: WaitForError) -> RpcError {
    let variant = match err {
        WaitForError::Path(_) => "Path",
        WaitForError::Query(_) => "Query",
        WaitForError::ZeroAttempts => "ZeroAttempts",
    };
    RpcError::invalid_params(variant)
}

fn handle_scene_screenshot(
    screenshot: Option<Screenshot>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.path missing or not a string",
        ));
    };
    // R1062 §5.12 — validate the path shape via the SSOT (optional
    // `/window[id]/` prefix + an EMPTY scene-path tail); the window scope
    // itself was already consumed by the AppShell entry to pick which
    // surface to capture.
    crate::screenshot::validate_screenshot_path(path).map_err(screenshot_error_to_rpc)?;
    // R1060 §5.12 §5.16 — return the embedder's pre-captured live-surface
    // pixels. Absent (headless / single-window entry with no live
    // surface, or a capture failure) → the typed `RenderBackendUnavailable`
    // the v0 stub always returned.
    match screenshot {
        Some(shot) => Ok(screenshot_to_json(&shot)),
        None => Err(screenshot_error_to_rpc(
            ScreenshotError::RenderBackendUnavailable,
        )),
    }
}

fn screenshot_to_json(shot: &Screenshot) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("width".to_string(), Value::Number(shot.width.into()));
    obj.insert("height".to_string(), Value::Number(shot.height.into()));
    if let Some(path) = &shot.out_path {
        // R1061 §5.12 — file-output mode: the embedder already wrote the
        // captured frame to `out_path` as PNG, so the wire returns the
        // path instead of a multi-MB `pixels_rgba8` array.
        obj.insert("out_path".to_string(), Value::String(path.clone()));
    } else {
        // Inline mode: raw bytes as a JSON array of u8. A future slice may
        // switch to base64 (single string) to halve frame size.
        let pixels = shot
            .pixels_rgba8
            .iter()
            .map(|b| Value::Number((*b).into()))
            .collect();
        obj.insert("pixels_rgba8".to_string(), Value::Array(pixels));
    }
    Value::Object(obj)
}

fn screenshot_error_to_rpc(err: ScreenshotError) -> RpcError {
    let variant = match err {
        ScreenshotError::Path(_) => "Path",
        ScreenshotError::UnsupportedPath => "UnsupportedPath",
        ScreenshotError::RenderBackendUnavailable => "RenderBackendUnavailable",
    };
    RpcError::invalid_params(variant)
}

fn handle_scene_invoke(scene: &mut Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.path missing or not a string",
        ));
    };
    let Some(args_json) = params.get("args") else {
        return Err(RpcError::invalid_params("params.args missing"));
    };
    let Some(args) = json_to_introspect_value(args_json) else {
        return Err(RpcError::invalid_params(
            "params.args is not a representable IntrospectValue",
        ));
    };

    match invoke(scene, path, args) {
        Ok(value) => Ok(introspect_value_to_json(value)),
        Err(err) => Err(invoke_error_to_rpc(&err)),
    }
}

fn handle_scene_intents(scene: &mut Scene) -> Result<Value, RpcError> {
    // §5.20 scene/intents: poll-form drain, no params consumed in v0.
    // A `path` filter / per-window scoping land as carry-forward when
    // multi-window scene addressing settles.
    drain_intents(scene)
        .map(|batch| Value::Array(batch.iter().map(intent_to_json).collect()))
        .map_err(intents_error_to_rpc)
}

fn intent_to_json(intent: &Intent) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "tag".to_string(),
        Value::String(intent.tag_str().to_string()),
    );
    obj.insert(
        "payload".to_string(),
        introspect_value_to_json(intent.payload.clone()),
    );
    Value::Object(obj)
}

fn intents_error_to_rpc(err: IntentsError) -> RpcError {
    // `IntentsError` is `#[non_exhaustive]` with no v0 variants; the
    // match is exhaustive over the empty set via `match err {}`. The
    // wildcard guard preserves a clean error surface when future
    // variants land.
    match err {}
}

/// R51.161 §5.23 + R51.162 §5.23 — `scene/commands` typed handler.
/// 10th method per §5.7. Returns
/// `{ pending: [...], in_flight: [...] }`:
///
/// - `pending` — every queued [`Command`](pinion_core::Command) on
///   the substrate's root [`Owner`] sub-tree, in
///   [`Owner::pending_commands_recursive`](pinion_core::reactive::Owner::pending_commands_recursive)
///   traversal order. Non-draining.
/// - `in_flight` — every command currently tracked by the
///   [`CommandExecutor`](pinion_runtime::CommandExecutor)
///   `in_flight` map (R51.158 cancellation tracker), in `scope_id`
///   ascending order via the underlying [`BTreeMap`].
///
/// `runtime_owner` is required (pending source); `commands_executor`
/// is optional — when absent, `result.in_flight` is an empty array.
fn handle_scene_commands(
    runtime_owner: Option<&Owner>,
    commands_executor: Option<&pinion_runtime::CommandExecutor>,
) -> Result<Value, RpcError> {
    let Some(owner) = runtime_owner else {
        return Err(RpcError::invalid_params("commands view unavailable"));
    };
    let pending = list_pending_commands(owner).map_err(commands_error_to_rpc)?;
    let pending_json = serde_json::to_value(&pending).map_err(|e| {
        RpcError::internal_error(format!(
            "scene/commands: failed to serialize pending list: {e}",
        ))
    })?;
    // R51.162 §5.23 — snapshot in-flight commands (empty when no
    // executor injected) and project through the same view shape.
    let in_flight_views: Vec<crate::commands::PendingCommandView> = commands_executor
        .map(crate::commands::list_in_flight_commands)
        .transpose()
        .map_err(commands_error_to_rpc)?
        .unwrap_or_default();
    let in_flight_json = serde_json::to_value(&in_flight_views).map_err(|e| {
        RpcError::internal_error(format!(
            "scene/commands: failed to serialize in_flight list: {e}",
        ))
    })?;
    let mut obj = serde_json::Map::new();
    obj.insert("pending".to_string(), pending_json);
    obj.insert("in_flight".to_string(), in_flight_json);
    Ok(Value::Object(obj))
}

fn commands_error_to_rpc(err: CommandsError) -> RpcError {
    // `CommandsError` is `#[non_exhaustive]` with no v0 variants —
    // see [`crate::commands::CommandsError`].
    match err {}
}

/// R598 §5.50 — `scene/theme_tokens` typed handler. 17th
/// `scene/*` method. Returns the bound
/// [`ThemeProvider`](pinion_core::theme::ThemeProvider)'s snapshot
/// projected into the [`ThemeTokensOutcome`] shape — see
/// [`crate::theme`] for the JSON wire shape.
///
/// `params.tag` is optional; when omitted the lookup resolves
/// against [`crate::theme::DEFAULT_THEME_TAG`] (`"app"`,
/// matching every `examples/hello-*` binary).
fn handle_scene_theme_tokens(
    runtime_owner: Option<&Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let tag = read_optional_tag(params)?;
    match theme_tokens(runtime_owner, tag) {
        Ok(outcome) => theme_tokens_outcome_to_json(&outcome),
        Err(err) => Err(theme_tokens_error_to_rpc(err)),
    }
}

fn theme_tokens_outcome_to_json(out: &ThemeTokensOutcome) -> Result<Value, RpcError> {
    serde_json::to_value(out).map_err(|e| {
        RpcError::internal_error(format!(
            "scene/theme_tokens: failed to serialize outcome: {e}",
        ))
    })
}

fn theme_tokens_error_to_rpc(err: ThemeTokensError) -> RpcError {
    match err {
        ThemeTokensError::RuntimeOwnerUnavailable => {
            RpcError::invalid_params("theme view unavailable")
                .with_data_string("RuntimeOwnerUnavailable")
        }
        ThemeTokensError::NotBound { tag } => {
            RpcError::invalid_params(format!("theme provider not bound under tag {tag:?}"))
                .with_data_string("NotBound")
        }
    }
}

/// R599 §5.50 — `scene/set_theme_mode` typed handler. 18th
/// `scene/*` method, mutation pair to `scene/theme_tokens`. The
/// dispatcher's [`HandlerKind::Mutate`] tag bumps the
/// [`SceneRevision`] after this call returns `Ok`.
fn handle_scene_set_theme_mode(
    runtime_owner: Option<&Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params_value = require_params(params)?;
    let mode_str = read_required_str(params_value, "mode", "ModeRequired")?;
    let mode = crate::theme::parse_theme_mode(mode_str).ok_or_else(|| {
        RpcError::invalid_params(format!(
            "params.mode {mode_str:?} not one of \"light\" / \"dark\" / \"system\""
        ))
    })?;
    let tag = read_optional_tag(Some(params_value))?;
    let typed_params = SetThemeModeParams { tag, mode };
    match set_theme_mode(runtime_owner, &typed_params) {
        Ok(outcome) => set_theme_mode_outcome_to_json(&outcome),
        Err(err) => Err(set_theme_mode_error_to_rpc(err)),
    }
}

fn set_theme_mode_outcome_to_json(out: &SetThemeModeOutcome) -> Result<Value, RpcError> {
    serde_json::to_value(out).map_err(|e| {
        RpcError::internal_error(format!(
            "scene/set_theme_mode: failed to serialize outcome: {e}",
        ))
    })
}

fn set_theme_mode_error_to_rpc(err: SetThemeModeError) -> RpcError {
    match err {
        SetThemeModeError::RuntimeOwnerUnavailable => {
            RpcError::invalid_params("theme view unavailable")
                .with_data_string("RuntimeOwnerUnavailable")
        }
        SetThemeModeError::NotBound { tag } => {
            RpcError::invalid_params(format!("theme provider not bound under tag {tag:?}"))
                .with_data_string("NotBound")
        }
    }
}

/// R608 §5.50 — `scene/set_theme_palettes` typed handler. 23rd
/// `scene/*` method. Replaces both palettes on the bound
/// [`ThemeProvider`] in a single
/// [`reactive::batch`](pinion_core::reactive::batch); the dispatcher's
/// [`HandlerKind::Mutate`] match-arm tag bumps the
/// [`SceneRevision`](pinion_core::SceneRevision) after this call
/// returns `Ok`.
///
/// Wire shape mirrors [`scene/theme_tokens`](handle_scene_theme_tokens)
/// one-to-one: every `params.{light,dark}[*]` entry is
/// `{"role": "<name>", "color": "<hex>"}`, so an AI agent can call
/// `theme_tokens` → mutate per-role entries locally → `set_theme_palettes`
/// without rewriting the JSON. Per-palette parsing is delegated to
/// [`parse_palette_value`] so the typed parse-error variants surface
/// at the `error.data` level.
fn handle_scene_set_theme_palettes(
    runtime_owner: Option<&Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params_value = require_params(params)?;
    let light_value = params_value
        .get("light")
        .ok_or_else(|| RpcError::invalid_params("params.light missing"))?;
    let dark_value = params_value
        .get("dark")
        .ok_or_else(|| RpcError::invalid_params("params.dark missing"))?;
    let light = parse_palette_value(light_value, "light").map_err(palette_parse_error_to_rpc)?;
    let dark = parse_palette_value(dark_value, "dark").map_err(palette_parse_error_to_rpc)?;
    let tag = read_optional_tag(Some(params_value))?;
    let typed_params = SetThemePalettesParams { tag, light, dark };
    match set_theme_palettes(runtime_owner, &typed_params) {
        Ok(outcome) => set_theme_palettes_outcome_to_json(&outcome),
        Err(err) => Err(set_theme_palettes_error_to_rpc(err)),
    }
}

fn set_theme_palettes_outcome_to_json(out: &SetThemePalettesOutcome) -> Result<Value, RpcError> {
    serde_json::to_value(out).map_err(|e| {
        RpcError::internal_error(format!(
            "scene/set_theme_palettes: failed to serialize outcome: {e}",
        ))
    })
}

fn set_theme_palettes_error_to_rpc(err: SetThemePalettesError) -> RpcError {
    match err {
        SetThemePalettesError::RuntimeOwnerUnavailable => {
            RpcError::invalid_params("theme view unavailable")
                .with_data_string("RuntimeOwnerUnavailable")
        }
        SetThemePalettesError::NotBound { tag } => {
            RpcError::invalid_params(format!("theme provider not bound under tag {tag:?}"))
                .with_data_string("NotBound")
        }
    }
}

/// Map a [`PaletteParseError`] into a JSON-RPC `-32602 Invalid params`
/// envelope with the variant tag in `error.data` so AI clients can
/// pattern-match on the typed failure. The substrate-side variant is
/// `#[non_exhaustive]`, but the match below covers every current
/// constructor explicitly — a future variant addition that lands
/// without updating this arm fails compilation, which is the desired
/// developer experience for an API surface clients rely on.
fn palette_parse_error_to_rpc(err: PaletteParseError) -> RpcError {
    match err {
        PaletteParseError::NotArray { which } => RpcError::invalid_params(format!(
            "params.{which} must be a JSON array of role/color entries"
        ))
        .with_data_string("NotArray"),
        PaletteParseError::EntryNotObject { which, index } => RpcError::invalid_params(format!(
            "params.{which}[{index}] must be a JSON object"
        ))
        .with_data_string("EntryNotObject"),
        PaletteParseError::EntryMissingRole { which, index } => RpcError::invalid_params(format!(
            "params.{which}[{index}].role missing or not a string"
        ))
        .with_data_string("EntryMissingRole"),
        PaletteParseError::EntryMissingColor { which, index } => RpcError::invalid_params(format!(
            "params.{which}[{index}].color missing or not a string"
        ))
        .with_data_string("EntryMissingColor"),
        PaletteParseError::UnknownRole {
            which,
            index,
            role,
        } => RpcError::invalid_params(format!(
            "params.{which}[{index}].role {role:?} is not a canonical ColorRole name"
        ))
        .with_data_string("UnknownRole"),
        PaletteParseError::DuplicateRole { which, role } => RpcError::invalid_params(format!(
            "params.{which} binds role {role:?} more than once"
        ))
        .with_data_string("DuplicateRole"),
        PaletteParseError::InvalidColor {
            which,
            role,
            value,
        } => RpcError::invalid_params(format!(
            "params.{which}[role={role:?}].color {value:?} is not a #rrggbb / #rrggbbaa hex literal"
        ))
        .with_data_string("InvalidColor"),
        PaletteParseError::MissingRoles { which, missing } => RpcError::invalid_params(format!(
            "params.{which} is incomplete; missing role(s): {missing:?}"
        ))
        .with_data_string("MissingRoles"),
    }
}

/// R600 §5.28 — `scene/animation_state` typed handler. 19th
/// `scene/*` method, read-only. Returns
/// `{ active: bool, epsilon: f32 }` — see [`crate::animation_state`]
/// for the wire shape.
///
/// `params.epsilon` is optional; when omitted the handler defers to
/// [`crate::animation_state::animation_state`] which falls back to
/// [`pinion_core::animation::DEFAULT_REST_EPSILON`].
fn handle_scene_animation_state(
    runtime_owner: Option<&Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let epsilon = params
        .and_then(|p| p.get("epsilon"))
        .map(|v| {
            v.as_f64().ok_or_else(|| {
                RpcError::invalid_params("params.epsilon must be a number when present")
            })
        })
        .transpose()?
        .map(|v| {
            // f64 → f32: the spring solver works in f32, so the
            // truncation is intentional; rejection of non-finite +
            // negative happens inside animation_state().
            #[allow(
                clippy::cast_possible_truncation,
                reason = "spring solver evaluates in f32; out-of-range / NaN \
                          is rejected by animation_state() as InvalidEpsilon"
            )]
            {
                v as f32
            }
        });
    match animation_state(runtime_owner, epsilon) {
        Ok(outcome) => animation_state_outcome_to_json(outcome),
        Err(err) => Err(animation_state_error_to_rpc(&err)),
    }
}

fn animation_state_outcome_to_json(out: AnimationStateOutcome) -> Result<Value, RpcError> {
    serde_json::to_value(out).map_err(|e| {
        RpcError::internal_error(format!(
            "scene/animation_state: failed to serialize outcome: {e}",
        ))
    })
}

fn animation_state_error_to_rpc(err: &AnimationStateError) -> RpcError {
    match err {
        AnimationStateError::RuntimeOwnerUnavailable => {
            RpcError::invalid_params("animation state unavailable")
                .with_data_string("RuntimeOwnerUnavailable")
        }
        AnimationStateError::InvalidEpsilon { value } => {
            RpcError::invalid_params(format!("params.epsilon {value} must be finite and >= 0",))
                .with_data_string("InvalidEpsilon")
        }
    }
}

/// R682.B §5.16 — `scene/cache_stats` typed handler. Reads the
/// per-window [`pinion_runtime::paint_adapter::FragmentCacheStats`]
/// snapshot the embedder pre-resolved on
/// [`DispatchContext::fragment_cache_stats`] and emits the wire
/// [`CacheStatsOutcome`] shape.
///
/// Read-only — `HandlerKind::Read` upstream skips the
/// [`SceneRevision`] bump.
fn handle_scene_cache_stats(
    stats: Option<pinion_runtime::FragmentCacheStats>,
) -> Result<Value, RpcError> {
    match cache_stats(stats) {
        Ok(outcome) => cache_stats_outcome_to_json(outcome),
        Err(err) => Err(cache_stats_error_to_rpc(&err)),
    }
}

fn cache_stats_outcome_to_json(out: CacheStatsOutcome) -> Result<Value, RpcError> {
    serde_json::to_value(out).map_err(|e| {
        RpcError::internal_error(format!(
            "scene/cache_stats: failed to serialize outcome: {e}",
        ))
    })
}

fn cache_stats_error_to_rpc(err: &CacheStatsError) -> RpcError {
    match err {
        CacheStatsError::CacheStatsUnavailable => {
            RpcError::invalid_params("fragment cache stats unavailable for this window")
                .with_data_string("CacheStatsUnavailable")
        }
    }
}

/// R907 §5.16 §5.7 — `scene/frame_timings` typed handler. Projects the
/// embedder-resolved [`pinion_runtime::FrameTimingsSnapshot`] onto the
/// nested wire shape (last frame + window aggregates + cumulative
/// count); `None` surfaces `FrameTimingsUnavailable`.
fn handle_scene_frame_timings(
    snapshot: Option<pinion_runtime::FrameTimingsSnapshot>,
) -> Result<Value, RpcError> {
    match frame_timings(snapshot) {
        Ok(outcome) => frame_timings_outcome_to_json(outcome),
        Err(err) => Err(frame_timings_error_to_rpc(&err)),
    }
}

fn frame_timings_outcome_to_json(out: FrameTimingsOutcome) -> Result<Value, RpcError> {
    serde_json::to_value(out).map_err(|e| {
        RpcError::internal_error(format!(
            "scene/frame_timings: failed to serialize outcome: {e}",
        ))
    })
}

fn frame_timings_error_to_rpc(err: &FrameTimingsError) -> RpcError {
    match err {
        FrameTimingsError::FrameTimingsUnavailable => {
            RpcError::invalid_params("frame timings unavailable for this window")
                .with_data_string("FrameTimingsUnavailable")
        }
    }
}

/// R1036 §5.16 §5.7 §2 #7 — `scene/render_fidelity` typed handler (PR-17).
///
/// Projects the embedder-resolved presented-frame
/// [`pinion_runtime::RenderFidelity`] record and, when a paint producer is
/// wired, recomputes current producer state at the record's viewport so the
/// outcome carries a per-`TextGrid` displayed-vs-state divergence verdict.
/// `None` record surfaces `RenderFidelityUnavailable`.
fn handle_scene_render_fidelity<F>(
    record: Option<&pinion_runtime::RenderFidelity>,
    paint_producer: Option<&mut F>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    // Recompute current producer state at the SAME viewport the frame was
    // encoded at (clamped to >= 1 so a degenerate record cannot 0-size the
    // producer). `None` producer (headless opt-out) ⟹ displayed record alone.
    let state_grids = match (record, paint_producer) {
        (Some(rec), Some(producer)) => {
            let scene = (producer)(rec.viewport_w.max(1), rec.viewport_h.max(1));
            Some(pinion_runtime::render_fidelity::grid_fidelity(&scene))
        }
        _ => None,
    };
    match render_fidelity(record, state_grids) {
        Ok(outcome) => serde_json::to_value(outcome).map_err(|e| {
            RpcError::internal_error(format!(
                "scene/render_fidelity: failed to serialize outcome: {e}",
            ))
        }),
        Err(err) => Err(render_fidelity_error_to_rpc(&err)),
    }
}

fn render_fidelity_error_to_rpc(err: &RenderFidelityError) -> RpcError {
    match err {
        RenderFidelityError::RenderFidelityUnavailable => {
            RpcError::invalid_params("render fidelity unavailable for this window")
                .with_data_string("RenderFidelityUnavailable")
        }
    }
}

/// R908 §5.53 — `scene/export_pdf`: render the addressed window's paint
/// scene to a vector PDF. The render peer of `scene/layout`: both
/// project the same per-window `last_paint_scene` borrow, only on their
/// own arm (R890.1).
fn handle_scene_export_pdf(
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    // Params are optional (an empty object / absent params exports the
    // scene-bounds page), so a missing object deserializes to the
    // all-default `ExportPdfParams` rather than erroring.
    let typed: ExportPdfParams = match params {
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|e| RpcError::invalid_params(format!("params shape: {e}")))?,
        None => ExportPdfParams::default(),
    };
    match export_pdf(last_paint_scene, &typed) {
        Ok(outcome) => serde_json::to_value(outcome).map_err(|e| {
            RpcError::internal_error(format!(
                "scene/export_pdf: failed to serialize outcome: {e}"
            ))
        }),
        Err(err) => Err(export_pdf_error_to_rpc(&err)),
    }
}

fn export_pdf_error_to_rpc(err: &ExportPdfError) -> RpcError {
    match err {
        ExportPdfError::NoPaintScene => {
            RpcError::invalid_params("no paint scene for this window yet")
                .with_data_string("NoPaintScene")
        }
        ExportPdfError::UnknownPageSize(name) => {
            RpcError::invalid_params(format!("unknown page size: {name}"))
                .with_data_string("UnknownPageSize")
        }
        ExportPdfError::UnknownOrientation(name) => {
            RpcError::invalid_params(format!("unknown orientation: {name}"))
                .with_data_string("UnknownOrientation")
        }
    }
}

/// R885 §5.49 — `scene/input_state` typed handler: the READ peer of
/// the out-of-band input writes. Serializes the embedder-resolved
/// [`pinion_core::InputStateSnapshot`] with each field mirroring its
/// write wire shape (read = inverse of write,
/// [[wire-form-read-write-symmetry]]):
///
/// * `modifiers` — the `scene/modifiers` param object
///   (`{shift, ctrl, alt, meta}`), or `null` when the backend keeps
///   no absolute modifier cache (the TUI §2 #6 carry) so an AI
///   client distinguishes "axis unavailable" from "none held".
/// * `held_keys` — array of canonical named keys from the held-chord
///   cache ([`pinion_core::HeldKeys`] — the chord VOCABULARY subset of
///   `scene/key state:"down"` writes, currently the `Space` pan chord;
///   a non-chord key's down/up edge is accepted but never cached, so it
///   reads back absent by design).
/// * `cursor` — `{x, y}` of the dispatch-scoped window's last mouse
///   cursor position (what every `scene/click` / `scene/hover` /
///   `scene/drag` write moves), or `null` before the first cursor
///   event.
/// * `key_dispatch` — R1074 §5.39 §5.16, the multi-window
///   keyboard-dispatch gate state, or `null` on a single-OS-window
///   backend (the TUI), the same "axis unavailable" honesty as
///   `modifiers`. When present:
///   `{ os_focused_window: <string>|null, key_press_owners: {<key>: <window>} }`
///   — `os_focused_window` is the OS-focused window the key gate admits
///   for (`null` = no window focused → gate fails OPEN); `key_press_owners`
///   maps each currently-held key to the window that owned its press's
///   rising edge. Together they make the close-during-dispatch gate
///   (R1073) — the admit decision — AI-observable.
///
/// Read-only — `HandlerKind::Read` upstream skips the
/// [`SceneRevision`] bump.
fn handle_scene_input_state(
    snapshot: Option<pinion_core::InputStateSnapshot>,
) -> Result<Value, RpcError> {
    let Some(snap) = snapshot else {
        return Err(
            RpcError::invalid_params("input state unavailable for this dispatch")
                .with_data_string("InputStateUnavailable"),
        );
    };
    let modifiers = snap.modifiers.map_or(Value::Null, |m| {
        serde_json::json!({
            "shift": m.shift,
            "ctrl": m.ctrl,
            "alt": m.alt,
            "meta": m.meta,
        })
    });
    let cursor = snap
        .cursor
        .map_or(Value::Null, |(x, y)| serde_json::json!({ "x": x, "y": y }));
    let key_dispatch = snap.key_dispatch.map_or(Value::Null, |kd| {
        // Owners arrive sorted by key (the producer's stable snapshot);
        // a JSON object keyed by key makes "which window owns this key"
        // a direct AI lookup.
        let owners: serde_json::Map<String, Value> = kd
            .key_press_owners
            .into_iter()
            .map(|(key, window)| (key, Value::String(window)))
            .collect();
        serde_json::json!({
            "os_focused_window": kd.os_focused_window,
            "key_press_owners": owners,
        })
    });
    Ok(serde_json::json!({
        "modifiers": modifiers,
        "held_keys": snap.held_keys,
        "cursor": cursor,
        "key_dispatch": key_dispatch,
    }))
}

/// R629 §5.28 — `scene/animate_settle` typed handler. 28th `scene/*`
/// method. Bulk-walks every animation registered on `runtime_owner`
/// (and descendant scopes) and lands each at its internal target
/// with zero velocity. The dispatcher's
/// [`HandlerKind::Mutate`] match-arm tag bumps the
/// [`SceneRevision`] after this call returns `Ok`.
///
/// Wire shape — request: no params (or `{}`); response:
/// `{ "visited": <usize> }`. See [`crate::animate_control`] for the
/// design rationale (bulk-only, no per-tag dispatch, deferred
/// `scene/animate_reset`).
fn handle_scene_animate_settle(runtime_owner: Option<&Owner>) -> Result<Value, RpcError> {
    match crate::animate_control::animate_settle(runtime_owner) {
        Ok(outcome) => animate_control_outcome_to_json(outcome, "scene/animate_settle"),
        Err(err) => Err(animate_control_error_to_rpc(&err)),
    }
}

/// R629 §5.28 — `scene/animate_cancel` typed handler. 29th `scene/*`
/// method. Bulk-walks every animation registered on `runtime_owner`
/// (and descendant scopes) and freezes each at its current value
/// with zero velocity. The dispatcher's
/// [`HandlerKind::Mutate`] match-arm tag bumps the
/// [`SceneRevision`] after this call returns `Ok`.
///
/// Wire shape — request: no params (or `{}`); response:
/// `{ "visited": <usize> }`. See [`crate::animate_control`] for the
/// design rationale.
fn handle_scene_animate_cancel(runtime_owner: Option<&Owner>) -> Result<Value, RpcError> {
    match crate::animate_control::animate_cancel(runtime_owner) {
        Ok(outcome) => animate_control_outcome_to_json(outcome, "scene/animate_cancel"),
        Err(err) => Err(animate_control_error_to_rpc(&err)),
    }
}

fn animate_control_outcome_to_json(
    out: crate::animate_control::AnimateControlOutcome,
    method: &str,
) -> Result<Value, RpcError> {
    serde_json::to_value(out).map_err(|e| {
        RpcError::internal_error(format!("{method}: failed to serialize outcome: {e}"))
    })
}

fn animate_control_error_to_rpc(err: &crate::animate_control::AnimateControlError) -> RpcError {
    match err {
        crate::animate_control::AnimateControlError::RuntimeOwnerUnavailable => {
            RpcError::invalid_params("animation control unavailable")
                .with_data_string("RuntimeOwnerUnavailable")
        }
    }
}

/// R602 §5.45 — `scene/scroll_state` typed handler. 20th `scene/*`
/// method, read-only. Returns the bound
/// [`ScrollState`](pinion_core::widgets::scroll::ScrollState)
/// projection — see [`crate::scroll_state`] for the wire shape.
///
/// `params.tag` is required (no canonical default for scroll
/// states). Post-R605 the handler reaches the cache via
/// [`Owner::cache_get_by_str`](pinion_core::reactive::Owner::cache_get_by_str)
/// directly — no `Box::leak` bridge, no unbounded growth.
fn handle_scene_scroll_state(
    runtime_owner: Option<&Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params_value = require_params(params)?;
    let tag = read_required_tag(params_value)?;
    // R605 §5.22 — `Owner::cache_get_by_str` accepts a borrowed
    // `&str`, no `Box::leak` bridge needed.
    match scroll_state(runtime_owner, tag) {
        Ok(outcome) => scroll_state_outcome_to_json(&outcome),
        Err(err) => Err(introspect_error_to_rpc(&err)),
    }
}

fn scroll_state_outcome_to_json(out: &ScrollStateOutcome) -> Result<Value, RpcError> {
    serde_json::to_value(out).map_err(|e| {
        RpcError::internal_error(format!(
            "scene/scroll_state: failed to serialize outcome: {e}",
        ))
    })
}

/// R609 §5.45 — `scene/set_scroll_offset` typed handler. 24th
/// `scene/*` method, mutation pair to `scene/scroll_state`. The
/// dispatcher's [`HandlerKind::Mutate`] tag bumps the
/// [`SceneRevision`] after this call returns `Ok`.
///
/// `params.tag` is required, `params.x` / `params.y` are required
/// integers (the [`ScrollState::scroll_to`](pinion_core::widgets::scroll::ScrollState::scroll_to)
/// substrate clamps the values against `[0, max]` automatically).
/// Wire shape — request:
/// `{"tag": "<scroll_tag>", "x": <i32>, "y": <i32>}`. Response is
/// the same [`ScrollStateOutcome`] shape `scene/scroll_state` returns.
fn handle_scene_set_scroll_offset(
    runtime_owner: Option<&Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params_value = require_params(params)?;
    let tag = read_required_tag(params_value)?;
    let x = read_i32_field(params_value, "x")?;
    let y = read_i32_field(params_value, "y")?;
    let typed_params = SetScrollOffsetParams { tag, x, y };
    match set_scroll_offset(runtime_owner, &typed_params) {
        Ok(outcome) => scroll_state_outcome_to_json(&outcome),
        Err(err) => Err(introspect_error_to_rpc(&err)),
    }
}

/// R621 §5.7 — `params: Option<&Value>` is the canonical JSON-RPC
/// envelope shape (the spec lets clients omit the `params` field
/// entirely), but the majority of typed handlers REQUIRE params and
/// must reject the absent case with a uniform error message.
///
/// Pre-R621 every such handler open-coded the unwrap:
///
/// ```text
/// let Some(params_value) = params else {
///     return Err(RpcError::invalid_params("missing params"));
/// };
/// // ... or, post-R613 ...
/// let params_value = params.ok_or_else(|| RpcError::invalid_params("missing params"))?;
/// ```
///
/// 21 byte-identical sites accumulated by R620 (~17 query / mutate /
/// preview / introspect handlers, plus a handful of font + text
/// helpers). R621 lifts the pattern to a single helper. Per
/// [[three-site-internal-duplication-substrate-lift]] this crossed
/// the rule of three at R602; the deferred lift cleared with R621.
///
/// # Errors
///
/// Returns `-32602 Invalid params` with the prose `"missing params"`
/// (no typed `error.data` — the prose is the wire identifier this
/// pattern has used since R16's first dispatch handlers and the
/// existing test suite pins it verbatim).
fn require_params(params: Option<&Value>) -> Result<&Value, RpcError> {
    params.ok_or_else(|| RpcError::invalid_params("missing params"))
}

/// R617 §5.7 — extract the optional `params.tag` field as a
/// `&str`. Paired with R613 [`read_required_tag`] for the
/// two distinct wire-shape contracts:
///
/// - **Required tag** (widget-axis RPCs like `scene/scroll_state`,
///   `scene/set_text`, …) — every scrollable widget / text-edit
///   field has its own cache tag; there is no canonical default,
///   so an absent `params.tag` is `-32602 Invalid params` with
///   typed `TagRequired`. See [`read_required_tag`].
///
/// - **Optional tag** (theme-axis RPCs like `scene/theme_tokens`,
///   `scene/set_theme_mode`, `scene/set_theme_palettes`) — every
///   `examples/hello-*` binary binds the [`ThemeProvider`] under
///   the canonical [`crate::theme::DEFAULT_THEME_TAG`] (`"app"`),
///   so omitting `params.tag` resolves against the default. The
///   typed fn signature accepts `Option<&str>` and falls back to
///   the default tag when the option is `None`.
///
/// `params` itself is `Option<&Value>` because the JSON-RPC
/// envelope makes `params` an optional field at the spec level —
/// `scene/theme_tokens` with no `params` block at all is a valid
/// request. Returns `Ok(None)` when `params` is absent OR
/// `params.tag` is absent; returns `Ok(Some(s))` when present and
/// a string; returns `Err(_)` when present but a non-string.
///
/// # Errors
///
/// Returns `-32602 Invalid params` (no typed `error.data` — the
/// pre-R617 prose was `"params.tag must be a string when present"`,
/// retained verbatim so existing client log-scrapers do not regress.)
/// when `params.tag` is present but not a JSON string.
fn read_optional_tag(params: Option<&Value>) -> Result<Option<&str>, RpcError> {
    params
        .and_then(|p| p.get("tag"))
        .map(|v| {
            v.as_str()
                .ok_or_else(|| RpcError::invalid_params("params.tag must be a string when present"))
        })
        .transpose()
}

/// R613 §5.7 — extract the required `params.tag` field as a `&str`.
///
/// Lifted in R613 from seven byte-identical dispatch sites that all
/// projected the same wire shape:
///
/// ```text
/// params_value
///     .get("tag")
///     .and_then(Value::as_str)
///     .ok_or_else(|| invalid_params(...).with_data_string("TagRequired"))?;
/// ```
///
/// The 4-line copy lived at: `handle_scene_scroll_state` (R602),
/// `handle_scene_set_scroll_offset` (R609),
/// `handle_scene_text_state` (R603),
/// `handle_scene_set_text` (R610),
/// `handle_scene_set_selection` (R611),
/// `handle_scene_set_caret` (R612),
/// `handle_scene_caret_state` (R604).
///
/// Per [[three-site-internal-duplication-substrate-lift]] a 3+
/// repeated within-binding pattern lifts to a substrate helper; this
/// pattern crossed the rule at R604 and the count kept climbing
/// through every R609-R612 setter. The lift collapses each call
/// site to one line and keeps the `TagRequired` wire-data identifier
/// in a single place — future scrub of the wire-message prose touches
/// one function instead of seven.
///
/// R627 §5.7 lifts the by-now generic shape of this helper into
/// [`read_required_str`]; this entry point keeps the canonical
/// `"tag"` / `"TagRequired"` defaults so the seven call sites stay
/// one-line and the test suite that pins the `TagRequired` wire-data
/// identifier (R613) anchors on a stable named function.
///
/// # Errors
///
/// Returns a `-32602 Invalid params` with `error.data = "TagRequired"`
/// when `params.tag` is missing or is a non-string value (number,
/// bool, null, array, object). Matches the typed
/// [`SubstrateIntrospectError::TagRequired`] variant the read-side
/// `lookup` helper documents.
fn read_required_tag(params: &Value) -> Result<&str, RpcError> {
    read_required_str(params, "tag", "TagRequired")
}

/// R627 §5.7 — extract a required string-typed `params.<field>` and
/// surface a typed `error.data` tag on failure. Generalises the R613
/// [`read_required_tag`] pattern after a third inline copy surfaced
/// at the `handle_scene_set_text` `"text"` field and a fourth at the
/// `handle_scene_set_theme_mode` `"mode"` field.
///
/// Per [[three-site-internal-duplication-substrate-lift]] three
/// byte-identical projections of the same wire shape (tag / text /
/// mode) lift to one helper; the data-tag argument keeps each call
/// site's typed `error.data` identifier intact so AI clients still
/// pattern-match on per-axis variant tags (`TagRequired`,
/// `TextRequired`, `ModeRequired`, …) at the `error.data` level.
///
/// # Errors
///
/// Returns a `-32602 Invalid params` with `error.data = data_tag`
/// when `params.<field>` is missing or is a non-string value.
fn read_required_str<'a>(
    params: &'a Value,
    field: &str,
    data_tag: &'static str,
) -> Result<&'a str, RpcError> {
    params.get(field).and_then(Value::as_str).ok_or_else(|| {
        RpcError::invalid_params(format!("params.{field} missing or not a string"))
            .with_data_string(data_tag)
    })
}

/// Parse `params.<field>` as an `i32`. Rejects floats with a
/// fractional part, missing fields, and out-of-range integers. JSON
/// `Number` is `f64`-or-`i64` at the `serde_json` layer; the cast guards
/// the `i32` range via `i64::try_into`.
fn read_i32_field(params: &Value, field: &str) -> Result<i32, RpcError> {
    let value = params.get(field).ok_or_else(|| {
        RpcError::invalid_params(format!("params.{field} missing"))
            .with_data_string("InvalidAxisValue")
    })?;
    if let Some(int_v) = value.as_i64() {
        return i32::try_from(int_v).map_err(|_| {
            RpcError::invalid_params(format!("params.{field} {int_v} out of i32 range",))
                .with_data_string("InvalidAxisValue")
        });
    }
    Err(
        RpcError::invalid_params(format!("params.{field} must be an integer"))
            .with_data_string("InvalidAxisValue"),
    )
}

/// R603 §5.22 — `scene/text_state` typed handler. 21st `scene/*`
/// method, read-only. Returns the bound
/// [`TextEditState`](pinion_core::widgets::text_edit::TextEditState)
/// projection — see [`crate::text_state`] for the wire shape.
///
/// `params.tag` is required (per-field tagged; no canonical
/// default). Reaches the cache via [`Owner::cache_get_by_str`](pinion_core::reactive::Owner::cache_get_by_str)
/// — R605 §5.22 lift, no `&'static str` bridge needed.
fn handle_scene_text_state(
    runtime_owner: Option<&Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params_value = require_params(params)?;
    let tag = read_required_tag(params_value)?;
    // R605 §5.22 — see scroll_state handler for rationale.
    match text_state(runtime_owner, tag) {
        Ok(outcome) => text_state_outcome_to_json(&outcome),
        Err(err) => Err(introspect_error_to_rpc(&err)),
    }
}

fn text_state_outcome_to_json(out: &TextStateOutcome) -> Result<Value, RpcError> {
    serde_json::to_value(out).map_err(|e| {
        RpcError::internal_error(format!(
            "scene/text_state: failed to serialize outcome: {e}",
        ))
    })
}

/// R610 §5.22 — `scene/set_text` typed handler. 25th `scene/*`
/// method, mutation pair to `scene/text_state`. The dispatcher's
/// [`HandlerKind::Mutate`] match-arm tag bumps the
/// [`SceneRevision`] after this call returns `Ok`.
///
/// Wire shape — request: `{"tag": "<field_tag>", "text": "<utf8>"}`.
/// Response is the same [`TextStateOutcome`] shape `scene/text_state`
/// returns. The substrate's `set_text` drops any active selection,
/// drops any IME preedit, and clamps the caret to the new text
/// length — all three side effects surface in the response.
fn handle_scene_set_text(
    runtime_owner: Option<&Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params_value = require_params(params)?;
    let tag = read_required_tag(params_value)?;
    let text = read_required_str(params_value, "text", "TextRequired")?;
    let typed_params = SetTextParams { tag, text };
    match set_text(runtime_owner, &typed_params) {
        Ok(outcome) => text_state_outcome_to_json(&outcome),
        Err(err) => Err(introspect_error_to_rpc(&err)),
    }
}

/// R611 §5.22 — `scene/set_selection` typed handler. 26th `scene/*`
/// method, mutation pair to `scene/text_state` for the
/// selection-axis specifically (paired alongside the text-axis
/// [`handle_scene_set_text`] and the caret-axis R612 setter).
///
/// Wire shape — request:
/// `{"tag": "<field_tag>", "anchor": <usize>, "focus": <usize>}`.
/// Response is the same [`TextStateOutcome`] shape `scene/text_state`
/// returns. The substrate snaps both offsets to `char` boundaries
/// and clamps them to `[0, text.len()]`; the response echoes the
/// post-snap state. When `anchor == focus` post-snap the selection
/// collapses to caret-only — surfaced as `selection: null` +
/// `has_selection: false`.
fn handle_scene_set_selection(
    runtime_owner: Option<&Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params_value = require_params(params)?;
    let tag = read_required_tag(params_value)?;
    let anchor = read_usize_field(params_value, "anchor")?;
    let focus = read_usize_field(params_value, "focus")?;
    let typed_params = SetSelectionParams { tag, anchor, focus };
    match set_selection(runtime_owner, &typed_params) {
        Ok(outcome) => text_state_outcome_to_json(&outcome),
        Err(err) => Err(introspect_error_to_rpc(&err)),
    }
}

/// R612 §5.22 — `scene/set_caret` typed handler. 27th `scene/*`
/// method, completes the write-side matrix for the text-axis
/// triplet (text / selection / caret). The dispatcher's
/// [`HandlerKind::Mutate`] match-arm tag drives the [`SceneRevision`]
/// after this call returns `Ok`.
///
/// Wire shape — request:
/// `{"tag": "<field_tag>", "pos": <usize>}`. Response is the same
/// [`TextStateOutcome`] shape `scene/text_state` returns. The
/// substrate clamps `pos` to `[0, text.len()]`, snaps to a `char`
/// boundary, and drops any active selection per the W3C
/// `selectionchange` canonical.
fn handle_scene_set_caret(
    runtime_owner: Option<&Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params_value = require_params(params)?;
    let tag = read_required_tag(params_value)?;
    let pos = read_usize_field(params_value, "pos")?;
    let typed_params = SetCaretParams { tag, pos };
    match set_caret(runtime_owner, &typed_params) {
        Ok(outcome) => text_state_outcome_to_json(&outcome),
        Err(err) => Err(introspect_error_to_rpc(&err)),
    }
}

/// Parse `params.<field>` as a non-negative `usize` (byte offset into
/// a UTF-8 string). Rejects missing fields, negative integers, and
/// floats. Surfaces a typed `InvalidByteOffset` data string at the
/// `error.data` level so AI clients pattern-match on the variant tag.
fn read_usize_field(params: &Value, field: &str) -> Result<usize, RpcError> {
    let value = params.get(field).ok_or_else(|| {
        RpcError::invalid_params(format!("params.{field} missing"))
            .with_data_string("InvalidByteOffset")
    })?;
    if let Some(int_v) = value.as_u64() {
        return usize::try_from(int_v).map_err(|_| {
            RpcError::invalid_params(format!("params.{field} {int_v} out of usize range",))
                .with_data_string("InvalidByteOffset")
        });
    }
    Err(
        RpcError::invalid_params(format!("params.{field} must be a non-negative integer",))
            .with_data_string("InvalidByteOffset"),
    )
}

/// R604 §5.22 — `scene/caret_state` typed handler. 22nd `scene/*`
/// method, read-only. Closes the AI-first observability matrix.
/// Returns the bound
/// [`CaretBlink`](pinion_core::widgets::caret_blink::CaretBlink)
/// projection — see [`crate::caret_state`] for the wire shape.
fn handle_scene_caret_state(
    runtime_owner: Option<&Owner>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params_value = require_params(params)?;
    let tag = read_required_tag(params_value)?;
    // R605 §5.22 — see scroll_state handler for rationale.
    match caret_state(runtime_owner, tag) {
        Ok(outcome) => caret_state_outcome_to_json(&outcome),
        Err(err) => Err(introspect_error_to_rpc(&err)),
    }
}

fn caret_state_outcome_to_json(out: &CaretStateOutcome) -> Result<Value, RpcError> {
    serde_json::to_value(out).map_err(|e| {
        RpcError::internal_error(format!(
            "scene/caret_state: failed to serialize outcome: {e}",
        ))
    })
}

/// R607 §5.7 §5.22 — shared
/// [`SubstrateIntrospectError`] → [`RpcError`] mapping. The
/// `error.data` wire identifier is sourced from
/// [`introspect_error_to_data`] so every callsite shares one
/// source of truth for the typed-name catalogue.
///
/// R616 §5.7 — inlined directly into the handler `Err(_)` arms;
/// each handler previously passed its own `domain` label string.
///
/// R625 §5.7 — `domain` parameter removed: pre-R625 prose
/// interpolated the per-axis label (e.g. `"scroll state
/// unavailable"`), but the method name in the originating JSON-RPC
/// request already identifies the axis, so the prose embedding was
/// information-redundant. R625 collapses to fixed prose per variant
/// — the typed `error.data` keeps the wire identifier; the prose
/// is now self-descriptive without needing the per-axis prefix.
/// Net effect: every handler `Err(_)` arm calls
/// `introspect_error_to_rpc(&err)` with no string argument; all 7
/// handlers identical at the call site.
fn introspect_error_to_rpc(err: &SubstrateIntrospectError) -> RpcError {
    let data = introspect_error_to_data(err);
    match err {
        SubstrateIntrospectError::RuntimeOwnerUnavailable => {
            RpcError::invalid_params("RPC runtime owner not registered").with_data_string(data)
        }
        SubstrateIntrospectError::TagRequired => {
            RpcError::invalid_params("params.tag is required").with_data_string(data)
        }
        SubstrateIntrospectError::NotBound { tag } => {
            RpcError::invalid_params(format!("cache slot not bound for tag {tag:?}"))
                .with_data_string(data)
        }
    }
}

fn handle_scene_locate(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(x) = params.get("x").and_then(Value::as_u64) else {
        return Err(RpcError::invalid_params(
            "params.x missing or not a non-negative integer",
        ));
    };
    let Some(y) = params.get("y").and_then(Value::as_u64) else {
        return Err(RpcError::invalid_params(
            "params.y missing or not a non-negative integer",
        ));
    };
    let x32 =
        u32::try_from(x).map_err(|_| RpcError::invalid_params("params.x exceeds u32 range"))?;
    let y32 =
        u32::try_from(y).map_err(|_| RpcError::invalid_params("params.y exceeds u32 range"))?;

    match locate(scene, x32, y32) {
        Ok(outcome) => Ok(locate_outcome_to_json(&outcome)),
        Err(err) => Err(locate_error_to_rpc(err)),
    }
}

fn locate_outcome_to_json(out: &LocateOutcome) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("path".into(), Value::String(out.path.clone()));
    map.insert("bbox".into(), bbox_to_json(&out.bbox));
    map.insert(
        "ancestors".into(),
        Value::Array(
            out.ancestor_paths
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );
    Value::Object(map)
}

fn bbox_to_json(r: &pinion_core::scene::Rect) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("x".into(), Value::from(r.x));
    m.insert("y".into(), Value::from(r.y));
    m.insert("w".into(), Value::from(r.w));
    m.insert("h".into(), Value::from(r.h));
    Value::Object(m)
}

fn handle_scene_locate_region(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let read_u32 = |k: &str| -> Result<u32, RpcError> {
        let raw = params.get(k).and_then(Value::as_u64).ok_or_else(|| {
            RpcError::invalid_params(format!("params.{k} missing or not a non-negative integer"))
        })?;
        u32::try_from(raw)
            .map_err(|_| RpcError::invalid_params(format!("params.{k} exceeds u32 range")))
    };
    let x = read_u32("x")?;
    let y = read_u32("y")?;
    let w = read_u32("w")?;
    let h = read_u32("h")?;

    let outcome = locate_region(scene, x, y, w, h);
    Ok(locate_region_outcome_to_json(&outcome))
}

fn locate_region_outcome_to_json(out: &LocateRegionOutcome) -> Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "paths".into(),
        Value::Array(out.paths.iter().cloned().map(Value::String).collect()),
    );
    map.insert(
        "common_ancestor".into(),
        Value::String(out.common_ancestor.clone()),
    );
    Value::Object(map)
}

fn locate_error_to_rpc(err: LocateError) -> RpcError {
    let variant = match err {
        LocateError::OutOfBounds => "OutOfBounds",
    };
    RpcError::invalid_params(variant)
}

fn handle_scene_bbox(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.path missing or not a string",
        ));
    };
    match bbox(scene, path) {
        Ok(r) => {
            let mut map = serde_json::Map::new();
            map.insert("bbox".into(), bbox_to_json(&r));
            Ok(Value::Object(map))
        }
        Err(err) => Err(bbox_error_to_rpc(err)),
    }
}

fn bbox_error_to_rpc(err: BboxError) -> RpcError {
    let variant = match err {
        BboxError::Path(_) => "Path",
        BboxError::UnknownPath => "UnknownPath",
    };
    RpcError::invalid_params(variant)
}

/// R47.7.1 §5.12 — `scene/layout` typed dispatcher entry. Deserializes
/// the request params into [`LayoutQueryParams`], invokes the
/// application's paint producer with the hypothetical viewport, and
/// serializes the resulting `LayoutNode` tree.
///
/// Generic over the producer type so a `dyn FnMut(u32, u32) -> Scene`
/// trait object (the canonical caller shape from `DispatchContext`) and
/// concrete closures both compile through `F: ?Sized` — the lifetime
/// of the inner trait object is elided into `F`'s bound.
fn handle_scene_layout<F>(
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let params = require_params(params)?;
    let typed: LayoutQueryParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcError::invalid_params(format!("params shape: {e}")))?;
    match layout_query(&typed, paint_producer, last_paint_scene) {
        Ok(node) => serde_json::to_value(&node)
            .map_err(|e| RpcError::invalid_params(format!("serialize: {e}"))),
        Err(err) => Err(layout_query_error_to_rpc(err)),
    }
}

fn layout_query_error_to_rpc(err: LayoutQueryError) -> RpcError {
    let variant = match err {
        LayoutQueryError::Path(_) => "Path",
        LayoutQueryError::PaintProducerUnavailable => "PaintProducerUnavailable",
        LayoutQueryError::InvalidViewport => "InvalidViewport",
        LayoutQueryError::NoLastPaintLayout => "NoLastPaintLayout",
    };
    RpcError::invalid_params(variant)
}

/// R47.7.4 §5.12 — `scene/resize` dispatch entry. Invokes the
/// application's `resize_request` closure with the requested logical
/// `(width, height)`.
fn handle_scene_resize<F>(
    resize_request: Option<&mut F>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) + ?Sized,
{
    let params = require_params(params)?;
    let typed: ResizeParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcError::invalid_params(format!("params shape: {e}")))?;
    match resize(typed, resize_request) {
        Ok(outcome) => serde_json::to_value(outcome)
            .map_err(|e| RpcError::invalid_params(format!("serialize: {e}"))),
        Err(err) => Err(resize_error_to_rpc(err)),
    }
}

fn resize_error_to_rpc(err: ResizeError) -> RpcError {
    let variant = match err {
        ResizeError::ClosureUnavailable => "ClosureUnavailable",
        ResizeError::InvalidSize => "InvalidSize",
    };
    RpcError::invalid_params(variant)
}

/// R1088 §5.16 §5.41 §2 #7 PR-31 — `scene/window_move` dispatch entry.
/// Invokes the application's `reposition_request` closure with the
/// requested declared window id + logical `(x, y)` position. The WRITE
/// peer of the `scene/windows` read.
fn handle_scene_window_move<F>(
    reposition_request: Option<&mut F>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(&str, i32, i32) -> bool + ?Sized,
{
    let params = require_params(params)?;
    let typed: WindowMoveParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcError::invalid_params(format!("params shape: {e}")))?;
    match window_move(typed, reposition_request) {
        Ok(outcome) => serde_json::to_value(outcome)
            .map_err(|e| RpcError::invalid_params(format!("serialize: {e}"))),
        Err(err) => Err(window_move_error_to_rpc(err)),
    }
}

fn window_move_error_to_rpc(err: WindowMoveError) -> RpcError {
    let variant = match err {
        WindowMoveError::ClosureUnavailable => "ClosureUnavailable",
        WindowMoveError::UnknownWindow => "UnknownWindow",
    };
    RpcError::invalid_params(variant)
}

fn handle_scene_cross_window_drop(
    resolved: Option<pinion_runtime::CrossWindowDrop>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    match crate::cross_window_drop::cross_window_drop(params, resolved) {
        Ok(outcome) => serde_json::to_value(outcome)
            .map_err(|e| RpcError::invalid_params(format!("serialize: {e}"))),
        Err(crate::cross_window_drop::CrossWindowDropError::MissingCursor) => {
            Err(RpcError::invalid_params("MissingCursor"))
        }
    }
}

fn handle_scene_cancel_preview(
    previews: &PreviewLedger,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(raw_id) = params.get("preview_id").and_then(Value::as_u64) else {
        return Err(RpcError::invalid_params(
            "params.preview_id missing or not a positive integer",
        ));
    };
    let Some(id) = PreviewId::try_new(raw_id) else {
        return Err(RpcError::invalid_params(
            "params.preview_id must be non-zero",
        ));
    };
    let cancelled = cancel_preview(previews, id);
    let mut map = serde_json::Map::new();
    map.insert("cancelled".into(), Value::Bool(cancelled));
    Ok(Value::Object(map))
}

fn handle_scene_propose_change(
    previews: &PreviewLedger,
    revision: &SceneRevision,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let proposal = parse_typed_proposal(params)?;
    let ttl_hint = match params.get("ttl_ms") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let Some(ms) = v.as_u64() else {
                return Err(RpcError::invalid_params(
                    "params.ttl_ms must be a non-negative integer (ms)",
                ));
            };
            Some(std::time::Duration::from_millis(ms))
        }
    };
    match propose_change(previews, revision, proposal, ttl_hint) {
        Ok(outcome) => Ok(propose_outcome_to_json(outcome)),
        Err(err) => Err(propose_error_to_rpc(&err)),
    }
}

fn parse_typed_proposal(params: &Value) -> Result<TypedProposal, RpcError> {
    let Some(kind) = params.get("kind").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.kind missing or not a string (expected one of: SetSignal, DispatchIntent, SetStyle, ReplaceView)",
        ));
    };
    match kind {
        "SetSignal" => {
            let Some(target_path) = params.get("target_path").and_then(Value::as_str) else {
                return Err(RpcError::invalid_params(
                    "params.target_path missing or not a string",
                ));
            };
            let Some(signal_path) = params.get("signal_path").and_then(Value::as_str) else {
                return Err(RpcError::invalid_params(
                    "params.signal_path missing or not a string",
                ));
            };
            let Some(value) = params.get("value") else {
                return Err(RpcError::invalid_params("params.value missing"));
            };
            Ok(TypedProposal::SetSignal {
                target_path: target_path.to_owned(),
                signal_path: signal_path.to_owned(),
                value: value.clone(),
            })
        }
        "DispatchIntent" => {
            let Some(target_path) = params.get("target_path").and_then(Value::as_str) else {
                return Err(RpcError::invalid_params(
                    "params.target_path missing or not a string",
                ));
            };
            let Some(intent_obj) = params.get("intent") else {
                return Err(RpcError::invalid_params("params.intent missing"));
            };
            let Some(tag) = intent_obj.get("tag").and_then(Value::as_str) else {
                return Err(RpcError::invalid_params(
                    "params.intent.tag missing or not a string",
                ));
            };
            let payload_value = intent_obj.get("payload").cloned().unwrap_or(Value::Null);
            let payload = json_to_introspect_value(&payload_value).ok_or_else(|| {
                RpcError::invalid_params(
                    "params.intent.payload not a representable IntrospectValue shape",
                )
            })?;
            Ok(TypedProposal::DispatchIntent {
                target_path: target_path.to_owned(),
                intent: Intent::new_owned(tag.to_owned(), payload),
            })
        }
        "SetStyle" => {
            let Some(target_path) = params.get("target_path").and_then(Value::as_str) else {
                return Err(RpcError::invalid_params(
                    "params.target_path missing or not a string",
                ));
            };
            let Some(style_obj) = params.get("style") else {
                return Err(RpcError::invalid_params("params.style missing"));
            };
            let style = parse_box_style(style_obj)?;
            Ok(TypedProposal::SetStyle {
                target_path: target_path.to_owned(),
                style,
            })
        }
        "ReplaceView" => {
            let Some(target_path) = params.get("target_path").and_then(Value::as_str) else {
                return Err(RpcError::invalid_params(
                    "params.target_path missing or not a string",
                ));
            };
            let Some(replacement_obj) = params.get("replacement") else {
                return Err(RpcError::invalid_params("params.replacement missing"));
            };
            let replacement = parse_view_blueprint(replacement_obj)?;
            Ok(TypedProposal::ReplaceView {
                target_path: target_path.to_owned(),
                replacement,
            })
        }
        other => Err(RpcError::invalid_params(format!(
            "UnknownProposalKind: {other}"
        ))),
    }
}

/// Wire→[`ViewBlueprint`] coercion for `ReplaceView` payloads
/// (R40.11 / R43). Recursive: `Container.children` invokes the same
/// parser per child. `kind` discriminates between blueprint variants;
/// `tag` is optional everywhere. `style` shape depends on the variant
/// (Box/Container → [`BoxStyle`], Text → text style, Path → stroke +
/// fill, Image → fit + tint).
///
/// Closed-by-design: `kind` values `"Effect"` and `"External"` are
/// rejected at this boundary — Effect has no declarative wire shape
/// and External requires an author-side factory registry (out of
/// scope for the JSON-RPC boundary).
fn parse_view_blueprint(v: &Value) -> Result<ViewBlueprint, RpcError> {
    let Some(kind) = v.get("kind").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.replacement.kind missing or not a string (expected one of: Box, Container, Text, Path, Image)",
        ));
    };
    let rect = parse_rect(v.get("rect"))?;
    let tag = parse_optional_tag(v.get("tag"))?;
    match kind {
        "Box" => {
            let style =
                parse_box_style(v.get("style").ok_or_else(|| {
                    RpcError::invalid_params("params.replacement.style missing")
                })?)?;
            Ok(ViewBlueprint::Box { rect, style, tag })
        }
        "Container" => {
            let style =
                parse_box_style(v.get("style").ok_or_else(|| {
                    RpcError::invalid_params("params.replacement.style missing")
                })?)?;
            let children_value = v.get("children").unwrap_or(&Value::Null);
            let children = match children_value {
                Value::Null => Vec::new(),
                Value::Array(arr) => arr
                    .iter()
                    .map(parse_view_blueprint)
                    .collect::<Result<Vec<_>, _>>()?,
                _ => {
                    return Err(RpcError::invalid_params(
                        "params.replacement.children must be an array",
                    ));
                }
            };
            Ok(ViewBlueprint::Container {
                rect,
                style,
                tag,
                children,
            })
        }
        "Text" => {
            let content = v
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    RpcError::invalid_params("params.replacement.content missing or not a string")
                })?
                .to_owned();
            let style = parse_text_style(v.get("style"))?;
            Ok(ViewBlueprint::Text {
                content,
                rect,
                style,
                tag,
            })
        }
        "Path" => {
            let commands = parse_path_commands(v.get("commands"))?;
            let style = parse_path_style(v.get("style"))?;
            Ok(ViewBlueprint::Path {
                commands,
                rect,
                style,
                tag,
            })
        }
        "Image" => {
            let source = v
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    RpcError::invalid_params("params.replacement.source missing or not a string")
                })?
                .to_owned();
            let style = parse_image_style(v.get("style"))?;
            Ok(ViewBlueprint::Image {
                source,
                rect,
                style,
                tag,
            })
        }
        "Effect" | "External" => Err(RpcError::invalid_params(format!(
            "params.replacement.kind {kind} not supported by wire (closed-by-design — Effect lacks declarative shape; External needs factory registry)"
        ))),
        other => Err(RpcError::invalid_params(format!(
            "params.replacement.kind unrecognised: {other} (expected one of: Box, Container, Text, Path, Image)"
        ))),
    }
}

/// Optional `tag` field shared by every `ViewBlueprint` variant.
fn parse_optional_tag(v: Option<&Value>) -> Result<Option<String>, RpcError> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(RpcError::invalid_params(
            "params.replacement.tag must be a string or null",
        )),
    }
}

/// Wire→[`pinion_core::style::TextStyle`] coercion. All fields
/// optional with [`TextStyle::new`] defaults; `fg_color` as u32 ARGB.
fn parse_text_style(v: Option<&Value>) -> Result<pinion_core::style::TextStyle, RpcError> {
    let Some(obj) = v else {
        return Ok(pinion_core::style::TextStyle::new());
    };
    let mut style = pinion_core::style::TextStyle::new();
    if let Some(font_size) = obj.get("font_size_px").and_then(Value::as_u64) {
        let n = u32::try_from(font_size).map_err(|_| {
            RpcError::invalid_params("params.replacement.style.font_size_px exceeds u32 range")
        })?;
        style.font_size_px = n;
    }
    if let Some(fg) = obj.get("fg_color").and_then(Value::as_u64) {
        let n = u32::try_from(fg).map_err(|_| {
            RpcError::invalid_params("params.replacement.style.fg_color exceeds u32 range")
        })?;
        style.fg_color = Color::from_argb(n);
    }
    if let Some(family) = obj.get("font_family").and_then(Value::as_str) {
        // Untyped wire string → typed family: a CSS generic keyword classifies
        // to `Generic`, anything else to `Named` (R1002).
        style.font_family = Some(pinion_core::style::FontFamily::parse_css(family.to_owned()));
    }
    Ok(style)
}

/// Wire→[`pinion_core::style::PathStyle`] coercion. Both `stroke` /
/// `fill` arms optional and independent. Empty style = no-op draw.
fn parse_path_style(v: Option<&Value>) -> Result<pinion_core::style::PathStyle, RpcError> {
    let Some(obj) = v else {
        return Ok(pinion_core::style::PathStyle::default());
    };
    let mut style = pinion_core::style::PathStyle::default();
    if let Some(fill) = obj.get("fill").and_then(Value::as_u64) {
        let n = u32::try_from(fill).map_err(|_| {
            RpcError::invalid_params("params.replacement.style.fill exceeds u32 range")
        })?;
        style.fill = Some(Color::from_argb(n));
    }
    if let Some(stroke_obj) = obj.get("stroke") {
        let stroke_color = stroke_obj
            .get("color")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                RpcError::invalid_params("params.replacement.style.stroke.color missing or not u64")
            })?;
        let stroke_width = stroke_obj
            .get("width")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                RpcError::invalid_params("params.replacement.style.stroke.width missing or not u64")
            })?;
        let c = u32::try_from(stroke_color).map_err(|_| {
            RpcError::invalid_params("params.replacement.style.stroke.color exceeds u32 range")
        })?;
        let w = u32::try_from(stroke_width).map_err(|_| {
            RpcError::invalid_params("params.replacement.style.stroke.width exceeds u32 range")
        })?;
        style.stroke = Some(pinion_core::style::Stroke::new(Color::from_argb(c), w));
    }
    Ok(style)
}

/// Wire→[`pinion_core::style::ImageStyle`] coercion. `fit` is a
/// string enum; `tint` is an optional u32 ARGB multiply overlay.
fn parse_image_style(v: Option<&Value>) -> Result<pinion_core::style::ImageStyle, RpcError> {
    let Some(obj) = v else {
        return Ok(pinion_core::style::ImageStyle::default());
    };
    let mut style = pinion_core::style::ImageStyle::default();
    if let Some(fit_str) = obj.get("fit").and_then(Value::as_str) {
        style.fit = match fit_str {
            "Fill" => pinion_core::style::Fit::Fill,
            "Contain" => pinion_core::style::Fit::Contain,
            "Cover" => pinion_core::style::Fit::Cover,
            "Tile" => pinion_core::style::Fit::Tile,
            other => {
                return Err(RpcError::invalid_params(format!(
                    "params.replacement.style.fit unrecognised: {other} (expected Fill/Contain/Cover/Tile)"
                )));
            }
        };
    }
    if let Some(tint) = obj.get("tint").and_then(Value::as_u64) {
        let n = u32::try_from(tint).map_err(|_| {
            RpcError::invalid_params("params.replacement.style.tint exceeds u32 range")
        })?;
        style.tint = Some(Color::from_argb(n));
    }
    Ok(style)
}

/// Wire→`Vec<PathCommand>` coercion. Each command is an object
/// `{op: "MoveTo"|"LineTo"|"CurveTo"|"Close", ...args}`.
fn parse_path_commands(
    v: Option<&Value>,
) -> Result<Vec<pinion_core::scene::PathCommand>, RpcError> {
    let Some(arr_val) = v else {
        return Ok(Vec::new());
    };
    let Some(arr) = arr_val.as_array() else {
        return Err(RpcError::invalid_params(
            "params.replacement.commands must be an array",
        ));
    };
    arr.iter().map(parse_path_command).collect()
}

fn parse_path_command(v: &Value) -> Result<pinion_core::scene::PathCommand, RpcError> {
    use pinion_core::scene::{PathCommand, PathPoint};
    let Some(op) = v.get("op").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.replacement.commands[].op missing or not a string",
        ));
    };
    let read_point = |field: &str| -> Result<PathPoint, RpcError> {
        let obj = v.get(field).ok_or_else(|| {
            RpcError::invalid_params(format!("params.replacement.commands[].{field} missing"))
        })?;
        let read_coord = |axis: &str| -> Result<f32, RpcError> {
            let n = obj.get(axis).and_then(Value::as_f64).ok_or_else(|| {
                RpcError::invalid_params(format!(
                    "params.replacement.commands[].{field}.{axis} missing or not numeric"
                ))
            })?;
            // `PathPoint` is f32 by §5.3 scene contract; the wire ships
            // f64 for JSON portability. NaN/±∞ are rejected upfront so
            // the f32 narrowing has explicit bounded-finite preconditions
            // rather than relying on `as` truncation semantics.
            if !n.is_finite() {
                return Err(RpcError::invalid_params(format!(
                    "params.replacement.commands[].{field}.{axis} must be finite"
                )));
            }
            #[allow(
                clippy::cast_possible_truncation,
                reason = "PathPoint stores f32 per §5.3; finite-bounded narrowing is the wire→scene contract."
            )]
            Ok(n as f32)
        };
        Ok(PathPoint::new(read_coord("x")?, read_coord("y")?))
    };
    match op {
        "MoveTo" => Ok(PathCommand::MoveTo(read_point("point")?)),
        "LineTo" => Ok(PathCommand::LineTo(read_point("point")?)),
        "CurveTo" => Ok(PathCommand::CurveTo {
            c1: read_point("c1")?,
            c2: read_point("c2")?,
            end: read_point("end")?,
        }),
        "Close" => Ok(PathCommand::Close),
        other => Err(RpcError::invalid_params(format!(
            "params.replacement.commands[].op unrecognised: {other} (expected MoveTo/LineTo/CurveTo/Close)"
        ))),
    }
}

/// Wire→[`pinion_core::scene::Rect`] coercion. Required fields
/// `x` / `y` / `w` / `h` as u32-bounded numbers. Used by
/// [`parse_view_blueprint`].
fn parse_rect(v: Option<&Value>) -> Result<pinion_core::scene::Rect, RpcError> {
    let Some(obj) = v else {
        return Err(RpcError::invalid_params("params.replacement.rect missing"));
    };
    let read = |field: &str| -> Result<u32, RpcError> {
        let n = obj.get(field).and_then(Value::as_u64).ok_or_else(|| {
            RpcError::invalid_params(format!(
                "params.replacement.rect.{field} missing or not an unsigned integer"
            ))
        })?;
        u32::try_from(n).map_err(|_| {
            RpcError::invalid_params(format!("params.replacement.rect.{field} exceeds u32 range"))
        })
    };
    Ok(pinion_core::scene::Rect::new(
        read("x")?,
        read("y")?,
        read("w")?,
        read("h")?,
    ))
}

/// Wire→[`BoxStyle`] coercion for `SetStyle` payloads (R40.10).
///
/// Required: `fill` (u32 ARGB). Optional: `border_color` (u32 ARGB),
/// `border_width` (u32), `corner_radius` (u32). Missing optionals
/// default per [`BoxStyle::filled`]. Unknown keys are ignored so the
/// wire stays forward-compatible with future `BoxStyle` additions.
fn parse_box_style(style: &Value) -> Result<BoxStyle, RpcError> {
    let Some(fill) = style.get("fill").and_then(Value::as_u64) else {
        return Err(RpcError::invalid_params(
            "params.style.fill missing or not an unsigned integer",
        ));
    };
    // u32 ARGB is the wire shape; clamp to u32 explicitly so callers
    // cannot smuggle high bits past the BoxStyle field type.
    let fill_argb = u32::try_from(fill)
        .map_err(|_| RpcError::invalid_params("params.style.fill exceeds u32 range"))?;
    let mut out = BoxStyle::filled(Color::from_argb(fill_argb));
    if let Some(corner) = style.get("corner_radius").and_then(Value::as_u64) {
        let radius = u32::try_from(corner).map_err(|_| {
            RpcError::invalid_params("params.style.corner_radius exceeds u32 range")
        })?;
        out = out.with_corner_radius(radius);
    }
    // Border requires both color + width; either alone is incoherent
    // and surfaces as invalid params rather than partial application.
    let border_color = style.get("border_color").and_then(Value::as_u64);
    let border_width = style.get("border_width").and_then(Value::as_u64);
    match (border_color, border_width) {
        (Some(c), Some(w)) => {
            let c = u32::try_from(c).map_err(|_| {
                RpcError::invalid_params("params.style.border_color exceeds u32 range")
            })?;
            let w = u32::try_from(w).map_err(|_| {
                RpcError::invalid_params("params.style.border_width exceeds u32 range")
            })?;
            out = out.with_border(Border::new(Color::from_argb(c), w));
        }
        (None, None) => {}
        _ => {
            return Err(RpcError::invalid_params(
                "params.style.border_color and border_width must be provided together",
            ));
        }
    }
    Ok(out)
}

fn propose_outcome_to_json(outcome: ProposeOutcome) -> Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "preview_id".into(),
        Value::Number(outcome.preview_id.get().into()),
    );
    map.insert(
        "base_revision".into(),
        Value::Number(outcome.base_revision.into()),
    );
    Value::Object(map)
}

fn handle_scene_apply_preview(
    scene: &mut Scene,
    revision: &SceneRevision,
    previews: &PreviewLedger,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(raw_id) = params.get("preview_id").and_then(Value::as_u64) else {
        return Err(RpcError::invalid_params(
            "params.preview_id missing or not a positive integer",
        ));
    };
    let Some(id) = PreviewId::try_new(raw_id) else {
        return Err(RpcError::invalid_params(
            "params.preview_id must be non-zero",
        ));
    };
    match apply_preview(scene, revision, previews, id) {
        Ok(outcome) => Ok(apply_outcome_to_json(&outcome)),
        Err(err) => Err(apply_error_to_rpc(&err)),
    }
}

fn apply_outcome_to_json(outcome: &ApplyOutcome) -> Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "preview_id".into(),
        Value::Number(outcome.preview_id.get().into()),
    );
    map.insert(
        "new_revision".into(),
        Value::Number(outcome.new_revision.into()),
    );
    // §5.34 R40.9: DispatchIntent variants emit intents through the
    // apply context's accumulator. Surface them on the wire as a
    // structured array under "emitted_intents" reusing the same
    // {tag, payload} shape `scene/intents` already returns.
    map.insert(
        "emitted_intents".into(),
        Value::Array(outcome.emitted_intents.iter().map(intent_to_json).collect()),
    );
    Value::Object(map)
}

fn apply_error_to_rpc(err: &ApplyError) -> RpcError {
    let mut data_obj = serde_json::Map::new();
    let variant = match err {
        ApplyError::UnknownPreview => "UnknownPreview",
        ApplyError::Expired => "Expired",
        ApplyError::BaseRevisionConflict { expected, actual } => {
            data_obj.insert("expected".into(), Value::Number((*expected).into()));
            data_obj.insert("actual".into(), Value::Number((*actual).into()));
            "BaseRevisionConflict"
        }
        ApplyError::ApplyRejected(tag) => {
            data_obj.insert("reason".into(), Value::String(tag.clone()));
            "ApplyRejected"
        }
    };
    data_obj.insert("variant".into(), Value::String(variant.to_string()));
    RpcError::new(-32602, "Invalid params").with_data(Value::Object(data_obj))
}

fn propose_error_to_rpc(err: &ProposeError) -> RpcError {
    let variant = match err {
        ProposeError::CapacityFull { .. } => "CapacityFull",
    };
    RpcError::invalid_params(variant)
}

// `Result<_, _>` shape kept for dispatcher consistency — every match
// arm in [`dispatch`] expects `Result<Value, RpcError>`. `list_previews`
// happens to be infallible today, but future filters / pagination
// would introduce error variants.
#[allow(clippy::unnecessary_wraps)]
fn handle_scene_list_previews(previews: &PreviewLedger) -> Result<Value, RpcError> {
    let now = std::time::Instant::now();
    let views = list_previews(previews, now);
    let mut map = serde_json::Map::new();
    let arr: Vec<Value> = views.iter().map(|v| preview_view_to_json(v, now)).collect();
    map.insert("previews".into(), Value::Array(arr));
    Ok(Value::Object(map))
}

fn preview_view_to_json(view: &PreviewView, now: std::time::Instant) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("preview_id".into(), Value::Number(view.id.get().into()));
    obj.insert(
        "base_revision".into(),
        Value::Number(view.base_revision.into()),
    );
    obj.insert(
        "target_path".into(),
        Value::String(view.target_path.clone()),
    );
    obj.insert(
        "affected_paths".into(),
        Value::Array(
            view.affected_paths
                .iter()
                .map(|p| Value::String(p.clone()))
                .collect(),
        ),
    );
    let age = now.saturating_duration_since(view.created_at).as_millis();
    let ttl = view.deadline.saturating_duration_since(now).as_millis();
    // Wire shape uses u64; `as_millis() -> u128` truncates only beyond
    // ~584 million years — safely under all real-world TTLs.
    obj.insert(
        "age_ms".into(),
        Value::Number((u64::try_from(age).unwrap_or(u64::MAX)).into()),
    );
    obj.insert(
        "ttl_remaining_ms".into(),
        Value::Number((u64::try_from(ttl).unwrap_or(u64::MAX)).into()),
    );
    Value::Object(obj)
}

fn invoke_error_to_rpc(err: &InvokeError) -> RpcError {
    let variant = match err {
        InvokeError::Path(_) => "Path",
        InvokeError::UnsupportedPath => "UnsupportedPath",
        InvokeError::NoExternalAtPath => "NoExternalAtPath",
        InvokeError::IntrospectionOptedOut => "IntrospectionOptedOut",
        InvokeError::UnknownInvokePath => "UnknownInvokePath",
        InvokeError::InvokeTypeMismatch => "InvokeTypeMismatch",
        InvokeError::InvokeRejected => "InvokeRejected",
    };
    RpcError::invalid_params(variant)
}

/// R56.1.f.3 §5.22 — `scene/intervene` typed handler. Parses
/// `params = {"path": str, "value": Json}` and routes through
/// [`intervene`] for the §5.15 item 7 write-side door. Mirror of
/// [`handle_scene_invoke`] (the read+execute peer); the trait-level
/// distinction is `intervene = set state slot` vs
/// `invoke = call action`.
fn handle_scene_intervene(scene: &mut Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.path missing or not a string",
        ));
    };
    let Some(value_json) = params.get("value") else {
        return Err(RpcError::invalid_params("params.value missing"));
    };
    let Some(value) = json_to_introspect_value(value_json) else {
        return Err(RpcError::invalid_params(
            "params.value is not a representable IntrospectValue",
        ));
    };

    match intervene(scene, path, value) {
        Ok(()) => Ok(Value::Null),
        Err(err) => Err(intervene_error_to_rpc(&err)),
    }
}

fn intervene_error_to_rpc(err: &InterveneError) -> RpcError {
    let variant = match err {
        InterveneError::Path(_) => "Path",
        InterveneError::UnsupportedPath => "UnsupportedPath",
        InterveneError::NoExternalAtPath => "NoExternalAtPath",
        InterveneError::IntrospectionOptedOut => "IntrospectionOptedOut",
        InterveneError::UnknownIntervenePath => "UnknownIntervenePath",
        InterveneError::InterveneTypeMismatch => "InterveneTypeMismatch",
        InterveneError::ReadOnly => "ReadOnly",
        InterveneError::OutOfRange => "OutOfRange",
    };
    RpcError::invalid_params(variant)
}

fn handle_font_parse(
    registry: Option<&FontRegistry>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let registry = registry.ok_or_else(font_registry_unavailable)?;
    let params = require_params(params)?;
    let Some(bytes_arr) = params.get("bytes").and_then(Value::as_array) else {
        return Err(RpcError::invalid_params(
            "params.bytes missing or not an array",
        ));
    };
    let mut bytes: Vec<u8> = Vec::with_capacity(bytes_arr.len());
    for (i, v) in bytes_arr.iter().enumerate() {
        let Some(n) = v.as_u64() else {
            return Err(RpcError::invalid_params(format!(
                "params.bytes[{i}] not an unsigned integer"
            )));
        };
        let byte = u8::try_from(n).map_err(|_| {
            RpcError::invalid_params(format!("params.bytes[{i}] = {n} out of u8 range"))
        })?;
        bytes.push(byte);
    }
    match font::parse(registry, bytes) {
        Ok(outcome) => {
            let mut map = serde_json::Map::new();
            map.insert("font_id".to_string(), Value::Number(outcome.font_id.into()));
            Ok(Value::Object(map))
        }
        Err(err) => Err(font_error_to_rpc(&err)),
    }
}

fn handle_font_family_name(
    registry: Option<&FontRegistry>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let registry = registry.ok_or_else(font_registry_unavailable)?;
    let font_id = font_id_from_params(params)?;
    match font::family_name(registry, font_id) {
        Ok(outcome) => {
            let mut map = serde_json::Map::new();
            map.insert(
                "name".to_string(),
                outcome.name.map_or(Value::Null, Value::String),
            );
            Ok(Value::Object(map))
        }
        Err(err) => Err(font_error_to_rpc(&err)),
    }
}

fn handle_font_glyph_id_for(
    registry: Option<&FontRegistry>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let registry = registry.ok_or_else(font_registry_unavailable)?;
    let font_id = font_id_from_params(params)?;
    // font_id_from_params already rejected the None case; this second
    // require_params is for type-system unwrap, not a new gate.
    let params = require_params(params)?;
    let Some(codepoint) = params.get("codepoint").and_then(Value::as_u64) else {
        return Err(RpcError::invalid_params(
            "params.codepoint missing or not an unsigned integer",
        ));
    };
    let codepoint = u32::try_from(codepoint)
        .map_err(|_| RpcError::invalid_params("params.codepoint out of u32 range"))?;
    match font::glyph_id_for(registry, font_id, codepoint) {
        Ok(outcome) => {
            let mut map = serde_json::Map::new();
            map.insert(
                "glyph_id".to_string(),
                outcome
                    .glyph_id
                    .map_or(Value::Null, |g| Value::Number(g.into())),
            );
            Ok(Value::Object(map))
        }
        Err(err) => Err(font_error_to_rpc(&err)),
    }
}

fn font_id_from_params(params: Option<&Value>) -> Result<u32, RpcError> {
    let params = require_params(params)?;
    let Some(font_id) = params.get("font_id").and_then(Value::as_u64) else {
        return Err(RpcError::invalid_params(
            "params.font_id missing or not an unsigned integer",
        ));
    };
    u32::try_from(font_id).map_err(|_| RpcError::invalid_params("params.font_id out of u32 range"))
}

fn font_registry_unavailable() -> RpcError {
    RpcError::internal_error("FontRegistryUnavailable")
}

fn font_error_to_rpc(err: &FontError) -> RpcError {
    let (code, message, variant): (i32, &str, &str) = match err {
        FontError::NotFound { .. } => (-32602, "Invalid params", "NotFound"),
        FontError::GlyphIdOutOfRange { .. } => (-32602, "Invalid params", "GlyphIdOutOfRange"),
        FontError::Parse(_) => (-32602, "Invalid params", "Parse"),
        FontError::RegistryExhausted => (-32603, "Internal error", "RegistryExhausted"),
        FontError::RegistryPoisoned => (-32603, "Internal error", "RegistryPoisoned"),
    };
    RpcError::new(code, message).with_data_string(variant)
}

fn handle_font_glyph_outline(
    registry: Option<&FontRegistry>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let registry = registry.ok_or_else(font_registry_unavailable)?;
    let font_id = font_id_from_params(params)?;
    let params = require_params(params)?;
    let Some(glyph_id) = params.get("glyph_id").and_then(Value::as_u64) else {
        return Err(RpcError::invalid_params(
            "params.glyph_id missing or not an unsigned integer",
        ));
    };
    let glyph_id = u16::try_from(glyph_id)
        .map_err(|_| RpcError::invalid_params("params.glyph_id out of u16 range"))?;
    match font::glyph_outline(registry, font_id, glyph_id) {
        Ok(outcome) => serialize_outcome(&outcome, "glyph_outline"),
        Err(err) => Err(font_error_to_rpc(&err)),
    }
}

fn handle_font_cmap_subtables(
    registry: Option<&FontRegistry>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let registry = registry.ok_or_else(font_registry_unavailable)?;
    let font_id = font_id_from_params(params)?;
    match font::cmap_subtables(registry, font_id) {
        Ok(outcome) => serialize_outcome(&outcome, "cmap_subtables"),
        Err(err) => Err(font_error_to_rpc(&err)),
    }
}

fn handle_font_metrics(
    registry: Option<&FontRegistry>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let registry = registry.ok_or_else(font_registry_unavailable)?;
    let font_id = font_id_from_params(params)?;
    match font::metrics(registry, font_id) {
        Ok(outcome) => serialize_outcome(&outcome, "metrics"),
        Err(err) => Err(font_error_to_rpc(&err)),
    }
}

fn handle_font_subfamily_name(
    registry: Option<&FontRegistry>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let registry = registry.ok_or_else(font_registry_unavailable)?;
    let font_id = font_id_from_params(params)?;
    match font::subfamily_name(registry, font_id) {
        Ok(outcome) => serialize_outcome(&outcome, "subfamily_name"),
        Err(err) => Err(font_error_to_rpc(&err)),
    }
}

fn handle_font_full_name(
    registry: Option<&FontRegistry>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let registry = registry.ok_or_else(font_registry_unavailable)?;
    let font_id = font_id_from_params(params)?;
    match font::full_name(registry, font_id) {
        Ok(outcome) => serialize_outcome(&outcome, "full_name"),
        Err(err) => Err(font_error_to_rpc(&err)),
    }
}

fn handle_font_postscript_name(
    registry: Option<&FontRegistry>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let registry = registry.ok_or_else(font_registry_unavailable)?;
    let font_id = font_id_from_params(params)?;
    match font::postscript_name(registry, font_id) {
        Ok(outcome) => serialize_outcome(&outcome, "postscript_name"),
        Err(err) => Err(font_error_to_rpc(&err)),
    }
}

fn handle_font_dispose(
    registry: Option<&FontRegistry>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let registry = registry.ok_or_else(font_registry_unavailable)?;
    let font_id = font_id_from_params(params)?;
    match font::dispose(registry, font_id) {
        Ok(outcome) => serialize_outcome(&outcome, "dispose"),
        Err(err) => Err(font_error_to_rpc(&err)),
    }
}

fn handle_font_list(registry: Option<&FontRegistry>) -> Result<Value, RpcError> {
    let registry = registry.ok_or_else(font_registry_unavailable)?;
    match font::list(registry) {
        Ok(outcome) => serialize_outcome(&outcome, "list"),
        Err(err) => Err(font_error_to_rpc(&err)),
    }
}

fn handle_text_normalize(params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(text) = params.get("text").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.text missing or not a string",
        ));
    };
    let Some(form_str) = params.get("form").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params(
            "params.form missing or not a string",
        ));
    };
    let form = match form_str {
        "NFC" => NormalizeForm::Nfc,
        "NFD" => NormalizeForm::Nfd,
        "NFKC" => NormalizeForm::Nfkc,
        "NFKD" => NormalizeForm::Nfkd,
        other => {
            return Err(RpcError::invalid_params(format!(
                "params.form must be NFC/NFD/NFKC/NFKD (got {other:?})"
            )));
        }
    };
    let outcome = text_normalize(text, form);
    Ok(normalize_outcome_to_json(&outcome))
}

fn normalize_outcome_to_json(outcome: &NormalizeOutcome) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("text".to_string(), Value::String(outcome.text.clone()));
    Value::Object(map)
}

/// Convert a serde-derived font outcome to a JSON `Value`. The
/// `method` tag goes into the error data when serialization fails so
/// the AI client can map the failure back to a method.
fn serialize_outcome<T: Serialize>(outcome: &T, method: &str) -> Result<Value, RpcError> {
    serde_json::to_value(outcome)
        .map_err(|e| RpcError::internal_error(format!("font/{method} serialize: {e}")))
}

fn json_to_introspect_value(v: &Value) -> Option<IntrospectValue> {
    match v {
        Value::Null => Some(IntrospectValue::Null),
        Value::Bool(b) => Some(IntrospectValue::Bool(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(IntrospectValue::Int)
            .or_else(|| n.as_f64().map(IntrospectValue::Float)),
        Value::String(s) => Some(IntrospectValue::Text(s.clone())),
        // Structured payloads round-trip via `IntrospectValue::Json`
        // (R37.6 #11): the reactive bridge `Signal<T>` for non-scalar
        // `T` reaches RPC through this variant.
        Value::Array(_) | Value::Object(_) => Some(IntrospectValue::Json(v.clone())),
    }
}

fn query_error_to_rpc(err: QueryError) -> RpcError {
    let variant = match err {
        QueryError::Path(_) => "Path",
        QueryError::UnsupportedPath => "UnsupportedPath",
        QueryError::NoExternalAtPath => "NoExternalAtPath",
        QueryError::IntrospectionOptedOut => "IntrospectionOptedOut",
        QueryError::UnknownIntrospectPath => "UnknownIntrospectPath",
    };
    RpcError::invalid_params(variant)
}

pub(crate) fn introspect_value_to_json(value: IntrospectValue) -> Value {
    match value {
        IntrospectValue::Bool(b) => Value::Bool(b),
        IntrospectValue::Int(n) => Value::Number(n.into()),
        IntrospectValue::Float(f) => {
            serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
        }
        IntrospectValue::Text(s) => Value::String(s),
        IntrospectValue::Json(v) => v,
        // `IntrospectValue::Null` collapses into the non_exhaustive
        // wildcard; future additive variants also land as JSON null
        // until §5.12 schema settles a richer projection.
        _ => Value::Null,
    }
}

fn error_response(
    id: Option<RequestId>,
    code: i32,
    message: &str,
    data: Option<Value>,
) -> Response {
    let mut err = RpcError::new(code, message);
    if let Some(d) = data {
        err = err.with_data(d);
    }
    Response {
        jsonrpc: JSONRPC_V2.to_string(),
        result: None,
        error: Some(err),
        id,
    }
}

fn serialize(resp: &Response) -> String {
    // serde_json on a well-formed Response cannot fail in practice.
    serde_json::to_string(resp).unwrap_or_else(|e| {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"error\":{{\"code\":-32603,\"message\":\"serialize failed: {e}\"}},\"id\":null}}"
        )
    })
}

// R626 §5.7 — the #[cfg(test)] tests module body lives in
// dispatch_tests.rs (5,643 lines of wire-integration tests for ~50
// handlers across query / mutate / preview / introspect / font /
// focus axes). Pre-R626 the body sat inline at the tail of
// dispatch.rs and pushed the file past 9,700 LOC; R626 lifts the
// body to a sibling file via #[path] so navigation, search, and
// recompile latency improve. The module path is still `tests` and
// the contents still have full private-item access via `use super::*`.
#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
