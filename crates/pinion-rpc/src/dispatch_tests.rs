use super::*;
use pinion_core::external::{CountedExternal, StubExternal};
use pinion_core::scene::ExternalNode;

fn counted_scene(n: i64) -> Scene {
    Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
}

fn parse_response(s: &str) -> Response {
    serde_json::from_str(s).expect("dispatch produced invalid response JSON")
}

// R613 §5.7 — read_required_tag helper unit tests. The full
// dispatch wire round-trip is exercised by the per-method R602+
// wire-integration suites, which all relied on this pattern
// pre-R613; the cases below pin the lifted helper's exact
// failure shape so a future tweak of the wire-data identifier
// surfaces here before downstream consumers regress.
#[test]
fn r613_read_required_tag_returns_string_when_present() {
    let v = serde_json::json!({ "tag": "list" });
    let out = read_required_tag(&v).unwrap();
    assert_eq!(out, "list");
}

#[test]
fn r613_read_required_tag_missing_errors_with_typed_data() {
    let v = serde_json::json!({});
    let err = read_required_tag(&v).unwrap_err();
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("TagRequired".into())));
}

#[test]
fn r613_read_required_tag_non_string_errors_with_typed_data() {
    // Number, bool, null, array, object — all rejected.
    for invalid in [
        serde_json::json!({"tag": 42}),
        serde_json::json!({"tag": true}),
        serde_json::json!({"tag": null}),
        serde_json::json!({"tag": ["a"]}),
        serde_json::json!({"tag": {"k": "v"}}),
    ] {
        let err = read_required_tag(&invalid).unwrap_err();
        assert_eq!(err.code, -32602);
        assert_eq!(
            err.data,
            Some(Value::String("TagRequired".into())),
            "non-string tag must surface TagRequired",
        );
    }
}

// R621 §5.7 — require_params helper unit tests.
#[test]
fn r621_require_params_returns_value_when_present() {
    let v = serde_json::json!({"any": "shape"});
    assert!(require_params(Some(&v)).is_ok());
}

#[test]
fn r621_require_params_errors_when_absent() {
    let err = require_params(None).unwrap_err();
    assert_eq!(err.code, -32602);
}

// R617 §5.7 — read_optional_tag helper unit tests.
#[test]
fn r617_read_optional_tag_returns_none_when_params_absent() {
    assert_eq!(read_optional_tag(None).unwrap(), None);
}

#[test]
fn r617_read_optional_tag_returns_none_when_field_absent() {
    let v = serde_json::json!({"other": "x"});
    assert_eq!(read_optional_tag(Some(&v)).unwrap(), None);
}

#[test]
fn r617_read_optional_tag_returns_string_when_present() {
    let v = serde_json::json!({"tag": "studio"});
    assert_eq!(read_optional_tag(Some(&v)).unwrap(), Some("studio"));
}

#[test]
fn r617_read_optional_tag_rejects_non_string_value() {
    // Theme-axis optional-tag shape — typed `error.data` is NOT
    // attached (prose-only error, matching pre-R617 behaviour).
    for invalid in [
        serde_json::json!({"tag": 42}),
        serde_json::json!({"tag": true}),
        serde_json::json!({"tag": null}),
        serde_json::json!({"tag": []}),
    ] {
        let err = read_optional_tag(Some(&invalid)).unwrap_err();
        assert_eq!(err.code, -32602);
    }
}

// R627 §5.7 — read_required_str helper unit tests. The R613
// read_required_tag suite (above) anchors the "tag" / "TagRequired"
// canonical defaults; the cases below pin the generic helper's
// behaviour at non-tag axes (text / mode) so a future field-name
// or data-tag tweak surfaces here before downstream consumers
// regress. Mirrors the R613 suite shape — present / missing /
// non-string — but parametrises on (field, data_tag).
#[test]
fn r627_read_required_str_returns_string_when_present() {
    let v = serde_json::json!({ "text": "hello" });
    let out = read_required_str(&v, "text", "TextRequired").unwrap();
    assert_eq!(out, "hello");
}

#[test]
fn r627_read_required_str_missing_errors_with_typed_data() {
    let v = serde_json::json!({});
    let err = read_required_str(&v, "text", "TextRequired").unwrap_err();
    assert_eq!(err.code, -32602);
    assert_eq!(
        err.data.as_ref().and_then(Value::as_str),
        Some("TextRequired")
    );
}

#[test]
fn r627_read_required_str_non_string_errors_with_typed_data() {
    for invalid in [
        serde_json::json!({"mode": 42}),
        serde_json::json!({"mode": true}),
        serde_json::json!({"mode": null}),
        serde_json::json!({"mode": []}),
        serde_json::json!({"mode": {}}),
    ] {
        let err = read_required_str(&invalid, "mode", "ModeRequired").unwrap_err();
        assert_eq!(err.code, -32602);
        assert_eq!(
            err.data.as_ref().and_then(Value::as_str),
            Some("ModeRequired"),
            "ModeRequired must surface at error.data even for non-string value: {invalid:?}",
        );
    }
}

#[test]
fn r627_read_required_str_field_axes_have_distinct_data_tags() {
    // The lifted helper takes both the field name and the typed
    // `error.data` identifier so each axis (tag / text / mode / …)
    // keeps its own variant tag clients pattern-match on. Pin the
    // distinct-data property here so a future caller cannot collapse
    // two axes to the same identifier by accident.
    let v = serde_json::json!({});
    let tag_err = read_required_str(&v, "tag", "TagRequired").unwrap_err();
    let text_err = read_required_str(&v, "text", "TextRequired").unwrap_err();
    let mode_err = read_required_str(&v, "mode", "ModeRequired").unwrap_err();
    assert_ne!(tag_err.data, text_err.data);
    assert_ne!(text_err.data, mode_err.data);
    assert_ne!(tag_err.data, mode_err.data);
}

#[test]
fn r627_read_required_tag_delegates_to_read_required_str() {
    // R613 entry point now delegates to R627 [`read_required_str`]
    // with `("tag", "TagRequired")`. The existing R613 suite above
    // (`r613_read_required_tag_*`) already pins the canonical
    // shape; this case confirms the delegate path returns the
    // same `Ok(&str)` for a happy-path lookup.
    let v = serde_json::json!({ "tag": "list" });
    assert_eq!(read_required_tag(&v).unwrap(), "list");
}

// R631 §5.7 — error-to-rpc converter prose + typed-data stability.
// R625 reworded the SubstrateIntrospectError prose and there was
// no compile-time guard against a future tweak. The cases below
// pin `err.code` + `err.message` + `err.data` for every converter
// fn in dispatch.rs so a regression surfaces here instead of
// downstream consumers. The wire-data identifier ("RuntimeOwner-
// Unavailable" / "TagRequired" / "NotBound") is the AI-agent
// pattern-match anchor — any rename must be a conscious decision.
//
// The `err.message` field is `"Invalid params"` for every variant
// in this catalogue (RpcError::invalid_params constructor); the
// prose detail passed to invalid_params lands in err.data first
// and is then overwritten by with_data_string. Pre-R631 the prose
// was effectively unobservable on the wire; the tests here verify
// the with_data_string overwrite IS being applied so a future
// refactor that drops it cannot silently surface the prose.

#[test]
fn r631_substrate_introspect_runtime_owner_unavailable_pinned() {
    let err = introspect_error_to_rpc(&SubstrateIntrospectError::RuntimeOwnerUnavailable);
    assert_eq!(err.code, -32602);
    assert_eq!(err.message, "Invalid params");
    assert_eq!(
        err.data,
        Some(Value::String("RuntimeOwnerUnavailable".into()))
    );
}

#[test]
fn r631_substrate_introspect_tag_required_pinned() {
    let err = introspect_error_to_rpc(&SubstrateIntrospectError::TagRequired);
    assert_eq!(err.code, -32602);
    assert_eq!(err.message, "Invalid params");
    assert_eq!(err.data, Some(Value::String("TagRequired".into())));
}

#[test]
fn r631_substrate_introspect_not_bound_pinned() {
    let err = introspect_error_to_rpc(&SubstrateIntrospectError::NotBound {
        tag: "list".to_string(),
    });
    assert_eq!(err.code, -32602);
    assert_eq!(err.message, "Invalid params");
    assert_eq!(err.data, Some(Value::String("NotBound".into())));
}

#[test]
fn r631_animation_state_runtime_owner_unavailable_pinned() {
    let err = animation_state_error_to_rpc(&AnimationStateError::RuntimeOwnerUnavailable);
    assert_eq!(err.code, -32602);
    assert_eq!(err.message, "Invalid params");
    assert_eq!(
        err.data,
        Some(Value::String("RuntimeOwnerUnavailable".into()))
    );
}

#[test]
fn r631_animation_state_invalid_epsilon_pinned() {
    let err = animation_state_error_to_rpc(&AnimationStateError::InvalidEpsilon { value: -0.1 });
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("InvalidEpsilon".into())));
}

#[test]
fn r631_animate_control_runtime_owner_unavailable_pinned() {
    let err = animate_control_error_to_rpc(
        &crate::animate_control::AnimateControlError::RuntimeOwnerUnavailable,
    );
    assert_eq!(err.code, -32602);
    assert_eq!(err.message, "Invalid params");
    assert_eq!(
        err.data,
        Some(Value::String("RuntimeOwnerUnavailable".into()))
    );
}

#[test]
fn r631_theme_tokens_errors_pinned() {
    let err1 = theme_tokens_error_to_rpc(ThemeTokensError::RuntimeOwnerUnavailable);
    assert_eq!(err1.code, -32602);
    assert_eq!(
        err1.data,
        Some(Value::String("RuntimeOwnerUnavailable".into()))
    );
    let err2 = theme_tokens_error_to_rpc(ThemeTokensError::NotBound {
        tag: "app".to_string(),
    });
    assert_eq!(err2.code, -32602);
    assert_eq!(err2.data, Some(Value::String("NotBound".into())));
}

#[test]
fn r631_set_theme_mode_errors_pinned() {
    let err1 = set_theme_mode_error_to_rpc(SetThemeModeError::RuntimeOwnerUnavailable);
    assert_eq!(err1.code, -32602);
    assert_eq!(
        err1.data,
        Some(Value::String("RuntimeOwnerUnavailable".into()))
    );
    let err2 = set_theme_mode_error_to_rpc(SetThemeModeError::NotBound {
        tag: "app".to_string(),
    });
    assert_eq!(err2.code, -32602);
    assert_eq!(err2.data, Some(Value::String("NotBound".into())));
}

#[test]
fn r631_set_theme_palettes_errors_pinned() {
    let err1 = set_theme_palettes_error_to_rpc(SetThemePalettesError::RuntimeOwnerUnavailable);
    assert_eq!(err1.code, -32602);
    assert_eq!(
        err1.data,
        Some(Value::String("RuntimeOwnerUnavailable".into()))
    );
    let err2 = set_theme_palettes_error_to_rpc(SetThemePalettesError::NotBound {
        tag: "app".to_string(),
    });
    assert_eq!(err2.code, -32602);
    assert_eq!(err2.data, Some(Value::String("NotBound".into())));
}

// Cross-converter consistency: every `RuntimeOwnerUnavailable`-emitting
// converter must produce the same wire-data identifier. A future
// regression that renames one path (e.g. to "RuntimeContextMissing")
// would currently pass its own per-axis test but break wire
// consistency for AI agents that pattern-match on the typed name.
#[test]
fn r631_runtime_owner_unavailable_identifier_is_uniform_across_converters() {
    let canonical = Some(Value::String("RuntimeOwnerUnavailable".into()));
    assert_eq!(
        introspect_error_to_rpc(&SubstrateIntrospectError::RuntimeOwnerUnavailable).data,
        canonical,
    );
    assert_eq!(
        animation_state_error_to_rpc(&AnimationStateError::RuntimeOwnerUnavailable).data,
        canonical,
    );
    assert_eq!(
        animate_control_error_to_rpc(
            &crate::animate_control::AnimateControlError::RuntimeOwnerUnavailable,
        )
        .data,
        canonical,
    );
    assert_eq!(
        theme_tokens_error_to_rpc(ThemeTokensError::RuntimeOwnerUnavailable).data,
        canonical,
    );
    assert_eq!(
        set_theme_mode_error_to_rpc(SetThemeModeError::RuntimeOwnerUnavailable).data,
        canonical,
    );
    assert_eq!(
        set_theme_palettes_error_to_rpc(SetThemePalettesError::RuntimeOwnerUnavailable).data,
        canonical,
    );
}

// Cross-converter consistency for the `NotBound` axis (tag-keyed
// converters). The wire-data identifier must be uniform; the
// per-axis prose embeds the tag but the typed name does not so
// the AI-agent pattern-match anchor stays simple.
#[test]
fn r631_not_bound_identifier_is_uniform_across_converters() {
    let canonical = Some(Value::String("NotBound".into()));
    let tag = || "app".to_string();
    assert_eq!(
        introspect_error_to_rpc(&SubstrateIntrospectError::NotBound { tag: tag() }).data,
        canonical,
    );
    assert_eq!(
        theme_tokens_error_to_rpc(ThemeTokensError::NotBound { tag: tag() }).data,
        canonical,
    );
    assert_eq!(
        set_theme_mode_error_to_rpc(SetThemeModeError::NotBound { tag: tag() }).data,
        canonical,
    );
    assert_eq!(
        set_theme_palettes_error_to_rpc(SetThemePalettesError::NotBound { tag: tag() }).data,
        canonical,
    );
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

// ---- R889 §5.49 — unknown-window gate (dispatch-entry rejection) ----

#[test]
fn r889_unknown_window_verdict_extracts_and_judges_the_in_band_param() {
    // One home for the extraction + judgment glue (GUI + TUI
    // ingresses both call this): absent / non-string / known →
    // None; supplied-but-unknown → Some(id).
    let known = |wid: &str| wid == "main";
    let parse = |s: &str| parse_request(s).expect("frame parses");

    let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"scene/query","params":{}}"#);
    assert_eq!(unknown_window_verdict(&req, known), None, "absent param");
    let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"scene/query"}"#);
    assert_eq!(
        unknown_window_verdict(&req, known),
        None,
        "absent params object"
    );
    let req =
        parse(r#"{"jsonrpc":"2.0","id":1,"method":"scene/query","params":{"window":"main"}}"#);
    assert_eq!(unknown_window_verdict(&req, known), None, "known window");
    let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"scene/query","params":{"window":42}}"#);
    assert_eq!(
        unknown_window_verdict(&req, known),
        None,
        "non-string is NOT this judgment's job — dispatch_parsed's type gate rejects it",
    );
    let req =
        parse(r#"{"jsonrpc":"2.0","id":1,"method":"scene/query","params":{"window":"side"}}"#);
    assert_eq!(
        unknown_window_verdict(&req, known),
        Some("side".to_owned()),
        "supplied-but-unknown id is the verdict payload",
    );
}

#[test]
fn r890_1_window_scope_is_the_one_extraction_home() {
    // Request::window_scope — absent / string / non-string. The
    // backend entries, the verdict, and the dispatch type gate all
    // read the param through this accessor (pre-R890.1 the GUI shell
    // hand-rolled a second byte-identical extraction).
    let parse = |s: &str| parse_request(s).expect("frame parses");
    let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"m"}"#);
    assert_eq!(req.window_scope().unwrap(), None, "absent params");
    let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"m","params":{}}"#);
    assert_eq!(req.window_scope().unwrap(), None, "absent param");
    let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"m","params":{"window":"side"}}"#);
    assert_eq!(req.window_scope().unwrap(), Some("side"), "string scope");
    let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"m","params":{"window":42}}"#);
    let err = req.window_scope().unwrap_err();
    assert_eq!(err.code, -32602, "non-string scope is a params type error");
}

#[test]
fn r890_1_non_string_window_param_is_rejected_not_silently_dropped() {
    // Pre-R890.1 `{"window": 42}` fell through the extraction and the
    // request acted on the primary — the R889 alias smell class in
    // the type-error corner. dispatch_parsed now rejects the frame.
    let mut scene = counted_scene(7);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision);
    let req =
        r#"{"jsonrpc":"2.0","id":3,"method":"scene/query","params":{"window":42,"path":"/"}}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).expect("error frame"));
    let err = resp.error.expect("rejected");
    assert_eq!(err.code, -32602);
    assert_eq!(resp.id, Some(RequestId::Num(3)), "id echoes the request");
}

#[test]
fn r889_unknown_window_ctx_rejects_before_method_routing() {
    // The verdict threaded on the context rejects the WHOLE request
    // with `-32602 unknown_window` — READ and WRITE methods alike,
    // before the method match (a bogus scope must not mutate or read
    // anything; pre-R889 the GUI aliased it onto the primary). The
    // frame is a MUTATE-kind method (scene/click), so the untouched
    // OCC token below is a real pin (R890.1: the pre-fix assert used
    // a Read-kind method that never bumps — vacuous).
    let mut scene = counted_scene(7);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision)
        .with_unknown_window("bogus".to_owned());
    let req = r#"{"jsonrpc":"2.0","id":9,"method":"scene/click","params":{"window":"bogus","at":{"x":1.0,"y":1.0}}}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).expect("error frame"));
    let err = resp.error.expect("rejected");
    assert_eq!(err.code, -32602);
    assert_eq!(err.message, "unknown_window");
    assert_eq!(err.data, Some(Value::String("bogus".into())));
    // OCC token untouched — the Mutate-kind arm never ran.
    assert_eq!(revision.current(), 0, "no method ran, no revision bump");
}

#[test]
fn r890_1_unknown_window_notification_stays_silent() {
    // JSON-RPC 2.0: the server MUST NOT reply to a notification —
    // errors in notifications are ignored. The gate honors the same
    // rule the method-routing tail does for -32601 (pre-R890.1 the
    // gate answered notifications with an id:null error frame).
    let mut scene = counted_scene(7);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision)
        .with_unknown_window("bogus".to_owned());
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"window":"bogus","at":{"x":1.0,"y":1.0}}}"#;
    assert_eq!(dispatch(&mut ctx, req), None, "notification gets no frame");
    // Type-error variant honors the same silence.
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision);
    let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"window":42}}"#;
    assert_eq!(
        dispatch(&mut ctx, req),
        None,
        "type-error notification silent too"
    );
}

// ---- R51.25 §5.12 — Response::result nullable_present deserialize ----

#[test]
fn response_result_explicit_null_deserializes_to_some_null() {
    // JSON-RPC success response carrying `result: null` (e.g. a
    // method whose return value is `IntrospectValue::Null`).
    // Plain `Option<Value>` would collapse this to `None`; the
    // R51.25 `deserialize_nullable_present` keeps it as
    // `Some(Value::Null)` so callers can distinguish from an
    // error response.
    let wire = r#"{"jsonrpc":"2.0","result":null,"id":1}"#;
    let resp: Response = serde_json::from_str(wire).unwrap();
    assert_eq!(resp.result, Some(Value::Null));
    assert!(resp.error.is_none());
}

#[test]
fn response_result_absent_deserializes_to_none() {
    // JSON-RPC error response — `result` field is absent. The
    // `#[serde(default)]` on the field supplies `None` without
    // running `deserialize_nullable_present`.
    let wire = r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"x"},"id":1}"#;
    let resp: Response = serde_json::from_str(wire).unwrap();
    assert_eq!(resp.result, None);
    assert_eq!(resp.error.as_ref().unwrap().code, -32601);
}

#[test]
fn response_result_value_deserializes_to_some_value() {
    // JSON-RPC success response with a concrete value. The custom
    // deserializer must be transparent for non-null values.
    let wire = r#"{"jsonrpc":"2.0","result":42,"id":1}"#;
    let resp: Response = serde_json::from_str(wire).unwrap();
    assert_eq!(resp.result, Some(Value::Number(42.into())));
    assert!(resp.error.is_none());
}

#[test]
fn response_result_some_null_serializes_to_explicit_null() {
    // Round-trip — `Some(Value::Null)` must serialize as
    // `"result":null`, not be elided like `None`.
    let resp = Response {
        jsonrpc: JSONRPC_V2.to_owned(),
        result: Some(Value::Null),
        error: None,
        id: Some(RequestId::Num(7)),
    };
    let wire = serde_json::to_string(&resp).unwrap();
    assert!(
        wire.contains("\"result\":null"),
        "Some(Value::Null) must serialize explicit null, got: {wire}",
    );
}

#[test]
fn response_result_none_is_elided_on_serialize() {
    // Round-trip — `None` (error response) must skip serialization
    // so the wire form omits the `result` key entirely.
    let resp = Response {
        jsonrpc: JSONRPC_V2.to_owned(),
        result: None,
        error: Some(RpcError::new(-32601, "method not found")),
        id: Some(RequestId::Num(7)),
    };
    let wire = serde_json::to_string(&resp).unwrap();
    assert!(
        !wire.contains("\"result\""),
        "None must elide the result field, got: {wire}",
    );
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
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":4}"#;
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
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/anything"},"id":7}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert_eq!(
        err.data,
        Some(Value::String("IntrospectionOptedOut".to_string()))
    );
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

// ---- R51.197 §5.49 §5.45 — scene/key injection ----

#[test]
fn scene_key_enqueues_arrow_down_into_inbox() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":180.0,"y":160.0},"key":"ArrowDown"},"id":300}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Key {
        x,
        y,
        ref key,
        state,
    } = inbox[0]
    else {
        panic!("expected Key variant, got {:?}", inbox[0]);
    };
    assert!((x - 180.0).abs() < f64::EPSILON);
    assert!((y - 160.0).abs() < f64::EPSILON);
    assert_eq!(key, "ArrowDown");
    // R882 — `state` absent ⇒ the legacy atomic press.
    assert_eq!(state, KeyWireState::Press);
}

#[test]
fn scene_key_without_inbox_is_unavailable() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":0.0,"y":0.0},"key":"ArrowDown"},"id":301}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("InputInjectionUnavailable"), "data: {data:?}");
}

#[test]
fn scene_key_empty_string_is_invalid() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":0.0,"y":0.0},"key":""},"id":302}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("empty"), "data: {data:?}");
    assert!(inbox.is_empty());
}

#[test]
fn scene_key_missing_key_is_invalid() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":0.0,"y":0.0}},"id":303}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("key"), "data: {data:?}");
}

// ---- R666 §5.37 — scene/key character vs named discriminator ----

#[test]
fn r666_scene_key_single_codepoint_routes_as_character_key() {
    // R666 §5.37: single-char `key` ("a") → CharacterKey variant so
    // the shell drain dispatches `handle_character_key` →
    // `V::keybinding` (typed-event channel) before falling through
    // to `apply_key`. Closes [[scene-key-character-named-gap]].
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":10.0,"y":20.0},"key":"a"},"id":400}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(inbox.len(), 1);
    let DeferredInput::CharacterKey {
        x,
        y,
        ref character,
        state,
    } = inbox[0]
    else {
        panic!("expected CharacterKey, got {:?}", inbox[0]);
    };
    assert!((x - 10.0).abs() < f64::EPSILON);
    assert!((y - 20.0).abs() < f64::EPSILON);
    assert_eq!(character, "a");
    // R882 — `state` absent ⇒ the legacy atomic press.
    assert_eq!(state, KeyWireState::Press);
}

#[test]
fn r666_scene_key_space_codepoint_routes_as_character_key() {
    // U+0020 SPACE is a single codepoint and arrives as
    // `Key::Character(" ")` on every real keyboard surface (winit /
    // crossterm). The Toggle Space arc reaches it via
    // `handle_named_key("Space")` separately, so the character " "
    // *must* go through the character path.
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":0.0,"y":0.0},"key":" "},"id":401}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::CharacterKey { ref character, .. } = inbox[0] else {
        panic!("expected CharacterKey for U+0020 space, got {:?}", inbox[0]);
    };
    assert_eq!(character, " ");
}

// ---- R882 §5.49 §5.39 — scene/key state ("down" / "up") edges ----

#[test]
fn r882_scene_key_state_down_enqueues_down_edge() {
    // `state: "down"` is the winit `Pressed` mirror: the drain updates
    // the held-key cache AND dispatches, so the wire carries the edge.
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":5.0,"y":6.0},"key":"Space","state":"down"},"id":410}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::Key { ref key, state, .. } = inbox[0] else {
        panic!("expected Key variant, got {:?}", inbox[0]);
    };
    assert_eq!(key, "Space");
    assert_eq!(state, KeyWireState::Down);
}

#[test]
fn r882_scene_key_state_up_enqueues_up_edge() {
    // `state: "up"` is the winit `Released` mirror — held-key cache
    // update only, no dispatch (the drain's policy; the wire carries
    // the edge). The single-codepoint discriminator still applies:
    // a character key with an edge keeps the CharacterKey variant.
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":5.0,"y":6.0},"key":"Space","state":"up"},"id":411}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::Key { ref key, state, .. } = inbox[0] else {
        panic!("expected Key variant, got {:?}", inbox[0]);
    };
    assert_eq!(key, "Space");
    assert_eq!(state, KeyWireState::Up);

    // The single-codepoint discriminator still applies on the edge
    // wire: a character key with `state` keeps the CharacterKey
    // variant. Fresh context — the dispatcher consumes its inbox
    // handle per call.
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":5.0,"y":6.0},"key":"a","state":"up"},"id":412}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::CharacterKey {
        ref character,
        state,
        ..
    } = inbox[0]
    else {
        panic!("expected CharacterKey variant, got {:?}", inbox[0]);
    };
    assert_eq!(character, "a");
    assert_eq!(state, KeyWireState::Up);
}

#[test]
fn r882_1_scene_key_state_up_is_positionless() {
    // A release edge mirrors winit `Released`, which carries no
    // cursor: `at`/`path` may be omitted for `state:"up"` (the drain
    // dispatches nothing and moves no cursor for it). `state:"down"`
    // still requires a position — it dispatches like a press.
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/key","params":{"key":"Space","state":"up"},"id":420}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::Key { ref key, state, .. } = inbox[0] else {
        panic!("expected Key variant, got {:?}", inbox[0]);
    };
    assert_eq!(key, "Space");
    assert_eq!(state, KeyWireState::Up);

    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"key":"Space","state":"down"},"id":421}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp
        .error
        .expect("a dispatching edge still requires a position");
    assert_eq!(err.code, -32602);
    assert!(inbox.is_empty());
}

#[test]
fn r882_scene_key_state_out_of_vocabulary_rejects() {
    // A typo'd edge must reject loudly (invalid_params), never decay
    // to a silent atomic press — the R773 closed-vocabulary rule.
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    for bad in [r#""pressed""#, r#""DOWN""#, "3", r#""""#] {
        // Fresh context per request — the dispatcher consumes its
        // inbox handle per call, and each rejection must be the
        // vocabulary's, not a missing-inbox artefact.
        let mut inbox: Vec<DeferredInput> = Vec::new();
        let mut ctx =
            DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/key","params":{{"at":{{"x":0.0,"y":0.0}},"key":"Space","state":{bad}}},"id":413}}"#
        );
        let resp = parse_response(&dispatch(&mut ctx, &req).unwrap());
        let err = resp.error.expect("out-of-vocabulary state must reject");
        assert_eq!(err.code, -32602, "state={bad}");
        let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
        assert!(
            data.contains("state"),
            "the rejection names the state param: {data:?}"
        );
        assert!(inbox.is_empty(), "rejected requests must enqueue nothing");
    }
}

#[test]
fn r882_key_wire_state_round_trips() {
    // decode == inverse(encode), including the omitted-param Press.
    for state in [KeyWireState::Press, KeyWireState::Down, KeyWireState::Up] {
        assert_eq!(
            KeyWireState::from_wire_param(state.as_wire_param()),
            Some(state)
        );
    }
    assert_eq!(
        KeyWireState::from_wire_param(None),
        Some(KeyWireState::Press)
    );
    assert_eq!(KeyWireState::from_wire_param(Some("sideways")), None);
    assert_eq!(KeyWireState::default(), KeyWireState::Press);
}

#[test]
fn r666_scene_key_precomposed_cjk_codepoint_routes_as_character_key() {
    // R56.1.g carry: a single pre-composed CJK syllable like "안"
    // (U+C548) is exactly one codepoint, so the chars().count()
    // discriminator routes it as Character. Multi-syllable IME
    // composition output (e.g. "안녕") has >1 codepoint and routes
    // as Named, where TextField's `is_printable_key` rejects it and
    // the preedit buffer substrate takes over.
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":0.0,"y":0.0},"key":"안"},"id":402}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::CharacterKey { ref character, .. } = inbox[0] else {
        panic!("expected CharacterKey, got {:?}", inbox[0]);
    };
    assert_eq!(character, "안");
}

#[test]
fn r666_scene_key_multi_codepoint_named_string_still_routes_as_named() {
    // Named keys ("ArrowDown", "Enter", "PageUp", "F1", "Space")
    // are always ≥ 2 chars per pinion-shell::named_key_str so the
    // discriminator keeps the v0 backwards-compat path intact.
    for named in ["ArrowDown", "Enter", "PageUp", "Space", "Home", "End"] {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let mut inbox: Vec<DeferredInput> = Vec::new();
        {
            let mut ctx = DispatchContext::new(&mut scene, &previews, &revision)
                .with_deferred_inputs(&mut inbox);
            let req = format!(
                r#"{{"jsonrpc":"2.0","method":"scene/key","params":{{"at":{{"x":0.0,"y":0.0}},"key":"{named}"}},"id":403}}"#
            );
            let resp = parse_response(&dispatch(&mut ctx, &req).unwrap());
            assert!(resp.error.is_none(), "{:?}", resp.error);
        }
        let DeferredInput::Key { ref key, .. } = inbox[0] else {
            panic!("expected Key (named) for {named}, got {:?}", inbox[0]);
        };
        assert_eq!(key, named);
    }
}

// ---- R770 §5.49 §5.15 — OS file drag-drop RPC peers ----

#[test]
fn r770_scene_drop_file_enqueues_file_drop_with_path() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/drop_file","params":{"path":"/tmp/report.pdf"},"id":770}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));
    let DeferredInput::FileDrop { ref path } = inbox[0] else {
        panic!("expected FileDrop, got {:?}", inbox[0]);
    };
    assert_eq!(path, "/tmp/report.pdf");
}

#[test]
fn r770_scene_hover_file_enqueues_file_hover_with_path() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/hover_file","params":{"path":"/tmp/a.png"},"id":771}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::FileHover { ref path } = inbox[0] else {
        panic!("expected FileHover, got {:?}", inbox[0]);
    };
    assert_eq!(path, "/tmp/a.png");
}

#[test]
fn r770_scene_hover_file_cancel_enqueues_cancel_without_params() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/hover_file_cancel","id":772}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert!(matches!(inbox[0], DeferredInput::FileHoverCancel));
}

#[test]
fn r770_scene_drop_file_missing_path_rejects() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/drop_file","params":{},"id":773}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_some(), "missing path must reject");
    assert!(inbox.is_empty(), "no event enqueued on a rejected request");
}

// ---- R770.1 §5.36 §5.49 — TextStyle wire round-trip (decode ∘ encode = id) ----

#[test]
fn r770_1_text_style_json_decode_is_inverse_of_encode() {
    // The run read (`text_style_to_json`, here) and the `apply-style`
    // write (`json_to_text_style`, pinion-core) are a decode-mirrors-
    // encode pair — an AI round-trips a run: snapshot → mutate → write.
    // This test pins them in sync (R743.1: decode is the inverse of
    // encode; R615 from_hex/to_hex precedent).
    use pinion_core::style::{
        Color, FontStyle, FontWeight, LineHeight, TextAlign, TextDecoration, TextOverflow,
        TextStyle,
    };
    use pinion_core::widgets::text_field::json_to_text_style;

    let mut a = TextStyle::new();
    a.fg_color = Color::rgba(0xD0, 0x28, 0x28, 0xC0);
    a.font_size_px = 24;
    a.font_weight = FontWeight::BOLD;
    a.font_style = FontStyle::Italic;

    let mut b = TextStyle::new();
    b.font_style = FontStyle::Oblique(Some(-14));
    b.line_height = LineHeight::Px(30);
    b.letter_spacing = 3;
    b.text_align = TextAlign::Center;
    b.decoration = TextDecoration::none()
        .with_underline(true)
        .with_strikethrough(true);
    b.overflow = TextOverflow::Ellipsis;
    b.font_family = Some(pinion_core::style::FontFamily::Named("Inter".into()));

    let mut c = TextStyle::new();
    c.line_height = LineHeight::MultiplierX100(150);
    c.font_style = FontStyle::Oblique(None);
    c.text_align = TextAlign::Justify;
    c.overflow = TextOverflow::Clip;
    // Generic-family wire round-trip (R1002): the keyword survives the
    // string wire and re-classifies to `Generic`, not `Named`.
    c.font_family = Some(pinion_core::style::FontFamily::Generic(
        pinion_core::style::GenericFontFamily::Monospace,
    ));

    for sample in [TextStyle::new(), a, b, c] {
        let encoded = text_style_to_json(&sample);
        let decoded = json_to_text_style(encoded.as_object().unwrap());
        assert_eq!(
            decoded, sample,
            "json_to_text_style must invert text_style_to_json"
        );
    }
}

// ---- R51.196 §5.49 — scene/click v1 (DeferredInput::Click) ----

#[test]
fn scene_click_v1_enqueues_click_at_coordinate() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/click","params":{"at":{"x":120.0,"y":80.0}},"id":9}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Click { x, y } = inbox[0] else {
        panic!("expected Click variant, got {:?}", inbox[0]);
    };
    assert!((x - 120.0).abs() < f64::EPSILON);
    assert!((y - 80.0).abs() < f64::EPSILON);
}

// ---- R887 §5.49 §5.53 — scene/click button param (ClickButton) ----

#[test]
fn r887_scene_click_right_button_enqueues_secondary_click() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"at":{"x":64.0,"y":48.0},"button":"right"},"id":1}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));
    assert_eq!(inbox.len(), 1);
    let DeferredInput::SecondaryClick { x, y } = inbox[0] else {
        panic!("expected SecondaryClick variant, got {:?}", inbox[0]);
    };
    assert!((x - 64.0).abs() < f64::EPSILON);
    assert!((y - 48.0).abs() < f64::EPSILON);
}

#[test]
fn r887_scene_click_path_form_with_right_button_resolves_rect_centre() {
    // R888.1 — the `path` selector and the `button` axis compose: the
    // dispatcher walks the paint scene for the tag's rect centre and
    // enqueues the secondary press there (pre-R888.1 only the demo
    // covered this composition).
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut produce = |_w: u32, _h: u32| -> Scene {
        use pinion_core::scene::{BoxNode, ContainerNode};
        let inner = Scene::Box(
            BoxNode::filled(
                pinion_core::scene::Rect::new(10, 20, 40, 20),
                pinion_core::Color::default(),
            )
            .with_tag("target"),
        );
        let mut outer = ContainerNode::new(vec![inner]);
        outer.rect = pinion_core::scene::Rect::new(0, 0, 360, 220);
        Scene::Container(outer)
    };
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision)
        .with_deferred_inputs(&mut inbox)
        .with_paint_producer(&mut produce);
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"target","button":"right"},"id":1}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(inbox.len(), 1);
    let DeferredInput::SecondaryClick { x, y } = inbox[0] else {
        panic!("expected SecondaryClick variant, got {:?}", inbox[0]);
    };
    assert!((x - 30.0).abs() < f64::EPSILON, "rect centre x");
    assert!((y - 30.0).abs() < f64::EPSILON, "rect centre y");
}

#[test]
fn r887_scene_click_explicit_left_button_enqueues_click() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"at":{"x":1.0,"y":2.0},"button":"left"},"id":1}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(inbox.len(), 1);
    assert!(
        matches!(inbox[0], DeferredInput::Click { .. }),
        "explicit \"left\" takes the same arc as the omitted default, got {:?}",
        inbox[0]
    );
}

#[test]
fn r887_scene_click_middle_button_redirects_to_drag() {
    // "middle" is a *gesture* (pan vs paste decided by movement), so the
    // click wire rejects it with a redirect instead of guessing an arc.
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"at":{"x":1.0,"y":2.0},"button":"middle"},"id":1}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.expect("middle button must error");
    assert_eq!(err.code, -32602);
    let detail = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(
        detail.contains("scene/drag"),
        "redirect names the owning method: {detail}"
    );
    assert!(inbox.is_empty(), "rejected request enqueues nothing");
}

#[test]
fn r887_scene_click_unknown_or_non_string_button_is_invalid() {
    for bad in [r#""rigth""#, r#""""#, "2", "null", "true"] {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let mut inbox: Vec<DeferredInput> = Vec::new();
        let mut ctx =
            DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/click","params":{{"at":{{"x":1.0,"y":2.0}},"button":{bad}}},"id":1}}"#
        );
        let resp = parse_response(&dispatch(&mut ctx, &req).unwrap());
        let err = resp
            .error
            .unwrap_or_else(|| panic!("button {bad} must error"));
        assert_eq!(err.code, -32602, "button {bad}");
        assert!(inbox.is_empty(), "rejected request enqueues nothing");
    }
}

#[test]
fn r887_click_button_wire_pair_round_trips() {
    // decode == inverse(encode) for the whole vocabulary; unknown
    // names decode to None (the R773 wire-vocabulary SSOT guard).
    for b in [ClickButton::Left, ClickButton::Right] {
        assert_eq!(ClickButton::from_wire_name(b.as_wire_name()), Some(b));
    }
    assert_eq!(ClickButton::from_wire_name("middle"), None);
    assert_eq!(ClickButton::from_wire_name(""), None);
    assert_eq!(ClickButton::default(), ClickButton::Left);
}

#[test]
fn click_and_drag_button_vocabularies_share_left() {
    // R773 cross-vocab pin: ClickButton and DragButton are parallel
    // vocabularies (deliberately NOT folded — each names exactly the
    // buttons its method can mirror), so the one shared token must
    // stay byte-identical across both.
    assert_eq!(
        ClickButton::Left.as_wire_name(),
        DragButton::Left.as_wire_name()
    );
}

// ---- R881 §5.35 §5.49 — scene/drag button param (DragButton) ----

#[test]
fn r881_scene_drag_defaults_to_left_button() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/drag","params":{"from":{"x":10.0,"y":10.0},"to":{"x":50.0,"y":50.0}},"id":1}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Drag { steps, button, .. } = inbox[0] else {
        panic!("expected Drag variant, got {:?}", inbox[0]);
    };
    assert_eq!(steps, 8, "default steps");
    assert_eq!(button, DragButton::Left, "omitted button defaults to left");
}

#[test]
fn r881_scene_drag_middle_button_enqueues_middle() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/drag","params":{"from":{"x":10.0,"y":10.0},"to":{"x":50.0,"y":50.0},"button":"middle"},"id":2}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Drag { button, .. } = inbox[0] else {
        panic!("expected Drag variant, got {:?}", inbox[0]);
    };
    assert_eq!(button, DragButton::Middle);
}

#[test]
fn r881_scene_drag_unknown_button_is_invalid_params() {
    // Out-of-vocabulary names reject loudly (no silent left-drag) —
    // the DragButton decode is the single gate.
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/drag","params":{"from":{"x":10.0,"y":10.0},"to":{"x":50.0,"y":50.0},"button":"right"},"id":3}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.expect("unknown button must error");
    assert_eq!(err.code, -32602);
    assert!(inbox.is_empty(), "rejected request enqueues nothing");
}

#[test]
fn r881_drag_button_wire_pair_round_trips() {
    // decode == inverse(encode) for the whole vocabulary; unknown
    // names decode to None (the R773 wire-vocabulary SSOT guard).
    for b in [DragButton::Left, DragButton::Middle] {
        assert_eq!(DragButton::from_wire_name(b.as_wire_name()), Some(b));
    }
    assert_eq!(DragButton::from_wire_name("right"), None);
    assert_eq!(DragButton::from_wire_name(""), None);
    assert_eq!(DragButton::default(), DragButton::Left);
}

// ---- R724 §5.28 — scene/tick (DeferredInput::Tick) ----

#[test]
fn scene_tick_enqueues_tick_with_dt() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/tick","params":{"dt":0.25},"id":7}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Tick { dt } = inbox[0] else {
        panic!("expected Tick variant, got {:?}", inbox[0]);
    };
    assert!((dt - 0.25).abs() < 1e-6);
}

#[test]
fn scene_tick_missing_dt_errors() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/tick","params":{},"id":7}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_some(), "missing dt must error");
    assert!(inbox.is_empty(), "no tick enqueued on error");
}

#[test]
fn scene_tick_rejects_negative_and_nan_dt() {
    for bad in [r#"{"dt":-1.0}"#, r#"{"dt":"x"}"#] {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let mut inbox: Vec<DeferredInput> = Vec::new();
        let mut ctx =
            DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
        let req = format!(r#"{{"jsonrpc":"2.0","method":"scene/tick","params":{bad},"id":7}}"#);
        let resp = parse_response(&dispatch(&mut ctx, &req).unwrap());
        assert!(resp.error.is_some(), "dt {bad} must error");
        assert!(inbox.is_empty(), "no tick enqueued for {bad}");
    }
}

// ---- R829 §2 #4 §5.28 — scene/set_fps (DeferredInput::SetTargetFps) ----

#[test]
fn scene_set_fps_enqueues_target_fps() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision)
        .with_deferred_inputs(&mut inbox)
        .with_pacing_state(PacingState::DefaultPolicy);
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_fps","params":{"fps":0},"id":7}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));
    assert_eq!(inbox.len(), 1);
    let DeferredInput::SetTargetFps { fps } = inbox[0] else {
        panic!("expected SetTargetFps variant, got {:?}", inbox[0]);
    };
    assert_eq!(fps, Some(0), "fps=0 pauses the per-window paint clock");
}

#[test]
fn scene_set_fps_accepts_positive_rate() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision)
        .with_deferred_inputs(&mut inbox)
        .with_pacing_state(PacingState::DefaultPolicy);
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_fps","params":{"fps":144},"id":7}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::SetTargetFps { fps } = inbox[0] else {
        panic!("expected SetTargetFps variant, got {:?}", inbox[0]);
    };
    assert_eq!(fps, Some(144));
}

#[test]
fn r888_scene_set_fps_null_clears_the_override() {
    // R888 — `{"fps": null}` enqueues the clear (restore the adaptive
    // default policy); the boot state is wire-reachable again.
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision)
        .with_deferred_inputs(&mut inbox)
        .with_pacing_state(PacingState::DefaultPolicy);
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_fps","params":{"fps":null},"id":7}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::SetTargetFps { fps } = inbox[0] else {
        panic!("expected SetTargetFps variant, got {:?}", inbox[0]);
    };
    assert_eq!(fps, None, "null clears the override");
}

#[test]
fn r888_1_set_fps_without_pacing_capability_is_unavailable() {
    // R888.1 — a backend that cannot answer `scene/pacing_state`
    // (no pacing clock: the TUI) must reject the write too, not
    // accept-and-drop it; one availability signal, one wire token.
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_fps","params":{"fps":30},"id":7}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.expect("must error without pacing capability");
    assert_eq!(err.code, -32602);
    assert_eq!(
        err.data.as_ref().and_then(Value::as_str),
        Some("PacingStateUnavailable"),
        "write and read share the availability token",
    );
    assert!(inbox.is_empty(), "rejected write enqueues nothing");
}

// ---- R888 §5.49 §5.28 — scene/pacing_state (READ peer of set_fps) ----

#[test]
fn r888_pacing_state_without_snapshot_is_unavailable() {
    // No embedder pre-resolve (headless fixture / TUI: no pacing
    // clock) -> PacingStateUnavailable, NOT a fake "default policy".
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision);
    let req = r#"{"jsonrpc":"2.0","method":"scene/pacing_state","id":7}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.expect("must error without a snapshot");
    assert_eq!(err.code, -32602);
    assert_eq!(
        err.data.as_ref().and_then(Value::as_str),
        Some("PacingStateUnavailable"),
    );
}

#[test]
fn r888_pacing_state_reports_default_policy_as_null_fps() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision)
        .with_pacing_state(PacingState::DefaultPolicy);
    let req = r#"{"jsonrpc":"2.0","method":"scene/pacing_state","id":7}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(
        resp.result,
        Some(serde_json::json!({ "fps": null })),
        "no override -> fps null (the scene/set_fps null write's mirror)",
    );
}

#[test]
fn r888_pacing_state_reports_override_including_paused_zero() {
    for (state, expect) in [
        (
            PacingState::Override(144),
            serde_json::json!({ "fps": 144 }),
        ),
        (PacingState::Override(0), serde_json::json!({ "fps": 0 })),
    ] {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let mut ctx =
            DispatchContext::new(&mut scene, &previews, &revision).with_pacing_state(state);
        let req = r#"{"jsonrpc":"2.0","method":"scene/pacing_state","id":7}"#;
        let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
        assert!(resp.error.is_none(), "{state:?}: {:?}", resp.error);
        assert_eq!(resp.result, Some(expect), "{state:?}");
    }
}

#[test]
fn scene_set_fps_rejects_missing_negative_and_non_integer() {
    for bad in [r"{}", r#"{"fps":-1}"#, r#"{"fps":"x"}"#, r#"{"fps":1.5}"#] {
        let mut scene = counted_scene(0);
        let previews = PreviewLedger::default();
        let revision = SceneRevision::default();
        let mut inbox: Vec<DeferredInput> = Vec::new();
        let mut ctx = DispatchContext::new(&mut scene, &previews, &revision)
            .with_deferred_inputs(&mut inbox)
            .with_pacing_state(PacingState::DefaultPolicy);
        let req = format!(r#"{{"jsonrpc":"2.0","method":"scene/set_fps","params":{bad},"id":7}}"#);
        let resp = parse_response(&dispatch(&mut ctx, &req).unwrap());
        assert!(resp.error.is_some(), "fps {bad} must error");
        assert!(inbox.is_empty(), "no set_fps enqueued for {bad}");
    }
}

// ---- R763 §5.49 §5.39 — scene/modifiers (DeferredInput::SetModifiers) ----

#[test]
fn scene_modifiers_enqueues_absolute_state() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/modifiers","params":{"shift":true,"ctrl":true},"id":7}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));
    assert_eq!(inbox.len(), 1);
    let DeferredInput::SetModifiers {
        shift,
        ctrl,
        alt,
        meta,
    } = inbox[0]
    else {
        panic!("expected SetModifiers variant, got {:?}", inbox[0]);
    };
    // Present keys carry through; absent keys default to released.
    assert!(shift, "shift present true");
    assert!(ctrl, "ctrl present true");
    assert!(!alt, "alt absent -> released");
    assert!(!meta, "meta absent -> released");
}

#[test]
fn scene_modifiers_empty_params_releases_all() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/modifiers","params":{},"id":7}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(inbox.len(), 1);
    assert_eq!(
        inbox[0],
        DeferredInput::SetModifiers {
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        },
        "empty params = all modifiers released (key-up reset)",
    );
}

#[test]
fn scene_modifiers_rejects_non_boolean() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/modifiers","params":{"shift":1},"id":7}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_some(), "non-boolean modifier must error");
    assert!(
        inbox.is_empty(),
        "no SetModifiers enqueued on malformed call"
    );
}

// ---- R695 §5.49 §5.35 — scene/hover (DeferredInput::Hover) ----

#[test]
fn scene_hover_enqueues_hover_at_coordinate() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/hover","params":{"at":{"x":64.0,"y":48.0}},"id":91}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Hover { x, y } = inbox[0] else {
        panic!("expected Hover variant, got {:?}", inbox[0]);
    };
    assert!((x - 64.0).abs() < f64::EPSILON);
    assert!((y - 48.0).abs() < f64::EPSILON);
}

#[test]
fn scene_hover_without_inbox_is_unavailable() {
    let mut scene = counted_scene(0);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/hover","params":{"at":{"x":0.0,"y":0.0}},"id":92}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("InputInjectionUnavailable"), "data: {data:?}");
}

#[test]
fn scene_hover_missing_at_is_invalid() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/hover","params":{},"id":93}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(inbox.is_empty());
}

#[test]
fn scene_click_v1_without_inbox_is_unavailable() {
    let mut scene = counted_scene(0);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/click","params":{"at":{"x":0.0,"y":0.0}},"id":10}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("InputInjectionUnavailable"), "data: {data:?}");
}

#[test]
fn scene_click_v1_missing_at_is_invalid() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{},"id":11}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(inbox.is_empty());
}

#[test]
fn scene_click_v1_at_missing_y_is_invalid() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"at":{"x":1.0}},"id":12}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("at.y"), "data: {data:?}");
}

#[test]
fn scene_rewind_writes_then_query_observes_new_value() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/count","value":123},"id":11}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert!(resp.error.is_none());

    let req2 =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":12}"#;
    let resp2 = parse_response(&dispatch_t(&mut scene, req2).unwrap());
    assert_eq!(resp2.result.unwrap(), Value::Number(123.into()));
}

#[test]
fn scene_rewind_missing_value_param_is_invalid() {
    let mut scene = counted_scene(0);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/count"},"id":13}"#;
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
fn scene_snapshot_container_wire_carries_tag_and_children_array() {
    use pinion_core::scene::ContainerNode;
    let mut scene = Scene::Container(ContainerNode::new(vec![counted_scene(7)]).with_tag("root"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/snapshot","params":{"path":""},"id":140}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result.get("type"), Some(&Value::String("Container".into())));
    assert_eq!(result.get("tag"), Some(&Value::String("root".into())));
    let children = result.get("children").unwrap().as_array().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(
        children[0].get("type"),
        Some(&Value::String("External".into())),
    );
    let intro = children[0].get("introspect").unwrap().as_object().unwrap();
    assert_eq!(intro.get("count"), Some(&Value::Number(7.into())));
}

#[test]
fn scene_snapshot_paint_mode_uses_producer_scene() {
    use pinion_core::scene::{ContainerNode, Rect, ScrollNode};
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut produce = |_w: u32, _h: u32| -> Scene {
        Scene::Container(
            ContainerNode::new(vec![Scene::Scroll(
                ScrollNode::new(
                    Rect::new(0, 0, 220, 164),
                    Scene::Container(ContainerNode::new(vec![]).with_tag("rows")),
                )
                .with_tag("main_list_scroll"),
            )])
            .with_tag("root"),
        )
    };
    let mut ctx =
        DispatchContext::new(&mut state, &previews, &revision).with_paint_producer(&mut produce);
    let req = r#"{"jsonrpc":"2.0","method":"scene/snapshot","params":{"path":"","from":"paint","viewport":{"w":360,"h":320}},"id":142}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result.get("type"), Some(&Value::String("Container".into())));
    assert_eq!(result.get("tag"), Some(&Value::String("root".into())));
    let scroll = &result.get("children").unwrap().as_array().unwrap()[0];
    assert_eq!(scroll.get("type"), Some(&Value::String("Scroll".into())));
    assert_eq!(
        scroll.get("tag"),
        Some(&Value::String("main_list_scroll".into())),
    );
}

#[test]
fn scene_snapshot_paint_mode_without_producer_is_unavailable() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/snapshot","params":{"path":"","from":"paint"},"id":143}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("PaintProducerUnavailable"), "data: {data:?}",);
}

// ---- R51.195 §5.49 §5.45 — scene/wheel injection ----

#[test]
fn scene_wheel_enqueues_lines_delta_into_inbox() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/wheel","params":{"at":{"x":180.0,"y":160.0},"delta":{"lines":{"dx":0.0,"dy":3.0}}},"id":200}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Wheel { x, y, delta } = inbox[0] else {
        panic!("expected Wheel variant, got {:?}", inbox[0]);
    };
    assert!((x - 180.0).abs() < f64::EPSILON);
    assert!((y - 160.0).abs() < f64::EPSILON);
    let WheelDelta::Lines { dx, dy } = delta else {
        panic!("expected Lines variant, got {delta:?}");
    };
    assert!(dx.abs() < f32::EPSILON);
    assert!((dy - 3.0).abs() < f32::EPSILON);
}

#[test]
fn scene_wheel_enqueues_pixels_delta_into_inbox() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/wheel","params":{"at":{"x":50.0,"y":40.0},"delta":{"pixels":{"dx":0.0,"dy":60.0}}},"id":201}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none());
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Wheel { delta, .. } = inbox[0] else {
        panic!("expected Wheel variant, got {:?}", inbox[0]);
    };
    let WheelDelta::Pixels { dx, dy } = delta else {
        panic!("expected Pixels variant, got {delta:?}");
    };
    assert!(dx.abs() < f32::EPSILON);
    assert!((dy - 60.0).abs() < f32::EPSILON);
}

#[test]
fn scene_wheel_without_inbox_is_unavailable() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/wheel","params":{"at":{"x":0.0,"y":0.0},"delta":{"lines":{"dx":0.0,"dy":1.0}}},"id":202}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("InputInjectionUnavailable"), "data: {data:?}");
}

#[test]
fn scene_wheel_missing_at_is_invalid() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/wheel","params":{"delta":{"lines":{"dx":0.0,"dy":1.0}}},"id":203}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("at"), "data: {data:?}");
    assert!(inbox.is_empty());
}

#[test]
fn scene_wheel_delta_with_both_lines_and_pixels_is_invalid() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/wheel","params":{"at":{"x":0.0,"y":0.0},"delta":{"lines":{"dx":0.0,"dy":1.0},"pixels":{"dx":0.0,"dy":60.0}}},"id":204}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("pick one"), "data: {data:?}");
}

#[test]
fn scene_wheel_delta_missing_both_is_invalid() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut scene, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/wheel","params":{"at":{"x":0.0,"y":0.0},"delta":{}},"id":205}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("requires"), "data: {data:?}");
}

#[test]
fn scene_snapshot_invalid_from_value_is_rejected() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/snapshot","params":{"path":"","from":"bogus"},"id":144}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("from"), "data: {data:?}");
}

#[test]
fn scene_snapshot_scroll_wire_carries_viewport_offset_tag_content() {
    use pinion_core::scene::{ContainerNode, Rect, ScrollNode};
    let inner = counted_scene(3);
    let mut scene = Scene::Scroll(
        ScrollNode::new(
            Rect::new(0, 0, 50, 80),
            Scene::Container(ContainerNode::new(vec![inner]).with_tag("rows")),
        )
        .with_tag("listbox_scroll")
        .with_offset(0, 60),
    );
    let req = r#"{"jsonrpc":"2.0","method":"scene/snapshot","params":{"path":""},"id":141}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result.get("type"), Some(&Value::String("Scroll".into())));
    assert_eq!(
        result.get("tag"),
        Some(&Value::String("listbox_scroll".into())),
    );
    let viewport = result.get("viewport").unwrap().as_object().unwrap();
    assert_eq!(viewport.get("w"), Some(&Value::Number(50.into())));
    assert_eq!(viewport.get("h"), Some(&Value::Number(80.into())));
    assert_eq!(result.get("offset_x"), Some(&Value::Number(0.into())));
    assert_eq!(result.get("offset_y"), Some(&Value::Number(60.into())));
    let content = result.get("content").unwrap().as_object().unwrap();
    assert_eq!(
        content.get("type"),
        Some(&Value::String("Container".into())),
    );
    assert_eq!(content.get("tag"), Some(&Value::String("rows".into())));
}

// ---- R51.198 §5.49 — leaf primitive rect / tag wire format ----

fn snapshot_rect_obj(node: &Value) -> &serde_json::Map<String, Value> {
    node.get("rect").unwrap().as_object().unwrap()
}

fn snapshot_request_root_state() -> &'static str {
    r#"{"jsonrpc":"2.0","method":"scene/snapshot","params":{"path":""},"id":300}"#
}

#[test]
fn scene_snapshot_box_wire_carries_rect_and_tag() {
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, Rect};
    let node = BoxNode::filled(Rect::new(10, 20, 30, 40), Color::default()).with_tag("box_tag");
    let mut scene = Scene::Box(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result.get("type"), Some(&Value::String("Box".into())));
    assert_eq!(result.get("tag"), Some(&Value::String("box_tag".into())));
    let rect = snapshot_rect_obj(&result);
    assert_eq!(rect.get("x"), Some(&Value::Number(10.into())));
    assert_eq!(rect.get("y"), Some(&Value::Number(20.into())));
    assert_eq!(rect.get("w"), Some(&Value::Number(30.into())));
    assert_eq!(rect.get("h"), Some(&Value::Number(40.into())));
}

#[test]
fn scene_snapshot_untagged_box_wire_reports_null_tag() {
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, Rect};
    let mut scene = Scene::Box(BoxNode::filled(Rect::new(0, 0, 1, 1), Color::default()));
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result.get("tag"), Some(&Value::Null));
}

#[test]
fn r55_g8_scene_snapshot_box_wire_carries_style_object() {
    // R55.G.8 §5.49 — BoxStyle (fill + border + corner_radius)
    // round-trips through the wire as a `{fill, border, corner_radius}`
    // JSON object. Border serializes nested with placement variant
    // string. AI clients can verify the painted chrome without OCR.
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, Rect};
    use pinion_core::style::{Border, BorderPlacement, BoxStyle};
    let mut node = BoxNode::filled(Rect::new(0, 0, 50, 50), Color::default());
    node.style = BoxStyle::filled(Color::rgba(0x11, 0x22, 0x33, 0xff))
        .with_border(
            Border::new(Color::rgba(0xaa, 0xbb, 0xcc, 0xff), 2)
                .with_placement(BorderPlacement::Outside),
        )
        .with_corner_radius(6);
    let mut scene = Scene::Box(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let style = result
        .get("style")
        .expect("style field present")
        .as_object()
        .unwrap();
    let fill = style.get("fill").unwrap().as_object().unwrap();
    assert_eq!(fill.get("r"), Some(&Value::Number(0x11.into())));
    assert_eq!(fill.get("g"), Some(&Value::Number(0x22.into())));
    assert_eq!(fill.get("b"), Some(&Value::Number(0x33.into())));
    assert_eq!(fill.get("a"), Some(&Value::Number(0xff.into())));
    assert_eq!(style.get("corner_radius"), Some(&Value::Number(6.into())));
    let border = style.get("border").unwrap().as_object().unwrap();
    assert_eq!(border.get("width"), Some(&Value::Number(2.into())));
    assert_eq!(
        border.get("placement"),
        Some(&Value::String("Outside".into()))
    );
}

#[test]
fn r55_g8_scene_snapshot_text_wire_carries_style_object() {
    // R55.G.8 §5.49 — TextStyle visual axis (family / size / colour /
    // weight / style) round-trips through the wire.
    use pinion_core::Color;
    use pinion_core::scene::{Rect, TextNode};
    use pinion_core::style::{FontStyle, FontWeight, TextStyle};
    let mut node = TextNode::new("hi", Rect::new(0, 0, 30, 16));
    node.style = TextStyle::new()
        .with_size_px(18)
        .with_fg(Color::rgba(0x44, 0x55, 0x66, 0xff))
        .with_weight(FontWeight::BOLD)
        .with_style(FontStyle::Italic);
    let mut scene = Scene::Text(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let style = result
        .get("style")
        .expect("style field present")
        .as_object()
        .unwrap();
    assert_eq!(style.get("font_size_px"), Some(&Value::Number(18.into())));
    assert_eq!(style.get("font_weight"), Some(&Value::Number(700.into())));
    assert_eq!(
        style.get("font_style"),
        Some(&Value::String("Italic".into()))
    );
    let fg = style.get("fg_color").unwrap().as_object().unwrap();
    assert_eq!(fg.get("r"), Some(&Value::Number(0x44.into())));
    assert_eq!(fg.get("a"), Some(&Value::Number(0xff.into())));
}

#[test]
fn r55_g8_scene_snapshot_text_oblique_wire_carries_angle() {
    // R55.G.8 §5.49 — FontStyle::Oblique(angle) emits a `{kind,
    // angle}` object so the optional CCW degrees survive the wire.
    use pinion_core::scene::{Rect, TextNode};
    use pinion_core::style::{FontStyle, TextStyle};
    let mut node = TextNode::new("h", Rect::default());
    node.style = TextStyle::new().with_style(FontStyle::Oblique(Some(12)));
    let mut scene = Scene::Text(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let style = result.get("style").unwrap().as_object().unwrap();
    let fs = style.get("font_style").unwrap().as_object().unwrap();
    assert_eq!(fs.get("kind"), Some(&Value::String("Oblique".into())));
    assert_eq!(fs.get("angle"), Some(&Value::Number(12.into())));
}

#[test]
fn r55_g11_scene_snapshot_path_wire_carries_style() {
    // R55.G.11 §5.49 — PathStyle (stroke + fill) round-trips
    // through the wire as `{stroke: {color, width, cap}, fill}`.
    use pinion_core::scene::{PathCommand, PathNode, PathPoint, Rect};
    use pinion_core::style::{PathStyle, Stroke, StrokeCap};
    let mut node = PathNode::new(
        Rect::new(0, 0, 50, 50),
        vec![PathCommand::MoveTo(PathPoint::new(0.0, 0.0))],
        PathStyle::default(),
    );
    node.style = PathStyle::stroked(
        Stroke::new(Color::rgba(0x10, 0x20, 0x30, 0xff), 5).with_cap(StrokeCap::Square),
    )
    .with_fill(Color::rgba(0xaa, 0xbb, 0xcc, 0xff));
    let mut scene = Scene::Path(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let style = result
        .get("style")
        .expect("style present")
        .as_object()
        .unwrap();
    let stroke = style.get("stroke").unwrap().as_object().unwrap();
    assert_eq!(stroke.get("width"), Some(&Value::Number(5.into())));
    assert_eq!(stroke.get("cap"), Some(&Value::String("Square".into())));
    let stroke_color = stroke.get("color").unwrap().as_object().unwrap();
    assert_eq!(stroke_color.get("r"), Some(&Value::Number(0x10.into())));
    let fill = style.get("fill").unwrap().as_object().unwrap();
    assert_eq!(fill.get("g"), Some(&Value::Number(0xbb.into())));
}

#[test]
fn r55_g11_scene_snapshot_path_wire_null_arms_when_absent() {
    // R55.G.11 §5.49 — `PathStyle::default()` carries `None`
    // stroke and `None` fill; the wire emits `null` for both
    // (no degenerate zero-channel colour or zero-width stroke).
    use pinion_core::scene::{PathCommand, PathNode, PathPoint, Rect};
    use pinion_core::style::PathStyle;
    let node = PathNode::new(
        Rect::new(0, 0, 1, 1),
        vec![PathCommand::MoveTo(PathPoint::new(0.0, 0.0))],
        PathStyle::default(),
    );
    let mut scene = Scene::Path(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let style = result.get("style").unwrap().as_object().unwrap();
    assert_eq!(style.get("stroke"), Some(&Value::Null));
    assert_eq!(style.get("fill"), Some(&Value::Null));
}

#[test]
fn r55_g11_scene_snapshot_image_wire_carries_style() {
    // R55.G.11 §5.49 — ImageStyle (fit + tint) round-trips through
    // the wire. Optional tint is `null` when absent.
    use pinion_core::scene::{ImageNode, Rect};
    use pinion_core::style::{Fit, ImageStyle};
    let mut node = ImageNode::new("asset://icon.png".to_string(), Rect::new(0, 0, 64, 64));
    node.style = ImageStyle::default()
        .with_fit(Fit::Cover)
        .with_tint(Color::rgba(0x44, 0x55, 0x66, 0xff));
    let mut scene = Scene::Image(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let style = result.get("style").unwrap().as_object().unwrap();
    assert_eq!(style.get("fit"), Some(&Value::String("Cover".into())));
    let tint = style.get("tint").unwrap().as_object().unwrap();
    assert_eq!(tint.get("r"), Some(&Value::Number(0x44.into())));
}

#[test]
fn r55_g11_scene_snapshot_image_wire_null_tint_when_absent() {
    // R55.G.11 §5.49 — `ImageStyle.tint = None` -> wire `null`.
    use pinion_core::scene::{ImageNode, Rect};
    let node = ImageNode::new("asset://".to_string(), Rect::new(0, 0, 1, 1));
    let mut scene = Scene::Image(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let style = result.get("style").unwrap().as_object().unwrap();
    assert_eq!(style.get("tint"), Some(&Value::Null));
    assert_eq!(style.get("fit"), Some(&Value::String("Fill".into())));
}

#[test]
fn r55_g10_scene_snapshot_text_wire_carries_layout_axis() {
    // R55.G.10 §5.49 — line_height / letter_spacing / text_align /
    // decoration / overflow round-trip through the wire alongside
    // the visual-axis fields landed by R55.G.8.
    use pinion_core::scene::{Rect, TextNode};
    use pinion_core::style::{LineHeight, TextAlign, TextDecoration, TextOverflow, TextStyle};
    let mut node = TextNode::new("hi", Rect::default());
    node.style = TextStyle::new()
        .with_line_height(LineHeight::MultiplierX100(150))
        .with_letter_spacing(2)
        .with_align(TextAlign::Center)
        .with_decoration(TextDecoration::underline())
        .with_overflow(TextOverflow::Ellipsis);
    let mut scene = Scene::Text(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let style = result.get("style").unwrap().as_object().unwrap();
    // line_height with data variant emits {kind, value}.
    let lh = style.get("line_height").unwrap().as_object().unwrap();
    assert_eq!(
        lh.get("kind"),
        Some(&Value::String("MultiplierX100".into())),
    );
    assert_eq!(lh.get("value"), Some(&Value::Number(150.into())));
    // letter_spacing is a bare signed integer.
    assert_eq!(style.get("letter_spacing"), Some(&Value::Number(2.into())));
    assert_eq!(
        style.get("text_align"),
        Some(&Value::String("Center".into())),
    );
    let dec = style.get("decoration").unwrap().as_object().unwrap();
    assert_eq!(dec.get("underline"), Some(&Value::Bool(true)));
    assert_eq!(dec.get("strikethrough"), Some(&Value::Bool(false)));
    assert_eq!(
        style.get("overflow"),
        Some(&Value::String("Ellipsis".into())),
    );
}

#[test]
fn r55_g10_scene_snapshot_text_wire_line_height_normal_bare_string() {
    // R55.G.10 §5.49 — `LineHeight::Normal` serializes as a bare
    // `"Normal"` string (no {kind,value} wrapper for unit variant).
    use pinion_core::scene::{Rect, TextNode};
    let node = TextNode::new("h", Rect::default());
    let mut scene = Scene::Text(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let style = result.get("style").unwrap().as_object().unwrap();
    assert_eq!(
        style.get("line_height"),
        Some(&Value::String("Normal".into())),
    );
}

#[test]
fn r55_g10_scene_snapshot_text_wire_line_height_px_variant() {
    // R55.G.10 §5.49 — `LineHeight::Px` data variant emits
    // `{kind: "Px", value: 24}`.
    use pinion_core::scene::{Rect, TextNode};
    use pinion_core::style::{LineHeight, TextStyle};
    let mut node = TextNode::new("h", Rect::default());
    node.style = TextStyle::new().with_line_height(LineHeight::Px(24));
    let mut scene = Scene::Text(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let style = result.get("style").unwrap().as_object().unwrap();
    let lh = style.get("line_height").unwrap().as_object().unwrap();
    assert_eq!(lh.get("kind"), Some(&Value::String("Px".into())));
    assert_eq!(lh.get("value"), Some(&Value::Number(24.into())));
}

#[test]
fn r55_g8_scene_snapshot_box_wire_null_border_when_absent() {
    // R55.G.8 §5.49 — `BoxStyle.border = None` serializes as JSON
    // `null` so wire consumers can distinguish "no border" from a
    // zero-width / transparent border.
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, Rect};
    let node = BoxNode::filled(Rect::new(0, 0, 10, 10), Color::default());
    let mut scene = Scene::Box(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let style = result.get("style").unwrap().as_object().unwrap();
    assert_eq!(style.get("border"), Some(&Value::Null));
}

#[test]
fn scene_snapshot_text_wire_carries_rect_tag_and_content() {
    use pinion_core::scene::{Rect, TextNode};
    let node = TextNode::new("Hello", Rect::new(5, 6, 50, 14)).with_tag("greeting");
    let mut scene = Scene::Text(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result.get("type"), Some(&Value::String("Text".into())));
    assert_eq!(result.get("tag"), Some(&Value::String("greeting".into())));
    assert_eq!(result.get("content"), Some(&Value::String("Hello".into())));
    let rect = snapshot_rect_obj(&result);
    assert_eq!(rect.get("w"), Some(&Value::Number(50.into())));
    assert_eq!(rect.get("h"), Some(&Value::Number(14.into())));
}

#[test]
fn scene_snapshot_path_wire_carries_rect_tag_and_commands() {
    use pinion_core::scene::{PathCommand, PathNode, PathPoint, Rect};
    use pinion_core::style::PathStyle;
    let node = PathNode::new(
        Rect::new(0, 0, 100, 100),
        vec![
            PathCommand::MoveTo(PathPoint::new(0.0, 0.0)),
            PathCommand::LineTo(PathPoint::new(50.0, 0.0)),
            PathCommand::CurveTo {
                c1: PathPoint::new(60.0, 0.0),
                c2: PathPoint::new(70.0, 10.0),
                end: PathPoint::new(70.0, 20.0),
            },
            PathCommand::Close,
        ],
        PathStyle::default(),
    )
    .with_tag("chevron");
    let mut scene = Scene::Path(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result.get("type"), Some(&Value::String("Path".into())));
    assert_eq!(result.get("tag"), Some(&Value::String("chevron".into())));
    let rect = snapshot_rect_obj(&result);
    assert_eq!(rect.get("w"), Some(&Value::Number(100.into())));
    // R51.198 carry — `commands` is serialized as an array of
    // tagged objects, one per `PathCommand` variant.
    let cmds = result.get("commands").unwrap().as_array().unwrap();
    assert_eq!(cmds.len(), 4);
    assert_eq!(cmds[0].get("type"), Some(&Value::String("MoveTo".into())),);
    assert_eq!(cmds[1].get("type"), Some(&Value::String("LineTo".into())),);
    assert_eq!(cmds[2].get("type"), Some(&Value::String("CurveTo".into())),);
    assert!(cmds[2].get("c1").is_some());
    assert!(cmds[2].get("c2").is_some());
    assert!(cmds[2].get("end").is_some());
    assert_eq!(cmds[3].get("type"), Some(&Value::String("Close".into())));
}

#[test]
fn scene_snapshot_image_wire_carries_rect_tag_and_source() {
    use pinion_core::scene::{ImageNode, Rect};
    let node = ImageNode::new("icon.png", Rect::new(8, 8, 16, 16)).with_tag("logo");
    let mut scene = Scene::Image(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result.get("type"), Some(&Value::String("Image".into())));
    assert_eq!(result.get("tag"), Some(&Value::String("logo".into())));
    assert_eq!(
        result.get("source"),
        Some(&Value::String("icon.png".into())),
    );
    let rect = snapshot_rect_obj(&result);
    assert_eq!(rect.get("x"), Some(&Value::Number(8.into())));
    assert_eq!(rect.get("w"), Some(&Value::Number(16.into())));
}

#[test]
fn scene_snapshot_container_wire_now_carries_rect() {
    use pinion_core::scene::{ContainerNode, Rect};
    let mut container = ContainerNode::new(vec![]).with_tag("root");
    container.rect = Rect::new(0, 0, 360, 220);
    let mut scene = Scene::Container(container);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result.get("type"), Some(&Value::String("Container".into())));
    assert_eq!(result.get("tag"), Some(&Value::String("root".into())));
    let rect = snapshot_rect_obj(&result);
    assert_eq!(rect.get("w"), Some(&Value::Number(360.into())));
    assert_eq!(rect.get("h"), Some(&Value::Number(220.into())));
}

#[test]
fn scene_snapshot_external_wire_now_carries_rect_and_tag() {
    use pinion_core::external::CountedExternal;
    use pinion_core::scene::{ExternalNode, Rect};
    let mut node = ExternalNode::new(Box::new(CountedExternal::new(7))).with_tag("main_toggle");
    node.rect = Rect::new(100, 50, 64, 32);
    let mut scene = Scene::External(node);
    let resp = parse_response(&dispatch_t(&mut scene, snapshot_request_root_state()).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result.get("type"), Some(&Value::String("External".into())));
    assert_eq!(
        result.get("tag"),
        Some(&Value::String("main_toggle".into()))
    );
    let rect = snapshot_rect_obj(&result);
    assert_eq!(rect.get("x"), Some(&Value::Number(100.into())));
    assert_eq!(rect.get("y"), Some(&Value::Number(50.into())));
    assert_eq!(rect.get("w"), Some(&Value::Number(64.into())));
    assert_eq!(rect.get("h"), Some(&Value::Number(32.into())));
    // The introspect dump still rides alongside rect / tag.
    let intro = result.get("introspect").unwrap().as_object().unwrap();
    assert_eq!(intro.get("count"), Some(&Value::Number(7.into())));
}

// ---- R55.F §5.45 — scene/scroll programmatic mutation ----

// R55.F §5.45 — `scene/scroll` needs a paint producer to walk
// because `Scene::Scroll` lives in the paint scene (V::view
// output). The shared fixture builds an empty state scene and
// a producer that returns a Scroll wrapping a `ScrollState`
// the caller can inspect.
fn scroll_test_state() -> std::rc::Rc<pinion_core::widgets::scroll::ScrollState> {
    use pinion_core::widgets::scroll::ScrollState;
    use std::rc::Rc;
    let state = Rc::new(ScrollState::new());
    state.set_max(0, 500);
    state
}

fn build_scroll_producer(
    tag: &'static str,
    state: std::rc::Rc<pinion_core::widgets::scroll::ScrollState>,
) -> impl FnMut(u32, u32) -> Scene {
    use pinion_core::scene::{ContainerNode, Rect, ScrollNode};
    use std::rc::Rc;
    move |_w: u32, _h: u32| -> Scene {
        let content = Scene::Container(ContainerNode::new(vec![]));
        let scroll = ScrollNode::new(Rect::new(0, 0, 220, 164), content)
            .with_tag(tag)
            .with_state(Rc::clone(&state));
        // R55.G.15: `Scene::scroll_state_at` descends through
        // Container children only when Container.rect contains the
        // coord — production layout fills this in, so the fixture
        // mirrors that by covering the Scroll's viewport rect.
        // Path-based lookup is unaffected (it walks children
        // without rect-checking the Container).
        let mut root = ContainerNode::new(vec![Scene::Scroll(scroll)]);
        root.rect = Rect::new(0, 0, 220, 164);
        Scene::Container(root)
    }
}

fn dispatch_scene_scroll(producer: &mut dyn FnMut(u32, u32) -> Scene, req: &str) -> Response {
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut ctx =
        DispatchContext::new(&mut state, &previews, &revision).with_paint_producer(producer);
    parse_response(&dispatch(&mut ctx, req).unwrap())
}

#[test]
fn scene_scroll_to_updates_state_offset() {
    let state = scroll_test_state();
    let mut produce = build_scroll_producer("main_list_scroll", state.clone());
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"path":"main_list_scroll","to":{"x":0,"y":80}},"id":900}"#;
    let resp = dispatch_scene_scroll(&mut produce, req);
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));
    assert_eq!(state.offset(), (0, 80));
}

#[test]
fn scene_scroll_by_adds_delta_to_state_offset() {
    let state = scroll_test_state();
    state.scroll_to(0, 100);
    let mut produce = build_scroll_producer("main_list_scroll", state.clone());
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"path":"main_list_scroll","by":{"dx":0,"dy":25}},"id":901}"#;
    let resp = dispatch_scene_scroll(&mut produce, req);
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(state.offset(), (0, 125));
}

#[test]
fn scene_scroll_clamps_to_bounds() {
    let state = scroll_test_state();
    let mut produce = build_scroll_producer("main_list_scroll", state.clone());
    // state.max_y = 500; request y = 9999 → clamped to 500.
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"path":"main_list_scroll","to":{"x":0,"y":9999}},"id":902}"#;
    let resp = dispatch_scene_scroll(&mut produce, req);
    assert!(resp.error.is_none());
    assert_eq!(state.offset(), (0, 500));
}

#[test]
fn scene_scroll_missing_tag_returns_invalid_params() {
    let state = scroll_test_state();
    let mut produce = build_scroll_producer("main_list_scroll", state);
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"path":"nonexistent","to":{"x":0,"y":0}},"id":903}"#;
    let resp = dispatch_scene_scroll(&mut produce, req);
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("not found"), "data: {data:?}");
}

#[test]
fn scene_scroll_to_and_by_together_is_invalid() {
    let state = scroll_test_state();
    let mut produce = build_scroll_producer("main_list_scroll", state);
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"path":"main_list_scroll","to":{"x":0,"y":0},"by":{"dx":0,"dy":0}},"id":904}"#;
    let resp = dispatch_scene_scroll(&mut produce, req);
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("mutually exclusive"), "data: {data:?}");
}

#[test]
fn scene_scroll_neither_to_nor_by_is_invalid() {
    let state = scroll_test_state();
    let mut produce = build_scroll_producer("main_list_scroll", state);
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"path":"main_list_scroll"},"id":905}"#;
    let resp = dispatch_scene_scroll(&mut produce, req);
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("requires"), "data: {data:?}");
}

#[test]
fn scene_scroll_without_paint_producer_is_unavailable() {
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut ctx = DispatchContext::new(&mut state, &previews, &revision);
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"path":"any","to":{"x":0,"y":0}},"id":906}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("PaintProducerUnavailable"), "data: {data:?}");
}

// ---- R55.G.15 §5.49 §5.45 — scene/scroll `at: {x, y}` variant ----
//
// Mirror of the click / wheel / key `at` shape so coord-based
// scroll mutation hits the same `Scene::scroll_state_at` substrate
// the InputRouter walks for wheel dispatch. The path-based tests
// above stay valid (R55.F default locator); these add the
// consistency-deviating coord locator R55.G.15 closes.

#[test]
fn scene_scroll_at_inside_viewport_resolves_to_attached_state() {
    // Scroll viewport = (0, 0, 220, 164); `at: {x:50, y:50}` lies
    // inside, so `Scene::scroll_state_at` returns the attached
    // state and `to: {x:0, y:80}` applies to that state.
    let state = scroll_test_state();
    let mut produce = build_scroll_producer("main_list_scroll", state.clone());
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"at":{"x":50,"y":50},"to":{"x":0,"y":80}},"id":910}"#;
    let resp = dispatch_scene_scroll(&mut produce, req);
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));
    assert_eq!(state.offset(), (0, 80));
}

#[test]
fn scene_scroll_at_outside_any_scroll_returns_invalid_params() {
    // `at: {x:300, y:300}` falls outside the (0,0,220,164)
    // viewport — `Scene::scroll_state_at` returns None and the
    // wire surfaces a typed "no Scroll" error rather than a
    // silent no-op.
    let state = scroll_test_state();
    let mut produce = build_scroll_producer("main_list_scroll", state);
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"at":{"x":300,"y":300},"to":{"x":0,"y":0}},"id":911}"#;
    let resp = dispatch_scene_scroll(&mut produce, req);
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("no Scroll"), "data: {data:?}");
    assert!(data.contains("(300, 300)"), "data: {data:?}");
}

#[test]
fn scene_scroll_at_and_path_together_is_invalid() {
    // Locator XOR — same shape as `to`/`by` mutual exclusion so
    // the AI client never has to guess which side wins.
    let state = scroll_test_state();
    let mut produce = build_scroll_producer("main_list_scroll", state);
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"path":"main_list_scroll","at":{"x":50,"y":50},"to":{"x":0,"y":0}},"id":912}"#;
    let resp = dispatch_scene_scroll(&mut produce, req);
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("mutually exclusive"), "data: {data:?}");
    assert!(
        data.contains("path") && data.contains("at"),
        "data: {data:?}"
    );
}

#[test]
fn scene_scroll_neither_path_nor_at_returns_invalid_params() {
    // No locator at all — wire fails with the "requires either"
    // shape before any paint producer call.
    let state = scroll_test_state();
    let mut produce = build_scroll_producer("main_list_scroll", state);
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"to":{"x":0,"y":0}},"id":913}"#;
    let resp = dispatch_scene_scroll(&mut produce, req);
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(
        data.contains("requires") && data.contains("path"),
        "data: {data:?}"
    );
    assert!(data.contains("at"), "data: {data:?}");
}

#[test]
fn scene_scroll_at_negative_coord_rejected_with_typed_error() {
    // Negative x → JSON i64 cannot try_from u32; the wire surfaces
    // the out-of-range error instead of silently wrapping to a
    // huge u32 that would never hit any Scroll.
    let state = scroll_test_state();
    let mut produce = build_scroll_producer("main_list_scroll", state);
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll","params":{"at":{"x":-1,"y":0},"to":{"x":0,"y":0}},"id":914}"#;
    let resp = dispatch_scene_scroll(&mut produce, req);
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("out of u32 range"), "data: {data:?}");
    assert!(data.contains("-1"), "data: {data:?}");
}

// ---- R51.200 §5.49 — nested-scroll absolute coord translation ----

#[test]
fn scene_click_path_inside_scroll_translates_to_absolute_coords() {
    // A row tagged inside a `Scene::Scroll.content` has a
    // scroll-local rect; the click target must be the window-
    // absolute rect (`viewport.{x,y} + row.{x,y} - offset`) so
    // the InputRouter hit-test lands on the right cell.
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, ScrollNode};
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut produce = |_w: u32, _h: u32| -> Scene {
        // Row at content-local (0, 34, 220, 28) inside a Scroll
        // at viewport (70, 78, 220, 164) with offset_y = 0.
        let row = Scene::Box(
            BoxNode::filled(Rect::new(0, 34, 220, 28), Color::default()).with_tag("main_list#1"),
        );
        let content = Scene::Container(ContainerNode::new(vec![row]).with_tag("rows"));
        let scroll =
            ScrollNode::new(Rect::new(70, 78, 220, 164), content).with_tag("main_list_scroll");
        Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]))
    };
    let mut ctx = DispatchContext::new(&mut state, &previews, &revision)
        .with_paint_producer(&mut produce)
        .with_deferred_inputs(&mut inbox);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"main_list#1"},"id":800}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Click { x, y } = inbox[0] else {
        panic!("expected Click variant");
    };
    // Absolute = (70 + 0 + 220/2, 78 + 34 + 28/2) = (180, 126).
    assert!((x - 180.0).abs() < f64::EPSILON, "click x: {x}");
    assert!((y - 126.0).abs() < f64::EPSILON, "click y: {y}");
}

#[test]
fn scene_click_path_inside_scroll_with_offset_subtracts_offset() {
    // Same shape but the scroll has `offset_y = 30`, so the
    // visible row position shifts up by 30 pixels.
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, ScrollNode};
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut produce = |_w: u32, _h: u32| -> Scene {
        let row = Scene::Box(
            BoxNode::filled(Rect::new(0, 100, 220, 28), Color::default()).with_tag("main_list#3"),
        );
        let content = Scene::Container(ContainerNode::new(vec![row]).with_tag("rows"));
        let scroll = ScrollNode::new(Rect::new(70, 78, 220, 164), content)
            .with_tag("main_list_scroll")
            .with_offset(0, 30);
        Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]))
    };
    let mut ctx = DispatchContext::new(&mut state, &previews, &revision)
        .with_paint_producer(&mut produce)
        .with_deferred_inputs(&mut inbox);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"main_list#3"},"id":801}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::Click { x, y } = inbox[0] else {
        panic!("expected Click variant");
    };
    // Absolute y = 78 + (100 - 30) + 28/2 = 78 + 70 + 14 = 162.
    assert!((x - 180.0).abs() < f64::EPSILON, "click x: {x}");
    assert!((y - 162.0).abs() < f64::EPSILON, "click y: {y}");
}

#[test]
fn r55_g7_scene_click_path_overwide_row_clips_to_viewport() {
    // R55.G.7 carry-of-R51.200 §5.49 — a content row whose intrinsic
    // width exceeds the Scroll viewport (e.g. a horizontally
    // scrollable table column header) used to return its full
    // un-clipped width, yielding a click centre far outside the
    // viewport. The new translate_and_clip step intersects the
    // row's window-abs rect with the viewport stack so the click
    // lands inside the visible 220-wide slice.
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, ScrollNode};
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut produce = |_w: u32, _h: u32| -> Scene {
        // Row is 800 wide (overflows viewport 220) at content-local
        // (0, 0, 800, 28) — viewport at (70, 78, 220, 164).
        let row = Scene::Box(
            BoxNode::filled(Rect::new(0, 0, 800, 28), Color::default()).with_tag("wide_row"),
        );
        let content = Scene::Container(ContainerNode::new(vec![row]));
        let scroll = ScrollNode::new(Rect::new(70, 78, 220, 164), content);
        Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]))
    };
    let mut ctx = DispatchContext::new(&mut state, &previews, &revision)
        .with_paint_producer(&mut produce)
        .with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"wide_row"},"id":810}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::Click { x, y } = inbox[0] else {
        panic!("expected Click variant");
    };
    // Visible portion = (70, 78, 220, 28). Centre = (70 + 110, 78 + 14) = (180, 92).
    // Pre-R55.G.7 the centre would have been at x = 70 + 400 = 470,
    // way outside the 220-wide viewport.
    assert!((x - 180.0).abs() < f64::EPSILON, "click x: {x}");
    assert!((y - 92.0).abs() < f64::EPSILON, "click y: {y}");
}

#[test]
fn r55_g7_scene_click_path_partially_scrolled_off_row_clips_to_visible() {
    // R55.G.7 carry-of-R51.200 — row that bleeds past the
    // viewport's bottom edge (height extends beyond clip): the
    // returned rect now matches only the visible portion.
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, ScrollNode};
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut produce = |_w: u32, _h: u32| -> Scene {
        // 100-tall row at content-local (0, 0, 220, 100), viewport
        // height = 50, offset = 0 → bottom half is clipped.
        let row = Scene::Box(
            BoxNode::filled(Rect::new(0, 0, 220, 100), Color::default()).with_tag("tall_row"),
        );
        let content = Scene::Container(ContainerNode::new(vec![row]));
        let scroll = ScrollNode::new(Rect::new(0, 0, 220, 50), content);
        Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]))
    };
    let mut ctx = DispatchContext::new(&mut state, &previews, &revision)
        .with_paint_producer(&mut produce)
        .with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"tall_row"},"id":811}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let DeferredInput::Click { x, y } = inbox[0] else {
        panic!("expected Click variant");
    };
    // Visible (0, 0, 220, 50) → centre (110, 25). Pre-fix would be
    // centre.y = 50 (row's geometric centre, outside viewport).
    assert!((x - 110.0).abs() < f64::EPSILON, "click x: {x}");
    assert!((y - 25.0).abs() < f64::EPSILON, "click y: {y}");
}

#[test]
fn r55_g7_scene_click_path_fully_scrolled_off_row_returns_not_found() {
    // R55.G.7 carry-of-R51.200 — row completely past the viewport
    // edge (scrolled off) no longer returns a degenerate (0,0)
    // saturation; the upstream handler surfaces "tag not found".
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, ScrollNode};
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut produce = |_w: u32, _h: u32| -> Scene {
        // Row at content-local (0, 500, 220, 28) — far below
        // viewport of (0, 0, 220, 100) with offset 0.
        let row = Scene::Box(
            BoxNode::filled(Rect::new(0, 500, 220, 28), Color::default()).with_tag("offscreen_row"),
        );
        let content = Scene::Container(ContainerNode::new(vec![row]));
        let scroll = ScrollNode::new(Rect::new(0, 0, 220, 100), content);
        Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]))
    };
    let mut ctx = DispatchContext::new(&mut state, &previews, &revision)
        .with_paint_producer(&mut produce)
        .with_deferred_inputs(&mut inbox);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"offscreen_row"},"id":812}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    // Pre-R55.G.7 this returned success with click coords saturated
    // to (110, 0) — a phantom click at the window top. Now the
    // handler explicitly fails: tag is not visible.
    assert!(
        resp.error.is_some(),
        "expected error for fully-scrolled-off tag"
    );
    assert_eq!(inbox.len(), 0, "no phantom click enqueued");
}

// ---- R51.202 §5.49 — path-based scene/wheel + scene/key ----

#[test]
fn scene_wheel_path_resolves_to_tag_rect_center() {
    use pinion_core::scene::{ContainerNode, Rect, ScrollNode};
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut produce = |_w: u32, _h: u32| -> Scene {
        let scroll = ScrollNode::new(
            Rect::new(70, 78, 220, 164),
            Scene::Container(ContainerNode::new(vec![])),
        )
        .with_tag("main_list_scroll");
        Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]))
    };
    let mut ctx = DispatchContext::new(&mut state, &previews, &revision)
        .with_paint_producer(&mut produce)
        .with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/wheel","params":{"path":"main_list_scroll","delta":{"lines":{"dx":0.0,"dy":3.0}}},"id":700}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Wheel { x, y, .. } = inbox[0] else {
        panic!("expected Wheel variant");
    };
    // viewport (70, 78, 220, 164) → centre (180, 160).
    assert!((x - 180.0).abs() < f64::EPSILON, "wheel x: {x}");
    assert!((y - 160.0).abs() < f64::EPSILON, "wheel y: {y}");
}

#[test]
fn scene_key_path_resolves_to_tag_rect_center() {
    use pinion_core::scene::{ContainerNode, Rect, ScrollNode};
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut produce = |_w: u32, _h: u32| -> Scene {
        let scroll = ScrollNode::new(
            Rect::new(0, 0, 220, 164),
            Scene::Container(ContainerNode::new(vec![])),
        )
        .with_tag("main_list_scroll");
        Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]))
    };
    let mut ctx = DispatchContext::new(&mut state, &previews, &revision)
        .with_paint_producer(&mut produce)
        .with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"path":"main_list_scroll","key":"PageDown"},"id":701}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Key {
        x,
        y,
        ref key,
        state: _,
    } = inbox[0]
    else {
        panic!("expected Key variant");
    };
    assert_eq!(key, "PageDown");
    // viewport (0, 0, 220, 164) → centre (110, 82).
    assert!((x - 110.0).abs() < f64::EPSILON, "key x: {x}");
    assert!((y - 82.0).abs() < f64::EPSILON, "key y: {y}");
}

// ---- R51.201 §5.49 — path-based scene/click ----

#[test]
fn scene_click_path_resolves_to_tag_rect_center() {
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, ContainerNode};
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    // Paint scene contains a tagged Container at (100, 50, 64, 32).
    let mut produce = |_w: u32, _h: u32| -> Scene {
        let inner = Scene::Box(
            BoxNode::filled(
                pinion_core::scene::Rect::new(100, 50, 64, 32),
                Color::default(),
            )
            .with_tag("main_toggle"),
        );
        let mut outer = ContainerNode::new(vec![inner]);
        outer.rect = pinion_core::scene::Rect::new(0, 0, 360, 220);
        Scene::Container(outer)
    };
    let mut ctx = DispatchContext::new(&mut state, &previews, &revision)
        .with_paint_producer(&mut produce)
        .with_deferred_inputs(&mut inbox);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"main_toggle"},"id":600}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert_eq!(inbox.len(), 1);
    let DeferredInput::Click { x, y } = inbox[0] else {
        panic!("expected Click variant");
    };
    // (100 + 64/2, 50 + 32/2) = (132, 66).
    assert!((x - 132.0).abs() < f64::EPSILON, "click x: {x}");
    assert!((y - 66.0).abs() < f64::EPSILON, "click y: {y}");
}

#[test]
fn scene_click_path_without_paint_producer_is_unavailable() {
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    // No paint producer registered.
    let mut ctx =
        DispatchContext::new(&mut state, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"any_tag"},"id":601}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("PaintProducerUnavailable"), "data: {data:?}");
    assert!(inbox.is_empty());
}

#[test]
fn scene_click_path_missing_tag_returns_invalid_params() {
    use pinion_core::scene::ContainerNode;
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut produce = |_w: u32, _h: u32| -> Scene {
        Scene::Container(ContainerNode::new(vec![]).with_tag("root"))
    };
    let mut ctx = DispatchContext::new(&mut state, &previews, &revision)
        .with_paint_producer(&mut produce)
        .with_deferred_inputs(&mut inbox);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/click","params":{"path":"nonexistent"},"id":602}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("not found"), "data: {data:?}");
    assert!(inbox.is_empty());
}

#[test]
fn scene_click_at_and_path_together_is_invalid() {
    use pinion_core::scene::ContainerNode;
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut produce = |_w: u32, _h: u32| -> Scene { Scene::Container(ContainerNode::new(vec![])) };
    let mut ctx = DispatchContext::new(&mut state, &previews, &revision)
        .with_paint_producer(&mut produce)
        .with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"at":{"x":0.0,"y":0.0},"path":"x"},"id":603}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("mutually exclusive"), "data: {data:?}");
}

#[test]
fn scene_click_neither_at_nor_path_is_invalid() {
    let mut state = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut inbox: Vec<DeferredInput> = Vec::new();
    let mut ctx =
        DispatchContext::new(&mut state, &previews, &revision).with_deferred_inputs(&mut inbox);
    let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{},"id":604}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    let data = err.data.as_ref().and_then(Value::as_str).unwrap_or("");
    assert!(data.contains("requires"), "data: {data:?}");
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
    let q_req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":17}"#;
    let q_resp = parse_response(&dispatch_t(&mut scene, q_req).unwrap());
    assert_eq!(q_resp.result.unwrap(), Value::Number(3.into()));
}

#[test]
fn scene_dry_run_missing_value_param_is_invalid() {
    let mut scene = counted_scene(0);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/dry_run","params":{"path":"/external/count"},"id":18}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
}

#[test]
fn scene_simulate_two_steps_returns_compound_snapshot_and_rolls_back() {
    // R646 §5.12 — AI-native "if I do A then B" scenario explorer.
    // Two steps on the same path; snapshot reflects the LATEST value
    // (final step in the sequence); rollback restores the pre-call
    // original (per-unique-path save semantics).
    let mut scene = counted_scene(5);
    let req = r#"{"jsonrpc":"2.0","method":"scene/simulate","params":{"steps":[{"path":"/external/count","value":42},{"path":"/external/count","value":999}]},"id":160}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert!(resp.error.is_none(), "unexpected error: {:?}", resp.error);
    let result = resp.result.unwrap();
    let intro = result.get("introspect").unwrap().as_object().unwrap();
    assert_eq!(intro.get("count"), Some(&Value::Number(999.into())));

    // Pre-call value (5) restored, not the intermediate (42).
    let q_req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":161}"#;
    let q_resp = parse_response(&dispatch_t(&mut scene, q_req).unwrap());
    assert_eq!(q_resp.result.unwrap(), Value::Number(5.into()));
}

#[test]
fn scene_simulate_empty_steps_array_is_invalid() {
    // EmptySteps surfaces at the JSON-RPC boundary before the
    // simulate() core ever sees the call; the dispatcher rejects
    // empty arrays to keep the typed `EmptySteps` error path internal.
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/simulate","params":{"steps":[]},"id":162}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
}

#[test]
fn scene_simulate_missing_steps_param_is_invalid() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/simulate","params":{},"id":163}"#;
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

#[test]
fn r1060_scene_screenshot_returns_embedder_captured_pixels() {
    // R1060 §5.12 §5.16 — when the AppShell windowed entry pre-captured
    // the live presented surface (via `VelloRenderer::capture_rgba8`),
    // the handler returns those exact pixels instead of the v0
    // `RenderBackendUnavailable` stub. A 2x1 RGBA frame stands in for the
    // real swapchain readback (no GPU in this unit test — the live
    // readback itself is realgpu-verified, like the demo sweep). The
    // absent-snapshot path stays `RenderBackendUnavailable`, pinned by
    // `scene_screenshot_returns_render_backend_unavailable_tag` above.
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let pixels = vec![10, 20, 30, 255, 40, 50, 60, 255];
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision)
        .with_screenshot(crate::screenshot::Screenshot::new(2, 1, pixels.clone()));
    let req = r#"{"jsonrpc":"2.0","method":"scene/screenshot","params":{"path":""},"id":23}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "a captured frame must not error");
    let result = resp.result.expect("result present");
    assert_eq!(result["width"].as_u64(), Some(2));
    assert_eq!(result["height"].as_u64(), Some(1));
    let got: Vec<u8> = result["pixels_rgba8"]
        .as_array()
        .expect("pixels_rgba8 is an array")
        .iter()
        .map(|v| u8::try_from(v.as_u64().unwrap()).unwrap())
        .collect();
    assert_eq!(got, pixels, "wire pixels round-trip the captured frame");
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
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/state"},"id":42}"#;
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
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/nope"},"id":43}"#;
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
    let rewind_req = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/count","value":11},"id":61}"#;
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
    assert_eq!(err.data, Some(Value::String("InvokeRejected".to_string())));
}

// ---- §5.32 R39.1: scene/locate JSON-RPC wire ----

fn box_scene(x: u32, y: u32, w: u32, h: u32) -> Scene {
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, Rect};
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
    let bbox = obj
        .get("bbox")
        .and_then(Value::as_object)
        .expect("bbox object");
    assert_eq!(bbox.get("x"), Some(&Value::Number(10.into())));
    assert_eq!(bbox.get("y"), Some(&Value::Number(20.into())));
    assert_eq!(bbox.get("w"), Some(&Value::Number(50.into())));
    assert_eq!(bbox.get("h"), Some(&Value::Number(30.into())));
    let ancestors = obj
        .get("ancestors")
        .and_then(Value::as_array)
        .expect("ancestors array");
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
    let paths = result
        .get("paths")
        .and_then(Value::as_array)
        .expect("paths");
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
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/bbox","params":{"path":"/window[main]"},"id":107}"#;
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
    fn apply(&self, _ctx: &mut crate::preview::ApplyContext<'_>) -> Result<(), String> {
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
    let resp = parse_response(
        &dispatch_full(&mut scene, &previews, &SceneRevision::default(), &req).unwrap(),
    );
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
    let resp = parse_response(
        &dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap(),
    );
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
    let first = parse_response(
        &dispatch_full(&mut scene, &previews, &SceneRevision::default(), &req).unwrap(),
    );
    assert_eq!(
        first.result.unwrap().get("cancelled"),
        Some(&Value::Bool(true))
    );
    let second = parse_response(
        &dispatch_full(&mut scene, &previews, &SceneRevision::default(), &req).unwrap(),
    );
    assert_eq!(
        second.result.unwrap().get("cancelled"),
        Some(&Value::Bool(false))
    );
}

#[test]
fn scene_cancel_preview_missing_id_is_invalid_params() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let req = r#"{"jsonrpc":"2.0","method":"scene/cancel_preview","params":{},"id":204}"#;
    let resp = parse_response(
        &dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap(),
    );
    assert_eq!(resp.error.unwrap().code, -32602);
}

#[test]
fn scene_cancel_preview_zero_id_is_invalid_params() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/cancel_preview","params":{"preview_id":0},"id":205}"#;
    let resp = parse_response(
        &dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap(),
    );
    assert_eq!(resp.error.unwrap().code, -32602);
}

#[test]
fn scene_cancel_preview_string_id_is_invalid_params() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let req = r#"{"jsonrpc":"2.0","method":"scene/cancel_preview","params":{"preview_id":"abc"},"id":206}"#;
    let resp = parse_response(
        &dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap(),
    );
    assert_eq!(resp.error.unwrap().code, -32602);
}

// ---- R40.3: scene/list_previews JSON-RPC wire ----

#[test]
fn scene_list_previews_empty_ledger_returns_empty_array() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let req = r#"{"jsonrpc":"2.0","method":"scene/list_previews","id":301}"#;
    let resp = parse_response(
        &dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap(),
    );
    assert!(resp.error.is_none());
    let arr = resp.result.unwrap().get("previews").cloned().unwrap();
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
    let resp = parse_response(
        &dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap(),
    );
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
    assert!(
        obj.get("ttl_remaining_ms")
            .and_then(Value::as_u64)
            .is_some()
    );
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
    let resp = parse_response(
        &dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap(),
    );
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
    let resp = parse_response(
        &dispatch_full(&mut scene, &previews, &SceneRevision::default(), req).unwrap(),
    );
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
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":403}"#;
    let _ = dispatch_full(&mut scene, &previews, &revision, req);
    assert_eq!(revision.current(), 0, "scene/query is read-only");
}

#[test]
fn dispatch_does_not_bump_revision_on_preview_lifecycle_methods() {
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    // cancel_preview touches the ledger but not the scene tree.
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/cancel_preview","params":{"preview_id":1},"id":404}"#;
    let _ = dispatch_full(&mut scene, &previews, &revision, req);
    assert_eq!(
        revision.current(),
        0,
        "preview lifecycle does not bump scene revision"
    );
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
    assert!(
        apply_resp.error.is_none(),
        "unexpected error: {:?}",
        apply_resp.error
    );
    let obj = apply_resp.result.unwrap();
    assert_eq!(
        obj.get("preview_id"),
        Some(&Value::Number(preview_id.into()))
    );
    assert_eq!(obj.get("new_revision"), Some(&Value::Number(1.into())));

    // 3) query confirms scene now reflects the apply.
    let query_req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":603}"#;
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
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/apply_preview","params":{"preview_id":9999},"id":604}"#;
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
    let preview_id =
        parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap())
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
    let preview_id =
        parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap())
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
    let preview_id =
        parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap())
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
    let preview_id =
        parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap())
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
    let preview_id =
        parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap())
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
    let preview_id =
        parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap())
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
    let preview_id =
        parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap())
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

// ---- R43: ViewBlueprint Text/Path/Image variants + closed kinds ----

#[test]
fn scene_propose_replace_view_with_text_round_trips() {
    let mut scene = box_container_scene_for_set_style();
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"ReplaceView","target_path":"/btn","replacement":{"kind":"Text","content":"Save","rect":{"x":0,"y":0,"w":40,"h":20},"style":{"font_size_px":18,"fg_color":255},"tag":"label"}},"id":740}"#;
    let preview_id =
        parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap())
            .result
            .unwrap()
            .get("preview_id")
            .and_then(Value::as_u64)
            .unwrap();
    let apply_req = format!(
        r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":741}}"#
    );
    let _ = dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap();
    if let Scene::Container(c) = &scene {
        if let Scene::Text(t) = &c.children[0] {
            assert_eq!(t.content, "Save");
            assert_eq!(t.style.font_size_px, 18);
            assert_eq!(t.tag.as_deref(), Some("label"));
        } else {
            panic!("child 0 not Text");
        }
    }
}

#[test]
fn scene_propose_replace_view_with_path_round_trips() {
    let mut scene = box_container_scene_for_set_style();
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"ReplaceView","target_path":"/btn","replacement":{"kind":"Path","rect":{"x":0,"y":0,"w":32,"h":32},"style":{"fill":255},"commands":[{"op":"MoveTo","point":{"x":0,"y":0}},{"op":"LineTo","point":{"x":10,"y":10}},{"op":"Close"}],"tag":"logo"}},"id":742}"#;
    let preview_id =
        parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap())
            .result
            .unwrap()
            .get("preview_id")
            .and_then(Value::as_u64)
            .unwrap();
    let apply_req = format!(
        r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":743}}"#
    );
    let _ = dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap();
    if let Scene::Container(c) = &scene {
        if let Scene::Path(p) = &c.children[0] {
            assert_eq!(p.commands.len(), 3);
            assert_eq!(p.tag.as_deref(), Some("logo"));
        } else {
            panic!("child 0 not Path");
        }
    }
}

#[test]
fn scene_propose_replace_view_with_image_round_trips() {
    let mut scene = box_container_scene_for_set_style();
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"ReplaceView","target_path":"/btn","replacement":{"kind":"Image","source":"file:///tmp/icon.png","rect":{"x":0,"y":0,"w":24,"h":24},"style":{"fit":"Contain"},"tag":"avatar"}},"id":744}"#;
    let preview_id =
        parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap())
            .result
            .unwrap()
            .get("preview_id")
            .and_then(Value::as_u64)
            .unwrap();
    let apply_req = format!(
        r#"{{"jsonrpc":"2.0","method":"scene/apply_preview","params":{{"preview_id":{preview_id}}},"id":745}}"#
    );
    let _ = dispatch_full(&mut scene, &previews, &revision, &apply_req).unwrap();
    if let Scene::Container(c) = &scene {
        if let Scene::Image(i) = &c.children[0] {
            assert_eq!(i.source, "file:///tmp/icon.png");
            assert_eq!(i.style.fit, pinion_core::style::Fit::Contain);
            assert_eq!(i.tag.as_deref(), Some("avatar"));
        } else {
            panic!("child 0 not Image");
        }
    }
}

#[test]
fn scene_propose_replace_view_effect_kind_rejected() {
    let mut scene = box_container_scene_for_set_style();
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"ReplaceView","target_path":"/btn","replacement":{"kind":"Effect","rect":{"x":0,"y":0,"w":1,"h":1}}},"id":746}"#;
    let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(
        err.data
            .as_ref()
            .and_then(|d| d.as_str())
            .is_some_and(|s| s.contains("Effect")),
        "error data must mention rejected Effect kind: {:?}",
        err.data
    );
}

#[test]
fn scene_propose_replace_view_external_kind_rejected() {
    let mut scene = box_container_scene_for_set_style();
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"ReplaceView","target_path":"/btn","replacement":{"kind":"External","rect":{"x":0,"y":0,"w":1,"h":1}}},"id":747}"#;
    let resp = parse_response(&dispatch_full(&mut scene, &previews, &revision, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(
        err.data
            .as_ref()
            .and_then(|d| d.as_str())
            .is_some_and(|s| s.contains("External")),
        "error data must mention rejected External kind: {:?}",
        err.data
    );
}

#[test]
fn scene_propose_replace_view_unknown_target_surfaces_apply_rejected() {
    let mut scene = box_container_scene_for_set_style();
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let propose_req = r#"{"jsonrpc":"2.0","method":"scene/propose_change","params":{"kind":"ReplaceView","target_path":"/ghost","replacement":{"kind":"Box","rect":{"x":0,"y":0,"w":1,"h":1},"style":{"fill":0}}},"id":726}"#;
    let preview_id =
        parse_response(&dispatch_full(&mut scene, &previews, &revision, propose_req).unwrap())
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

// --- R50.X.1 §5.37.2 font/* RPC routing tests ---

const NOTO_SANS_FONT: &[u8] =
    include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");

/// Build a JSON array literal of `bytes` for embedding in a
/// `font/parse` request body.
fn bytes_as_json_array(bytes: &[u8]) -> String {
    let mut s = String::from("[");
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&b.to_string());
    }
    s.push(']');
    s
}

/// Dispatch helper with a font registry attached (R50.X.1 §5.37.2).
fn dispatch_with_font(scene: &mut Scene, registry: &FontRegistry, req: &str) -> Option<String> {
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut ctx = DispatchContext::new(scene, &previews, &revision).with_font_registry(registry);
    dispatch(&mut ctx, req)
}

#[test]
fn font_parse_without_registry_returns_registry_unavailable() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"font/parse","params":{"bytes":[0]},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32603);
    assert_eq!(
        err.data.as_ref().and_then(|d| d.as_str()),
        Some("FontRegistryUnavailable"),
    );
}

#[test]
fn font_parse_noto_sans_returns_font_id() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/parse","params":{{"bytes":{bytes}}},"id":1}}"#,
        bytes = bytes_as_json_array(NOTO_SANS_FONT),
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &body).unwrap());
    let font_id = resp
        .result
        .unwrap()
        .get("font_id")
        .and_then(Value::as_u64)
        .expect("font_id present in result");
    assert!(font_id >= 1, "font_id starts at 1, got {font_id}");
    assert_eq!(registry.len(), 1);
}

#[test]
fn font_parse_rejects_non_byte_in_array() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let req = r#"{"jsonrpc":"2.0","method":"font/parse","params":{"bytes":[0,1,256]},"id":1}"#;
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(
        err.data
            .as_ref()
            .and_then(|d| d.as_str())
            .is_some_and(|s| s.contains("out of u8 range")),
        "expected u8 range message, got {:?}",
        err.data,
    );
}

#[test]
fn font_parse_rejects_empty_bytes() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let req = r#"{"jsonrpc":"2.0","method":"font/parse","params":{"bytes":[]},"id":1}"#;
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert_eq!(err.data.as_ref().and_then(|d| d.as_str()), Some("Parse"),);
}

#[test]
fn font_family_name_round_trip_noto_sans() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let parse_body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/parse","params":{{"bytes":{bytes}}},"id":1}}"#,
        bytes = bytes_as_json_array(NOTO_SANS_FONT),
    );
    let parsed = parse_response(&dispatch_with_font(&mut scene, &registry, &parse_body).unwrap());
    let font_id = parsed
        .result
        .unwrap()
        .get("font_id")
        .and_then(Value::as_u64)
        .unwrap();
    let family_body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/family_name","params":{{"font_id":{font_id}}},"id":2}}"#,
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &family_body).unwrap());
    let name = resp
        .result
        .unwrap()
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap();
    assert_eq!(name, "Noto Sans");
}

#[test]
fn font_family_name_rejects_zero_id() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let req = r#"{"jsonrpc":"2.0","method":"font/family_name","params":{"font_id":0},"id":1}"#;
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert_eq!(err.data.as_ref().and_then(|d| d.as_str()), Some("NotFound"),);
}

#[test]
fn font_glyph_id_for_letter_a_round_trip() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let parse_body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/parse","params":{{"bytes":{bytes}}},"id":1}}"#,
        bytes = bytes_as_json_array(NOTO_SANS_FONT),
    );
    let font_id = parse_response(&dispatch_with_font(&mut scene, &registry, &parse_body).unwrap())
        .result
        .unwrap()
        .get("font_id")
        .and_then(Value::as_u64)
        .unwrap();
    let glyph_body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/glyph_id_for","params":{{"font_id":{font_id},"codepoint":65}},"id":2}}"#,
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &glyph_body).unwrap());
    let gid = resp
        .result
        .unwrap()
        .get("glyph_id")
        .and_then(Value::as_u64)
        .expect("glyph_id present");
    assert_ne!(gid, 0, "'A' must not fall back to .notdef");
}

#[test]
fn font_glyph_id_for_codepoint_out_of_u32_range() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let req = r#"{"jsonrpc":"2.0","method":"font/glyph_id_for","params":{"font_id":1,"codepoint":4294967296},"id":1}"#;
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(
        err.data
            .as_ref()
            .and_then(|d| d.as_str())
            .is_some_and(|s| s.contains("codepoint")),
        "expected codepoint range error, got {:?}",
        err.data,
    );
}

// --- R50.X.2 §5.37.2 extended method E2E tests ---

fn parse_noto_sans_via_dispatch(scene: &mut Scene, registry: &FontRegistry) -> u64 {
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/parse","params":{{"bytes":{bytes}}},"id":1}}"#,
        bytes = bytes_as_json_array(NOTO_SANS_FONT),
    );
    parse_response(&dispatch_with_font(scene, registry, &body).unwrap())
        .result
        .unwrap()
        .get("font_id")
        .and_then(Value::as_u64)
        .unwrap()
}

#[test]
fn font_glyph_outline_notdef_is_simple_kind() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let font_id = parse_noto_sans_via_dispatch(&mut scene, &registry);
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/glyph_outline","params":{{"font_id":{font_id},"glyph_id":0}},"id":2}}"#,
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &body).unwrap());
    let result = resp.result.unwrap();
    assert_eq!(
        result.get("kind").and_then(Value::as_str),
        Some("Simple"),
        "Noto Sans .notdef expected Simple kind, got {result:?}",
    );
    // Simple variant must carry points + header.
    assert!(result.get("header").is_some());
    assert!(result.get("points").is_some());
}

#[test]
fn font_glyph_outline_rejects_glyph_id_overflow() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let font_id = parse_noto_sans_via_dispatch(&mut scene, &registry);
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/glyph_outline","params":{{"font_id":{font_id},"glyph_id":65536}},"id":3}}"#,
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &body).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(
        err.data
            .as_ref()
            .and_then(|d| d.as_str())
            .is_some_and(|s| s.contains("u16 range")),
        "expected u16 range message, got {:?}",
        err.data,
    );
}

#[test]
fn font_glyph_outline_rejects_out_of_range_glyph_id() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let font_id = parse_noto_sans_via_dispatch(&mut scene, &registry);
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/glyph_outline","params":{{"font_id":{font_id},"glyph_id":65535}},"id":4}}"#,
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &body).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert_eq!(
        err.data.as_ref().and_then(|d| d.as_str()),
        Some("GlyphIdOutOfRange"),
    );
}

#[test]
fn font_cmap_subtables_round_trip_noto_sans() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let font_id = parse_noto_sans_via_dispatch(&mut scene, &registry);
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/cmap_subtables","params":{{"font_id":{font_id}}},"id":5}}"#,
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &body).unwrap());
    let result = resp.result.unwrap();
    let subtables = result.get("subtables").and_then(Value::as_array).unwrap();
    assert!(!subtables.is_empty());
    // Every subtable row carries platform_id, encoding_id, format, supported.
    for row in subtables {
        assert!(row.get("platform_id").is_some());
        assert!(row.get("encoding_id").is_some());
        assert!(row.get("format").is_some());
        assert!(row.get("supported").is_some());
    }
}

#[test]
fn font_metrics_round_trip_noto_sans() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let font_id = parse_noto_sans_via_dispatch(&mut scene, &registry);
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/metrics","params":{{"font_id":{font_id}}},"id":6}}"#,
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &body).unwrap());
    let m = resp.result.unwrap();
    assert_eq!(m.get("units_per_em").and_then(Value::as_u64), Some(1000));
    assert_eq!(m.get("weight_class").and_then(Value::as_u64), Some(400));
    assert_eq!(m.get("is_monospace").and_then(Value::as_bool), Some(false));
}

#[test]
fn font_subfamily_name_round_trip_regular() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let font_id = parse_noto_sans_via_dispatch(&mut scene, &registry);
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/subfamily_name","params":{{"font_id":{font_id}}},"id":7}}"#,
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &body).unwrap());
    assert_eq!(
        resp.result.unwrap().get("name").and_then(Value::as_str),
        Some("Regular"),
    );
}

#[test]
fn font_full_name_round_trip_contains_family() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let font_id = parse_noto_sans_via_dispatch(&mut scene, &registry);
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/full_name","params":{{"font_id":{font_id}}},"id":8}}"#,
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &body).unwrap());
    let name = resp
        .result
        .unwrap()
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap();
    assert!(name.contains("Noto Sans"), "full name = {name}");
}

#[test]
fn font_postscript_name_round_trip_starts_with_notosans() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let font_id = parse_noto_sans_via_dispatch(&mut scene, &registry);
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/postscript_name","params":{{"font_id":{font_id}}},"id":9}}"#,
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &body).unwrap());
    let name = resp
        .result
        .unwrap()
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap();
    assert!(name.starts_with("NotoSans"), "postscript name = {name}");
}

#[test]
fn font_extended_methods_without_registry_return_registry_unavailable() {
    for method in [
        "font/glyph_outline",
        "font/cmap_subtables",
        "font/metrics",
        "font/subfamily_name",
        "font/full_name",
        "font/postscript_name",
    ] {
        let mut scene = counted_scene(0);
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"{method}","params":{{"font_id":1,"glyph_id":0}},"id":1}}"#,
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32603, "method {method}");
        assert_eq!(
            err.data.as_ref().and_then(|d| d.as_str()),
            Some("FontRegistryUnavailable"),
            "method {method}",
        );
    }
}

// --- R50.X.3 §5.37.2 lifecycle E2E tests ---

#[test]
fn font_dispose_round_trip_removes_handle() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let font_id = parse_noto_sans_via_dispatch(&mut scene, &registry);
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"font/dispose","params":{{"font_id":{font_id}}},"id":10}}"#,
    );
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, &body).unwrap());
    assert_eq!(
        resp.result.unwrap().get("existed").and_then(Value::as_bool),
        Some(true),
    );
    assert_eq!(registry.len(), 0);
}

#[test]
fn font_dispose_unknown_handle_existed_false() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let req = r#"{"jsonrpc":"2.0","method":"font/dispose","params":{"font_id":9999},"id":11}"#;
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, req).unwrap());
    assert_eq!(
        resp.result.unwrap().get("existed").and_then(Value::as_bool),
        Some(false),
    );
}

#[test]
fn font_list_round_trip_returns_handles() {
    let mut scene = counted_scene(0);
    let registry = FontRegistry::new();
    let a = parse_noto_sans_via_dispatch(&mut scene, &registry);
    let b = parse_noto_sans_via_dispatch(&mut scene, &registry);
    let req = r#"{"jsonrpc":"2.0","method":"font/list","id":12}"#;
    let resp = parse_response(&dispatch_with_font(&mut scene, &registry, req).unwrap());
    let ids: Vec<u64> = resp
        .result
        .unwrap()
        .get("font_ids")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![a, b]);
}

#[test]
fn font_lifecycle_without_registry_returns_registry_unavailable() {
    for (method, body) in [
        (
            "font/dispose",
            r#"{"jsonrpc":"2.0","method":"font/dispose","params":{"font_id":1},"id":13}"#,
        ),
        (
            "font/list",
            r#"{"jsonrpc":"2.0","method":"font/list","id":14}"#,
        ),
    ] {
        let mut scene = counted_scene(0);
        let resp = parse_response(&dispatch_t(&mut scene, body).unwrap());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32603, "method {method}");
        assert_eq!(
            err.data.as_ref().and_then(|d| d.as_str()),
            Some("FontRegistryUnavailable"),
            "method {method}",
        );
    }
}

// --- R50.2.X §5.37.2 text/normalize RPC E2E tests ---

/// Build a `text/normalize` JSON-RPC body. The input text is
/// hex-escaped (`\uXXXX`) so the dispatch.rs source stays pure
/// NFC ASCII and the test's input cannot drift from an
/// editor-side normalization pass.
fn text_normalize_body(text: &str, form: &str, id: u32) -> String {
    use std::fmt::Write as _;
    let mut escaped = String::with_capacity(text.chars().count() * 6);
    for c in text.chars() {
        write!(escaped, "\\u{:04X}", c as u32).expect("String write infallible");
    }
    format!(
        r#"{{"jsonrpc":"2.0","method":"text/normalize","params":{{"text":"{escaped}","form":"{form}"}},"id":{id}}}"#
    )
}

#[test]
fn text_normalize_nfc_recomposes_decomposed() {
    let mut scene = counted_scene(0);
    let body = text_normalize_body("A\u{0300}", "NFC", 1);
    let resp = parse_response(&dispatch_t(&mut scene, &body).unwrap());
    let text = resp
        .result
        .unwrap()
        .get("text")
        .and_then(Value::as_str)
        .expect("text in result")
        .to_owned();
    assert_eq!(text, "\u{00C0}");
}

#[test]
fn text_normalize_nfd_decomposes_precomposed() {
    let mut scene = counted_scene(0);
    let body = text_normalize_body("\u{00C0}", "NFD", 2);
    let resp = parse_response(&dispatch_t(&mut scene, &body).unwrap());
    let text = resp
        .result
        .unwrap()
        .get("text")
        .and_then(Value::as_str)
        .expect("text in result")
        .to_owned();
    assert_eq!(text, "A\u{0300}");
}

#[test]
fn text_normalize_nfkd_strips_ligature() {
    let mut scene = counted_scene(0);
    // ﬁ (U+FB01) → "fi" via compatibility decomposition.
    let body = text_normalize_body("\u{FB01}", "NFKD", 3);
    let resp = parse_response(&dispatch_t(&mut scene, &body).unwrap());
    let text = resp
        .result
        .unwrap()
        .get("text")
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    assert_eq!(text, "fi");
}

#[test]
fn text_normalize_hangul_jamo_compose() {
    let mut scene = counted_scene(0);
    // ᄒ ᅡ ᆫ → 한 (U+D55C) via algorithmic Hangul composition.
    let body = text_normalize_body("\u{1112}\u{1161}\u{11AB}", "NFC", 4);
    let resp = parse_response(&dispatch_t(&mut scene, &body).unwrap());
    let text = resp
        .result
        .unwrap()
        .get("text")
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    assert_eq!(text, "\u{D55C}");
}

#[test]
fn text_normalize_missing_text_param() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"text/normalize","params":{"form":"NFC"},"id":5}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(
        err.data
            .as_ref()
            .and_then(|d| d.as_str())
            .is_some_and(|s| s.contains("text")),
        "expected text-missing message, got {:?}",
        err.data
    );
}

#[test]
fn text_normalize_missing_form_param() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"text/normalize","params":{"text":"abc"},"id":6}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(
        err.data
            .as_ref()
            .and_then(|d| d.as_str())
            .is_some_and(|s| s.contains("form"))
    );
}

#[test]
fn text_normalize_unknown_form_rejected() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"text/normalize","params":{"text":"abc","form":"NFXC"},"id":7}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(
        err.data
            .as_ref()
            .and_then(|d| d.as_str())
            .is_some_and(|s| s.contains("NFC/NFD/NFKC/NFKD"))
    );
}

#[test]
fn text_normalize_ascii_passthrough() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"text/normalize","params":{"text":"hello","form":"NFC"},"id":8}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let text = resp
        .result
        .unwrap()
        .get("text")
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    assert_eq!(text, "hello");
}

// ---- R51.8 §5.38 — Toggle widget e2e through JSON-RPC envelope ----
//
// Mirrors the ButtonExternal R12 e2e suite above for the R51.2
// Toggle widget. Validates the full §5.15 8-item contract over
// the wire (`scene/query` state+value, `scene/rewind` value
// intervene, `scene/invoke` send action, `scene/intents` drain)
// plus the R51.4 IntentEmitter integration: a complete activate
// cycle queues exactly one `"toggle"` intent with the new bool
// value as `IntrospectValue::Bool` payload.

#[test]
fn scene_query_on_toggle_external_returns_initial_state_and_value() {
    use pinion_core::widgets::toggle::ToggleExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(ToggleExternal::new())));
    let req_state =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/state"},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_state).unwrap());
    let s = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(s.contains("Idle"), "got {s}");

    let req_value =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/value"},"id":2}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_value).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(v.contains("false"), "got {v}");
}

#[test]
fn scene_rewind_on_toggle_external_sets_value_without_intent() {
    use pinion_core::widgets::toggle::ToggleExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(ToggleExternal::new())));
    let rewind = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/value","value":true},"id":10}"#;
    let resp = parse_response(&dispatch_t(&mut scene, rewind).unwrap());
    assert!(resp.error.is_none(), "rewind error: {:?}", resp.error);

    let req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/value"},"id":11}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(v.contains("true"), "got {v}");

    // intervene-set must NOT fire a `"toggle"` intent — only
    // activate transitions do. Confirms the §5.20 channel stays
    // discriminating: model-driven sets cannot accidentally
    // forge user activate signals.
    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":12}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert!(arr.is_empty(), "intervene must not queue intents");
}

#[test]
fn scene_invoke_on_toggle_external_drives_transition() {
    use pinion_core::widgets::toggle::ToggleExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(ToggleExternal::new())));
    let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/send","args":"PointerEnter"},"id":20}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert!(resp.error.is_none(), "invoke error: {:?}", resp.error);
    let s = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(s.contains("Hover"), "got {s}");
}

#[test]
fn scene_invoke_full_cycle_on_toggle_external_emits_toggle_intent() {
    use pinion_core::widgets::toggle::ToggleExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(ToggleExternal::new())));
    for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/invoke","params":{{"path":"/external/send","args":"{ev}"}},"id":30}}"#
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        assert!(resp.error.is_none(), "invoke {ev} error: {:?}", resp.error);
    }

    // Drain intents — full activate cycle should produce
    // exactly one "toggle" intent carrying the flipped value.
    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":31}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert_eq!(arr.len(), 1, "expected exactly one toggle intent");
    let entry = arr[0].as_object().unwrap();
    // §5.20 R22: widget emits only the kind; runtime walk
    // prefixes ExternalNode.tag if any. Bare ExternalNode here
    // has no tag, so the wire form is just "toggle".
    assert_eq!(entry.get("tag").and_then(Value::as_str), Some("toggle"));
    assert_eq!(entry.get("payload"), Some(&Value::Bool(true)));

    // Final state observation through the same envelope.
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/value"},"id":32}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(
        v.contains("true"),
        "value after activate should be true: {v}"
    );
}

// ---- R51.13 §5.38 — Checkbox widget e2e through JSON-RPC envelope ----
//
// Mirrors the Toggle e2e suite for the R51.5 Checkbox widget.
// Schema slot is `"checked"` (vs Toggle's `"value"`) and the
// intent name is `"checked"` (vs `"toggle"`), so form-bound
// listeners can subscribe to checkboxes independently from
// settings switches. Semantics otherwise identical: activate
// flips the value, intervene sets directly without firing intent.

#[test]
fn scene_query_on_checkbox_external_returns_initial_state_and_checked() {
    use pinion_core::widgets::checkbox::CheckboxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(CheckboxExternal::new())));
    let req_state =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/state"},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_state).unwrap());
    let s = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(s.contains("Idle"), "got {s}");

    let req_checked =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/checked"},"id":2}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_checked).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(v.contains("false"), "got {v}");
}

#[test]
fn scene_rewind_on_checkbox_external_sets_checked_without_intent() {
    use pinion_core::widgets::checkbox::CheckboxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(CheckboxExternal::new())));
    let rewind = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/checked","value":true},"id":10}"#;
    let resp = parse_response(&dispatch_t(&mut scene, rewind).unwrap());
    assert!(resp.error.is_none(), "rewind error: {:?}", resp.error);

    let req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/checked"},"id":11}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(v.contains("true"), "got {v}");

    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":12}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert!(arr.is_empty(), "intervene must not queue intents");
}

#[test]
fn scene_invoke_on_checkbox_external_drives_transition() {
    use pinion_core::widgets::checkbox::CheckboxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(CheckboxExternal::new())));
    let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/send","args":"PointerEnter"},"id":20}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert!(resp.error.is_none(), "invoke error: {:?}", resp.error);
    let s = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(s.contains("Hover"), "got {s}");
}

#[test]
fn scene_invoke_full_cycle_on_checkbox_external_emits_checked_intent() {
    use pinion_core::widgets::checkbox::CheckboxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(CheckboxExternal::new())));
    for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/invoke","params":{{"path":"/external/send","args":"{ev}"}},"id":30}}"#
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        assert!(resp.error.is_none(), "invoke {ev} error: {:?}", resp.error);
    }

    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":31}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert_eq!(arr.len(), 1, "expected exactly one checked intent");
    let entry = arr[0].as_object().unwrap();
    assert_eq!(entry.get("tag").and_then(Value::as_str), Some("checked"));
    assert_eq!(entry.get("payload"), Some(&Value::Bool(true)));

    let req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/checked"},"id":32}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(
        v.contains("true"),
        "checked after activate should be true: {v}"
    );
}

// ---- R51.13 §5.38 — Radio widget e2e through JSON-RPC envelope ----
//
// Mirrors the Toggle e2e suite for the R51.6 Radio widget with
// two semantic differences ratified at the binding layer:
//
//   * Activate is **set-not-flip** — value goes false → true and
//     stays true until explicit deselect. Re-activating an
//     already-selected Radio is idempotent and silent on §5.20
//     (the first activate is the only emitter in a sequence).
//   * Intent payload is `Null`, not `Bool(after_value)` — Radio
//     selection is identity-only; the scene-side `ExternalNode.tag`
//     carries which option was picked.

#[test]
fn scene_query_on_radio_external_returns_initial_state_and_selected() {
    use pinion_core::widgets::radio::RadioExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(RadioExternal::new())));
    let req_state =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/state"},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_state).unwrap());
    let s = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(s.contains("Idle"), "got {s}");

    let req_selected =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/selected"},"id":2}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_selected).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(v.contains("false"), "got {v}");
}

#[test]
fn scene_rewind_on_radio_external_sets_selected_without_intent() {
    use pinion_core::widgets::radio::RadioExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(RadioExternal::new())));
    let rewind = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/selected","value":true},"id":10}"#;
    let resp = parse_response(&dispatch_t(&mut scene, rewind).unwrap());
    assert!(resp.error.is_none(), "rewind error: {:?}", resp.error);

    let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/selected"},"id":11}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(v.contains("true"), "got {v}");

    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":12}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert!(arr.is_empty(), "intervene must not queue intents");
}

#[test]
fn scene_invoke_on_radio_external_drives_transition() {
    use pinion_core::widgets::radio::RadioExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(RadioExternal::new())));
    let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/send","args":"PointerEnter"},"id":20}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert!(resp.error.is_none(), "invoke error: {:?}", resp.error);
    let s = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(s.contains("Hover"), "got {s}");
}

#[test]
fn scene_invoke_full_cycle_on_radio_external_emits_selected_intent_once() {
    use pinion_core::widgets::radio::RadioExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(RadioExternal::new())));

    // First activate cycle — fires "selected" intent.
    for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/invoke","params":{{"path":"/external/send","args":"{ev}"}},"id":30}}"#
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        assert!(resp.error.is_none(), "invoke {ev} error: {:?}", resp.error);
    }

    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":31}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert_eq!(
        arr.len(),
        1,
        "first activate fires exactly one selected intent"
    );
    let entry = arr[0].as_object().unwrap();
    assert_eq!(entry.get("tag").and_then(Value::as_str), Some("selected"));
    assert_eq!(entry.get("payload"), Some(&Value::Null));

    // Second activate cycle — Radio is set-not-flip, so the
    // re-activate emits no intent (idempotent). Validates the
    // R51.6 binding-layer semantic over the wire.
    for ev in ["PointerDown", "PointerUp"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/invoke","params":{{"path":"/external/send","args":"{ev}"}},"id":32}}"#
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        assert!(resp.error.is_none(), "invoke {ev} error: {:?}", resp.error);
    }
    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":33}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert!(arr.is_empty(), "re-activate must be silent on §5.20");
}

// ---- R51.13 §5.38 — Slider widget e2e through JSON-RPC envelope ----
//
// Mirrors the Toggle e2e suite for the R51.7 Slider widget with
// two semantic divergences ratified at the binding layer:
//
//   * Two-phase intent stream — `"value_changing"` fires on every
//     effective value change (including model-driven `intervene`
//     because Slider's intervene path routes through `set_value`
//     to give two-way data binding a single source of truth);
//     `"value_committed"` fires once on drag-end (`Pressed →
//     Hover`). Toggle / Checkbox have a single activate intent.
//   * Value payload is `IntrospectValue::Float` (f64 on the wire);
//     the SCXML state and the f32 value sidecar both surface
//     through §5.15 `query`.

#[test]
fn scene_query_on_slider_external_returns_initial_state_and_value() {
    use pinion_core::widgets::slider::SliderExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(SliderExternal::new())));
    let req_state =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/state"},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_state).unwrap());
    let s = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(s.contains("Idle"), "got {s}");

    let req_value =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/value"},"id":2}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_value).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    // f64 zero serializes as "0.0" through serde_json.
    assert!(v.contains('0'), "got {v}");
}

#[test]
fn scene_rewind_on_slider_external_changes_value_and_fires_value_changing() {
    use pinion_core::widgets::slider::SliderExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(SliderExternal::new())));
    let rewind = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/value","value":0.5},"id":10}"#;
    let resp = parse_response(&dispatch_t(&mut scene, rewind).unwrap());
    assert!(resp.error.is_none(), "rewind error: {:?}", resp.error);

    let req =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/value"},"id":11}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(v.contains("0.5"), "got {v}");

    // Slider semantic divergence vs Toggle/Checkbox/Radio:
    // intervene-set DOES fire `"value_changing"` (single source of
    // truth for value mutation — routes through set_value). The
    // commit-side `"value_committed"` is the activate-only intent
    // and is NOT armed by intervene.
    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":12}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert_eq!(arr.len(), 1, "intervene fires exactly one value_changing");
    let entry = arr[0].as_object().unwrap();
    assert_eq!(
        entry.get("tag").and_then(Value::as_str),
        Some("value_changing")
    );
}

#[test]
fn scene_invoke_on_slider_external_drives_transition() {
    use pinion_core::widgets::slider::SliderExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(SliderExternal::new())));
    let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/send","args":"PointerEnter"},"id":20}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert!(resp.error.is_none(), "invoke error: {:?}", resp.error);
    let s = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(s.contains("Hover"), "got {s}");
}

#[test]
fn scene_invoke_full_drag_cycle_on_slider_external_emits_value_committed() {
    use pinion_core::widgets::slider::SliderExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(SliderExternal::new())));

    // Drag cycle: Idle → Hover → Pressed → (set value during press
    // via intervene → fires value_changing) → Hover (PointerUp
    // fires value_committed).
    for ev in ["PointerEnter", "PointerDown"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/invoke","params":{{"path":"/external/send","args":"{ev}"}},"id":30}}"#
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        assert!(resp.error.is_none(), "invoke {ev} error: {:?}", resp.error);
    }
    // Set value mid-drag (intervene routes through set_value,
    // fires value_changing).
    let rewind = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/value","value":0.75},"id":31}"#;
    let resp = parse_response(&dispatch_t(&mut scene, rewind).unwrap());
    assert!(resp.error.is_none(), "rewind error: {:?}", resp.error);

    // Drag-end commit.
    let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/send","args":"PointerUp"},"id":32}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert!(
        resp.error.is_none(),
        "invoke PointerUp error: {:?}",
        resp.error
    );

    // Drain — two intents in order: value_changing then
    // value_committed. Both carry Float(0.75).
    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":33}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert_eq!(arr.len(), 2, "expected value_changing then value_committed");
    assert_eq!(
        arr[0]
            .as_object()
            .unwrap()
            .get("tag")
            .and_then(Value::as_str),
        Some("value_changing")
    );
    let committed = arr[1].as_object().unwrap();
    assert_eq!(
        committed.get("tag").and_then(Value::as_str),
        Some("value_committed")
    );
    // Payload check: f64 0.75 round-trips through serde_json.
    let payload = committed.get("payload").unwrap();
    let val = payload.as_f64().expect("Float payload must be a number");
    assert!((val - 0.75).abs() < 1e-4, "got {val}");
}

// ---- R51.16 §5.38 — RadioGroup e2e through JSON-RPC envelope ----
//
// Mirrors the R51.13 widget e2e pattern for the R51.15 RadioGroup
// primitive. Validates the §5.15 8-item contract over the wire for
// a multi-Radio composite widget: schema declares count +
// selected_index + send; query reads count and current selection
// (Int or Null); intervene restores selected_index; invoke "send"
// takes "<index>:<EventName>" args and drives the indexed Radio;
// a full activate cycle queues exactly one "selected" intent with
// the selected index as IntrospectValue::Int payload.

#[test]
fn scene_query_on_radio_group_returns_count_and_initial_selected_index() {
    use pinion_core::widgets::radio_group::RadioGroupExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(RadioGroupExternal::new(3))));

    let req_count =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_count).unwrap());
    let s = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(s.contains('3'), "got {s}");

    // selected_index None must surface as JSON null in the result
    // envelope — R51.25 lifts the R51.16 raw-envelope carry: the
    // Response deserializer now preserves `result: null` as
    // `Some(Value::Null)` instead of collapsing it to `None`, so a
    // typed assertion suffices.
    let req_idx = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/selected_index"},"id":2}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_idx).unwrap());
    assert_eq!(resp.result, Some(Value::Null));
    assert!(resp.error.is_none());
}

#[test]
fn scene_rewind_on_radio_group_sets_selected_index_without_intent() {
    use pinion_core::widgets::radio_group::RadioGroupExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(RadioGroupExternal::new(4))));

    let rewind = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/selected_index","value":2},"id":10}"#;
    let resp = parse_response(&dispatch_t(&mut scene, rewind).unwrap());
    assert!(resp.error.is_none(), "rewind error: {:?}", resp.error);

    let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/selected_index"},"id":11}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(v.contains('2'), "got {v}");

    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":12}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert!(arr.is_empty(), "intervene must not queue intents");
}

#[test]
fn scene_invoke_on_radio_group_drives_indexed_radio() {
    use pinion_core::widgets::radio_group::RadioGroupExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(RadioGroupExternal::new(3))));

    // PointerEnter on index 1 — moves that Radio to Hover, no
    // selection change yet (no activate). selected_index stays
    // None, which surfaces as JSON null in the result envelope.
    // R51.25 — typed assertion replaces the R51.16 raw-envelope
    // workaround.
    let req = r#"{"jsonrpc":"2.0","method":"scene/invoke","params":{"path":"/external/send","args":"1:PointerEnter"},"id":20}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert_eq!(resp.result, Some(Value::Null));
    assert!(resp.error.is_none());
}

#[test]
fn scene_invoke_full_cycle_on_radio_group_emits_selected_with_index() {
    use pinion_core::widgets::radio_group::RadioGroupExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(RadioGroupExternal::new(3))));

    // Drive a full activate on index 2 over the wire.
    for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/invoke","params":{{"path":"/external/send","args":"2:{ev}"}},"id":30}}"#
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        assert!(resp.error.is_none(), "invoke {ev} error: {:?}", resp.error);
    }

    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":31}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert_eq!(arr.len(), 1, "expected exactly one selected intent");
    let entry = arr[0].as_object().unwrap();
    assert_eq!(entry.get("tag").and_then(Value::as_str), Some("selected"));
    // Payload is the new index as Int — wire form is JSON number.
    assert_eq!(
        entry.get("payload").and_then(Value::as_i64),
        Some(2),
        "intent payload must carry the new selected index"
    );

    // Final state observation through the same envelope.
    let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/selected_index"},"id":32}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(
        v.contains('2'),
        "selected_index after activate should be 2: {v}"
    );
}

// ---- R51.100 §5.38 — ListBox e2e through JSON-RPC envelope ----
//
// Mirrors the R51.16 RadioGroup e2e pattern for the R51.96 ListBox
// primitive. Validates the §5.15 8-item contract over the wire for
// the composite single-select Listbox and the R51.98 multi-select
// variant. Single mode parity with RadioGroup: count + multiselect
// + selected_index + send. Multi mode adds: `multiselect=true`
// exposed via query, `selected_index` reads as `null` regardless
// of selection, `selected.<i>` is intervene-able, and `send`
// activations emit `"selected"` Int(toggled_index) for both
// toggle-on and toggle-off transitions.
//
// Composite cancel propagation: R51.93 `PointerCancel` over the
// wire format `"<i>:PointerCancel"` lands the addressed item back
// at `Idle` without firing the `"selected"` intent, regardless of
// mode. Verified separately to guard the R51.93.1 regression at
// the JSON-RPC boundary.
//
// The tests address `RadioGroup` and `ListBoxExternal` directly
// (not through `WidgetView`) because the §5.15 contract is the
// External adapter's responsibility — the dispatch layer just
// routes JSON requests to the adapter's introspect surface.

#[test]
fn r51_100_scene_query_on_listbox_single_returns_count_and_mode() {
    use pinion_core::widgets::listbox::ListBoxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(ListBoxExternal::new(3))));

    let req_count =
        r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/count"},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_count).unwrap());
    let s = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(s.contains('3'), "count: got {s}");

    // multiselect is exposed even in single mode for AI clients.
    let req_mode = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/multiselect"},"id":2}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_mode).unwrap());
    assert_eq!(resp.result, Some(Value::Bool(false)));

    let req_idx = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/selected_index"},"id":3}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req_idx).unwrap());
    assert_eq!(resp.result, Some(Value::Null));
}

#[test]
fn r51_100_scene_rewind_listbox_single_selected_index_no_intent() {
    use pinion_core::widgets::listbox::ListBoxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(ListBoxExternal::new(4))));

    let rewind = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/selected_index","value":2},"id":10}"#;
    let resp = parse_response(&dispatch_t(&mut scene, rewind).unwrap());
    assert!(resp.error.is_none(), "rewind error: {:?}", resp.error);

    let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/selected_index"},"id":11}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let v = serde_json::to_string(&resp.result.unwrap()).unwrap();
    assert!(v.contains('2'), "selected_index after rewind: {v}");

    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":12}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert!(arr.is_empty(), "intervene must not queue intents");
}

#[test]
fn r51_100_scene_invoke_listbox_full_activate_emits_selected() {
    use pinion_core::widgets::listbox::ListBoxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(ListBoxExternal::new(3))));

    for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/invoke","params":{{"path":"/external/send","args":"1:{ev}"}},"id":20}}"#
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        assert!(resp.error.is_none(), "invoke {ev} error: {:?}", resp.error);
    }

    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":21}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert_eq!(arr.len(), 1, "single activate emits one intent");
    let entry = arr[0].as_object().unwrap();
    assert_eq!(entry.get("tag").and_then(Value::as_str), Some("selected"));
    assert_eq!(
        entry.get("payload").and_then(Value::as_i64),
        Some(1),
        "payload = new selected index"
    );
}

#[test]
fn r51_100_scene_invoke_listbox_pointer_cancel_silent() {
    use pinion_core::widgets::listbox::ListBoxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(ListBoxExternal::new(3))));

    // Pressed → cancel without commit. R51.93 PointerCancel must
    // not fire `"selected"` even through the composite wire.
    for ev in ["PointerEnter", "PointerDown", "PointerCancel"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/invoke","params":{{"path":"/external/send","args":"2:{ev}"}},"id":30}}"#
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        assert!(resp.error.is_none(), "invoke {ev}: {:?}", resp.error);
    }
    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":31}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap();
    let arr = arr.as_array().expect("intents result must be array");
    assert!(arr.is_empty(), "PointerCancel must not queue intents");

    // selected_index stays Null (no commit happened).
    let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/selected_index"},"id":32}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert_eq!(resp.result, Some(Value::Null));
}

#[test]
fn r51_100_scene_query_listbox_multi_returns_multiselect_true() {
    use pinion_core::widgets::listbox::ListBoxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(
        ListBoxExternal::with_multiselect(3),
    )));

    let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/multiselect"},"id":40}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert_eq!(resp.result, Some(Value::Bool(true)));

    // selected_index always Null in multi.
    let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/selected_index"},"id":41}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert_eq!(resp.result, Some(Value::Null));
}

#[test]
fn r51_100_scene_rewind_listbox_multi_selected_dot_writes_per_row() {
    use pinion_core::widgets::listbox::ListBoxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(
        ListBoxExternal::with_multiselect(4),
    )));

    // Set indices 1 + 3 via per-row intervene.
    for i in [1, 3] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/rewind","params":{{"path":"/external/selected.{i}","value":true}},"id":50}}"#
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        assert!(
            resp.error.is_none(),
            "rewind selected.{i}: {:?}",
            resp.error
        );
    }

    // Verify per-row selected reads true.
    for i in [1, 3] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/query","params":{{"path":"/external/selected.{i}"}},"id":51}}"#
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        assert_eq!(resp.result, Some(Value::Bool(true)), "selected.{i}");
    }
    // And the others false.
    for i in [0, 2] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/query","params":{{"path":"/external/selected.{i}"}},"id":52}}"#
        );
        let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
        assert_eq!(resp.result, Some(Value::Bool(false)), "selected.{i}");
    }
    // Slot-assignment: no intent.
    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":53}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap().as_array().unwrap().clone();
    assert!(arr.is_empty(), "intervene must not queue intents");
}

#[test]
fn r51_100_scene_rewind_listbox_single_selected_dot_rejected() {
    use pinion_core::widgets::listbox::ListBoxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(ListBoxExternal::new(3))));
    // Single-mode rejects per-row intervene (the mutual-exclusion
    // invariant lives in `selected_index`, not `selected.<i>`).
    let req = r#"{"jsonrpc":"2.0","method":"scene/rewind","params":{"path":"/external/selected.1","value":true},"id":60}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert!(resp.error.is_some(), "single-mode selected.<i> must error");
}

#[test]
fn r51_100_scene_invoke_listbox_multi_full_cycle_emits_toggle_on_and_off() {
    use pinion_core::widgets::listbox::ListBoxExternal;
    let mut scene = Scene::External(ExternalNode::new(Box::new(
        ListBoxExternal::with_multiselect(3),
    )));

    // First activate row 2 = toggle on.
    for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/invoke","params":{{"path":"/external/send","args":"2:{ev}"}},"id":70}}"#
        );
        let _ = dispatch_t(&mut scene, &req).unwrap();
    }
    // Second activate row 2 = toggle off.
    for ev in ["PointerDown", "PointerUp"] {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/invoke","params":{{"path":"/external/send","args":"2:{ev}"}},"id":71}}"#
        );
        let _ = dispatch_t(&mut scene, &req).unwrap();
    }
    // Both transitions emit "selected" Int(2) — payload identifies
    // the toggled row; AI follow-up query learns the direction.
    let drain = r#"{"jsonrpc":"2.0","method":"scene/intents","id":72}"#;
    let resp = parse_response(&dispatch_t(&mut scene, drain).unwrap());
    let arr = resp.result.unwrap().as_array().unwrap().clone();
    assert_eq!(arr.len(), 2, "expected toggle-on + toggle-off intents");
    for entry in &arr {
        assert_eq!(entry.get("tag").and_then(Value::as_str), Some("selected"),);
        assert_eq!(
            entry.get("payload").and_then(Value::as_i64),
            Some(2),
            "payload = toggled index"
        );
    }
    // Final per-row query: row 2 is now false.
    let req = r#"{"jsonrpc":"2.0","method":"scene/query","params":{"path":"/external/selected.2"},"id":73}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    assert_eq!(resp.result, Some(Value::Bool(false)));
}

// ---- R51.73 §5.40 — focus/set + focus/get JSON-RPC wire ----

fn dispatch_with_focus(
    scene: &mut Scene,
    focus: &mut pinion_runtime::FocusManager,
    req: &str,
) -> Option<String> {
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut ctx = DispatchContext::new(scene, &previews, &revision).with_focus_manager(focus);
    dispatch(&mut ctx, req)
}

#[test]
fn focus_set_wire_round_trip() {
    let mut scene = counted_scene(0);
    let mut focus = pinion_runtime::FocusManager::new();
    focus.update_focusable_tags(vec!["main_btn".to_owned(), "main_cb".to_owned()]);
    let req = r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":"main_cb"},"id":40}"#;
    let resp = parse_response(&dispatch_with_focus(&mut scene, &mut focus, req).unwrap());
    let result = resp.result.expect("focus/set returned no result");
    assert_eq!(
        result.get("focused").and_then(Value::as_str),
        Some("main_cb"),
    );
    assert_eq!(focus.focused(), Some("main_cb"));
}

#[test]
fn focus_set_null_clears() {
    let mut scene = counted_scene(0);
    let mut focus = pinion_runtime::FocusManager::new();
    focus.update_focusable_tags(vec!["main_btn".to_owned()]);
    let _ = focus.focus_set("main_btn");
    let req = r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":null},"id":41}"#;
    let resp = parse_response(&dispatch_with_focus(&mut scene, &mut focus, req).unwrap());
    let result = resp.result.unwrap();
    assert!(result.get("focused").unwrap().is_null());
    assert!(focus.focused().is_none());
}

#[test]
fn focus_set_unknown_tag_errors() {
    let mut scene = counted_scene(0);
    let mut focus = pinion_runtime::FocusManager::new();
    focus.update_focusable_tags(vec!["main_btn".to_owned()]);
    let req = r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":"bogus"},"id":42}"#;
    let resp = parse_response(&dispatch_with_focus(&mut scene, &mut focus, req).unwrap());
    let err = resp.error.expect("expected error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.message, "tag_not_focusable");
}

#[test]
fn focus_get_returns_state_with_tab_order() {
    let mut scene = counted_scene(0);
    let mut focus = pinion_runtime::FocusManager::new();
    focus.update_focusable_tags(vec!["a".to_owned(), "b".to_owned()]);
    let _ = focus.focus_set("b");
    let req = r#"{"jsonrpc":"2.0","method":"focus/get","id":43}"#;
    let resp = parse_response(&dispatch_with_focus(&mut scene, &mut focus, req).unwrap());
    let result = resp.result.unwrap();
    assert_eq!(result.get("focused").and_then(Value::as_str), Some("b"));
    let order = result.get("tab_order").and_then(Value::as_array).unwrap();
    assert_eq!(order.len(), 2);
}

#[test]
fn focus_set_without_manager_errors() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":"main_btn"},"id":44}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("expected unavailable error");
    assert_eq!(err.code, -32004);
    assert_eq!(err.message, "focus manager unavailable");
}

#[test]
fn focus_next_wire_advances() {
    let mut scene = counted_scene(0);
    let mut focus = pinion_runtime::FocusManager::new();
    focus.update_focusable_tags(vec!["a".to_owned(), "b".to_owned()]);
    let _ = focus.focus_set("a");
    let req = r#"{"jsonrpc":"2.0","method":"focus/next","id":50}"#;
    let resp = parse_response(&dispatch_with_focus(&mut scene, &mut focus, req).unwrap());
    let result = resp.result.unwrap();
    assert_eq!(result.get("focused").and_then(Value::as_str), Some("b"));
}

#[test]
fn focus_prev_wire_steps_back() {
    let mut scene = counted_scene(0);
    let mut focus = pinion_runtime::FocusManager::new();
    focus.update_focusable_tags(vec!["a".to_owned(), "b".to_owned()]);
    let _ = focus.focus_set("b");
    let req = r#"{"jsonrpc":"2.0","method":"focus/prev","id":51}"#;
    let resp = parse_response(&dispatch_with_focus(&mut scene, &mut focus, req).unwrap());
    let result = resp.result.unwrap();
    assert_eq!(result.get("focused").and_then(Value::as_str), Some("a"));
}

#[test]
fn focus_set_already_focused_succeeds_idempotent() {
    let mut scene = counted_scene(0);
    let mut focus = pinion_runtime::FocusManager::new();
    focus.update_focusable_tags(vec!["main_btn".to_owned()]);
    let _ = focus.focus_set("main_btn");
    let req = r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":"main_btn"},"id":45}"#;
    let resp = parse_response(&dispatch_with_focus(&mut scene, &mut focus, req).unwrap());
    assert!(resp.error.is_none(), "idempotent set must not error");
    assert_eq!(
        resp.result.unwrap().get("focused").and_then(Value::as_str),
        Some("main_btn"),
    );
}

// R632 §5.7 — shared dispatch test helpers for the R598-R612 +
// R629 sibling files. Pre-R632 these two helpers lived inline in
// the theme-axis test section; R632 moves them to the parent
// scope so all three sibling axes (theme / animation / widget)
// can call them without per-file duplication.

fn dispatch_with_runtime_owner(scene: &mut Scene, owner: &Owner, req: &str) -> Option<String> {
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    let mut ctx = DispatchContext::new(scene, &previews, &revision).with_runtime_owner(owner);
    dispatch(&mut ctx, req)
}

fn dispatch_with_runtime_owner_and_revision(
    scene: &mut Scene,
    owner: &Owner,
    revision: &SceneRevision,
    req: &str,
) -> Option<String> {
    let previews = PreviewLedger::default();
    let mut ctx = DispatchContext::new(scene, &previews, revision).with_runtime_owner(owner);
    dispatch(&mut ctx, req)
}

// R708 §5.50 — gradient-fill wire serialization. `box_style_to_json`
// must surface the optional `Gradient` overlay so AI clients can read a
// box's gradient ramp as data (§2 #7 scene-as-data); `null` when absent.
#[test]
fn r708_box_style_to_json_emits_gradient() {
    use pinion_core::style::{BoxStyle, Color, Gradient};

    // No gradient -> the key is present and null (stable wire shape).
    let solid = box_style_to_json(&BoxStyle::filled(Color::rgb(1, 2, 3)));
    assert_eq!(solid["gradient"], serde_json::Value::Null);

    // Linear gradient -> geometry kind + endpoints + stops + extend.
    let style = BoxStyle::filled(Color::TRANSPARENT).with_gradient(
        Gradient::horizontal()
            .with_stop(0.0, Color::rgb(0xff, 0, 0))
            .with_stop(1.0, Color::rgb(0, 0, 0xff)),
    );
    let json = box_style_to_json(&style);
    let gradient = &json["gradient"];
    assert_eq!(gradient["geometry"]["kind"], "linear");
    assert_eq!(gradient["geometry"]["start"]["u"], 0.0);
    assert_eq!(gradient["geometry"]["end"]["u"], 1.0);
    assert_eq!(gradient["extend"], "Pad");
    let stops = gradient["stops"].as_array().expect("stops array");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0]["offset"], 0.0);
    assert_eq!(stops[0]["color"]["r"], 0xff);
    assert_eq!(stops[1]["color"]["b"], 0xff);

    // Radial gradient -> center + radius geometry.
    let radial = box_style_to_json(
        &BoxStyle::filled(Color::TRANSPARENT).with_gradient(Gradient::radial((0.5, 0.5), 0.25)),
    );
    assert_eq!(radial["gradient"]["geometry"]["kind"], "radial");
    assert_eq!(radial["gradient"]["geometry"]["center"]["v"], 0.5);
    assert_eq!(radial["gradient"]["geometry"]["radius"], 0.25);
}

// R632 §5.7 — per-axis dispatch test sibling files.
//
// Pre-R632 the AI-first read/write matrix wire-integration tests
// (R598-R612, +R629) all lived inline at the tail of this file,
// pushing the monolith past 6,000 LOC. R626 lifted the `tests`
// module body out of dispatch.rs via `#[path]`; R632 carves the
// per-axis sub-bodies into siblings via the same #[path] idiom so
// navigation and per-axis recompile latency improve. Each sibling
// re-imports `super::*` so the parent `tests` mod's helpers
// (`counted_scene`, `dispatch_t`, `dispatch_with_runtime_owner_*`,
// `parse_response`, …) remain in scope.

// ---- R979 §5.40 §2 #7 — `scene/access` accessibility-tree dump ----

#[test]
fn r979_scene_access_dumps_the_tree_with_focus_and_value_range() {
    use pinion_a11y::{AccessFocus, AccessNode, AccessState, AccessValue, AriaRole};
    let mut scene = counted_scene(0);
    let previews = PreviewLedger::default();
    let revision = SceneRevision::default();
    // A producer mirroring what the shell runs: a focused button plus a
    // slider whose value range was previously RPC-invisible (R966 carry).
    let mut produce_access = || -> (Vec<AccessNode>, Option<AccessFocus>) {
        let save = AccessNode::new("save", AriaRole::Button)
            .with_name("Save")
            .with_state(AccessState {
                focused: true,
                ..AccessState::default()
            });
        let opacity = AccessNode::new("opacity", AriaRole::Slider).with_value(AccessValue::Float {
            value: 0.5,
            min: 0.0,
            max: 1.0,
        });
        (vec![save, opacity], Some(AccessFocus::atomic("save")))
    };
    let mut ctx = DispatchContext::new(&mut scene, &previews, &revision)
        .with_access_producer(&mut produce_access);
    let req = r#"{"jsonrpc":"2.0","method":"scene/access","id":1}"#;
    let resp = parse_response(&dispatch(&mut ctx, req).unwrap());
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result["count"], 2);
    assert_eq!(result["focus"]["tag"], "save");
    let nodes = result["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes[0]["tag"], "save");
    assert_eq!(nodes[0]["role"], "button");
    assert_eq!(nodes[0]["name"], "Save");
    assert_eq!(nodes[0]["state"]["focused"], true);
    // The slider's valuenow / valuemin / valuemax is now over the wire.
    assert_eq!(nodes[1]["role"], "slider");
    assert_eq!(nodes[1]["value"]["float"]["value"], 0.5);
    assert_eq!(nodes[1]["value"]["float"]["min"], 0.0);
    assert_eq!(nodes[1]["value"]["float"]["max"], 1.0);
    // A clean atomic node omits default fields (no bounds / selected here).
    assert!(nodes[0].get("bounds").is_none());
}

#[test]
fn r979_scene_access_without_producer_errors() {
    // A headless fixture with no a11y build wired surfaces the honesty
    // token rather than aliasing onto an empty tree.
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/access","id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).expect("error frame"));
    let err = resp.error.expect("no producer -> error frame");
    assert_eq!(
        err.data,
        Some(Value::String("AccessTreeUnavailable".into()))
    );
}

#[path = "dispatch_tests_theme.rs"]
mod r632_theme;

#[path = "dispatch_tests_animation.rs"]
mod r632_animation;

#[path = "dispatch_tests_axes.rs"]
mod r632_axes;
