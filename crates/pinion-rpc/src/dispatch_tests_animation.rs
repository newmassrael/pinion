use super::*;

// ─────────────────────────────────────────────────────────────────
// R600 §5.28 — scene/animation_state wire integration
//
// The fine-grained handler logic is covered by the test battery in
// `crate::animation_state::tests::r600_*`. The cases below exercise
// the dispatcher's wire round-trip — params parsing, epsilon
// normalization, and error → RpcError mapping.
// ─────────────────────────────────────────────────────────────────

#[test]
fn r600_scene_animation_state_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/animation_state","id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("RuntimeOwnerUnavailable".into())));
}

#[test]
fn r600_scene_animation_state_empty_owner_reports_inactive() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/animation_state","id":2}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none(), "empty owner must not error");
    let result = resp.result.expect("result present");
    assert_eq!(result.get("active"), Some(&Value::Bool(false)));
    assert!(result.get("epsilon").and_then(Value::as_f64).is_some());
}

#[test]
fn r600_scene_animation_state_custom_epsilon_echoed() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/animation_state","params":{"epsilon":0.05},"id":3}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none());
    let echoed = resp.result.unwrap()["epsilon"].as_f64().unwrap();
    assert!((echoed - 0.05).abs() < 1e-5);
}

#[test]
fn r600_scene_animation_state_rejects_negative_epsilon_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/animation_state","params":{"epsilon":-0.1},"id":4}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("negative epsilon must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("InvalidEpsilon".into())));
}

#[test]
fn r600_scene_animation_state_rejects_non_numeric_epsilon() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/animation_state","params":{"epsilon":"fast"},"id":5}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("non-numeric epsilon must error");
    assert_eq!(err.code, -32602);
}

// ─────────────────────────────────────────────────────────────────
// R629 §5.28 — scene/animate_settle + scene/animate_cancel wire
// integration. Substrate logic + walk semantics live in
// `crate::animate_control::tests::r629_*` and
// `pinion_core::reactive::owner::tests::animation_active::r629_*`;
// the cases below exercise the dispatcher's wire round-trip —
// runtime_owner injection, HandlerKind::Mutate OCC bump, and the
// outcome JSON shape.
// ─────────────────────────────────────────────────────────────────

#[test]
fn r629_scene_animate_settle_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/animate_settle","id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(
        err.data,
        Some(Value::String("RuntimeOwnerUnavailable".into())),
    );
}

#[test]
fn r629_scene_animate_cancel_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/animate_cancel","id":2}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(
        err.data,
        Some(Value::String("RuntimeOwnerUnavailable".into())),
    );
}

#[test]
fn r629_scene_animate_settle_empty_owner_returns_zero_visited() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/animate_settle","id":3}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none());
    let result = resp.result.expect("result present");
    assert_eq!(result.get("visited"), Some(&Value::from(0_u64)));
}

#[test]
fn r629_scene_animate_cancel_empty_owner_returns_zero_visited() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/animate_cancel","id":4}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none());
    let result = resp.result.expect("result present");
    assert_eq!(result.get("visited"), Some(&Value::from(0_u64)));
}

#[test]
fn r629_scene_animate_settle_walks_real_animation() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let a = pinion_core::animation::Animation::new(
        &owner,
        0.0_f32,
        pinion_core::animation::SpringConfig::DEFAULT,
    );
    a.set_target(7.0);
    owner.tick_animations(0.016);
    assert!(a.value() < 7.0, "mid-flight precondition");
    let req = r#"{"jsonrpc":"2.0","method":"scene/animate_settle","id":5}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap().get("visited"), Some(&Value::from(1_u64)));
    assert!((a.value() - 7.0).abs() < f32::EPSILON);
    assert!(a.is_at_rest());
}

#[test]
fn r629_scene_animate_cancel_walks_real_animation() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let a = pinion_core::animation::Animation::new(
        &owner,
        0.0_f32,
        pinion_core::animation::SpringConfig::DEFAULT,
    );
    a.set_target(100.0);
    for _ in 0..5 {
        owner.tick_animations(0.016);
    }
    let mid = a.value();
    assert!(mid > 0.0 && mid < 100.0, "mid-flight precondition");
    let req = r#"{"jsonrpc":"2.0","method":"scene/animate_cancel","id":6}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap().get("visited"), Some(&Value::from(1_u64)));
    assert!((a.value() - mid).abs() < f32::EPSILON);
    assert!(a.is_at_rest());
}

#[test]
fn r629_scene_animate_settle_bumps_revision_on_success() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = r#"{"jsonrpc":"2.0","method":"scene/animate_settle","id":7}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    let after = revision.current();
    assert!(
        after > before,
        "animate_settle must bump the OCC token (before={before}, after={after})",
    );
}

#[test]
fn r629_scene_animate_cancel_bumps_revision_on_success() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = r#"{"jsonrpc":"2.0","method":"scene/animate_cancel","id":8}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    let after = revision.current();
    assert!(
        after > before,
        "animate_cancel must bump the OCC token (before={before}, after={after})",
    );
}

