//! JSON-RPC 2.0 envelope and method-dispatch entry (§5.7, R16 slice 10).
//!
//! Realizes the wire shape ratified in §5.7: parse a JSON-RPC 2.0
//! request envelope, route to a typed handler (§5.12), and emit a
//! response envelope. Registered methods (R612 — 27 typed):
//! `scene/query`, `scene/click`, `scene/rewind`, `scene/snapshot`,
//! `scene/dry_run`, `scene/waitFor`, `scene/screenshot`,
//! `scene/invoke`, `scene/intents`, `scene/locate`,
//! `scene/locate_region`, `scene/bbox`, `scene/cancel_preview`,
//! `scene/list_previews`, `scene/propose_change`,
//! `scene/apply_preview`, `scene/theme_tokens`,
//! `scene/set_theme_mode`, `scene/set_theme_palettes`,
//! `scene/animation_state`, `scene/scroll_state`,
//! `scene/set_scroll_offset`, `scene/text_state`,
//! `scene/set_text`, `scene/set_selection`, `scene/set_caret`,
//! `scene/caret_state`. The preview-lifecycle methods take the
//! `&PreviewLedger` and `&SceneRevision` arguments the dispatcher
//! receives from its caller alongside the scene.
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

use pinion_core::event::WheelDelta;
use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::style::{Border, BoxStyle, Color};
use pinion_core::{Owner, Scene, SceneRevision};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::animation_state::{animation_state, AnimationStateError, AnimationStateOutcome};
use crate::cache_stats::{cache_stats, CacheStatsError, CacheStatsOutcome};
use crate::caret_state::{caret_state, CaretStateOutcome};
use crate::commands::{list_pending_commands, CommandsError};
use crate::dry_run::{dry_run, DryRunError};
use crate::simulate::{simulate, simulate_with_owner, SimulateError, SimulateStep};
use crate::font::{self, FontError, FontRegistry};
use crate::intents::{drain_intents, IntentsError};
use crate::intervene::{intervene, InterveneError};
use crate::invoke::{invoke, InvokeError};
use crate::layout_query::{
    layout_query, LayoutNode, LayoutQueryError, LayoutQueryParams,
};
use crate::locate::{
    bbox, locate, locate_region, BboxError, LocateError, LocateOutcome, LocateRegionOutcome,
};
use crate::resize::{resize, ResizeError, ResizeParams};
use crate::preview::{
    apply_preview, cancel_preview, list_previews, propose_change, ApplyError, ApplyOutcome,
    PreviewId, PreviewLedger, PreviewView, ProposeError, ProposeOutcome, TypedProposal,
    ViewBlueprint,
};
use crate::query::{query, QueryError};
use crate::rewind::{rewind, RewindError};
use crate::screenshot::{screenshot, Screenshot, ScreenshotError};
use crate::scroll_state::{
    scroll_state, set_scroll_offset, ScrollStateOutcome, SetScrollOffsetParams,
};
use crate::substrate_introspect::{introspect_error_to_data, SubstrateIntrospectError};
use crate::snapshot::{snapshot, SnapshotError, SnapshotNode};
use crate::text::{text_normalize, NormalizeForm, NormalizeOutcome};
use crate::text_state::{
    set_caret, set_selection, set_text, text_state, SetCaretParams, SetSelectionParams,
    SetTextParams, TextStateOutcome,
};
use crate::theme::{
    parse_palette_value, set_theme_mode, set_theme_palettes, theme_tokens, PaletteParseError,
    SetThemeModeError, SetThemeModeOutcome, SetThemeModeParams, SetThemePalettesError,
    SetThemePalettesOutcome, SetThemePalettesParams, ThemeTokensError, ThemeTokensOutcome,
};
use crate::wait_for::{wait_for, WaitForError, WaitOutcome};

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
        skip_serializing_if = "Option::is_none",
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
fn deserialize_nullable_present<'de, D>(
    deserializer: D,
) -> Result<Option<Value>, D::Error>
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
    /// R47.7.4 §5.12 — application-supplied resize request hook.
    /// Invoked by `scene/resize` with the requested logical
    /// `(width, height)`. The application typically calls
    /// `winit::window::Window::request_inner_size` inside the
    /// closure so winit emits a `Resized` event on the next loop
    /// iteration. Asynchronous — AI clients pair with
    /// `scene/wait_for_frame` for stable observation.
    pub resize_request: Option<&'a mut (dyn FnMut(u32, u32) + 'a)>,
    /// R47.7.5 §5.12 — application's most recent winit-rendered
    /// frame snapshot. `scene/layout` with `viewport: null` returns
    /// a clone of this value. The application refreshes the
    /// snapshot at the end of each `render()` pass via
    /// `layout_query::build_layout_node`. `None` until winit has
    /// rendered the first frame — `scene/layout {viewport: null}`
    /// errors with `NoLastPaintLayout` in that window.
    pub last_paint_layout: Option<&'a LayoutNode>,
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
    Key { x: f64, y: f64, key: String },
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
    CharacterKey { x: f64, y: f64, character: String },
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
    /// `cursor_moved(MOUSE, from_x, from_y)`, then `mouse_pressed(MOUSE)`,
    /// then `steps` interpolated `cursor_moved` frames marching linearly
    /// toward `(to_x, to_y)`, then `mouse_released(MOUSE)`. Mirrors the
    /// real-mouse drag arc winit emits for a `MouseInput::Pressed`
    /// followed by a sequence of `CursorMoved` and the matching
    /// `Released`, exercising the `InputRouter`'s R51.34 capture lock
    /// plus the receiving widget's `pointer_move` fractional dispatch
    /// — R55.D.3 `ScrollBar` drag math today; future `Slider` drag
    /// rides the same primitive.
    Drag {
        from_x: f64,
        from_y: f64,
        to_x: f64,
        to_y: f64,
        steps: u32,
    },
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
            resize_request: None,
            last_paint_layout: None,
            last_paint_scene: None,
            font_registry: None,
            focus_manager: None,
            runtime_owner: None,
            commands_executor: None,
            deferred_inputs: None,
            window_id: None,
            fragment_cache_stats: None,
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

    /// Builder: attach the window resize request closure (R47.7.4
    /// §5.12). The closure is invoked by `scene/resize` with the
    /// requested logical `(width, height)`; it typically calls
    /// `winit::window::Window::request_inner_size` so winit emits a
    /// `Resized` event on the next loop iteration.
    #[must_use]
    pub fn with_resize_request(
        mut self,
        request: &'a mut (dyn FnMut(u32, u32) + 'a),
    ) -> Self {
        self.resize_request = Some(request);
        self
    }

    /// Builder: attach the most recent winit-rendered frame snapshot
    /// (R47.7.5 §5.12). `scene/layout {viewport: null}` returns a
    /// clone; the application refreshes the snapshot at the end of
    /// each `render()` pass.
    #[must_use]
    pub fn with_last_paint_layout(mut self, snapshot: &'a LayoutNode) -> Self {
        self.last_paint_layout = Some(snapshot);
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
    pub fn with_focus_manager(
        mut self,
        focus: &'a mut pinion_runtime::FocusManager,
    ) -> Self {
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
    pub fn with_commands_executor(
        mut self,
        executor: &'a pinion_runtime::CommandExecutor,
    ) -> Self {
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
    pub fn with_fragment_cache_stats(
        mut self,
        stats: pinion_runtime::FragmentCacheStats,
    ) -> Self {
        self.fragment_cache_stats = Some(stats);
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

/// R671 §5.7 — dispatch a pre-parsed [`Request`] against the live
/// context. Identical method routing as [`dispatch`] but skips the
/// envelope parse step so callers that have already parsed the
/// request (typically to extract out-of-band scope params like
/// `{window: "<id>"}`) hand the same object through without paying
/// a second parse.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "the routing match is the single source of truth for method names — growing with each method addition is the textbook canonical evolution path (currently 22 scene/* + 11 font/* + 1 text/* + 4 focus/*)"
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
    let mut resize_request = ctx.resize_request.take();
    // R51.73 §5.40 — same split-borrow pattern for the focus manager:
    // `focus/set` mutates, `focus/get` reads; both need exclusive
    // access during the route arm.
    let mut focus_manager = ctx.focus_manager.take();
    // R47.7.5 — snapshot read-only; safe to copy the &LayoutNode out
    // of the context for the dispatch lifetime.
    let last_paint_layout = ctx.last_paint_layout;
    // R705 §5.12 §2 #7 — the stored paint scene (displayed frame).
    // `scene/snapshot from: paint` serializes this instead of
    // re-rendering at query time. Copied out for the dispatch lifetime
    // (read-only borrow, same shape as `last_paint_layout`).
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

    // R620 §5.7 — every match arm returns `(handler_outcome, kind)`;
    // the kind is the OCC bump contract right at the arm so the
    // compiler enforces the pairing (missing kind = tuple shape
    // mismatch). See `HandlerKind` docstring for the read-vs-mutate
    // taxonomy.
    let (outcome, kind) = match request.method.as_str() {
        "scene/query" => (handle_scene_query(scene, request.params.as_ref()), HandlerKind::Read),
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
                handle_scene_click(inbox, producer, last_paint_layout, request.params.as_ref()),
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
                handle_scene_hover(inbox, producer, last_paint_layout, request.params.as_ref()),
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
                    last_paint_layout,
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
                handle_scene_drag(inbox, producer, last_paint_layout, request.params.as_ref()),
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
                handle_scene_snapshot(
                    scene,
                    producer,
                    last_paint_scene,
                    request.params.as_ref(),
                ),
                HandlerKind::Read,
            )
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
            handle_scene_screenshot(scene, request.params.as_ref()),
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
                handle_scene_layout(producer, last_paint_layout, request.params.as_ref()),
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
                handle_scene_key(inbox, producer, last_paint_layout, request.params.as_ref()),
                // Input enqueue — mutation deferred to next dispatch
                // cycle; no immediate OCC bump.
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
                handle_scene_wheel(inbox, producer, last_paint_layout, request.params.as_ref()),
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
                handle_scene_scroll(producer, last_paint_layout, request.params.as_ref()),
                HandlerKind::Read,
            )
        }
        "scene/cancel_preview" => (
            handle_scene_cancel_preview(previews, request.params.as_ref()),
            HandlerKind::Read,
        ),
        "scene/list_previews" => (
            handle_scene_list_previews(previews),
            HandlerKind::Read,
        ),
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
            crate::focus::handle_focus_set(
                focus_manager.as_deref_mut(),
                request.params.as_ref(),
            ),
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
            Err(RpcError::new(-32601, "Method not found")
                .with_data_string(request.method.clone())),
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

fn handle_scene_query(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params("params.path missing or not a string"));
    };

    match query(scene, path) {
        Ok(value) => Ok(introspect_value_to_json(value)),
        Err(err) => Err(query_error_to_rpc(err)),
    }
}

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
fn handle_scene_click<F>(
    inbox: Option<&mut Vec<DeferredInput>>,
    paint_producer: Option<&mut F>,
    last_paint_layout: Option<&LayoutNode>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let params = require_params(params)?;
    let (x, y) = resolve_at_or_path(params, paint_producer, last_paint_layout)?;
    inbox.push(DeferredInput::Click { x, y });
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
    last_paint_layout: Option<&LayoutNode>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let params = require_params(params)?;
    let (x, y) = resolve_at_or_path(params, paint_producer, last_paint_layout)?;
    inbox.push(DeferredInput::Hover { x, y });
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
    last_paint_layout: Option<&LayoutNode>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(inbox) = inbox else {
        return Err(RpcError::invalid_params("InputInjectionUnavailable"));
    };
    let params = require_params(params)?;
    let (x, y) = resolve_at_or_path(params, paint_producer, last_paint_layout)?;
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
///   "from": {"x": <f64>, "y": <f64>},   // or "from_path": "<tag>"
///   "to":   {"x": <f64>, "y": <f64>},   // or "to_path":   "<tag>"
///   "steps": <u32>                       // optional, default 8
/// }
/// ```
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
    last_paint_layout: Option<&LayoutNode>,
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
        last_paint_layout,
    )?;
    #[allow(
        clippy::option_as_ref_deref,
        reason = "manual reborrow for the 2nd resolve (same rationale as the 1st)"
    )]
    let producer_for_to = paint_producer.as_mut().map(|p| &mut **p);
    let to = resolve_drag_endpoint(
        params,
        "to",
        "to_path",
        producer_for_to,
        last_paint_layout,
    )?;
    let steps = match params.get("steps") {
        Some(v) => v
            .as_u64()
            .ok_or_else(|| RpcError::invalid_params("params.steps must be a non-negative integer"))
            .and_then(|n| {
                u32::try_from(n).map_err(|_| {
                    RpcError::invalid_params("params.steps does not fit in u32")
                })
            })?,
        None => 8,
    };
    inbox.push(DeferredInput::Drag {
        from_x: from.0,
        from_y: from.1,
        to_x: to.0,
        to_y: to.1,
        steps,
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
    last_paint_layout: Option<&LayoutNode>,
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
        (None, Some(tag)) => resolve_path_to_center(tag, paint_producer, last_paint_layout),
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
    last_paint_layout: Option<&LayoutNode>,
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
        (None, Some(tag)) => resolve_path_to_center(tag, paint_producer, last_paint_layout),
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
    last_paint_layout: Option<&LayoutNode>,
) -> Result<(f64, f64), RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let Some(producer) = paint_producer else {
        return Err(RpcError::invalid_params("PaintProducerUnavailable"));
    };
    // The tag's rect only matches the live window's hit-test if the
    // paint pass runs at the same viewport size as the window. Pull
    // it from the last-paint snapshot the shell wired up (the root
    // `LayoutNode.rect` IS the live window's geometry); fall back
    // to a 720×480 default for headless / no-frame-yet callers.
    let (vw, vh) = last_paint_layout
        .map_or((720, 480), |l| (l.rect.w.max(1), l.rect.h.max(1)));
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
    last_paint_layout: Option<&LayoutNode>,
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
    let state = resolve_scroll_target_at_or_path(params, paint_producer, last_paint_layout)?;
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
    last_paint_layout: Option<&LayoutNode>,
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
    let (vw, vh) = last_paint_layout
        .map_or((720, 480), |l| (l.rect.w.max(1), l.rect.h.max(1)));
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
    let coord_x = u32::try_from(raw_x).map_err(|_| {
        RpcError::invalid_params(format!("params.at.x out of u32 range: {raw_x}"))
    })?;
    let coord_y = u32::try_from(raw_y).map_err(|_| {
        RpcError::invalid_params(format!("params.at.y out of u32 range: {raw_y}"))
    })?;
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
        return Err(RpcError::invalid_params("params.path missing or not a string"));
    };
    let Some(value_json) = params.get("value") else {
        return Err(RpcError::invalid_params("params.value missing"));
    };
    let Some(value) = json_to_introspect_value(value_json) else {
        return Err(RpcError::invalid_params("params.value unsupported (v0: null/bool/number/string only)"));
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
        return Err(RpcError::invalid_params("params.path missing or not a string"));
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
/// Params: `{at: {x: f64, y: f64}, key: <W3C KeyboardEvent.key string>}`.
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
    last_paint_layout: Option<&LayoutNode>,
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
    // R51.202 §5.49 — key location is either an explicit cursor
    // coordinate or a tag lookup via the paint scene, mirroring
    // `scene/click`'s shape.
    let (x, y) = resolve_at_or_path(params, paint_producer, last_paint_layout)?;
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
        });
    } else {
        inbox.push(DeferredInput::Key {
            x,
            y,
            key: key.to_owned(),
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
    last_paint_layout: Option<&LayoutNode>,
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
    let (x, y) = resolve_at_or_path(params, paint_producer, last_paint_layout)?;
    inbox.push(DeferredInput::Wheel { x, y, delta });
    Ok(Value::Null)
}

fn parse_wheel_delta(obj: &serde_json::Map<String, Value>) -> Result<WheelDelta, RpcError> {
    let extract = |key: &str| -> Result<Option<(f32, f32)>, RpcError> {
        let Some(inner) = obj.get(key).and_then(Value::as_object) else {
            return Ok(None);
        };
        let dx = inner
            .get("dx")
            .and_then(Value::as_f64)
            .ok_or_else(|| RpcError::invalid_params(format!("params.delta.{key}.dx missing")))?;
        let dy = inner
            .get("dy")
            .and_then(Value::as_f64)
            .ok_or_else(|| RpcError::invalid_params(format!("params.delta.{key}.dy missing")))?;
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
        }
        SnapshotNode::Path(snap) => {
            obj.insert("rect".to_string(), snapshot_rect_to_json(snap.rect));
            obj.insert("tag".to_string(), snapshot_tag_to_json(snap.tag.as_deref()));
            let cmds: Vec<Value> = snap
                .commands
                .iter()
                .map(path_command_to_json)
                .collect();
            obj.insert("commands".to_string(), Value::Array(cmds));
            obj.insert("style".to_string(), path_style_to_json(snap.style));
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
            obj.insert(
                "offset_x".to_string(),
                Value::Number(snap.offset_x.into()),
            );
            obj.insert(
                "offset_y".to_string(),
                Value::Number(snap.offset_y.into()),
            );
            obj.insert(
                "content".to_string(),
                snapshot_node_to_json(*snap.content),
            );
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
        _ => {}
    }

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
            serde_json::Number::from_f64(f64::from(p.x))
                .map_or(Value::Null, Value::Number),
        );
        obj.insert(
            "y".to_string(),
            serde_json::Number::from_f64(f64::from(p.y))
                .map_or(Value::Null, Value::Number),
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
fn path_style_to_json(style: pinion_core::style::PathStyle) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "stroke".to_string(),
        style.stroke.map_or(Value::Null, stroke_to_json),
    );
    obj.insert(
        "fill".to_string(),
        style.fill.map_or(Value::Null, color_to_json),
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
fn text_style_to_json(style: &pinion_core::style::TextStyle) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "font_family".to_string(),
        style
            .font_family
            .as_ref()
            .map_or(Value::Null, |f| Value::String(f.as_ref().to_string())),
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
    obj.insert("font_style".to_string(), font_style_to_json(style.font_style));
    obj.insert("line_height".to_string(), line_height_to_json(style.line_height));
    obj.insert(
        "letter_spacing".to_string(),
        Value::Number(style.letter_spacing.into()),
    );
    obj.insert("text_align".to_string(), text_align_to_json(style.text_align));
    obj.insert(
        "decoration".to_string(),
        text_decoration_to_json(style.decoration),
    );
    obj.insert("overflow".to_string(), text_overflow_to_json(style.overflow));
    Value::Object(obj)
}

fn handle_scene_dry_run(scene: &mut Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params("params.path missing or not a string"));
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
        return Err(RpcError::invalid_params("params.steps missing or not an array"));
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
        steps.push(SimulateStep { path: path.to_string(), value });
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
        return Err(RpcError::invalid_params("params.path missing or not a string"));
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
        return Err(RpcError::invalid_params("params.max_attempts missing or not u64"));
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
    obj.insert("attempts".to_string(), Value::Number(outcome.attempts.into()));
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

fn handle_scene_screenshot(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params("params.path missing or not a string"));
    };

    match screenshot(scene, path) {
        Ok(shot) => Ok(screenshot_to_json(&shot)),
        Err(err) => Err(screenshot_error_to_rpc(err)),
    }
}

fn screenshot_to_json(shot: &Screenshot) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("width".to_string(), Value::Number(shot.width.into()));
    obj.insert("height".to_string(), Value::Number(shot.height.into()));
    // v0 wire shape carries raw bytes as a JSON array of u8. Future
    // slices may switch to base64 (single string) to halve frame size.
    let pixels = shot
        .pixels_rgba8
        .iter()
        .map(|b| Value::Number((*b).into()))
        .collect();
    obj.insert("pixels_rgba8".to_string(), Value::Array(pixels));
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
        return Err(RpcError::invalid_params("params.path missing or not a string"));
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
    obj.insert("tag".to_string(), Value::String(intent.tag_str().to_string()));
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
            v.as_f64()
                .ok_or_else(|| RpcError::invalid_params("params.epsilon must be a number when present"))
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
            { v as f32 }
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
            RpcError::invalid_params(format!(
                "params.epsilon {value} must be finite and >= 0",
            ))
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
            v.as_str().ok_or_else(|| {
                RpcError::invalid_params("params.tag must be a string when present")
            })
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
    params
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| {
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
            RpcError::invalid_params(format!(
                "params.{field} {int_v} out of i32 range",
            ))
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
    let typed_params = SetSelectionParams {
        tag,
        anchor,
        focus,
    };
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
            RpcError::invalid_params(format!(
                "params.{field} {int_v} out of usize range",
            ))
            .with_data_string("InvalidByteOffset")
        });
    }
    Err(
        RpcError::invalid_params(format!(
            "params.{field} must be a non-negative integer",
        ))
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
        return Err(RpcError::invalid_params("params.x missing or not a non-negative integer"));
    };
    let Some(y) = params.get("y").and_then(Value::as_u64) else {
        return Err(RpcError::invalid_params("params.y missing or not a non-negative integer"));
    };
    let x32 = u32::try_from(x).map_err(|_| RpcError::invalid_params("params.x exceeds u32 range"))?;
    let y32 = u32::try_from(y).map_err(|_| RpcError::invalid_params("params.y exceeds u32 range"))?;

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
        Value::Array(out.ancestor_paths.iter().cloned().map(Value::String).collect()),
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
        let raw = params
            .get(k)
            .and_then(Value::as_u64)
            .ok_or_else(|| RpcError::invalid_params(format!("params.{k} missing or not a non-negative integer")))?;
        u32::try_from(raw).map_err(|_| RpcError::invalid_params(format!("params.{k} exceeds u32 range")))
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
    map.insert("common_ancestor".into(), Value::String(out.common_ancestor.clone()));
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
        return Err(RpcError::invalid_params("params.path missing or not a string"));
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
    last_paint_layout: Option<&LayoutNode>,
    params: Option<&Value>,
) -> Result<Value, RpcError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    let params = require_params(params)?;
    let typed: LayoutQueryParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcError::invalid_params(format!("params shape: {e}")))?;
    match layout_query(&typed, paint_producer, last_paint_layout) {
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
        return Err(RpcError::invalid_params("params.preview_id must be non-zero"));
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
                RpcError::invalid_params("params.intent.payload not a representable IntrospectValue shape")
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
            let style = parse_box_style(
                v.get("style").ok_or_else(|| RpcError::invalid_params("params.replacement.style missing"))?,
            )?;
            Ok(ViewBlueprint::Box { rect, style, tag })
        }
        "Container" => {
            let style = parse_box_style(
                v.get("style").ok_or_else(|| RpcError::invalid_params("params.replacement.style missing"))?,
            )?;
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
        let n = u32::try_from(font_size)
            .map_err(|_| RpcError::invalid_params("params.replacement.style.font_size_px exceeds u32 range"))?;
        style.font_size_px = n;
    }
    if let Some(fg) = obj.get("fg_color").and_then(Value::as_u64) {
        let n = u32::try_from(fg)
            .map_err(|_| RpcError::invalid_params("params.replacement.style.fg_color exceeds u32 range"))?;
        style.fg_color = Color::from_argb(n);
    }
    if let Some(family) = obj.get("font_family").and_then(Value::as_str) {
        style.font_family = Some(std::borrow::Cow::Owned(family.to_owned()));
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
        let n = u32::try_from(fill)
            .map_err(|_| RpcError::invalid_params("params.replacement.style.fill exceeds u32 range"))?;
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
        let c = u32::try_from(stroke_color)
            .map_err(|_| RpcError::invalid_params("params.replacement.style.stroke.color exceeds u32 range"))?;
        let w = u32::try_from(stroke_width)
            .map_err(|_| RpcError::invalid_params("params.replacement.style.stroke.width exceeds u32 range"))?;
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
        let n = u32::try_from(tint)
            .map_err(|_| RpcError::invalid_params("params.replacement.style.tint exceeds u32 range"))?;
        style.tint = Some(Color::from_argb(n));
    }
    Ok(style)
}

/// Wire→`Vec<PathCommand>` coercion. Each command is an object
/// `{op: "MoveTo"|"LineTo"|"CurveTo"|"Close", ...args}`.
fn parse_path_commands(v: Option<&Value>) -> Result<Vec<pinion_core::scene::PathCommand>, RpcError> {
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
        let radius = u32::try_from(corner)
            .map_err(|_| RpcError::invalid_params("params.style.corner_radius exceeds u32 range"))?;
        out = out.with_corner_radius(radius);
    }
    // Border requires both color + width; either alone is incoherent
    // and surfaces as invalid params rather than partial application.
    let border_color = style.get("border_color").and_then(Value::as_u64);
    let border_width = style.get("border_width").and_then(Value::as_u64);
    match (border_color, border_width) {
        (Some(c), Some(w)) => {
            let c = u32::try_from(c)
                .map_err(|_| RpcError::invalid_params("params.style.border_color exceeds u32 range"))?;
            let w = u32::try_from(w)
                .map_err(|_| RpcError::invalid_params("params.style.border_width exceeds u32 range"))?;
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
        return Err(RpcError::invalid_params("params.preview_id must be non-zero"));
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
fn handle_scene_intervene(
    scene: &mut Scene,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let params = require_params(params)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(RpcError::invalid_params("params.path missing or not a string"));
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
        return Err(RpcError::invalid_params("params.bytes missing or not an array"));
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
            map.insert(
                "font_id".to_string(),
                Value::Number(outcome.font_id.into()),
            );
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
    u32::try_from(font_id)
        .map_err(|_| RpcError::invalid_params("params.font_id out of u32 range"))
}

fn font_registry_unavailable() -> RpcError {
    RpcError::internal_error("FontRegistryUnavailable")
}

fn font_error_to_rpc(err: &FontError) -> RpcError {
    let (code, message, variant): (i32, &str, &str) = match err {
        FontError::NotFound { .. } => (-32602, "Invalid params", "NotFound"),
        FontError::GlyphIdOutOfRange { .. } => {
            (-32602, "Invalid params", "GlyphIdOutOfRange")
        }
        FontError::Parse(_) => (-32602, "Invalid params", "Parse"),
        FontError::RegistryExhausted => {
            (-32603, "Internal error", "RegistryExhausted")
        }
        FontError::RegistryPoisoned => {
            (-32603, "Internal error", "RegistryPoisoned")
        }
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
        IntrospectValue::Float(f) => serde_json::Number::from_f64(f)
            .map_or(Value::Null, Value::Number),
        IntrospectValue::Text(s) => Value::String(s),
        IntrospectValue::Json(v) => v,
        // `IntrospectValue::Null` collapses into the non_exhaustive
        // wildcard; future additive variants also land as JSON null
        // until §5.12 schema settles a richer projection.
        _ => Value::Null,
    }
}

fn error_response(id: Option<RequestId>, code: i32, message: &str, data: Option<Value>) -> Response {
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
