use super::*;

// ─────────────────────────────────────────────────────────────────
// R598 §5.50 — scene/theme_tokens wire integration
//
// The fine-grained handler logic is covered by the test battery in
// `crate::theme::tests`. The cases below exercise the
// dispatcher's wire round-trip — params parsing, runtime_owner
// injection through the builder, and error → RpcError mapping.
// ─────────────────────────────────────────────────────────────────

#[test]
fn r598_scene_theme_tokens_without_runtime_owner_errors() {
    // No runtime_owner attached on the context — the handler must
    // surface `theme view unavailable` as invalid_params with the
    // typed variant name in `data`.
    let mut scene = counted_scene(0);
    let req = r#"{"jsonrpc":"2.0","method":"scene/theme_tokens","id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(
        err.data,
        Some(Value::String("RuntimeOwnerUnavailable".into()))
    );
}

#[test]
fn r598_scene_theme_tokens_unbound_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/theme_tokens","id":2}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("unbound tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("NotBound".into())));
}

#[test]
fn r598_scene_theme_tokens_returns_palette_catalogue_via_dispatch() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    // Bind the default-tag ThemeProvider so the dispatcher hits
    // the happy path.
    let _provider = owner.run(|| pinion_core::theme::use_theme("app"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/theme_tokens","id":3}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none(), "happy path must not error");
    let result = resp.result.expect("result present");
    assert_eq!(result.get("tag").and_then(Value::as_str), Some("app"));
    assert!(result.get("mode").and_then(Value::as_str).is_some());
    assert!(result["palettes"]["light"].is_array());
    assert!(result["palettes"]["dark"].is_array());
    assert!(!result["palettes"]["light"].as_array().unwrap().is_empty());
}

#[test]
fn r598_scene_theme_tokens_rejects_non_string_tag() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _provider = owner.run(|| pinion_core::theme::use_theme("app"));
    // params.tag is a number, not a string — invalid_params.
    let req = r#"{"jsonrpc":"2.0","method":"scene/theme_tokens","params":{"tag":42},"id":4}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("non-string tag must error");
    assert_eq!(err.code, -32602);
}

#[test]
fn r598_scene_theme_tokens_custom_tag_round_trips() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _provider = owner.run(|| pinion_core::theme::use_theme("custom-scope"));
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/theme_tokens","params":{"tag":"custom-scope"},"id":5}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none());
    assert_eq!(
        resp.result.unwrap().get("tag").and_then(Value::as_str),
        Some("custom-scope"),
    );
}

// ─────────────────────────────────────────────────────────────────
// R599 §5.50 — scene/set_theme_mode wire integration
//
// Mutation pair to scene/theme_tokens. The fine-grained handler
// logic is covered by the test battery in
// `crate::theme::tests::r599_*`. The cases below exercise the
// dispatcher's wire round-trip — params parsing, runtime_owner
// injection, error → RpcError mapping, and the
// HandlerKind::Mutate match-arm OCC bump (R620).
// ─────────────────────────────────────────────────────────────────

#[test]
fn r599_scene_set_theme_mode_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/set_theme_mode","params":{"mode":"dark"},"id":1}"#;
    let resp = parse_response(&dispatch_t(&mut scene, req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(
        err.data,
        Some(Value::String("RuntimeOwnerUnavailable".into()))
    );
}

#[test]
fn r599_scene_set_theme_mode_missing_mode_param_errors() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _provider = owner.run(|| pinion_core::theme::use_theme("app"));
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_theme_mode","params":{},"id":2}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing mode must error");
    assert_eq!(err.code, -32602);
}

#[test]
fn r599_scene_set_theme_mode_rejects_unknown_mode_slug() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _provider = owner.run(|| pinion_core::theme::use_theme("app"));
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/set_theme_mode","params":{"mode":"AUTO"},"id":3}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("unknown mode slug must error");
    assert_eq!(err.code, -32602);
}

#[test]
fn r599_scene_set_theme_mode_happy_path_returns_post_state() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let provider = owner.run(|| pinion_core::theme::use_theme("app"));
    provider.set_mode(pinion_core::theme::ThemeMode::Light);
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/set_theme_mode","params":{"mode":"dark"},"id":4}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    assert!(resp.error.is_none());
    let result = resp.result.expect("result present");
    assert_eq!(result.get("mode").and_then(Value::as_str), Some("dark"));
    assert_eq!(result.get("active").and_then(Value::as_str), Some("dark"));
    assert_eq!(result.get("tag").and_then(Value::as_str), Some("app"));
    // Provider state is the post-mutation value.
    assert_eq!(provider.mode(), pinion_core::theme::ThemeMode::Dark);
}

#[test]
fn r599_scene_set_theme_mode_unbound_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    // No `use_theme` binding — the cache_contains gate trips.
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/set_theme_mode","params":{"mode":"dark"},"id":5}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("unbound tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("NotBound".into())));
}

#[test]
fn r599_scene_set_theme_mode_bumps_revision_on_success() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _provider = owner.run(|| pinion_core::theme::use_theme("app"));
    let revision = SceneRevision::default();
    let before = revision.current();
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/set_theme_mode","params":{"mode":"dark"},"id":6}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    let after = revision.current();
    assert!(
        after > before,
        "set_theme_mode must bump the OCC token (before={before}, after={after})",
    );
}

#[test]
fn r599_scene_set_theme_mode_does_not_bump_revision_on_failure() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    // No theme bound → handler errors. The dispatcher's
    // HandlerKind::Mutate arm only bumps on Ok; Err must not.
    let revision = SceneRevision::default();
    let before = revision.current();
    let req =
        r#"{"jsonrpc":"2.0","method":"scene/set_theme_mode","params":{"mode":"dark"},"id":7}"#;
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, req);
    assert_eq!(
        revision.current(),
        before,
        "a failed set_theme_mode must not bump the OCC token",
    );
}

// ─────────────────────────────────────────────────────────────────
// R608 §5.50 — scene/set_theme_palettes wire integration
//
// Mutation pair to scene/theme_tokens for the palette pair (the
// mode pair lives at scene/set_theme_mode). The fine-grained
// handler logic + parse-error battery is covered in
// `crate::theme::tests::r608_*`. The cases below exercise the
// dispatcher's wire round-trip — required `light`/`dark` params,
// typed parse-error data, runtime_owner injection, and the
// HandlerKind::Mutate match-arm OCC bump (R620).
// ─────────────────────────────────────────────────────────────────

/// Build a full role/color array of the canonical
/// [`pinion_core::theme::Theme::light`] palette, for use as the
/// `params.light` value inside a wire-shape JSON request.
fn light_palette_json_array() -> serde_json::Value {
    use pinion_core::theme::{ColorRole, Theme};
    let theme = Theme::light();
    let entries: Vec<serde_json::Value> = ColorRole::all()
        .iter()
        .map(|role| {
            let c = theme.resolve(*role);
            let color = if c.a == 0xff {
                format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
            } else {
                format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
            };
            serde_json::json!({ "role": role.name(), "color": color })
        })
        .collect();
    serde_json::Value::Array(entries)
}

fn dark_palette_json_array() -> serde_json::Value {
    use pinion_core::theme::{ColorRole, Theme};
    let theme = Theme::dark();
    let entries: Vec<serde_json::Value> = ColorRole::all()
        .iter()
        .map(|role| {
            let c = theme.resolve(*role);
            let color = if c.a == 0xff {
                format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
            } else {
                format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
            };
            serde_json::json!({ "role": role.name(), "color": color })
        })
        .collect();
    serde_json::Value::Array(entries)
}

/// Build a full `{light: [...], dark: [...]}` params payload.
fn full_palettes_params() -> serde_json::Value {
    serde_json::json!({
        "light": light_palette_json_array(),
        "dark": dark_palette_json_array(),
    })
}

#[test]
fn r608_scene_set_theme_palettes_without_runtime_owner_errors() {
    let mut scene = counted_scene(0);
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "scene/set_theme_palettes",
        "params": full_palettes_params(),
        "id": 1,
    })
    .to_string();
    let resp = parse_response(&dispatch_t(&mut scene, &req).unwrap());
    let err = resp.error.expect("missing runtime_owner must error");
    assert_eq!(err.code, -32602);
    assert_eq!(
        err.data,
        Some(Value::String("RuntimeOwnerUnavailable".into()))
    );
}

#[test]
fn r608_scene_set_theme_palettes_missing_params_errors() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = r#"{"jsonrpc":"2.0","method":"scene/set_theme_palettes","id":2}"#;
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, req).unwrap());
    let err = resp.error.expect("missing params must error");
    assert_eq!(err.code, -32602);
}

#[test]
fn r608_scene_set_theme_palettes_missing_light_errors() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "scene/set_theme_palettes",
        "params": {"dark": dark_palette_json_array()},
        "id": 3,
    })
    .to_string();
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, &req).unwrap());
    let err = resp.error.expect("missing light must error");
    assert_eq!(err.code, -32602);
}

#[test]
fn r608_scene_set_theme_palettes_missing_dark_errors() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "scene/set_theme_palettes",
        "params": {"light": light_palette_json_array()},
        "id": 4,
    })
    .to_string();
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, &req).unwrap());
    let err = resp.error.expect("missing dark must error");
    assert_eq!(err.code, -32602);
}

#[test]
fn r608_scene_set_theme_palettes_missing_role_surfaces_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _provider = owner.run(|| pinion_core::theme::use_theme("app"));
    // Light side bound to a 1-entry array → MissingRoles.
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "scene/set_theme_palettes",
        "params": {
            "light": [{"role": "surface", "color": "#ffffff"}],
            "dark": dark_palette_json_array(),
        },
        "id": 5,
    })
    .to_string();
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, &req).unwrap());
    let err = resp.error.expect("missing roles must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("MissingRoles".into())));
}

#[test]
fn r608_scene_set_theme_palettes_unknown_role_surfaces_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _provider = owner.run(|| pinion_core::theme::use_theme("app"));
    // Replace the first light entry with a typo'd role name.
    let mut light = light_palette_json_array();
    light[0] = serde_json::json!({"role": "Surface", "color": "#ffffff"});
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "scene/set_theme_palettes",
        "params": {"light": light, "dark": dark_palette_json_array()},
        "id": 6,
    })
    .to_string();
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, &req).unwrap());
    let err = resp.error.expect("unknown role must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("UnknownRole".into())));
}

#[test]
fn r608_scene_set_theme_palettes_invalid_color_surfaces_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _provider = owner.run(|| pinion_core::theme::use_theme("app"));
    let mut light = light_palette_json_array();
    light[0] = serde_json::json!({"role": "surface", "color": "not-a-hex"});
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "scene/set_theme_palettes",
        "params": {"light": light, "dark": dark_palette_json_array()},
        "id": 7,
    })
    .to_string();
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, &req).unwrap());
    let err = resp.error.expect("invalid color must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("InvalidColor".into())));
}

#[test]
fn r608_scene_set_theme_palettes_unbound_tag_errors_with_typed_data() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "scene/set_theme_palettes",
        "params": full_palettes_params(),
        "id": 8,
    })
    .to_string();
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, &req).unwrap());
    let err = resp.error.expect("unbound tag must error");
    assert_eq!(err.code, -32602);
    assert_eq!(err.data, Some(Value::String("NotBound".into())));
}

#[test]
fn r608_scene_set_theme_palettes_happy_path_returns_post_state() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let provider = owner.run(|| pinion_core::theme::use_theme("app"));
    // Pre-flip to Dark so we can observe `active` resolves to dark
    // after the palette swap (which preserves mode).
    provider.set_mode(pinion_core::theme::ThemeMode::Dark);
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "scene/set_theme_palettes",
        "params": full_palettes_params(),
        "id": 9,
    })
    .to_string();
    let resp = parse_response(&dispatch_with_runtime_owner(&mut scene, &owner, &req).unwrap());
    assert!(resp.error.is_none(), "happy path must not error");
    let result = resp.result.expect("result present");
    assert_eq!(result.get("mode").and_then(Value::as_str), Some("dark"));
    assert_eq!(result.get("active").and_then(Value::as_str), Some("dark"));
    assert_eq!(result.get("tag").and_then(Value::as_str), Some("app"));
    assert!(
        result
            .get("system_scheme")
            .and_then(Value::as_str)
            .is_some()
    );
}

#[test]
fn r608_scene_set_theme_palettes_bumps_revision_on_success() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    let _provider = owner.run(|| pinion_core::theme::use_theme("app"));
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "scene/set_theme_palettes",
        "params": full_palettes_params(),
        "id": 10,
    })
    .to_string();
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, &req);
    let after = revision.current();
    assert!(
        after > before,
        "set_theme_palettes must bump the OCC token (before={before}, after={after})",
    );
}

#[test]
fn r608_scene_set_theme_palettes_does_not_bump_revision_on_failure() {
    let mut scene = counted_scene(0);
    let owner = Owner::new();
    // No theme bound → handler errors. The dispatcher's
    // HandlerKind::Mutate arm only bumps on Ok; Err must not.
    let revision = SceneRevision::default();
    let before = revision.current();
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "scene/set_theme_palettes",
        "params": full_palettes_params(),
        "id": 11,
    })
    .to_string();
    let _ = dispatch_with_runtime_owner_and_revision(&mut scene, &owner, &revision, &req);
    assert_eq!(
        revision.current(),
        before,
        "a failed set_theme_palettes must not bump the OCC token",
    );
}
