//! JSON-RPC 2.0 envelope and method-dispatch entry (§5.7, R16 slice 10).
//!
//! Realizes the wire shape ratified in §5.7: parse a JSON-RPC 2.0
//! request envelope, route to a typed handler (§5.12), and emit a
//! response envelope. Today the only routed method is `scene/query`
//! (§5.12 method 1 of 7); subsequent slices register `click`, `dry_run`,
//! `snapshot`, `rewind`, `waitFor`, `screenshot`.
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
use pinion_core::Scene;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::click::{click, ClickError, ClickOutcome};
use crate::dry_run::{dry_run, DryRunError};
use crate::query::{query, QueryError};
use crate::rewind::{rewind, RewindError};
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

/// Dispatch one JSON-RPC 2.0 frame against `scene`.
///
/// Takes `&mut Scene` because some methods (e.g. `scene/rewind`) mutate
/// External state through introspection. Read-only methods accept a
/// reborrowed `&Scene` internally.
///
/// Returns `Some(json)` for call requests (any with an `id`), `None`
/// for notifications. Parse errors return a `Some(json)` carrying
/// id=null per the spec.
#[must_use]
pub fn dispatch(scene: &mut Scene, request_json: &str) -> Option<String> {
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

    let outcome = match request.method.as_str() {
        "scene/query" => handle_scene_query(scene, request.params.as_ref()),
        "scene/click" => handle_scene_click(scene, request.params.as_ref()),
        "scene/rewind" => handle_scene_rewind(scene, request.params.as_ref()),
        "scene/snapshot" => handle_scene_snapshot(scene, request.params.as_ref()),
        "scene/dry_run" => handle_scene_dry_run(scene, request.params.as_ref()),
        "scene/waitFor" => handle_scene_wait_for(scene, request.params.as_ref()),
        _ => Err(RpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: Some(Value::String(request.method.clone())),
        }),
    };

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

fn json_to_introspect_value(v: &Value) -> Option<IntrospectValue> {
    match v {
        Value::Null => Some(IntrospectValue::Null),
        Value::Bool(b) => Some(IntrospectValue::Bool(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(IntrospectValue::Int)
            .or_else(|| n.as_f64().map(IntrospectValue::Float)),
        Value::String(s) => Some(IntrospectValue::Text(s.clone())),
        // v0: Array/Object not yet represented in IntrospectValue.
        Value::Array(_) | Value::Object(_) => None,
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
        // `IntrospectValue::Null` collapses into the non_exhaustive
        // wildcard; future variants also land as JSON null until §5.12
        // schema settles a richer projection.
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

    #[test]
    fn parse_error_on_invalid_json() {
        let mut scene = counted_scene(0);
        let resp = parse_response(&dispatch(&mut scene, "{not json").unwrap());
        assert_eq!(resp.error.unwrap().code, -32700);
        assert_eq!(resp.id, None);
    }

    #[test]
    fn invalid_request_on_wrong_jsonrpc_version() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"1.0","method":"scene/query","id":1}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32600);
        assert_eq!(resp.id, Some(RequestId::Num(1)));
    }

    #[test]
    fn method_not_found() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/unknown","id":2}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn invalid_params_when_path_missing() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{},"id":3}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn success_query_with_id_num() {
        let mut scene = counted_scene(42);
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":4}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap(), Value::Number(42.into()));
        assert_eq!(resp.id, Some(RequestId::Num(4)));
    }

    #[test]
    fn success_query_with_id_string() {
        let mut scene = counted_scene(5);
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":"req-a"}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        assert_eq!(resp.id, Some(RequestId::Str("req-a".to_string())));
    }

    #[test]
    fn notification_emits_no_response() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"}}"#;
        assert!(dispatch(&mut scene, req).is_none());
    }

    #[test]
    fn query_error_maps_to_invalid_params_with_variant_tag() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/anything"},"id":7}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
        assert_eq!(err.data, Some(Value::String("IntrospectionOptedOut".to_string())));
    }

    #[test]
    fn path_error_inside_query_also_maps_to_invalid_params() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/window[main/external/count"},"id":8}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
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
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result.get("handled"), Some(&Value::Bool(false)));
    }

    #[test]
    fn scene_click_missing_x_param_is_invalid() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"/external","y":2.0},"id":10}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_rewind_writes_then_query_observes_new_value() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/count","value":123},"id":11}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        assert!(resp.error.is_none());

        let req2 = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":12}"#;
        let resp2 = parse_response(&dispatch(&mut scene, req2).unwrap());
        assert_eq!(resp2.result.unwrap(), Value::Number(123.into()));
    }

    #[test]
    fn scene_rewind_missing_value_param_is_invalid() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/count"},"id":13}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_snapshot_returns_type_and_introspect_object() {
        let mut scene = counted_scene(99);
        let req = r#"{"jsonrpc":"2.0","method":"scene/snapshot","params":{"path":""},"id":14}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
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
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_dry_run_returns_hypothetical_snapshot_and_rolls_back() {
        let mut scene = counted_scene(3);
        let req = r#"{"jsonrpc":"2.0","method":"scene/dry_run","params":{"path":"/external/count","value":77},"id":16}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let intro = result.get("introspect").unwrap().as_object().unwrap();
        assert_eq!(intro.get("count"), Some(&Value::Number(77.into())));

        // Follow-up query confirms the scene was rolled back.
        let q_req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":17}"#;
        let q_resp = parse_response(&dispatch(&mut scene, q_req).unwrap());
        assert_eq!(q_resp.result.unwrap(), Value::Number(3.into()));
    }

    #[test]
    fn scene_dry_run_missing_value_param_is_invalid() {
        let mut scene = counted_scene(0);
        let req = r#"{"jsonrpc":"2.0","method":"scene/dry_run","params":{"path":"/external/count"},"id":18}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn scene_wait_for_returns_matched_when_target_equals_current() {
        let mut scene = counted_scene(42);
        let req = r#"{"jsonrpc":"2.0","method":"scene/waitFor","params":{"path":"/external/count","target":42,"max_attempts":3},"id":19}"#;
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
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
        let resp = parse_response(&dispatch(&mut scene, req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }
}
