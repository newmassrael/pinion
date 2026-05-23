use super::*;

// ─────────────────────────────────────────────────────────────────
// R602 §5.45 — scene/scroll_state wire integration
//
// The fine-grained handler logic is covered by the test battery in
// `crate::scroll_state::tests::r602_*`. The cases below exercise
// the dispatcher's wire round-trip — params parsing (tag required),
// runtime_owner injection, and error → RpcError mapping.
// ─────────────────────────────────────────────────────────────────

#[test]
fn r602_scene_scroll_state_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll_state","params":{"tag":"list"},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("RuntimeOwnerUnavailable".into())));
}

#[test]
fn r602_scene_scroll_state_missing_tag_param_errors() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll_state","params":{},"id":2}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("TagRequired".into())));
}

#[test]
fn r602_scene_scroll_state_unbound_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll_state","params":{"tag":"ghost"},"id":3}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("unbound tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("NotBound".into())));
}

#[test]
fn r602_scene_scroll_state_happy_path_returns_projection() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let state = owner.run(|| pinion_core::widgets::scroll::use_scroll_state("list"));
    state.set_max(0, 480);
    state.scroll_to(0, 240);
    let req = r#"{"jsonrpc":"2.0","method":"scene/scroll_state","params":{"tag":"list"},"id":4}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none());
    let result = resp.result.expect("result present");
    assert_eq!(result["tag"], "list");
    assert_eq!(result["offset"]["y"], 240);
    assert_eq!(result["max"]["y"], 480);
    assert_eq!(result["edges"]["at_bottom"], false);
    assert_eq!(result["edges"]["at_top"], false);
}

// ─────────────────────────────────────────────────────────────────
// R609 §5.45 — scene/set_scroll_offset wire integration
//
// Mutation pair to scene/scroll_state. The fine-grained handler
// logic is covered by the test battery in
// `crate::scroll_state::tests::r609_*`. The cases below exercise
// the dispatcher's wire round-trip — required tag, integer field
// parsing, error → RpcError mapping, and the
// HandlerKind::Mutate match-arm OCC bump (R620).
// ─────────────────────────────────────────────────────────────────

#[test]
fn r609_scene_set_scroll_offset_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_scroll_offset","params":{"tag":"list","x":0,"y":240},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("RuntimeOwnerUnavailable".into())));
}

#[test]
fn r609_scene_set_scroll_offset_missing_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_scroll_offset","params":{"x":0,"y":240},"id":2}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("TagRequired".into())));
}

#[test]
fn r609_scene_set_scroll_offset_missing_x_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::scroll::use_scroll_state("list"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_scroll_offset","params":{"tag":"list","y":240},"id":3}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing x must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("InvalidAxisValue".into())));
}

#[test]
fn r609_scene_set_scroll_offset_missing_y_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::scroll::use_scroll_state("list"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_scroll_offset","params":{"tag":"list","x":0},"id":4}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing y must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("InvalidAxisValue".into())));
}

#[test]
fn r609_scene_set_scroll_offset_rejects_non_integer_axis() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::scroll::use_scroll_state("list"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_scroll_offset","params":{"tag":"list","x":"0","y":240},"id":5}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("non-integer x must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("InvalidAxisValue".into())));
}

#[test]
fn r609_scene_set_scroll_offset_unbound_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_scroll_offset","params":{"tag":"ghost","x":0,"y":240},"id":6}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("unbound tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("NotBound".into())));
}

#[test]
fn r609_scene_set_scroll_offset_happy_path_returns_clamped_post_state() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let state = owner.run(|| pinion_core::widgets::scroll::use_scroll_state("list"));
    state.set_max(0, 480);
    // Request y=999 → substrate clamps to 480 and the wire response
    // echoes the clamped post-state.
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_scroll_offset","params":{"tag":"list","x":0,"y":999},"id":7}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none(), "happy path must not error");
    let result = resp.result.expect("result present");
    assert_eq!(result["tag"], "list");
    assert_eq!(result["offset"]["y"], 480);
    assert_eq!(result["max"]["y"], 480);
    assert_eq!(result["edges"]["at_bottom"], true);
}

#[test]
fn r609_scene_set_scroll_offset_bumps_revision_on_success() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::scroll::use_scroll_state("list"));
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_scroll_offset","params":{"tag":"list","x":0,"y":120},"id":8}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    let after = revision.current();
    assert!(
        after > before,
        "set_scroll_offset must bump the OCC token (before={before}, after={after})",
    );
}

#[test]
fn r609_scene_set_scroll_offset_does_not_bump_revision_on_failure() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    // No scroll state bound → handler errors. The dispatcher's
    // HandlerKind::Mutate arm only bumps on Ok; Err must not.
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_scroll_offset","params":{"tag":"ghost","x":0,"y":120},"id":9}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    assert_eq!(
        revision.current(),
        before,
        "a failed set_scroll_offset must not bump the OCC token",
    );
}

// ─────────────────────────────────────────────────────────────────
// R603 §5.22 — scene/text_state wire integration
//
// The fine-grained handler logic is covered by the test battery in
// `crate::text_state::tests::r603_*`. The cases below exercise the
// dispatcher's wire round-trip — params parsing (tag required),
// runtime_owner injection, and error → RpcError mapping.
// ─────────────────────────────────────────────────────────────────

#[test]
fn r603_scene_text_state_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/text_state","params":{"tag":"field"},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("RuntimeOwnerUnavailable".into())));
}

#[test]
fn r603_scene_text_state_missing_tag_param_errors() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/text_state","params":{},"id":2}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("TagRequired".into())));
}

#[test]
fn r603_scene_text_state_unbound_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/text_state","params":{"tag":"ghost"},"id":3}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("unbound tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("NotBound".into())));
}

// ─────────────────────────────────────────────────────────────────
// R610 §5.22 — scene/set_text wire integration
//
// Mutation pair to scene/text_state. The fine-grained handler
// logic is covered in `crate::text_state::tests::r610_*`. The
// cases below exercise the dispatcher's wire round-trip — typed
// params parsing, error → RpcError mapping, and OCC bump.
// ─────────────────────────────────────────────────────────────────

#[test]
fn r610_scene_set_text_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_text","params":{"tag":"field","text":"Hi"},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("RuntimeOwnerUnavailable".into())));
}

#[test]
fn r610_scene_set_text_missing_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_text","params":{"text":"Hi"},"id":2}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("TagRequired".into())));
}

#[test]
fn r610_scene_set_text_missing_text_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_text","params":{"tag":"field"},"id":3}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing text must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("TextRequired".into())));
}

#[test]
fn r610_scene_set_text_rejects_non_string_text() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_text","params":{"tag":"field","text":42},"id":4}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("non-string text must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("TextRequired".into())));
}

#[test]
fn r610_scene_set_text_unbound_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_text","params":{"tag":"ghost","text":"Hi"},"id":5}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("unbound tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("NotBound".into())));
}

#[test]
fn r610_scene_set_text_happy_path_returns_post_state() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_text","params":{"tag":"field","text":"Hello, world!"},"id":6}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none(), "happy path must not error");
    let result = resp.result.expect("result present");
    assert_eq!(result["tag"], "field");
    assert_eq!(result["text"], "Hello, world!");
    assert_eq!(result["has_selection"], false);
    assert_eq!(state.text(), "Hello, world!");
}

#[test]
fn r610_scene_set_text_bumps_revision_on_success() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_text","params":{"tag":"field","text":"Hi"},"id":7}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    let after = revision.current();
    assert!(
        after > before,
        "set_text must bump the OCC token (before={before}, after={after})",
    );
}

#[test]
fn r610_scene_set_text_does_not_bump_revision_on_failure() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_text","params":{"tag":"ghost","text":"x"},"id":8}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    assert_eq!(
        revision.current(),
        before,
        "a failed set_text must not bump the OCC token",
    );
}

// ─────────────────────────────────────────────────────────────────
// R611 §5.22 — scene/set_selection wire integration
//
// Mutation pair to scene/text_state for the selection-axis. The
// fine-grained handler logic is covered in
// `crate::text_state::tests::r611_*`. The cases below exercise
// the dispatcher's wire round-trip — typed params parsing,
// error → RpcError mapping, OCC bump.
// ─────────────────────────────────────────────────────────────────

#[test]
fn r611_scene_set_selection_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_selection","params":{"tag":"field","anchor":0,"focus":3},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("RuntimeOwnerUnavailable".into())));
}

#[test]
fn r611_scene_set_selection_missing_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_selection","params":{"anchor":0,"focus":3},"id":2}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("TagRequired".into())));
}

#[test]
fn r611_scene_set_selection_missing_anchor_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_selection","params":{"tag":"field","focus":3},"id":3}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing anchor must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("InvalidByteOffset".into())));
}

#[test]
fn r611_scene_set_selection_missing_focus_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_selection","params":{"tag":"field","anchor":0},"id":4}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing focus must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("InvalidByteOffset".into())));
}

#[test]
fn r611_scene_set_selection_rejects_negative_offset() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    // Negative integers are rejected by serde_json's `as_u64` path.
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_selection","params":{"tag":"field","anchor":-1,"focus":3},"id":5}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("negative anchor must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("InvalidByteOffset".into())));
}

#[test]
fn r611_scene_set_selection_unbound_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_selection","params":{"tag":"ghost","anchor":0,"focus":3},"id":6}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("unbound tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("NotBound".into())));
}

#[test]
fn r611_scene_set_selection_happy_path_returns_post_state() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    state.set_text("Hello world".to_owned());
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_selection","params":{"tag":"field","anchor":0,"focus":5},"id":7}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none(), "happy path must not error");
    let result = resp.result.expect("result present");
    assert_eq!(result["tag"], "field");
    assert_eq!(result["caret"], 5);
    assert_eq!(result["has_selection"], true);
    assert_eq!(result["selection"]["start"], 0);
    assert_eq!(result["selection"]["end"], 5);
    assert_eq!(result["selection"]["anchor"], 0);
}

#[test]
fn r611_scene_set_selection_bumps_revision_on_success() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    state.set_text("Hello".to_owned());
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_selection","params":{"tag":"field","anchor":0,"focus":3},"id":8}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    let after = revision.current();
    assert!(
        after > before,
        "set_selection must bump the OCC token (before={before}, after={after})",
    );
}

#[test]
fn r611_scene_set_selection_does_not_bump_revision_on_failure() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_selection","params":{"tag":"ghost","anchor":0,"focus":3},"id":9}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    assert_eq!(
        revision.current(),
        before,
        "a failed set_selection must not bump the OCC token",
    );
}

// ─────────────────────────────────────────────────────────────────
// R612 §5.22 — scene/set_caret wire integration
//
// Closes the AI-first write-side matrix. The fine-grained handler
// logic is covered in `crate::text_state::tests::r612_*`. The
// cases below exercise the dispatcher's wire round-trip — typed
// params parsing, error → RpcError mapping, OCC bump.
// ─────────────────────────────────────────────────────────────────

#[test]
fn r612_scene_set_caret_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_caret","params":{"tag":"field","pos":3},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("RuntimeOwnerUnavailable".into())));
}

#[test]
fn r612_scene_set_caret_missing_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_caret","params":{"pos":3},"id":2}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("TagRequired".into())));
}

#[test]
fn r612_scene_set_caret_missing_pos_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_caret","params":{"tag":"field"},"id":3}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing pos must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("InvalidByteOffset".into())));
}

#[test]
fn r612_scene_set_caret_rejects_negative_pos() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_caret","params":{"tag":"field","pos":-1},"id":4}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("negative pos must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("InvalidByteOffset".into())));
}

#[test]
fn r612_scene_set_caret_unbound_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_caret","params":{"tag":"ghost","pos":3},"id":5}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("unbound tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("NotBound".into())));
}

#[test]
fn r612_scene_set_caret_happy_path_returns_post_state() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    state.set_text("Hello world".to_owned());
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_caret","params":{"tag":"field","pos":5},"id":6}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none(), "happy path must not error");
    let result = resp.result.expect("result present");
    assert_eq!(result["tag"], "field");
    assert_eq!(result["caret"], 5);
    assert_eq!(result["has_selection"], false);
    assert_eq!(state.caret(), 5);
}

#[test]
fn r612_scene_set_caret_bumps_revision_on_success() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let state = owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    state.set_text("Hello".to_owned());
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_caret","params":{"tag":"field","pos":3},"id":7}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    let after = revision.current();
    assert!(
        after > before,
        "set_caret must bump the OCC token (before={before}, after={after})",
    );
}

#[test]
fn r612_scene_set_caret_does_not_bump_revision_on_failure() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_caret","params":{"tag":"ghost","pos":3},"id":8}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    assert_eq!(
        revision.current(),
        before,
        "a failed set_caret must not bump the OCC token",
    );
}

// ─────────────────────────────────────────────────────────────────
// R604 §5.22 — scene/caret_state wire integration
// ─────────────────────────────────────────────────────────────────

#[test]
fn r604_scene_caret_state_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/caret_state","params":{"tag":"field"},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("RuntimeOwnerUnavailable".into())));
}

#[test]
fn r604_scene_caret_state_missing_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/caret_state","params":{},"id":2}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing tag must error");
    assert_eq!(err.data, Some(Value::String("TagRequired".into())));
}

#[test]
fn r604_scene_caret_state_happy_path_returns_projection() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let state = owner.run(|| pinion_core::widgets::caret_blink::use_caret_blink("field"));
    state.set_enabled(true);
    let req = r#"{"jsonrpc":"2.0","method":"scene/caret_state","params":{"tag":"field"},"id":3}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none());
    let result = resp.result.expect("result present");
    assert_eq!(result["tag"], "field");
    assert_eq!(result["enabled"], true);
    assert_eq!(result["visible"], true);
    assert!(result["period_secs"].as_f64().is_some());
}

#[test]
fn r603_scene_text_state_happy_path_with_selection_returns_full_projection() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let state =
        owner.run(|| pinion_core::widgets::text_edit::use_text_edit_state("field"));
    state.set_text("Hello".to_owned());
    // set_selection(anchor, focus) leaves caret at focus = 3
    // per the W3C Selection canonical contract.
    state.set_selection(0, 3);
    let req = r#"{"jsonrpc":"2.0","method":"scene/text_state","params":{"tag":"field"},"id":4}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none());
    let result = resp.result.expect("result present");
    assert_eq!(result["tag"], "field");
    assert_eq!(result["text"], "Hello");
    assert_eq!(result["caret"], 3);
    assert_eq!(result["has_selection"], true);
    assert_eq!(result["selection"]["start"], 0);
    assert_eq!(result["selection"]["end"], 3);
}
