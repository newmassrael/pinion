//! Concrete [`Proposal`] variants the JSON-RPC boundary materialises
//! from wire payloads (§5.34 R40.5 / R40.9).
//!
//! [`TypedProposal`] is the closed enum implementing the open
//! [`Proposal`] trait. Variants are added one at a time per sub-slice:
//! R40.5 landed `SetSignal` (scalar reactive write); R40.9 adds
//! `DispatchIntent` (intent-stream emission). `ReplaceView` and
//! `SetStyle` arrive in subsequent R40.x sub-slices.
//!
//! The enum is `#[non_exhaustive]` so adding variants is non-breaking
//! per Hyrum / Bloch API-evolution conventions.

use pinion_core::Scene;
use pinion_core::intent::Intent;
use pinion_core::style::BoxStyle;

use crate::rewind::{RewindError, rewind};

use super::{ApplyContext, Proposal, ViewBlueprint};

/// Closed enum of typed proposals carried by the preview ledger
/// (§5.34 R40.5+). Each variant carries its own payload shape; the
/// shared [`Proposal`] surface (`target_path` / `affected_paths`) is
/// derived from the variant.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum TypedProposal {
    /// Set a reactive [`pinion_core::Signal`] to a new value while a
    /// preview is in flight. `target_path` is the scene anchor the
    /// AI agent is reasoning about (typically the widget whose render
    /// output reflects the signal); `signal_path` is the addressable
    /// signal id within the scene (e.g. `/button[main]/count`); the
    /// JSON value is the proposed new state. Type coercion (i64 / f64
    /// / String / JSON) happens at apply time (R40.6) so the ledger
    /// can store an opaque payload without committing to the
    /// signal's `T` here.
    SetSignal {
        /// Scene path the AI agent anchors on (overlay highlight,
        /// `scene/bbox` lookup target).
        target_path: String,
        /// Addressable signal id. Resolved against the live scene at
        /// apply time; mismatch surfaces as
        /// `crate::preview::ApplyError::BaseRevisionConflict` only if
        /// the `scene_revision` moved — a missing signal is a separate
        /// apply-time error (R40.6).
        signal_path: String,
        /// New value. Stored verbatim as `serde_json::Value`; type
        /// coercion is the apply step's responsibility.
        value: serde_json::Value,
    },
    /// Emit an [`Intent`] into the apply-time intent accumulator
    /// (§5.34 R40.9). The intent is surfaced in
    /// [`ApplyOutcome::emitted_intents`](super::ApplyOutcome) so the
    /// wire caller observes it on the apply response without going
    /// through the `scene/intents` poll path. `target_path` carries
    /// the scene anchor the AI agent is reasoning about (which widget
    /// the proposed intent is "for"); the intent's own
    /// [`Intent::tag`] already encodes the receiver per §5.20 R22
    /// (`<widget>.<kind>`).
    ///
    /// Unlike [`Self::SetSignal`], this variant does **not** mutate
    /// the scene tree. `affected_paths` still returns `[target_path]`
    /// so overlay-highlight / dirty-region consumers see the same
    /// anchor a `SetSignal` at the same path would emit.
    DispatchIntent {
        /// Scene anchor the AI agent is reasoning about. Surfaced in
        /// `list_previews` and used for overlay highlighting; does
        /// not gate intent delivery.
        target_path: String,
        /// The intent payload to emit. Copied into
        /// [`ApplyContext::emitted_intents`](super::ApplyContext)
        /// once at apply time.
        intent: Intent,
    },
    /// Replace the [`BoxStyle`] sidecar of a `Scene::Box` or
    /// `Scene::Container` at `target_path` (§5.34 R40.10).
    ///
    /// The walk goes through [`Scene::lookup_path_mut`]; segments
    /// resolve by tag-first / index-second per
    /// [`Scene::hit_test`](pinion_core::Scene::hit_test) conventions.
    /// Non-stylable variants (`Text`, `Path`, `Image`, `Effect`,
    /// `External`) at the resolved path surface
    /// `"UnsupportedStyleTarget"` via [`ApplyError::ApplyRejected`](super::ApplyError::ApplyRejected).
    ///
    /// v0 carries a full [`BoxStyle`] — every field (`fill`, `border`,
    /// `corner_radius`) is replaced atomically. A partial-patch shape
    /// (`SetStyleField` / per-field deltas) is future R40.x territory;
    /// stays consistent with the §5.34 "all-or-nothing apply" caveat.
    SetStyle {
        /// Scene path the proposal walks to locate the target node.
        /// Same shape as `SetSignal::target_path` but here the walk
        /// is the side-effect, not just an anchor.
        target_path: String,
        /// Full replacement style. Variants that already have a
        /// `BoxStyle` (`Box`, `Container`) overwrite their sidecar;
        /// other variants fail apply with `"UnsupportedStyleTarget"`.
        style: BoxStyle,
    },
    /// Swap the scene subtree at `target_path` for the materialised
    /// form of `replacement` (§5.34 R40.11). Closes the §5.34 four-
    /// variant set (`SetSignal` / `DispatchIntent` / `SetStyle` /
    /// `ReplaceView`).
    ///
    /// The replacement is carried as a [`ViewBlueprint`] (not a
    /// `Scene`) because `Scene` is intentionally `!Send + !Sync +
    /// !Clone` — `ExternalNode` owns a `Box<dyn External>` without
    /// those bounds. A blueprint is the textbook bridge: closed-form
    /// description that materialises into a `Scene` exactly once at
    /// apply time, preserving the [`Proposal`] trait's `Send + Sync +
    /// 'static` bound.
    ///
    /// Walks through [`Scene::lookup_path_mut`]. An empty/root path
    /// (`/window[main]`) replaces the whole scene; non-existent
    /// paths surface `"UnknownTarget"` via
    /// [`ApplyError::ApplyRejected`](super::ApplyError::ApplyRejected).
    ReplaceView {
        /// Scene path identifying the subtree to swap. Same walk
        /// semantics as `SetStyle` (tag-first, index-second).
        target_path: String,
        /// New subtree description. Consumed exactly once by
        /// [`ViewBlueprint::materialize`] at apply time.
        replacement: ViewBlueprint,
    },
}

impl Proposal for TypedProposal {
    fn target_path(&self) -> &str {
        match self {
            Self::SetSignal { target_path, .. }
            | Self::DispatchIntent { target_path, .. }
            | Self::SetStyle { target_path, .. }
            | Self::ReplaceView { target_path, .. } => target_path,
        }
    }

    fn affected_paths(&self) -> Vec<String> {
        match self {
            Self::SetSignal { target_path, .. }
            | Self::DispatchIntent { target_path, .. }
            | Self::SetStyle { target_path, .. }
            | Self::ReplaceView { target_path, .. } => vec![target_path.clone()],
        }
    }

    fn apply(&self, ctx: &mut ApplyContext<'_>) -> Result<(), String> {
        match self {
            Self::SetSignal {
                signal_path, value, ..
            } => apply_set_signal(ctx.scene, signal_path, value),
            Self::DispatchIntent { intent, .. } => {
                ctx.emitted_intents.push(intent.clone());
                Ok(())
            }
            Self::SetStyle {
                target_path, style, ..
            } => apply_set_style(ctx.scene, target_path, style.clone()),
            Self::ReplaceView {
                target_path,
                replacement,
            } => apply_replace_view(ctx.scene, target_path, replacement.clone()),
        }
    }
}

fn apply_replace_view(
    scene: &mut Scene,
    target_path: &str,
    replacement: ViewBlueprint,
) -> Result<(), String> {
    let segments = scene_segments(target_path);
    let Some(node) = scene.lookup_path_mut(&segments) else {
        return Err("UnknownTarget".to_string());
    };
    *node = replacement.materialize();
    Ok(())
}

fn apply_set_style(scene: &mut Scene, target_path: &str, style: BoxStyle) -> Result<(), String> {
    let segments = scene_segments(target_path);
    let Some(node) = scene.lookup_path_mut(&segments) else {
        return Err("UnknownTarget".to_string());
    };
    match node {
        Scene::Box(b) => {
            b.style = style;
            Ok(())
        }
        Scene::Container(c) => {
            c.style = style;
            Ok(())
        }
        // Text / Path / Image carry their own style sidecar shape;
        // Effect / External have no BoxStyle. Stay variant-aware so a
        // future SetStyle widening (Text colour, Path stroke, etc.)
        // remains an additive R40.x sub-slice.
        _ => Err("UnsupportedStyleTarget".to_string()),
    }
}

/// Split a `target_path` into segments, stripping any leading
/// `/window[<id>]/` prefix. Mirrors the helper in
/// [`crate::path::resolve`] but without window-topology resolution —
/// the caller already trusts the path is in-window; only the segment
/// list is needed for `lookup_path_mut`.
fn scene_segments(target_path: &str) -> Vec<String> {
    let scene_path = match target_path.strip_prefix("/window[") {
        Some(rest) => match rest.find(']') {
            Some(close) => &rest[close + 1..],
            None => target_path,
        },
        None => target_path,
    };
    scene_path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn apply_set_signal(
    scene: &mut Scene,
    signal_path: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    let intro_value =
        json_to_introspect_value(value).ok_or_else(|| "UnsupportedValueShape".to_string())?;
    rewind(scene, signal_path, intro_value).map_err(rewind_error_tag)
}

fn rewind_error_tag(err: RewindError) -> String {
    match err {
        // R1386 — forward the inner PathError reason via the shared
        // `PathError::wire_tag` SSOT, not the collapsed blanket "Path".
        RewindError::Path(inner) => inner.wire_tag().to_string(),
        RewindError::UnsupportedPath => "UnsupportedPath".to_string(),
        RewindError::NoExternalAtPath => "NoExternalAtPath".to_string(),
        RewindError::IntrospectionOptedOut => "IntrospectionOptedOut".to_string(),
        RewindError::Intervene(_) => "Intervene".to_string(),
    }
}

fn json_to_introspect_value(
    v: &serde_json::Value,
) -> Option<pinion_core::external::IntrospectValue> {
    use pinion_core::external::IntrospectValue;
    match v {
        serde_json::Value::Null => Some(IntrospectValue::Null),
        serde_json::Value::Bool(b) => Some(IntrospectValue::Bool(*b)),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(IntrospectValue::Int)
            .or_else(|| n.as_f64().map(IntrospectValue::Float)),
        serde_json::Value::String(s) => Some(IntrospectValue::Text(s.clone())),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Some(IntrospectValue::Json(v.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Color;
    use pinion_core::external::IntrospectValue;
    use pinion_core::scene::{BoxNode, Rect};

    fn dummy_scene() -> Scene {
        Scene::Box(BoxNode::filled(Rect::default(), Color::default()))
    }

    #[test]
    fn set_signal_target_path_is_borrowed() {
        let p = TypedProposal::SetSignal {
            target_path: "/widget[counter]".to_string(),
            signal_path: "/widget[counter]/count".to_string(),
            value: serde_json::json!(42),
        };
        assert_eq!(p.target_path(), "/widget[counter]");
    }

    #[test]
    fn set_signal_affected_paths_contains_target() {
        let p = TypedProposal::SetSignal {
            target_path: "/anchor".to_string(),
            signal_path: "/some/signal".to_string(),
            value: serde_json::json!(true),
        };
        let affected = p.affected_paths();
        assert_eq!(affected, vec!["/anchor".to_string()]);
    }

    #[test]
    fn set_signal_clones_cheaply_via_derive() {
        let p = TypedProposal::SetSignal {
            target_path: "/a".to_string(),
            signal_path: "/b".to_string(),
            value: serde_json::json!("hello"),
        };
        let q = p.clone();
        // Ensure both still report the same paths after Clone.
        assert_eq!(p.target_path(), q.target_path());
        assert_eq!(p.affected_paths(), q.affected_paths());
    }

    #[test]
    fn set_signal_bad_window_path_surfaces_concrete_reason() {
        // R1386 — the preview `set_signal` apply path shares the
        // `PathError::wire_tag` reason SSOT: a mistyped window id surfaces
        // the concrete `UnknownWindow`, never the collapsed blanket "Path"
        // (this surface used to hand-roll its own `Path => "Path"` tag).
        let mut scene = dummy_scene();
        let err = apply_set_signal(
            &mut scene,
            "/window[nope]/external/count",
            &serde_json::json!(1),
        )
        .unwrap_err();
        // R1387 — the reason echoes the offending id, staying prefix-matchable.
        assert_eq!(err, "UnknownWindow: \"nope\"");
        assert!(err.starts_with("UnknownWindow") && err != "Path");
    }

    #[test]
    fn dispatch_intent_target_path_is_borrowed() {
        let p = TypedProposal::DispatchIntent {
            target_path: "/widget[counter]".to_string(),
            intent: Intent::new_static("counter.click", IntrospectValue::Null),
        };
        assert_eq!(p.target_path(), "/widget[counter]");
    }

    #[test]
    fn dispatch_intent_affected_paths_contains_target_only() {
        // §5.34 R40.9: DispatchIntent does not mutate scene, so the
        // anchor path is the entire affected set. Overlay/dirty
        // consumers still get a single-entry list for consistency
        // with SetSignal.
        let p = TypedProposal::DispatchIntent {
            target_path: "/btn".to_string(),
            intent: Intent::new_static("btn.click", IntrospectValue::Null),
        };
        assert_eq!(p.affected_paths(), vec!["/btn".to_string()]);
    }

    #[test]
    fn dispatch_intent_apply_pushes_into_emitted_intents() {
        // apply receives an ApplyContext with an empty intent buffer;
        // the variant must push exactly one intent and not touch
        // ctx.scene.
        let mut scene = dummy_scene();
        let mut ctx = ApplyContext::new(&mut scene);
        let p = TypedProposal::DispatchIntent {
            target_path: "/btn".to_string(),
            intent: Intent::new_static("btn.click", IntrospectValue::Int(7)),
        };
        p.apply(&mut ctx).unwrap();
        assert_eq!(ctx.emitted_intents.len(), 1);
        assert_eq!(ctx.emitted_intents[0].tag_str(), "btn.click");
        assert_eq!(ctx.emitted_intents[0].payload, IntrospectValue::Int(7));
    }

    #[test]
    fn dispatch_intent_apply_does_not_mutate_scene() {
        // Variant semantics guard: emitting an intent must leave the
        // scene tree byte-identical. Constructed Box → Box equality
        // works since BoxNode is Clone and the variant has no
        // signal_path / rewind side-effect.
        let initial = dummy_scene();
        let mut scene = dummy_scene();
        let mut ctx = ApplyContext::new(&mut scene);
        let p = TypedProposal::DispatchIntent {
            target_path: "/btn".to_string(),
            intent: Intent::new_static("btn.click", IntrospectValue::Null),
        };
        p.apply(&mut ctx).unwrap();
        // Use Rect equality as a structural marker since Scene itself
        // is not PartialEq.
        assert_eq!(scene.rect(), initial.rect());
        assert_eq!(scene.tag(), initial.tag());
    }

    #[test]
    fn dispatch_intent_clones_cheaply_via_derive() {
        let p = TypedProposal::DispatchIntent {
            target_path: "/a".to_string(),
            intent: Intent::new_owned("a.x".to_string(), IntrospectValue::Bool(true)),
        };
        let q = p.clone();
        assert_eq!(p.target_path(), q.target_path());
    }

    // ---- R40.10: SetStyle ----

    fn red_style() -> BoxStyle {
        BoxStyle::filled(Color::from_argb(0x00ff_0000))
    }

    fn green_style() -> BoxStyle {
        BoxStyle::filled(Color::from_argb(0x0000_ff00))
    }

    fn tagged_box_scene(tag: &'static str, fill: Color) -> Scene {
        Scene::Box(BoxNode::filled(Rect::new(0, 0, 10, 10), fill).with_tag(tag))
    }

    fn container_with(children: Vec<Scene>) -> Scene {
        use pinion_core::scene::ContainerNode;
        let mut c = ContainerNode::new(children);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn set_style_target_and_affected_paths() {
        let p = TypedProposal::SetStyle {
            target_path: "/info_panel".to_string(),
            style: red_style(),
        };
        assert_eq!(p.target_path(), "/info_panel");
        assert_eq!(p.affected_paths(), vec!["/info_panel".to_string()]);
    }

    #[test]
    fn set_style_mutates_tagged_box_fill() {
        // Walk by tag → BoxNode at /btn → style replaced wholesale.
        let mut scene = container_with(vec![tagged_box_scene("btn", Color::default())]);
        let mut ctx = ApplyContext::new(&mut scene);
        let p = TypedProposal::SetStyle {
            target_path: "/btn".to_string(),
            style: green_style(),
        };
        p.apply(&mut ctx).unwrap();
        if let Scene::Container(c) = &scene {
            if let Scene::Box(b) = &c.children[0] {
                assert_eq!(b.style.fill, Color::from_argb(0x0000_ff00));
            } else {
                panic!("child not Box");
            }
        } else {
            panic!("scene not Container");
        }
    }

    #[test]
    fn set_style_with_window_prefix_works() {
        // §5.18 window-prefixed path resolves the same way as the
        // implicit form — the wire caller does not need to know
        // whether to strip the prefix.
        let mut scene = container_with(vec![tagged_box_scene("btn", Color::default())]);
        let mut ctx = ApplyContext::new(&mut scene);
        let p = TypedProposal::SetStyle {
            target_path: "/window[main]/btn".to_string(),
            style: red_style(),
        };
        p.apply(&mut ctx).unwrap();
    }

    #[test]
    fn set_style_unknown_path_returns_unknown_target() {
        let mut scene = container_with(vec![tagged_box_scene("btn", Color::default())]);
        let mut ctx = ApplyContext::new(&mut scene);
        let p = TypedProposal::SetStyle {
            target_path: "/ghost".to_string(),
            style: red_style(),
        };
        assert_eq!(p.apply(&mut ctx).unwrap_err(), "UnknownTarget");
    }

    #[test]
    fn set_style_on_unsupported_variant_returns_unsupported() {
        // External does not carry BoxStyle — apply must reject so a
        // future widening (Text colour, etc.) stays an additive
        // sub-slice rather than a silent corruption.
        use pinion_core::external::StubExternal;
        use pinion_core::scene::ExternalNode;
        let mut scene = container_with(vec![
            Scene::External(ExternalNode::new(Box::new(StubExternal::new())))
                .with_tag_unused_placeholder(),
        ]);
        // No tag on the ExternalNode → addressed by index "0".
        // (with_tag_unused_placeholder is a fluent no-op; written
        // this way to make the intent clear: the External lacks a
        // BoxStyle slot and SetStyle must refuse it.)
        let mut ctx = ApplyContext::new(&mut scene);
        let p = TypedProposal::SetStyle {
            target_path: "/0".to_string(),
            style: red_style(),
        };
        assert_eq!(p.apply(&mut ctx).unwrap_err(), "UnsupportedStyleTarget");
    }

    /// Tiny ergonomics shim used inside the unsupported-variant test
    /// to make the call site read as a single chain. Returns the
    /// scene unchanged.
    trait FluentScene {
        fn with_tag_unused_placeholder(self) -> Scene;
    }
    impl FluentScene for Scene {
        fn with_tag_unused_placeholder(self) -> Scene {
            self
        }
    }

    #[test]
    fn set_style_mutates_container_style() {
        // Container also carries BoxStyle (R24 slice 5). SetStyle at
        // the container's path must update its sidecar.
        let mut scene = container_with(vec![]);
        let mut ctx = ApplyContext::new(&mut scene);
        let p = TypedProposal::SetStyle {
            target_path: "/".to_string(),
            style: red_style(),
        };
        p.apply(&mut ctx).unwrap();
        if let Scene::Container(c) = &scene {
            assert_eq!(c.style.fill, Color::from_argb(0x00ff_0000));
        } else {
            panic!("root not container");
        }
    }

    // ---- R40.11: ReplaceView ----

    fn box_blueprint(tag: &str, fill: u32) -> ViewBlueprint {
        ViewBlueprint::Box {
            rect: Rect::new(0, 0, 10, 10),
            style: BoxStyle::filled(Color::from_argb(fill)),
            tag: Some(tag.to_string()),
        }
    }

    #[test]
    fn replace_view_target_and_affected_paths() {
        let p = TypedProposal::ReplaceView {
            target_path: "/info_panel".to_string(),
            replacement: box_blueprint("replaced", 0x00ff_0000),
        };
        assert_eq!(p.target_path(), "/info_panel");
        assert_eq!(p.affected_paths(), vec!["/info_panel".to_string()]);
    }

    #[test]
    fn replace_view_swaps_tagged_subtree() {
        let mut scene = container_with(vec![tagged_box_scene(
            "old_btn",
            Color::from_argb(0x0000_0000),
        )]);
        let mut ctx = ApplyContext::new(&mut scene);
        let p = TypedProposal::ReplaceView {
            target_path: "/old_btn".to_string(),
            replacement: box_blueprint("new_btn", 0x00ab_cdef),
        };
        p.apply(&mut ctx).unwrap();
        if let Scene::Container(c) = &scene {
            if let Scene::Box(b) = &c.children[0] {
                assert_eq!(b.tag.as_deref(), Some("new_btn"));
                assert_eq!(b.style.fill, Color::from_argb(0x00ab_cdef));
            } else {
                panic!("child 0 not Box");
            }
        }
    }

    #[test]
    fn replace_view_swaps_root_when_target_is_empty() {
        // Empty / root target → lookup_path_mut returns the scene
        // itself; replacing it must rewrite the root in place.
        let mut scene = container_with(vec![]);
        let mut ctx = ApplyContext::new(&mut scene);
        let p = TypedProposal::ReplaceView {
            target_path: "/".to_string(),
            replacement: box_blueprint("brand_new_root", 0x0011_2233),
        };
        p.apply(&mut ctx).unwrap();
        assert!(matches!(scene, Scene::Box(_)));
        assert_eq!(scene.tag(), Some("brand_new_root"));
    }

    #[test]
    fn replace_view_unknown_path_returns_unknown_target() {
        let mut scene =
            container_with(vec![tagged_box_scene("btn", Color::from_argb(0x0000_0000))]);
        let mut ctx = ApplyContext::new(&mut scene);
        let p = TypedProposal::ReplaceView {
            target_path: "/ghost".to_string(),
            replacement: box_blueprint("ignored", 0),
        };
        assert_eq!(p.apply(&mut ctx).unwrap_err(), "UnknownTarget");
    }

    #[test]
    fn replace_view_with_nested_container_blueprint_materializes() {
        let mut scene = container_with(vec![tagged_box_scene("placeholder", Color::default())]);
        let mut ctx = ApplyContext::new(&mut scene);
        let p = TypedProposal::ReplaceView {
            target_path: "/placeholder".to_string(),
            replacement: ViewBlueprint::Container {
                rect: Rect::new(0, 0, 40, 40),
                style: BoxStyle::default(),
                tag: Some("new_panel".to_string()),
                children: vec![box_blueprint("inner", 0x00ff_ff00)],
            },
        };
        p.apply(&mut ctx).unwrap();
        if let Scene::Container(c) = &scene {
            if let Scene::Container(inner) = &c.children[0] {
                assert_eq!(inner.tag.as_deref(), Some("new_panel"));
                assert_eq!(inner.children.len(), 1);
                assert_eq!(inner.children[0].tag(), Some("inner"));
            } else {
                panic!("not Container");
            }
        }
    }

    #[test]
    fn replace_view_clones_via_derive() {
        // TypedProposal must retain its Clone derive — the ledger
        // does not rely on Clone, but downstream code may (and the
        // existing variants test for it). ViewBlueprint::Clone bound
        // is what makes this still hold after R40.11.
        let p = TypedProposal::ReplaceView {
            target_path: "/x".to_string(),
            replacement: box_blueprint("y", 0),
        };
        let q = p.clone();
        assert_eq!(p.target_path(), q.target_path());
    }
}
