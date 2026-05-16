//! JSON-RPC 2.0 envelope and method-dispatch entry (§5.7, R16 slice 10).
//!
//! Realizes the wire shape ratified in §5.7: parse a JSON-RPC 2.0
//! request envelope, route to a typed handler (§5.12), and emit a
//! response envelope. Registered methods (R40.6 — 16 typed):
//! `scene/query`, `scene/click`, `scene/rewind`, `scene/snapshot`,
//! `scene/dry_run`, `scene/waitFor`, `scene/screenshot`,
//! `scene/invoke`, `scene/intents`, `scene/locate`,
//! `scene/locate_region`, `scene/bbox`, `scene/cancel_preview`,
//! `scene/list_previews`, `scene/propose_change`,
//! `scene/apply_preview`. The preview-lifecycle methods take the
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

use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::style::{Border, BoxStyle, Color};
use pinion_core::{Scene, SceneRevision};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::click::{click, ClickError, ClickOutcome};
use crate::dry_run::{dry_run, DryRunError};
use crate::intents::{drain_intents, IntentsError};
use crate::invoke::{invoke, InvokeError};
use crate::locate::{
    bbox, locate, locate_region, BboxError, LocateError, LocateOutcome, LocateRegionOutcome,
};
use crate::preview::{
    apply_preview, cancel_preview, list_previews, propose_change, ApplyError, ApplyOutcome,
    PreviewId, PreviewLedger, PreviewView, ProposeError, ProposeOutcome, TypedProposal,
    ViewBlueprint,
};
use crate::query::{query, QueryError};
use crate::rewind::{rewind, RewindError};
use crate::screenshot::{screenshot, Screenshot, ScreenshotError};
use crate::snapshot::{snapshot, ExternalSnapshot, SnapshotError, SnapshotNode};
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    pub id: Option<RequestId>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
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
}

impl<'a> DispatchContext<'a> {
    /// Build a context from the three borrowed runtime handles.
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
        }
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
    let scene: &mut Scene = &mut *ctx.scene;
    let previews: &PreviewLedger = ctx.previews;
    let revision: &SceneRevision = ctx.revision;
    let parsed: Result<Request, _> = serde_json::from_str(request_json);
    let request = match parsed {
        Ok(r) => r,
        Err(e) => {
            // Parse errors must respond with id=null per JSON-RPC 2.0.
            return Some(serialize(&error_response(
                None,
                -32700,
                "Parse error",
                Some(Value::String(e.to_string())),
            )));
        }
    };

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
    let method_name = request.method.clone();

    let outcome = match request.method.as_str() {
        "scene/query" => handle_scene_query(scene, request.params.as_ref()),
        "scene/click" => handle_scene_click(scene, request.params.as_ref()),
        "scene/rewind" => handle_scene_rewind(scene, request.params.as_ref()),
        "scene/snapshot" => handle_scene_snapshot(scene, request.params.as_ref()),
        "scene/dry_run" => handle_scene_dry_run(scene, request.params.as_ref()),
        "scene/waitFor" => handle_scene_wait_for(scene, request.params.as_ref()),
        "scene/screenshot" => handle_scene_screenshot(scene, request.params.as_ref()),
        "scene/invoke" => handle_scene_invoke(scene, request.params.as_ref()),
        "scene/intents" => handle_scene_intents(scene),
        "scene/locate" => handle_scene_locate(scene, request.params.as_ref()),
        "scene/locate_region" => handle_scene_locate_region(scene, request.params.as_ref()),
        "scene/bbox" => handle_scene_bbox(scene, request.params.as_ref()),
        "scene/cancel_preview" => handle_scene_cancel_preview(previews, request.params.as_ref()),
        "scene/list_previews" => handle_scene_list_previews(previews),
        "scene/propose_change" => {
            handle_scene_propose_change(previews, revision, request.params.as_ref())
        }
        "scene/apply_preview" => {
            handle_scene_apply_preview(scene, revision, previews, request.params.as_ref())
        }
        _ => Err(RpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: Some(Value::String(request.method.clone())),
        }),
    };

    // §5.34 R40.4: bump the OCC token after any mutating handler
    // succeeds. The match-on-method list is the single source of
    // truth for "does this method change visible scene state?";
    // conservative bumping (occasional spurious bump) is preferred
    // to a missed bump, which would silently mask preview staleness.
    if outcome.is_ok() && mutates_scene_on_success(method_name.as_str()) {
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

/// Single source of truth for "method mutates scene state on success".
///
/// `scene/click` / `scene/rewind` / `scene/invoke` mutate via
/// `External::invoke` semantics and the dispatcher bumps the
/// revision around them; `scene/dry_run` is explicitly non-mutating
/// per §2#3; `scene/intents` drains a queue without affecting the
/// rendered scene; the introspection methods are read-only.
///
/// `scene/propose_change` / `scene/cancel_preview` /
/// `scene/list_previews` touch only the ledger and so do not bump.
/// `scene/apply_preview` mutates the scene but bumps internally
/// (via [`crate::preview::apply_preview`]), so it is intentionally
/// **excluded** here to avoid a double-bump.
fn mutates_scene_on_success(method: &str) -> bool {
    matches!(
        method,
        "scene/click" | "scene/rewind" | "scene/invoke",
    )
}

fn handle_scene_query(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(invalid_params("params.path missing or not a string"));
    };

    match query(scene, path) {
        Ok(value) => Ok(introspect_value_to_json(value)),
        Err(err) => Err(query_error_to_rpc(err)),
    }
}

fn handle_scene_click(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(invalid_params("params.path missing or not a string"));
    };
    let Some(x) = params.get("x").and_then(Value::as_f64) else {
        return Err(invalid_params("params.x missing or not a number"));
    };
    let Some(y) = params.get("y").and_then(Value::as_f64) else {
        return Err(invalid_params("params.y missing or not a number"));
    };

    #[allow(clippy::cast_possible_truncation)]
    let outcome = click(scene, path, x as f32, y as f32);
    match outcome {
        Ok(outcome) => Ok(click_outcome_to_json(outcome)),
        Err(err) => Err(click_error_to_rpc(err)),
    }
}

fn click_outcome_to_json(outcome: ClickOutcome) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("handled".to_string(), Value::Bool(outcome.handled));
    Value::Object(map)
}

fn click_error_to_rpc(err: ClickError) -> RpcError {
    let variant = match err {
        ClickError::Path(_) => "Path",
        ClickError::UnsupportedPath => "UnsupportedPath",
        ClickError::NoExternalAtPath => "NoExternalAtPath",
    };
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(variant.to_string())),
    }
}

fn handle_scene_rewind(scene: &mut Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(invalid_params("params.path missing or not a string"));
    };
    let Some(value_json) = params.get("value") else {
        return Err(invalid_params("params.value missing"));
    };
    let Some(value) = json_to_introspect_value(value_json) else {
        return Err(invalid_params("params.value unsupported (v0: null/bool/number/string only)"));
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
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(variant.to_string())),
    }
}

fn handle_scene_snapshot(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(invalid_params("params.path missing or not a string"));
    };

    match snapshot(scene, path) {
        Ok(node) => Ok(snapshot_node_to_json(node)),
        Err(err) => Err(snapshot_error_to_rpc(err)),
    }
}

fn snapshot_error_to_rpc(err: SnapshotError) -> RpcError {
    let variant = match err {
        SnapshotError::Path(_) => "Path",
        SnapshotError::UnsupportedPath => "UnsupportedPath",
    };
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(variant.to_string())),
    }
}

fn snapshot_node_to_json(node: SnapshotNode) -> Value {
    let mut obj = serde_json::Map::new();
    let type_tag = match &node {
        SnapshotNode::Box => "Box",
        SnapshotNode::Text => "Text",
        SnapshotNode::Path => "Path",
        SnapshotNode::Image => "Image",
        SnapshotNode::Container => "Container",
        SnapshotNode::Effect => "Effect",
        SnapshotNode::External(_) => "External",
        // `SnapshotNode::Unknown` and future non_exhaustive additions
        // collapse to "Unknown".
        _ => "Unknown",
    };
    obj.insert("type".to_string(), Value::String(type_tag.to_string()));

    if let SnapshotNode::External(ExternalSnapshot { introspect }) = node {
        match introspect {
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

    Value::Object(obj)
}

fn handle_scene_dry_run(scene: &mut Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(invalid_params("params.path missing or not a string"));
    };
    let Some(value_json) = params.get("value") else {
        return Err(invalid_params("params.value missing"));
    };
    let Some(value) = json_to_introspect_value(value_json) else {
        return Err(invalid_params(
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
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(variant.to_string())),
    }
}

fn handle_scene_wait_for(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(invalid_params("params.path missing or not a string"));
    };
    let Some(target_json) = params.get("target") else {
        return Err(invalid_params("params.target missing"));
    };
    let Some(target) = json_to_introspect_value(target_json) else {
        return Err(invalid_params(
            "params.target unsupported (v0: null/bool/number/string only)",
        ));
    };
    let Some(max_attempts) = params.get("max_attempts").and_then(Value::as_u64) else {
        return Err(invalid_params("params.max_attempts missing or not u64"));
    };
    let max_attempts = u32::try_from(max_attempts)
        .map_err(|_| invalid_params("params.max_attempts exceeds u32 range"))?;

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
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(variant.to_string())),
    }
}

fn handle_scene_screenshot(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(invalid_params("params.path missing or not a string"));
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
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(variant.to_string())),
    }
}

fn handle_scene_invoke(scene: &mut Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(invalid_params("params.path missing or not a string"));
    };
    let Some(args_json) = params.get("args") else {
        return Err(invalid_params("params.args missing"));
    };
    let Some(args) = json_to_introspect_value(args_json) else {
        return Err(invalid_params(
            "params.args is not a representable IntrospectValue",
        ));
    };

    match invoke(scene, path, args) {
        Ok(value) => Ok(introspect_value_to_json(value)),
        Err(err) => Err(invoke_error_to_rpc(err)),
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

fn handle_scene_locate(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(x) = params.get("x").and_then(Value::as_u64) else {
        return Err(invalid_params("params.x missing or not a non-negative integer"));
    };
    let Some(y) = params.get("y").and_then(Value::as_u64) else {
        return Err(invalid_params("params.y missing or not a non-negative integer"));
    };
    let x32 = u32::try_from(x).map_err(|_| invalid_params("params.x exceeds u32 range"))?;
    let y32 = u32::try_from(y).map_err(|_| invalid_params("params.y exceeds u32 range"))?;

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
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let read_u32 = |k: &str| -> Result<u32, RpcError> {
        let raw = params
            .get(k)
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid_params(&format!("params.{k} missing or not a non-negative integer")))?;
        u32::try_from(raw).map_err(|_| invalid_params(&format!("params.{k} exceeds u32 range")))
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
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(variant.to_string())),
    }
}

fn handle_scene_bbox(scene: &Scene, params: Option<&Value>) -> Result<Value, RpcError> {
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(invalid_params("params.path missing or not a string"));
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
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(variant.to_string())),
    }
}

fn handle_scene_cancel_preview(
    previews: &PreviewLedger,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(raw_id) = params.get("preview_id").and_then(Value::as_u64) else {
        return Err(invalid_params(
            "params.preview_id missing or not a positive integer",
        ));
    };
    let Some(id) = PreviewId::try_new(raw_id) else {
        return Err(invalid_params("params.preview_id must be non-zero"));
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
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let proposal = parse_typed_proposal(params)?;
    let ttl_hint = match params.get("ttl_ms") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let Some(ms) = v.as_u64() else {
                return Err(invalid_params(
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
        return Err(invalid_params(
            "params.kind missing or not a string (expected one of: SetSignal, DispatchIntent, SetStyle, ReplaceView)",
        ));
    };
    match kind {
        "SetSignal" => {
            let Some(target_path) = params.get("target_path").and_then(Value::as_str) else {
                return Err(invalid_params(
                    "params.target_path missing or not a string",
                ));
            };
            let Some(signal_path) = params.get("signal_path").and_then(Value::as_str) else {
                return Err(invalid_params(
                    "params.signal_path missing or not a string",
                ));
            };
            let Some(value) = params.get("value") else {
                return Err(invalid_params("params.value missing"));
            };
            Ok(TypedProposal::SetSignal {
                target_path: target_path.to_owned(),
                signal_path: signal_path.to_owned(),
                value: value.clone(),
            })
        }
        "DispatchIntent" => {
            let Some(target_path) = params.get("target_path").and_then(Value::as_str) else {
                return Err(invalid_params(
                    "params.target_path missing or not a string",
                ));
            };
            let Some(intent_obj) = params.get("intent") else {
                return Err(invalid_params("params.intent missing"));
            };
            let Some(tag) = intent_obj.get("tag").and_then(Value::as_str) else {
                return Err(invalid_params(
                    "params.intent.tag missing or not a string",
                ));
            };
            let payload_value = intent_obj.get("payload").cloned().unwrap_or(Value::Null);
            let payload = json_to_introspect_value(&payload_value).ok_or_else(|| {
                invalid_params("params.intent.payload not a representable IntrospectValue shape")
            })?;
            Ok(TypedProposal::DispatchIntent {
                target_path: target_path.to_owned(),
                intent: Intent::new_owned(tag.to_owned(), payload),
            })
        }
        "SetStyle" => {
            let Some(target_path) = params.get("target_path").and_then(Value::as_str) else {
                return Err(invalid_params(
                    "params.target_path missing or not a string",
                ));
            };
            let Some(style_obj) = params.get("style") else {
                return Err(invalid_params("params.style missing"));
            };
            let style = parse_box_style(style_obj)?;
            Ok(TypedProposal::SetStyle {
                target_path: target_path.to_owned(),
                style,
            })
        }
        "ReplaceView" => {
            let Some(target_path) = params.get("target_path").and_then(Value::as_str) else {
                return Err(invalid_params(
                    "params.target_path missing or not a string",
                ));
            };
            let Some(replacement_obj) = params.get("replacement") else {
                return Err(invalid_params("params.replacement missing"));
            };
            let replacement = parse_view_blueprint(replacement_obj)?;
            Ok(TypedProposal::ReplaceView {
                target_path: target_path.to_owned(),
                replacement,
            })
        }
        other => Err(RpcError {
            code: -32602,
            message: "Invalid params".to_string(),
            data: Some(Value::String(format!("UnknownProposalKind: {other}"))),
        }),
    }
}


/// Wire→[`ViewBlueprint`] coercion for `ReplaceView` payloads
/// (R40.11). Recursive: `Container.children` invokes the same parser
/// per child. `kind` discriminates between blueprint variants;
/// `tag` is optional everywhere.
fn parse_view_blueprint(v: &Value) -> Result<ViewBlueprint, RpcError> {
    let Some(kind) = v.get("kind").and_then(Value::as_str) else {
        return Err(invalid_params(
            "params.replacement.kind missing or not a string (expected one of: Box, Container)",
        ));
    };
    let rect = parse_rect(v.get("rect"))?;
    let style = parse_box_style(
        v.get("style").ok_or_else(|| invalid_params("params.replacement.style missing"))?,
    )?;
    let tag = match v.get("tag") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        _ => {
            return Err(invalid_params(
                "params.replacement.tag must be a string or null",
            ));
        }
    };
    match kind {
        "Box" => Ok(ViewBlueprint::Box { rect, style, tag }),
        "Container" => {
            let children_value = v.get("children").unwrap_or(&Value::Null);
            let children = match children_value {
                Value::Null => Vec::new(),
                Value::Array(arr) => arr
                    .iter()
                    .map(parse_view_blueprint)
                    .collect::<Result<Vec<_>, _>>()?,
                _ => {
                    return Err(invalid_params(
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
        other => Err(invalid_params(&format!(
            "params.replacement.kind unrecognised: {other} (expected Box or Container)"
        ))),
    }
}

/// Wire→[`pinion_core::scene::Rect`] coercion. Required fields
/// `x` / `y` / `w` / `h` as u32-bounded numbers. Used by
/// [`parse_view_blueprint`].
fn parse_rect(v: Option<&Value>) -> Result<pinion_core::scene::Rect, RpcError> {
    let Some(obj) = v else {
        return Err(invalid_params("params.replacement.rect missing"));
    };
    let read = |field: &str| -> Result<u32, RpcError> {
        let n = obj.get(field).and_then(Value::as_u64).ok_or_else(|| {
            invalid_params(&format!(
                "params.replacement.rect.{field} missing or not an unsigned integer"
            ))
        })?;
        u32::try_from(n).map_err(|_| {
            invalid_params(&format!("params.replacement.rect.{field} exceeds u32 range"))
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
/// wire stays forward-compatible with future BoxStyle additions.
fn parse_box_style(style: &Value) -> Result<BoxStyle, RpcError> {
    let Some(fill) = style.get("fill").and_then(Value::as_u64) else {
        return Err(invalid_params(
            "params.style.fill missing or not an unsigned integer",
        ));
    };
    // u32 ARGB is the wire shape; clamp to u32 explicitly so callers
    // cannot smuggle high bits past the BoxStyle field type.
    let fill_argb = u32::try_from(fill)
        .map_err(|_| invalid_params("params.style.fill exceeds u32 range"))?;
    let mut out = BoxStyle::filled(Color::from_argb(fill_argb));
    if let Some(corner) = style.get("corner_radius").and_then(Value::as_u64) {
        let radius = u32::try_from(corner)
            .map_err(|_| invalid_params("params.style.corner_radius exceeds u32 range"))?;
        out = out.with_corner_radius(radius);
    }
    // Border requires both color + width; either alone is incoherent
    // and surfaces as invalid params rather than partial application.
    let border_color = style.get("border_color").and_then(Value::as_u64);
    let border_width = style.get("border_width").and_then(Value::as_u64);
    match (border_color, border_width) {
        (Some(c), Some(w)) => {
            let c = u32::try_from(c)
                .map_err(|_| invalid_params("params.style.border_color exceeds u32 range"))?;
            let w = u32::try_from(w)
                .map_err(|_| invalid_params("params.style.border_width exceeds u32 range"))?;
            out = out.with_border(Border::new(Color::from_argb(c), w));
        }
        (None, None) => {}
        _ => {
            return Err(invalid_params(
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
    let Some(params) = params else {
        return Err(invalid_params("missing params"));
    };
    let Some(raw_id) = params.get("preview_id").and_then(Value::as_u64) else {
        return Err(invalid_params(
            "params.preview_id missing or not a positive integer",
        ));
    };
    let Some(id) = PreviewId::try_new(raw_id) else {
        return Err(invalid_params("params.preview_id must be non-zero"));
    };
    match apply_preview(scene, revision, previews, id) {
        Ok(outcome) => Ok(apply_outcome_to_json(outcome)),
        Err(err) => Err(apply_error_to_rpc(&err)),
    }
}

fn apply_outcome_to_json(outcome: ApplyOutcome) -> Value {
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
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::Object(data_obj)),
    }
}

fn propose_error_to_rpc(err: &ProposeError) -> RpcError {
    let variant = match err {
        ProposeError::CapacityFull { .. } => "CapacityFull",
    };
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(variant.to_string())),
    }
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

fn invoke_error_to_rpc(err: InvokeError) -> RpcError {
    let variant = match err {
        InvokeError::Path(_) => "Path",
        InvokeError::UnsupportedPath => "UnsupportedPath",
        InvokeError::NoExternalAtPath => "NoExternalAtPath",
        InvokeError::IntrospectionOptedOut => "IntrospectionOptedOut",
        InvokeError::UnknownInvokePath => "UnknownInvokePath",
        InvokeError::InvokeTypeMismatch => "InvokeTypeMismatch",
        InvokeError::InvokeRejected => "InvokeRejected",
    };
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(variant.to_string())),
    }
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

fn invalid_params(detail: &str) -> RpcError {
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(detail.to_string())),
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
    RpcError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: Some(Value::String(variant.to_string())),
    }
}

fn introspect_value_to_json(value: IntrospectValue) -> Value {
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
    Response {
        jsonrpc: JSONRPC_V2.to_string(),
        result: None,
        error: Some(RpcError {
            code,
            message: message.to_string(),
            data,
        }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::ExternalNode;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    fn parse_response(s: &str) -> Response {
        serde_json::from_str(s).expect("dispatch produced invalid response JSON")
    }

    /// Test helper — calls [`dispatch`] with a freshly-allocated
    /// [`PreviewLedger`] and [`SceneRevision`]. Used by tests that do
    /// not exercise the preview lifecycle methods or revision bumping.
    fn dispatch_t(scene: &mut Scene, req: &str) -> Option<String> {
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let mut ctx = DispatchContext::new(scene, &previews, &revision);
        dispatch(&mut ctx, req)
    }

    /// Test helper — calls [`dispatch`] with a caller-supplied
    /// [`PreviewLedger`] and [`SceneRevision`]. Used by lifecycle /
    /// revision-aware tests so the same ledger and revision survive
    /// across multiple dispatch calls.
    fn dispatch_full(
        scene: &mut Scene,
        previews: &PreviewLedger,
        revision: &SceneRevision,
        req: &str,
    ) -> Option<String> {
        let mut ctx = DispatchContext::new(scene, previews, revision);
        dispatch(&mut ctx, req)
    }

    #[test]
    fn parse_error_on_invalid_json() {
        let mut scene = counted_scene(0);
        let resp = parse_response(&dispatch_t(&mut scene, "{not json").unwrap());
        assert_eq!(resp.error.unwrap().code, -32700);
        assert_eq!(resp.id, None);
    }

    #[test]
    fn invalid_request_on_wrong_jsonrpc_version() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"1.0","method":"scene/query","id":1}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32600);
        assert_eq!(resp.id, Some(RequestId::Num(1)));
    }

    #[test]
    fn method_not_found() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/unknown","id":2}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn invalid_params_when_path_missing() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{},"id":3}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn success_query_with_id_num() {
        let mut scene = counted_scene(42);
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":4}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap(), Value::Number(42.into()));
        assert_eq!(resp.id, Some(RequestId::Num(4)));
    }

    #[test]
    fn success_query_with_id_string() {
        let mut scene = counted_scene(5);
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":"req-a"}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert_eq!(resp.id, Some(RequestId::Str("req-a".to_string())));
    }

    #[test]
    fn notification_emits_no_response() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"}}"#;
        assert!(dispatch_t(&mut scene, req).is_none());
    }

    #[test]
    fn query_error_maps_to_invalid_params_with_variant_tag() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/anything"},"id":7}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
        assert_eq!(err.data, Some(Value::String("IntrospectionOptedOut".to_string())));
    }

    #[test]
    fn path_error_inside_query_also_maps_to_invalid_params() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/window[main/external/count"},"id":8}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
        assert_eq!(err.data, Some(Value::String("Path".to_string())));
    }

    #[test]
    fn scene_click_success_returns_handled_false() {
        // StubExternal's handles_event default returns false. The
        // dispatch round-trips that into result.handled.
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"/external","x":1.0,"y":2.0},"id":9}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result.get("handled"), Some(&Value::Bool(false)));
    }

    #[test]
    fn scene_click_missing_x_param_is_invalid() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"/external","y":2.0},"id":10}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_rewind_writes_then_query_observes_new_value() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/count","value":123},"id":11}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none());

        let req2 = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":12}"#;
        let resp2 = parse_response(&dispatch_t(&mut scene, req2).unwrap());
        assert_eq!(resp2.result.unwrap(), Value::Number(123.into()));
    }

    #[test]
    fn scene_rewind_missing_value_param_is_invalid() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/count"},"id":13}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_snapshot_returns_type_and_introspect_object() {
        let mut scene = counted_scene(99);
        let req = r#"{"jsonrpc":"2.0","method":"scene/snapshot","params":{"path":""},"id":14}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result.get("type"), Some(&Value::String("External".into())));
        let intro = result.get("introspect").unwrap().as_object().unwrap();
        assert_eq!(intro.get("count"), Some(&Value::Number(99.into())));
    }

    #[test]
    fn scene_snapshot_missing_path_is_invalid() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/snapshot","params":{},"id":15}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_dry_run_returns_hypothetical_snapshot_and_rolls_back() {
        let mut scene = counted_scene(3);
        let req = r#"{"jsonrpc":"2.0","method":"scene/dry_run","params":{"path":"/external/count","value":77},"id":16}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let intro = result.get("introspect").unwrap().as_object().unwrap();
        assert_eq!(intro.get("count"), Some(&Value::Number(77.into())));

        // Follow-up query confirms the scene was rolled back.
        let q_req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":17}"#;
        let q_resp = parse_response(&dispatch_t(&mut scene, q_req).unwrap());
        assert_eq!(q_resp.result.unwrap(), Value::Number(3.into()));
    }

    #[test]
    fn scene_dry_run_missing_value_param_is_invalid() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/dry_run","params":{"path":"/external/count"},"id":18}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_wait_for_returns_matched_when_target_equals_current() {
        let mut scene = counted_scene(42);
        let req = r#"{"jsonrpc":"2.0","method":"scene/waitFor","params":{"path":"/external/count","target":42,"max_attempts":3},"id":19}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result.get("matched"), Some(&Value::Bool(true)));
        assert_eq!(result.get("attempts"), Some(&Value::Number(1.into())));
        assert_eq!(result.get("final_value"), Some(&Value::Number(42.into())));
    }

    #[test]
    fn scene_wait_for_missing_target_param_is_invalid() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/waitFor","params":{"path":"/external/count","max_attempts":1},"id":20}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_screenshot_returns_render_backend_unavailable_tag() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/screenshot","params":{"path":""},"id":21}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
        assert_eq!(
            err.data,
            Some(Value::String("RenderBackendUnavailable".to_string())),
        );
    }

    #[test]
    fn scene_screenshot_missing_path_is_invalid() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/screenshot","params":{},"id":22}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    /// `ButtonExternal` end-to-end: a real R12 widget reaches the
    /// JSON-RPC envelope. Validates §5.15 item 8 dispatch chain
    /// (`dispatch` → `scene/query` → `External::introspect` →
    /// `ExternalIntrospect::query`) against widget state — the
    /// `CountedExternal` fixture covers the trait mechanics, this
    /// asserts the path also works for real-widget surfaces.
    #[test]
    fn scene_query_on_button_external_returns_state_text() {
        use pinion_core::widgets::button::{ButtonEvent, ButtonExternal};
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::PointerEnter);
        let mut scene = Scene::External(ExternalNode::new(Box::new(bx)));
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/state"},"id":42}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none(), "unexpected error: {:?}", resp.error);
        let result = resp.result.expect("scene/query produced no result");
        // Hybrid query result: typed IntrospectValue serialized
        // tagged-untagged; assert by matching the deserialized
        // string contents.
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(
            serialized.contains("Hover"),
            "expected ButtonExternal to surface Hover state, got {serialized}"
        );
    }

    #[test]
    fn scene_query_on_button_external_unknown_path_is_invalid() {
        use pinion_core::widgets::button::ButtonExternal;
        let mut scene = Scene::External(ExternalNode::new(Box::new(ButtonExternal::new())));
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/nope"},"id":43}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.expect("unknown path should error");
        assert_eq!(err.code, -32602);
        assert_eq!(
            err.data,
            Some(Value::String("UnknownIntrospectPath".to_string()))
        );
    }

    /// `scene/invoke` end-to-end through the JSON-RPC envelope.
    /// First wire-form exercise of the R17 bidirectional RPC spec
    /// round 8th method.
    #[test]
    fn scene_invoke_increment_returns_new_total() {
        let mut scene = counted_scene(10);
        let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/increment","args":5},"id":50}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none(), "unexpected error: {:?}", resp.error);
        let result = resp.result.expect("scene/invoke produced no result");
        assert_eq!(result, Value::Number(15.into()));
    }

    #[test]
    fn scene_invoke_on_button_external_drives_transition() {
        use pinion_core::widgets::button::ButtonExternal;
        let mut scene = Scene::External(ExternalNode::new(Box::new(ButtonExternal::new())));
        let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/send","args":"PointerEnter"},"id":51}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none(), "unexpected error: {:?}", resp.error);
        let result = resp.result.expect("scene/invoke produced no result");
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(
            serialized.contains("Hover"),
            "expected ButtonExternal to transition to Hover, got {serialized}"
        );
    }

    #[test]
    fn scene_invoke_missing_args_is_invalid_params() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/increment"},"id":52}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_invoke_unknown_action_path_is_invalid_params() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/ghost","args":1},"id":53}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
        assert_eq!(
            err.data,
            Some(Value::String("UnknownInvokePath".to_string()))
        );
    }

    /// `scene/intents` end-to-end through the JSON-RPC envelope.
    /// First wire-form exercise of the R18 §5.20 intent system 9th
    /// method.
    #[test]
    fn scene_intents_returns_empty_array_when_nothing_pending() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/intents","id":60}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none(), "unexpected error: {:?}", resp.error);
        let result = resp.result.expect("scene/intents produced no result");
        let arr = result.as_array().expect("intents result is an array");
        assert!(arr.is_empty());
    }

    #[test]
    fn scene_intents_drains_counted_changed_after_rewind() {
        // Drive `intervene` indirectly via `scene/rewind`, which is
        // wired through the dispatcher; the resulting intent surfaces
        // through `scene/intents` on the next call.
        let mut scene = counted_scene(0);
        let rewind_req =
            r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/count","value":11},"id":61}"#;
        let _ = dispatch_t(&mut scene, rewind_req).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"scene/intents","id":62}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let result = resp.result.expect("scene/intents produced no result");
        let arr = result.as_array().expect("intents result is an array");
        assert_eq!(arr.len(), 1);
        let entry = arr[0].as_object().unwrap();
        assert_eq!(
            entry.get("tag"),
            Some(&Value::String("counted.changed".into()))
        );
        assert_eq!(entry.get("payload"), Some(&Value::Number(11.into())));
    }

    #[test]
    fn scene_intents_drain_is_idempotent_on_clean_scene() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/intents","id":63}"#;
        let _ = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let resp2 = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let result = resp2.result.expect("scene/intents produced no result");
        assert!(result.as_array().unwrap().is_empty());
    }

    #[test]
    fn scene_invoke_rejected_event_returns_invoke_rejected() {
        use pinion_core::widgets::button::ButtonExternal;
        let mut scene = Scene::External(ExternalNode::new(Box::new(ButtonExternal::new())));
        let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/send","args":"Teleport"},"id":54}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
        assert_eq!(
            err.data,
            Some(Value::String("InvokeRejected".to_string()))
        );
    }

    // ---- §5.32 R39.1: scene/locate JSON-RPC wire ----

    fn box_scene(x: u32, y: u32, w: u32, h: u32) -> Scene {
        use pinion_core::scene::{BoxNode, Rect};
        use pinion_core::Color;
        Scene::Box(BoxNode::filled(Rect::new(x, y, w, h), Color::default()))
    }

    #[test]
    fn scene_locate_returns_path_bbox_ancestors() {
        let mut scene = box_scene(10, 20, 50, 30);
        let req = r#"{"jsonrpc":"2.0","method":"scene/locate","params":{"x":20,"y":25},"id":100}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none(), "got error: {:?}", resp.error);
        let result = resp.result.expect("scene/locate produced no result");
        let obj = result.as_object().expect("result must be object");
        assert!(obj.contains_key("path"));
        let bbox = obj.get("bbox").and_then(Value::as_object).expect("bbox object");
        assert_eq!(bbox.get("x"), Some(&Value::Number(10.into())));
        assert_eq!(bbox.get("y"), Some(&Value::Number(20.into())));
        assert_eq!(bbox.get("w"), Some(&Value::Number(50.into())));
        assert_eq!(bbox.get("h"), Some(&Value::Number(30.into())));
        let ancestors = obj.get("ancestors").and_then(Value::as_array).expect("ancestors array");
        assert!(ancestors.is_empty(), "root hit has no ancestors");
    }

    #[test]
    fn scene_locate_out_of_bounds_returns_invalid_params() {
        let mut scene = box_scene(10, 10, 5, 5);
        let req = r#"{"jsonrpc":"2.0","method":"scene/locate","params":{"x":99,"y":99},"id":101}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, -32602);
        assert_eq!(err.data, Some(Value::String("OutOfBounds".to_string())));
    }

    #[test]
    fn scene_locate_missing_x_is_invalid_params() {
        let mut scene = box_scene(0, 0, 10, 10);
        let req = r#"{"jsonrpc":"2.0","method":"scene/locate","params":{"y":5},"id":102}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_locate_negative_x_is_invalid_params() {
        // u64-only param schema rejects signed values; AI agents can't
        // pass negative coords without a typed protocol error.
        let mut scene = box_scene(0, 0, 10, 10);
        let req = r#"{"jsonrpc":"2.0","method":"scene/locate","params":{"x":-1,"y":5},"id":103}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_locate_region_returns_paths_and_common_ancestor() {
        let mut scene = box_scene(0, 0, 100, 100);
        let req = r#"{"jsonrpc":"2.0","method":"scene/locate_region","params":{"x":0,"y":0,"w":50,"h":50},"id":104}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none());
        let result = resp.result.expect("no result");
        let obj = result.as_object().expect("object");
        let paths = obj.get("paths").and_then(Value::as_array).expect("paths");
        assert_eq!(paths.len(), 1, "single root box hit");
        assert!(obj.get("common_ancestor").is_some());
    }

    #[test]
    fn scene_locate_region_disjoint_returns_empty_paths() {
        let mut scene = box_scene(0, 0, 10, 10);
        let req = r#"{"jsonrpc":"2.0","method":"scene/locate_region","params":{"x":500,"y":500,"w":10,"h":10},"id":105}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none(), "disjoint never errors");
        let result = resp.result.expect("no result");
        let paths = result.get("paths").and_then(Value::as_array).expect("paths");
        assert!(paths.is_empty());
    }

    #[test]
    fn scene_locate_region_missing_w_is_invalid_params() {
        let mut scene = box_scene(0, 0, 10, 10);
        let req = r#"{"jsonrpc":"2.0","method":"scene/locate_region","params":{"x":0,"y":0,"h":10},"id":106}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, -32602);
    }

    // ---- R39.3: scene/bbox JSON-RPC wire ----

    #[test]
    fn scene_bbox_returns_bbox_for_root_path() {
        let mut scene = box_scene(10, 20, 30, 40);
        let req = r#"{"jsonrpc":"2.0","method":"scene/bbox","params":{"path":"/window[main]"},"id":107}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert!(resp.error.is_none());
        let bbox = resp.result.unwrap().get("bbox").cloned().unwrap();
        let obj = bbox.as_object().unwrap();
        assert_eq!(obj.get("x"), Some(&Value::Number(10.into())));
        assert_eq!(obj.get("y"), Some(&Value::Number(20.into())));
        assert_eq!(obj.get("w"), Some(&Value::Number(30.into())));
        assert_eq!(obj.get("h"), Some(&Value::Number(40.into())));
    }

    #[test]
    fn scene_bbox_unknown_path_returns_invalid_params() {
        let mut scene = box_scene(0, 0, 10, 10);
        let req = r#"{"jsonrpc":"2.0","method":"scene/bbox","params":{"path":"/window[main]/ghost"},"id":108}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, -32602);
        assert_eq!(err.data, Some(Value::String("UnknownPath".to_string())));
    }

    #[test]
    fn scene_bbox_missing_path_is_invalid_params() {
        let mut scene = box_scene(0, 0, 10, 10);
        let req = r#"{"jsonrpc":"2.0","method":"scene/bbox","params":{},"id":109}"#;
        let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    // ---- R40.2: scene/cancel_preview JSON-RPC wire ----

    #[derive(Debug)]
    struct WireTestProposal {
        target: String,
    }

    impl WireTestProposal {
        fn new() -> Self {
            Self {
                target: "/wire-test".to_string(),
            }
        }
    }

    impl crate::preview::Proposal for WireTestProposal {
        fn target_path(&self) -> &str {
            &self.target
        }
        fn affected_paths(&self) -> Vec<String> {
            vec![self.target.clone()]
        }
        fn apply(
            &self,
            _ctx: &mut crate::preview::ApplyContext<'_>,
        ) -> Result<(), String> {
            // Used only by the wire-format tests for cancel /
            // list_previews; never reached because those flows do
            // not exercise apply_preview.
            Ok(())
        }
    }

    fn now_inst() -> std::time::Instant {
        std::time::Instant::now()
    }

    #[test]
    fn scene_cancel_preview_returns_cancelled_true_for_active_id() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let id = previews
            .propose(0, Box::new(WireTestProposal::new()), None, now_inst())
            .expect("propose succeeds with default ledger");
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/cancel_preview","params":{{"preview_id":{}}},"id":201}}"#,
            id.get()
        );
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &SceneRevision::default(), &req).unwrap());
        assert!(resp.error.is_none());
        let obj = resp.result.unwrap();
        assert_eq!(obj.get("cancelled"), Some(&Value::Bool(true)));
        assert!(previews.is_empty());
    }

    #[test]
    fn scene_cancel_preview_returns_cancelled_false_for_unknown_id() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/cancel_preview","params":{"preview_id":9999},"id":202}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap());
        assert!(resp.error.is_none());
        let obj = resp.result.unwrap();
        assert_eq!(obj.get("cancelled"), Some(&Value::Bool(false)));
    }

    #[test]
    fn scene_cancel_preview_is_idempotent_over_dispatch() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let id = previews
            .propose(0, Box::new(WireTestProposal::new()), None, now_inst())
            .unwrap();
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/cancel_preview","params":{{"preview_id":{}}},"id":203}}"#,
            id.get()
        );
        let first = parse_response(&dispatch_full(&mut scene, &previews, &SceneRevision::default(), &req).unwrap());
        assert_eq!(first.result.unwrap().get("cancelled"), Some(&Value::Bool(true)));
        let second = parse_response(&dispatch_full(&mut scene, &previews, &SceneRevision::default(), &req).unwrap());
        assert_eq!(second.result.unwrap().get("cancelled"), Some(&Value::Bool(false)));
    }

    #[test]
    fn scene_cancel_preview_missing_id_is_invalid_params() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/cancel_preview","params":{},"id":204}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn scene_cancel_preview_zero_id_is_invalid_params() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/cancel_preview","params":{"preview_id":0},"id":205}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn scene_cancel_preview_string_id_is_invalid_params() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/cancel_preview","params":{"preview_id":"abc"},"id":206}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    // ---- R40.3: scene/list_previews JSON-RPC wire ----

    #[test]
    fn scene_list_previews_empty_ledger_returns_empty_array() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/list_previews","id":301}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap());
        assert!(resp.error.is_none());
        let arr = resp
            .result
            .unwrap()
            .get("previews")
            .cloned()
            .unwrap();
        assert_eq!(arr.as_array().unwrap().len(), 0);
    }

    #[test]
    fn scene_list_previews_single_entry_surfaces_all_fields() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let id = previews
            .propose(7, Box::new(WireTestProposal::new()), None, now_inst())
            .unwrap();
        let req = r#"{"jsonrpc":"2.0","method":"scene/list_previews","id":302}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap());
        let arr = resp.result.unwrap().get("previews").cloned().unwrap();
        let entries = arr.as_array().unwrap();
        assert_eq!(entries.len(), 1);
        let obj = entries[0].as_object().unwrap();
        assert_eq!(obj.get("preview_id"), Some(&Value::Number(id.get().into())));
        assert_eq!(obj.get("base_revision"), Some(&Value::Number(7.into())));
        assert_eq!(
            obj.get("target_path"),
            Some(&Value::String("/wire-test".to_string()))
        );
        assert_eq!(
            obj.get("affected_paths"),
            Some(&Value::Array(vec![Value::String("/wire-test".to_string())]))
        );
        // age/ttl present, non-negative, ttl roughly 60s (default TTL).
        assert!(obj.get("age_ms").and_then(Value::as_u64).is_some());
        assert!(obj.get("ttl_remaining_ms").and_then(Value::as_u64).is_some());
    }

    #[test]
    fn scene_list_previews_in_id_order() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let a = previews
            .propose(0, Box::new(WireTestProposal::new()), None, now_inst())
            .unwrap();
        let b = previews
            .propose(0, Box::new(WireTestProposal::new()), None, now_inst())
            .unwrap();
        let c = previews
            .propose(0, Box::new(WireTestProposal::new()), None, now_inst())
            .unwrap();
        let req = r#"{"jsonrpc":"2.0","method":"scene/list_previews","id":303}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap());
        let arr = resp.result.unwrap().get("previews").cloned().unwrap();
        let entries = arr.as_array().unwrap();
        let ids: Vec<u64> = entries
            .iter()
            .map(|e| e.as_object().unwrap()["preview_id"].as_u64().unwrap())
            .collect();
        assert_eq!(ids, vec![a.get(), b.get(), c.get()]);
    }

    #[test]
    fn scene_list_previews_omits_params_ok() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        // Empty params object should be equivalent to omitted params.
        let req = r#"{"jsonrpc":"2.0","method":"scene/list_previews","params":{},"id":304}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap());
        assert!(resp.error.is_none());
    }

    // ---- R40.4: SceneRevision auto-bump on mutating dispatch ----

    #[test]
    fn dispatch_bumps_revision_on_invoke_success() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        assert_eq!(revision.current(), 0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/increment","args":3},"id":401}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        assert!(resp.error.is_none(), "unexpected error: {:?}", resp.error);
        assert_eq!(revision.current(), 1, "scene/invoke success bumps revision");
    }

    #[test]
    fn dispatch_does_not_bump_revision_on_invoke_failure() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        // Missing params triggers invalid_params (-32602) before any
        // mutation runs.
        let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{},"id":402}"#;
        let _ = dispatch_full(&mut scene, &previews, &revision, req);
        assert_eq!(revision.current(), 0, "failed dispatch leaves revision");
    }

    #[test]
    fn dispatch_does_not_bump_revision_on_read_only_methods() {
        let mut scene = counted_scene(42);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":403}"#;
        let _ = dispatch_full(&mut scene, &previews, &revision, req);
        assert_eq!(revision.current(), 0, "scene/query is read-only");
    }

    #[test]
    fn dispatch_does_not_bump_revision_on_preview_lifecycle_methods() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        // cancel_preview touches the ledger but not the scene tree.
        let req = r#"{"jsonrpc":"2.0","method":"scene/cancel_preview","params":{"preview_id":1},"id":404}"#;
        let _ = dispatch_full(&mut scene, &previews, &revision, req);
        assert_eq!(revision.current(), 0, "preview lifecycle does not bump scene revision");
        let req = r#"{"jsonrpc":"2.0","method":"scene/list_previews","id":405}"#;
        let _ = dispatch_full(&mut scene, &previews, &revision, req);
        assert_eq!(revision.current(), 0);
    }

    #[test]
    fn dispatch_does_not_bump_revision_on_intents_drain() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        // intents drain harvests events without changing the rendered
        // scene tree; revision must stay put.
        let req = r#"{"jsonrpc":"2.0","method":"scene/intents","id":406}"#;
        let _ = dispatch_full(&mut scene, &previews, &revision, req);
        assert_eq!(revision.current(), 0);
    }

    // ---- R40.5: scene/propose_change JSON-RPC wire ----

    #[test]
    fn scene_propose_change_set_signal_returns_id_and_base_rev() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        revision.bump();
        revision.bump();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetSignal","target_path":"/w/c","signal_path":"/w/c/count","value":42},"id":501}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        assert!(resp.error.is_none(), "unexpected error: {:?}", resp.error);
        let obj = resp.result.unwrap();
        assert!(obj.get("preview_id").and_then(Value::as_u64).is_some());
        assert_eq!(obj.get("base_revision"), Some(&Value::Number(2.into())));
        assert_eq!(previews.len(), 1);
    }

    #[test]
    fn scene_propose_change_does_not_bump_revision() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetSignal","target_path":"/a","signal_path":"/a/s","value":1},"id":502}"#;
        let _ = dispatch_full(&mut scene, &previews, &revision, req);
        assert_eq!(
            revision.current(),
            0,
            "propose_change mutates ledger only — must not bump scene revision"
        );
    }

    #[test]
    fn scene_propose_change_unknown_kind_is_invalid_params() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"Bogus","target_path":"/a","signal_path":"/a/s","value":1},"id":503}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
        let data = err.data.unwrap();
        assert!(
            data.as_str().unwrap().contains("UnknownProposalKind"),
            "expected UnknownProposalKind tag, got {data:?}"
        );
    }

    #[test]
    fn scene_propose_change_missing_kind_is_invalid_params() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"target_path":"/a","signal_path":"/a/s","value":1},"id":504}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn scene_propose_change_set_signal_missing_target_path_is_invalid_params() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetSignal","signal_path":"/a/s","value":1},"id":505}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn scene_propose_change_set_signal_missing_value_is_invalid_params() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetSignal","target_path":"/a","signal_path":"/a/s"},"id":506}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn scene_propose_change_capacity_full_surfaces_as_invalid_params() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::with_config(
            1,
            std::time::Duration::from_secs(60),
            std::time::Duration::from_secs(600),
        );
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetSignal","target_path":"/a","signal_path":"/a/s","value":1},"id":507}"#;
        let first = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        assert!(first.error.is_none());
        let second = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        let err = second.error.unwrap();
        assert_eq!(err.code, -32602);
        assert_eq!(err.data, Some(Value::String("CapacityFull".to_string())));
    }

    // ---- R40.6: scene/apply_preview JSON-RPC wire ----

    #[test]
    fn scene_apply_preview_writes_signal_end_to_end() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();

        // 1) propose
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetSignal","target_path":"/external/count","signal_path":"/external/count","value":77},"id":601}"#;
        let propose_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap());
        let preview_id = propose_resp
            .result
            .unwrap()
            .get("preview_id")
            .and_then(Value::as_u64)
            .unwrap();

        // 2) apply
        let apply_req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":602}}"#
        );
        let apply_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap());
        assert!(apply_resp.error.is_none(), "unexpected error: {:?}", apply_resp.error);
        let obj = apply_resp.result.unwrap();
        assert_eq!(obj.get("preview_id"), Some(&Value::Number(preview_id.into())));
        assert_eq!(obj.get("new_revision"), Some(&Value::Number(1.into())));

        // 3) query confirms scene now reflects the apply.
        let query_req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":603}"#;
        let query_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, query_req).unwrap());
        assert_eq!(query_resp.result.unwrap(), Value::Number(77.into()));
        assert!(previews.is_empty());
    }

    #[test]
    fn scene_apply_preview_unknown_id_surfaces_typed_variant() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/apply_preview","params":{"preview_id":9999},"id":604}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        let err = resp.error.unwrap();
        let data = err.data.unwrap();
        assert_eq!(
            data.get("variant"),
            Some(&Value::String("UnknownPreview".to_string()))
        );
    }

    #[test]
    fn scene_apply_preview_revision_conflict_keeps_entry() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        // Propose at base_revision = 0.
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetSignal","target_path":"/external/count","signal_path":"/external/count","value":42},"id":605}"#;
        let propose_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap());
        let preview_id = propose_resp
            .result
            .unwrap()
            .get("preview_id")
            .and_then(Value::as_u64)
            .unwrap();

        // Scene mutates underneath the preview (rewind bumps revision).
        let rewind_req = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/count","value":1},"id":606}"#;
        let _ = dispatch_full(&mut scene, &previews, &revision, rewind_req);

        // Now apply must fail with BaseRevisionConflict.
        let apply_req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":607}}"#
        );
        let apply_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap());
        let err = apply_resp.error.unwrap();
        let data = err.data.unwrap();
        assert_eq!(
            data.get("variant"),
            Some(&Value::String("BaseRevisionConflict".to_string()))
        );
        assert_eq!(data.get("expected"), Some(&Value::Number(0.into())));
        assert_eq!(data.get("actual"), Some(&Value::Number(1.into())));
        // Entry stays alive; revision still at 1 (apply did not bump on conflict).
        assert_eq!(previews.len(), 1);
        assert_eq!(revision.current(), 1);
    }

    #[test]
    fn scene_apply_preview_type_mismatch_surfaces_apply_rejected() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        // bool against an Int slot → Intervene type mismatch.
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetSignal","target_path":"/external/count","signal_path":"/external/count","value":true},"id":608}"#;
        let propose_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap());
        let preview_id = propose_resp
            .result
            .unwrap()
            .get("preview_id")
            .and_then(Value::as_u64)
            .unwrap();
        let apply_req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":609}}"#
        );
        let apply_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap());
        let err = apply_resp.error.unwrap();
        let data = err.data.unwrap();
        assert_eq!(
            data.get("variant"),
            Some(&Value::String("ApplyRejected".to_string()))
        );
        assert_eq!(
            data.get("reason"),
            Some(&Value::String("Intervene".to_string()))
        );
        // Entry consumed even on apply failure (one-shot).
        assert!(previews.is_empty());
    }

    #[test]
    fn scene_propose_change_round_trips_through_list_previews() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetSignal","target_path":"/btn","signal_path":"/btn/count","value":99,"ttl_ms":5000},"id":508}"#;
        let propose_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap());
        let preview_id = propose_resp
            .result
            .unwrap()
            .get("preview_id")
            .and_then(Value::as_u64)
            .unwrap();

        let list_req = r#"{"jsonrpc":"2.0","method":"scene/list_previews","id":509}"#;
        let list_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, list_req).unwrap());
        let arr = list_resp.result.unwrap().get("previews").cloned().unwrap();
        let entries = arr.as_array().unwrap();
        assert_eq!(entries.len(), 1);
        let obj = entries[0].as_object().unwrap();
        assert_eq!(obj["preview_id"].as_u64().unwrap(), preview_id);
        assert_eq!(obj["target_path"].as_str().unwrap(), "/btn");
        assert!(obj["ttl_remaining_ms"].as_u64().unwrap() <= 5000);
    }

    // ---- R40.9: TypedProposal::DispatchIntent JSON-RPC wire ----

    #[test]
    fn scene_apply_preview_outcome_contains_empty_intents_for_set_signal() {
        // R40.9 wire contract: every apply_preview response carries
        // `emitted_intents: []`. SetSignal never emits, but the field
        // must be present so AI clients can switch on its length
        // unconditionally without `Option`-handling boilerplate.
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetSignal","target_path":"/external/count","signal_path":"/external/count","value":1},"id":701}"#;
        let preview_id = parse_response(
            &dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap(),
        )
        .result
        .unwrap()
        .get("preview_id")
        .and_then(Value::as_u64)
        .unwrap();
        let apply_req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":702}}"#
        );
        let apply_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap());
        let obj = apply_resp.result.unwrap();
        let intents = obj.get("emitted_intents").unwrap().as_array().unwrap();
        assert!(
            intents.is_empty(),
            "SetSignal does not emit; field must be present-and-empty (got {intents:?})"
        );
    }

    #[test]
    fn scene_propose_dispatch_intent_then_apply_surfaces_intent_on_wire() {
        // End-to-end DispatchIntent: propose → apply → emitted_intents
        // visible in apply outcome with the {tag, payload} shape
        // `scene/intents` already uses.
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"DispatchIntent","target_path":"/save_btn","intent":{"tag":"save_btn.click","payload":42}},"id":703}"#;
        let preview_id = parse_response(
            &dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap(),
        )
        .result
        .unwrap()
        .get("preview_id")
        .and_then(Value::as_u64)
        .unwrap();
        let apply_req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":704}}"#
        );
        let apply_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap());
        let obj = apply_resp.result.unwrap();
        let intents = obj.get("emitted_intents").unwrap().as_array().unwrap();
        assert_eq!(intents.len(), 1);
        let entry = intents[0].as_object().unwrap();
        assert_eq!(entry["tag"].as_str().unwrap(), "save_btn.click");
        assert_eq!(entry["payload"].as_i64().unwrap(), 42);
        assert_eq!(obj["new_revision"].as_u64().unwrap(), 1);
    }

    #[test]
    fn scene_propose_dispatch_intent_missing_intent_is_invalid_params() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"DispatchIntent","target_path":"/btn"},"id":705}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_propose_dispatch_intent_missing_tag_is_invalid_params() {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"DispatchIntent","target_path":"/btn","intent":{"payload":null}},"id":706}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    // ---- R40.10: TypedProposal::SetStyle JSON-RPC wire ----

    fn box_container_scene_for_set_style() -> Scene {
        use pinion_core::scene::{BoxNode, ContainerNode};
        let child = Scene::Box(
            BoxNode::filled(
                pinion_core::scene::Rect::new(0, 0, 10, 10),
                Color::default(),
            )
            .with_tag("btn"),
        );
        let mut c = ContainerNode::new(vec![child]);
        c.rect = pinion_core::scene::Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn scene_propose_set_style_then_apply_changes_box_fill_end_to_end() {
        let mut scene = box_container_scene_for_set_style();
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        // Wire ARGB 0x00ff_00ff = magenta, fits in u32.
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetStyle","target_path":"/btn","style":{"fill":16711935}},"id":710}"#;
        let preview_id = parse_response(
            &dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap(),
        )
        .result
        .unwrap()
        .get("preview_id")
        .and_then(Value::as_u64)
        .unwrap();
        let apply_req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":711}}"#
        );
        let apply_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap());
        assert!(
            apply_resp.error.is_none(),
            "unexpected error: {:?}",
            apply_resp.error
        );
        // BoxNode.style.fill now matches the proposed value.
        if let Scene::Container(c) = &scene {
            if let Scene::Box(b) = &c.children[0] {
                assert_eq!(b.style.fill, Color::from_argb(16_711_935));
            } else {
                panic!("child 0 not Box");
            }
        } else {
            panic!("root not Container");
        }
    }

    #[test]
    fn scene_propose_set_style_unknown_target_surfaces_apply_rejected() {
        let mut scene = box_container_scene_for_set_style();
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetStyle","target_path":"/ghost","style":{"fill":255}},"id":712}"#;
        let preview_id = parse_response(
            &dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap(),
        )
        .result
        .unwrap()
        .get("preview_id")
        .and_then(Value::as_u64)
        .unwrap();
        let apply_req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":713}}"#
        );
        let apply_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap());
        let err = apply_resp.error.unwrap();
        let data = err.data.unwrap();
        assert_eq!(
            data.get("variant"),
            Some(&Value::String("ApplyRejected".to_string()))
        );
        assert_eq!(
            data.get("reason"),
            Some(&Value::String("UnknownTarget".to_string()))
        );
    }

    #[test]
    fn scene_propose_set_style_missing_fill_is_invalid_params() {
        let mut scene = box_container_scene_for_set_style();
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetStyle","target_path":"/btn","style":{}},"id":714}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_propose_set_style_with_border_round_trips() {
        // Optional border_color + border_width together must reach
        // apply intact — guards against the parse_box_style XOR check
        // accidentally rejecting a valid border-bearing request.
        let mut scene = box_container_scene_for_set_style();
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetStyle","target_path":"/btn","style":{"fill":0,"border_color":255,"border_width":3,"corner_radius":4}},"id":715}"#;
        let preview_id = parse_response(
            &dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap(),
        )
        .result
        .unwrap()
        .get("preview_id")
        .and_then(Value::as_u64)
        .unwrap();
        let apply_req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":716}}"#
        );
        let _ = dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap();
        if let Scene::Container(c) = &scene {
            if let Scene::Box(b) = &c.children[0] {
                let border = b.style.border.expect("border installed");
                assert_eq!(border.color, Color::from_argb(255));
                assert_eq!(border.width, 3);
                assert_eq!(b.style.corner_radius, 4);
            }
        }
    }

    #[test]
    fn scene_propose_set_style_partial_border_is_invalid_params() {
        let mut scene = box_container_scene_for_set_style();
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        // Only border_color, no border_width — incoherent.
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"SetStyle","target_path":"/btn","style":{"fill":0,"border_color":255}},"id":717}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    // ---- R40.11: TypedProposal::ReplaceView JSON-RPC wire ----

    #[test]
    fn scene_propose_replace_view_then_apply_swaps_box_end_to_end() {
        let mut scene = box_container_scene_for_set_style();
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        // Replace /btn (untagged after swap) with a fresh tagged Box.
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"ReplaceView","target_path":"/btn","replacement":{"kind":"Box","rect":{"x":0,"y":0,"w":20,"h":20},"style":{"fill":3735928559},"tag":"new_btn"}},"id":720}"#;
        let preview_id = parse_response(
            &dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap(),
        )
        .result
        .unwrap()
        .get("preview_id")
        .and_then(Value::as_u64)
        .unwrap();
        let apply_req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":721}}"#
        );
        let apply_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap());
        assert!(
            apply_resp.error.is_none(),
            "unexpected error: {:?}",
            apply_resp.error
        );
        // The /btn slot now carries the new tag + rect.
        if let Scene::Container(c) = &scene {
            if let Scene::Box(b) = &c.children[0] {
                assert_eq!(b.tag.as_deref(), Some("new_btn"));
                assert_eq!(b.rect, pinion_core::scene::Rect::new(0, 0, 20, 20));
            } else {
                panic!("child 0 not Box");
            }
        }
    }

    #[test]
    fn scene_propose_replace_view_with_nested_container_round_trips() {
        let mut scene = box_container_scene_for_set_style();
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"ReplaceView","target_path":"/btn","replacement":{"kind":"Container","rect":{"x":0,"y":0,"w":50,"h":50},"style":{"fill":0},"tag":"panel","children":[{"kind":"Box","rect":{"x":5,"y":5,"w":10,"h":10},"style":{"fill":255},"tag":"inner"}]}},"id":722}"#;
        let preview_id = parse_response(
            &dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap(),
        )
        .result
        .unwrap()
        .get("preview_id")
        .and_then(Value::as_u64)
        .unwrap();
        let apply_req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":723}}"#
        );
        let _ = dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap();
        if let Scene::Container(outer) = &scene {
            if let Scene::Container(inner) = &outer.children[0] {
                assert_eq!(inner.tag.as_deref(), Some("panel"));
                assert_eq!(inner.children[0].tag(), Some("inner"));
            }
        }
    }

    #[test]
    fn scene_propose_replace_view_unknown_kind_is_invalid_params() {
        let mut scene = box_container_scene_for_set_style();
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"ReplaceView","target_path":"/btn","replacement":{"kind":"Pyramid","rect":{"x":0,"y":0,"w":1,"h":1},"style":{"fill":0}}},"id":724}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn scene_propose_replace_view_missing_rect_is_invalid_params() {
        let mut scene = box_container_scene_for_set_style();
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"ReplaceView","target_path":"/btn","replacement":{"kind":"Box","style":{"fill":0}}},"id":725}"#;
        let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn scene_propose_replace_view_unknown_target_surfaces_apply_rejected() {
        let mut scene = box_container_scene_for_set_style();
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"ReplaceView","target_path":"/ghost","replacement":{"kind":"Box","rect":{"x":0,"y":0,"w":1,"h":1},"style":{"fill":0}}},"id":726}"#;
        let preview_id = parse_response(
            &dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap(),
        )
        .result
        .unwrap()
        .get("preview_id")
        .and_then(Value::as_u64)
        .unwrap();
        let apply_req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":727}}"#
        );
        let apply_resp =
            parse_response(&dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap());
        let data = apply_resp.error.unwrap().data.unwrap();
        assert_eq!(
            data.get("reason"),
            Some(&Value::String("UnknownTarget".to_string()))
        );
    }
}
