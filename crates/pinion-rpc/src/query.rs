//! `scene/query` RPC method dispatch (§5.12 hybrid query, R16 slice 9,
//! R42 nested External addressing).
//!
//! Wires three pieces together:
//!
//!   1. **§5.18 path resolution** — `/[window[id]/]<scene_path>` with
//!      single-window short-circuit (see [`crate::path`]).
//!   2. **§5.2 scene tree walk** — R42: the `/external/` literal acts
//!      as the separator between scene-walk segments and the introspect
//!      path. `/external/<intro>` keeps the v0 root-External shape;
//!      `/<seg>/.../external/<intro>` walks Container/Box descendants
//!      to find an `ExternalNode` before descending.
//!   3. **§5.15 item 8 introspect dispatch** — when the resolved
//!      target is an `ExternalNode`, the call descends through
//!      [`External::introspect`](pinion_core::external::External::introspect)
//!      and consults the [`ExternalIntrospect`] surface.
//!
//! Scene-path syntax accepted (R42): `/[<scene_segments>/]external/<introspect_path>`,
//! optionally preceded by `/window[id]/`. Other shapes return
//! [`QueryError::UnsupportedPath`].
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) is a separate slice — this
//! module exposes the typed dispatcher only.

use pinion_core::Scene;
use pinion_core::external::{ExternalIntrospect, IntrospectValue, SchemaChannel};
use serde_json::{Value, json};

use crate::origin::{AnswerOrigin, QueryRefusal, SceneSource};
use crate::path::PathError;
use crate::resolve::{
    ResolveExternalError, introspect_at, lookup_addressed, resolve_external_path,
};

/// R825 §5.12 — reserved introspect path that returns an external's
/// **declared schema** (every queryable path + its type tag) instead of a
/// value. The discovery primitive under the whole introspection surface: a
/// plain `scene/query` reads one *known* path, and `scene/snapshot` shows
/// the current value of each *scalar* path — but neither reveals the
/// **contract**. Querying `/<tag>/external/$schema` returns the full
/// `IntrospectSchema` as JSON, so an AI client discovers what it can ask for
/// without hard-coded knowledge ([[ai-first-rpc-introspection-obligation]]).
/// The `$` prefix cannot collide with a real introspect path (no widget
/// declares one).
///
/// R1353 — this is where **parametric** paths (`id_at.<pos>`, `width.<col>`,
/// `cell.<row>.<col>`) become discoverable rather than merely mentioned. They
/// are absent from a snapshot by declaration (a family is unbounded, so
/// expanding it would be a table scan), which used to make `$schema` their only
/// witness while `$schema` itself could not say they were parametric — it
/// rendered `("width","int")` exactly like the scalar `("total","int")`. The
/// render now carries the template and a typed `args` entry per placeholder;
/// see [`pinion_core::external::SchemaField`] for the declaration and for why
/// the under-declaration cost a consumer a livelock.
pub const SCHEMA_PATH: &str = "$schema";

/// R828 §5.12 — the schema-or-value decision SSOT shared by the
/// retained-`External` and immediate-mode-driver query branches. The
/// reserved `$schema` path (R825) returns the declared schema (discovery);
/// any other path reads the value, or fails [`QueryError::UnknownIntrospectPath`].
/// One site so the two branches cannot diverge on discovery support — a
/// new reserved path is handled for both at once.
fn introspect_or_schema(
    intro: &dyn ExternalIntrospect,
    introspect_path: &str,
) -> Result<IntrospectValue, QueryError> {
    if introspect_path == SCHEMA_PATH {
        Ok(schema_value(intro))
    } else {
        intro
            .query(introspect_path)
            .ok_or(QueryError::UnknownIntrospectPath)
    }
}

/// Render an external's declared schema as a JSON array of `{"path", "type"}`
/// objects, in the schema's declared field order.
///
/// R1353 §2 #2 — a **parametric** field additionally carries `"args"` (an
/// array, one entry per placeholder), and its `"path"` is the wire TEMPLATE
/// (`"width.<col>"`). The template is forwarded verbatim from the declared
/// `SchemaField::path` — nothing is composed here. What keeps the template
/// honest against the `query` impl's own `strip_prefix` is
/// `r1353_1_every_real_declaration_matches_its_template`, which scans every
/// declaration's source; this function only renders.
///
/// Why both a template and structured `args`: the template is what a reader
/// (human or model) needs to see the shape at a glance; the `arg` object is what
/// a client needs to act without parsing prose — the argument's name, its type,
/// and where its valid values come from. Before R1353 the schema said only
/// `{"path":"width","type":"int"}`, indistinguishable from a scalar like
/// `total`, so an agent had to guess that an argument existed at all. See
/// [`pinion_core::external::SchemaField`] for what that guess cost.
fn schema_value(intro: &dyn ExternalIntrospect) -> IntrospectValue {
    let fields: Vec<Value> = intro
        .schema()
        .fields
        .iter()
        .map(|f| {
            // R1504 — the channel, present only where it is not the default.
            // A reader that does not know the key sees the shape it always saw;
            // one that does can tell `set_section_alignment` from a path it can
            // read, which before this was a guess from the name's prefix.
            let channel = match f.channel {
                SchemaChannel::Invoke => Some("invoke"),
                _ => None,
            };
            if f.args.is_empty() {
                return match channel {
                    Some(c) => json!({ "path": f.path, "type": f.ty, "channel": c }),
                    None => json!({ "path": f.path, "type": f.ty }),
                };
            }
            let args: Vec<Value> = f
                .args
                .iter()
                .map(|a| {
                    json!({
                        "name": a.name,
                        "type": a.ty,
                        // Rendered by the enum itself — see `ArgDomain::to_wire`
                        // for why the match does not live here.
                        "domain": a.domain.to_wire(),
                    })
                })
                .collect();
            json!({ "path": f.path, "type": f.ty, "args": args })
        })
        .collect();
    IntrospectValue::Json(Value::Array(fields))
}

/// Reasons the typed [`query`] dispatcher can fail.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// Window-prefix parsing failed (see [`PathError`]).
    Path(PathError),
    /// Scene path does not match a v0-supported shape.
    UnsupportedPath,
    /// Path expected an `External` at the scene root, found a different
    /// primitive.
    NoExternalAtPath,
    /// The `External` did not opt in to §5.15 item 8 introspection.
    IntrospectionOptedOut,
    /// `External` opted in, but the introspect path is not in its schema.
    UnknownIntrospectPath,
}

impl From<PathError> for QueryError {
    fn from(err: PathError) -> Self {
        QueryError::Path(err)
    }
}

impl From<ResolveExternalError> for QueryError {
    fn from(err: ResolveExternalError) -> Self {
        match err {
            ResolveExternalError::Path(e) => QueryError::Path(e),
            ResolveExternalError::UnsupportedPath => QueryError::UnsupportedPath,
            ResolveExternalError::NoExternalAtPath => QueryError::NoExternalAtPath,
            ResolveExternalError::IntrospectionOptedOut => QueryError::IntrospectionOptedOut,
        }
    }
}

/// Resolve `raw_path` against `scene` and return the queried value.
///
/// See module docs for the v0 path syntax. The `scene` reference is
/// borrowed for the lifetime of the call; no scene mutation occurs.
///
/// # Errors
///
/// Returns [`QueryError`] when the path is malformed, the scene root
/// does not match the path shape, or the underlying `External` rejects
/// the introspect path.
pub fn query(scene: &Scene, raw_path: &str) -> Result<IntrospectValue, QueryError> {
    query_from(scene, SceneSource::State, raw_path)
        .map(|(value, _)| value)
        .map_err(|refusal| refusal.error)
}

/// Resolve `raw_path` against `scene` and return the queried value together
/// with [what the answer is](AnswerOrigin).
///
/// `source` names the scene the caller is handing over; the walk itself is
/// identical either way. This is the same resolution [`query`] performs —
/// [`query`] is the projection that drops the origin, for the callers that
/// hold only one scene and so already know it.
///
/// # Errors
///
/// Returns a [`QueryRefusal`] carrying the same [`QueryError`] [`query`]
/// reports, plus the surface that produced it when the walk reached one.
pub fn query_from(
    scene: &Scene,
    source: SceneSource,
    raw_path: &str,
) -> Result<(IntrospectValue, AnswerOrigin), QueryRefusal> {
    // R667 §5.34 — `/window[id]/<segs>/external/<intro>` parse into the
    // borrow-free (scene_segments, introspect_path) pair.
    //
    // R1485 — this stage runs before the scene is touched at all, so its
    // refusals name no surface. Each later stage attaches the origin at the
    // point that stage *knows* what it reached, rather than a single
    // after-the-fact classifier trying to re-derive it from the variant:
    // the same variant can arrive with a surface reached or not.
    let (scene_segments, introspect_path) =
        resolve_external_path(raw_path).map_err(QueryRefusal::unreached)?;
    // R828 §2 #4 §5.12 — immediate-mode driver introspect branch. An
    // [`Scene::ImmediateModeNode`] holds its driver behind
    // `Rc<RefCell<dyn ImmediateMode>>` (interior mutability), so unlike a
    // `Box<dyn External>` it is only *transiently* borrowable — the
    // borrow cannot outlive this call. That lifetime seam is exactly why
    // R681 deferred the introspect consumer (atomic 4). It dissolves on
    // the read path: `query` returns an *owned* [`IntrospectValue`], so
    // we borrow the driver, read the value WITHIN scope, and drop the
    // borrow before returning — no `&dyn ExternalIntrospect` escapes the
    // `RefCell`. Mirrors the External branch's §5.15 introspect + R825
    // `$schema` semantics. (`intervene` / `invoke` return a borrow that
    // *does* need to outlive resolution, so immediate-mode mutation stays
    // deferred until a consumer needs it — driver state is tick-driven,
    // read-only like the R823 tree introspect.)
    if let Some(Scene::ImmediateModeNode(node)) = lookup_addressed(scene, &scene_segments) {
        // R1485 — the walk has landed on a driver, so every outcome from
        // here names it, refusal included. Binding the origin once, before
        // the branches, is what keeps a refusal and an answer from this
        // same node reporting two different surfaces.
        let origin = AnswerOrigin::of(source, true);
        let driver = node.handle.borrow();
        let intro = driver
            .introspect()
            .ok_or_else(|| QueryRefusal::from_surface(QueryError::IntrospectionOptedOut, origin))?;
        let value = introspect_or_schema(intro, &introspect_path)
            .map_err(|e| QueryRefusal::from_surface(e, origin))?;
        return Ok((value, origin));
    }
    // §5.15 retained-`External` branch — `lookup_path_ref` +
    // `primary_external` + §5.15 introspect, lifted into
    // [`introspect_at`] (R667). `$schema` (R825) is intercepted after
    // resolution so an opted-out external still reports
    // `IntrospectionOptedOut` rather than a schema.
    let origin = AnswerOrigin::of(source, false);
    // R1485 classified this by hand; R1487 lifted the classification to
    // [`QueryRefusal::from_resolve`] once `scene/invoke` and
    // `scene/intervene` needed the identical split.
    let intro =
        introspect_at(scene, &scene_segments).map_err(|e| QueryRefusal::from_resolve(e, origin))?;
    let value = introspect_or_schema(intro, &introspect_path)
        .map_err(|e| QueryRefusal::from_surface(e, origin))?;
    Ok((value, origin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Color;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    #[test]
    fn introspect_count_via_short_circuit_path() {
        let scene = counted_scene(7);
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(7),
        );
    }

    #[test]
    fn introspect_count_via_explicit_window_prefix() {
        let scene = counted_scene(11);
        assert_eq!(
            query(&scene, "/window[main]/external/count").unwrap(),
            IntrospectValue::Int(11),
        );
    }

    #[test]
    fn stub_at_root_reports_introspection_opted_out() {
        let scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        assert_eq!(
            query(&scene, "/external/count").unwrap_err(),
            QueryError::IntrospectionOptedOut,
        );
    }

    #[test]
    fn box_at_root_reports_no_external() {
        let scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        assert_eq!(
            query(&scene, "/external/count").unwrap_err(),
            QueryError::NoExternalAtPath,
        );
    }

    #[test]
    fn unsupported_scene_path_rejected() {
        let scene = counted_scene(0);
        assert_eq!(
            query(&scene, "/some/other/shape").unwrap_err(),
            QueryError::UnsupportedPath,
        );
    }

    #[test]
    fn unknown_introspect_path_propagates() {
        let scene = counted_scene(0);
        assert_eq!(
            query(&scene, "/external/ghost").unwrap_err(),
            QueryError::UnknownIntrospectPath,
        );
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        let scene = counted_scene(0);
        assert_eq!(
            query(&scene, "/window[main/external/count").unwrap_err(),
            QueryError::Path(PathError::MalformedPrefix),
        );
    }

    // ---- §5.34 R42: nested External addressing ----

    fn container_with_nested_counted(tag: &'static str, count: i64) -> Scene {
        use pinion_core::scene::{ContainerNode, Rect};
        let ext =
            Scene::External(ExternalNode::new(Box::new(CountedExternal::new(count))).with_tag(tag));
        let mut c = ContainerNode::new(vec![ext]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn query_nested_external_by_tag() {
        // R42: scene root is a Container holding a tagged ExternalNode.
        // /counter/external/count walks "counter" → finds External →
        // introspect "count". Path walker extension prevents R40.8's
        // state/paint scene workaround.
        let scene = container_with_nested_counted("counter", 42);
        assert_eq!(
            query(&scene, "/counter/external/count").unwrap(),
            IntrospectValue::Int(42),
        );
    }

    #[test]
    fn query_nested_external_with_window_prefix() {
        let scene = container_with_nested_counted("counter", 7);
        assert_eq!(
            query(&scene, "/window[main]/counter/external/count").unwrap(),
            IntrospectValue::Int(7),
        );
    }

    #[test]
    fn query_nested_external_by_index() {
        // Untagged ExternalNode addressable via positional index.
        use pinion_core::scene::{ContainerNode, ExternalNode as ExtNode, Rect};
        let ext = Scene::External(ExtNode::new(Box::new(CountedExternal::new(5))));
        let mut c = ContainerNode::new(vec![ext]);
        c.rect = Rect::new(0, 0, 100, 100);
        let scene = Scene::Container(c);
        assert_eq!(
            query(&scene, "/0/external/count").unwrap(),
            IntrospectValue::Int(5),
        );
    }

    #[test]
    fn query_nested_unknown_segment_is_no_external_at_path() {
        let scene = container_with_nested_counted("counter", 0);
        assert_eq!(
            query(&scene, "/ghost/external/count").unwrap_err(),
            QueryError::NoExternalAtPath,
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R55.D.8 §5.45 §5.7 — RPC tag-addressable path against the
    // multi-External state-scene wrap that `CoreShell::new` composes
    // when `WidgetCore::create_extra_externals` is non-empty. The
    // R55.D.5 carry-forward bullet "scene/<tag>/external/<action>
    // RPC path syntax for addressing extra Externals by name" was
    // already covered by the §5.34 R42 nested-External walker
    // (`Scene::lookup_path_ref` matches children by tag); these
    // tests pin the contract on the canonical R55.D.5 wrap shape
    // (`Scene::Container([External, External])`) so a future
    // regression surfaces against that shape specifically.
    // ─────────────────────────────────────────────────────────────────

    fn multi_external_wrap(primary_tag: &'static str, extra_tag: &'static str) -> Scene {
        // Mirror the substrate composition `CoreShell::new` produces:
        // `Container([External(primary), External(extra)])`. Both
        // children carry `CountedExternal` so we can query and tell
        // them apart by initial count.
        use pinion_core::scene::{ContainerNode, ExternalNode as ExtNode, Rect};
        let primary =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(11))).with_tag(primary_tag));
        let extra =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(22))).with_tag(extra_tag));
        let mut c = ContainerNode::new(vec![primary, extra]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn r55_d8_external_root_resolves_primary_on_wrap() {
        // R55.D.8 — `external/<action>` against the wrap descends to
        // the first External (substrate's primary by convention).
        let scene = multi_external_wrap("primary", "extra");
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(11),
            "bare external/ resolves to primary (first in DFS pre-order)",
        );
    }

    #[test]
    fn r55_d8_tag_path_resolves_extra_external() {
        // R55.D.8 — `<extra_tag>/external/<action>` walks the wrap
        // Container by the named child's tag and reaches the matching
        // External. The trailing `primary_external` descent on
        // `Scene::External` returns self, so the tagged sibling is
        // queryable by symbolic name (the AI-introspection contract
        // §5.7 needs to surface sibling state).
        let scene = multi_external_wrap("primary", "extra");
        assert_eq!(
            query(&scene, "/extra/external/count").unwrap(),
            IntrospectValue::Int(22),
            "extra/external/ resolves to the tagged sibling",
        );
    }

    #[test]
    fn r55_d8_tag_path_resolves_primary_by_name() {
        // R55.D.8 — addressing the primary by its tag also works:
        // `primary/external/<action>` reaches the same node the bare
        // `external/<action>` short-circuit does, but via the typed
        // path. AI clients can use the symbolic form uniformly.
        let scene = multi_external_wrap("primary", "extra");
        assert_eq!(
            query(&scene, "/primary/external/count").unwrap(),
            IntrospectValue::Int(11),
            "primary/external/ resolves to the primary via tag walk",
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R1303 PR-51 §5.45 §5.7 §2 #2 §2 #7 — no-primary binding over the RPC
    // wire. `CoreShell` composes a no-primary binding's state scene as
    // `Container([extra, extra, ...])` marked no-primary-head (R1307). The
    // RPC path layer is V-agnostic (scene-as-data, §2 #7); the marker lets
    // it self-describe the absence. These pin the two contracts for that
    // shape: (a) every extra is reachable by its explicit tag — the stable
    // §2 #2 AI address a no-primary binding relies on; (b) the bare
    // `/external` shorthand (which names "the primary") REJECTS with
    // `NoExternalAtPath` because the marked container has no primary head —
    // self-describing, not a silent misroute to an arbitrary extra.
    // ─────────────────────────────────────────────────────────────────

    fn extras_only_wrap() -> Scene {
        // No primary: `Container([External(pane_a), External(pane_b)])`
        // marked no-primary-head — the exact shape
        // `compose_root(None, [a, b])` produces. Distinct counts tell the
        // two extras apart.
        use pinion_core::scene::{ContainerNode, ExternalNode as ExtNode, Rect};
        let a =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(101))).with_tag("pane_a"));
        let b =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(202))).with_tag("pane_b"));
        let mut c = ContainerNode::new(vec![a, b]).without_primary_head();
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn r1303_no_primary_extras_reachable_by_tag() {
        // §2 #2 — a no-primary binding's every surface is addressable by
        // its explicit tag over the wire (the stable AI address), with no
        // primary present.
        let scene = extras_only_wrap();
        assert_eq!(
            query(&scene, "/pane_a/external/count").unwrap(),
            IntrospectValue::Int(101),
        );
        assert_eq!(
            query(&scene, "/pane_b/external/count").unwrap(),
            IntrospectValue::Int(202),
            "the second extra is reachable by tag with no primary present",
        );
    }

    #[test]
    fn r1303_no_primary_bare_external_rejects() {
        // (R1307) The bare `/external` shorthand names "the primary"; the
        // marked no-primary-head container has none, so `primary_external`
        // returns `None` and the query REJECTS with `NoExternalAtPath` —
        // self-describing (§2 #7) rather than silently resolving pane_a.
        let scene = extras_only_wrap();
        assert_eq!(
            query(&scene, "/external/count").unwrap_err(),
            QueryError::NoExternalAtPath,
            "bare /external rejects for a no-primary-head container",
        );
    }

    #[test]
    fn query_nested_non_external_target_is_no_external_at_path() {
        // Walk lands on a Box (not External) → reject.
        use pinion_core::scene::{BoxNode, ContainerNode, Rect};
        let child = Scene::Box(BoxNode::filled(Rect::default(), Color::default()).with_tag("info"));
        let mut c = ContainerNode::new(vec![child]);
        c.rect = Rect::new(0, 0, 100, 100);
        let scene = Scene::Container(c);
        assert_eq!(
            query(&scene, "/info/external/count").unwrap_err(),
            QueryError::NoExternalAtPath,
        );
    }

    #[test]
    fn schema_path_returns_declared_fields_as_json() {
        // `$schema` returns the contract (paths + types), not a value.
        let scene = counted_scene(3);
        assert_eq!(
            query(&scene, "/external/$schema").unwrap(),
            IntrospectValue::Json(serde_json::json!([{ "path": "count", "type": "int" }])),
        );
    }

    #[test]
    fn schema_path_resolves_through_the_window_prefix() {
        let scene = counted_scene(0);
        assert_eq!(
            query(&scene, "/window[main]/external/$schema").unwrap(),
            IntrospectValue::Json(serde_json::json!([{ "path": "count", "type": "int" }])),
        );
    }

    #[test]
    fn schema_path_on_opted_out_external_reports_opted_out() {
        // An external with no introspect surface has no schema to report —
        // resolution fails before the `$schema` interception.
        let scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        assert_eq!(
            query(&scene, "/external/$schema").unwrap_err(),
            QueryError::IntrospectionOptedOut,
        );
    }

    // ---- R828 §2 #4 §5.12 — immediate-mode driver introspect ----

    use pinion_core::external::{InterveneError, IntrospectSchema, SchemaField};
    use pinion_core::scene::{ContainerNode, ImmediateMode, ImmediateModeNode};

    /// Read-only immediate-mode driver exposing one `pos` float — the
    /// query peer of `CountedExternal`.
    #[derive(Debug)]
    struct PosDriver {
        pos: f64,
    }

    impl ImmediateMode for PosDriver {
        fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
            Some(self)
        }
    }

    impl ExternalIntrospect for PosDriver {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(const { &[SchemaField::new("pos", "float")] })
        }
        fn query(&self, path: &str) -> Option<IntrospectValue> {
            (path == "pos").then_some(IntrospectValue::Float(self.pos))
        }
        fn intervene(&mut self, _: &str, _: IntrospectValue) -> Result<(), InterveneError> {
            Err(InterveneError::ReadOnly)
        }
    }

    /// Immediate-mode driver that does NOT opt in to introspection
    /// (default `introspect()` returns `None`).
    #[derive(Debug)]
    struct OpaqueDriver;
    impl ImmediateMode for OpaqueDriver {}

    fn immediate_root(pos: f64) -> Scene {
        Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            PosDriver { pos },
            Rect::default(),
        ))
    }

    #[test]
    fn immediate_driver_query_at_root() {
        let scene = immediate_root(0.5);
        assert_eq!(
            query(&scene, "/external/pos").unwrap(),
            IntrospectValue::Float(0.5),
        );
    }

    #[test]
    fn immediate_driver_query_via_tagged_descendant() {
        let node = Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(PosDriver { pos: 0.25 }, Rect::default())
                .with_tag("ball"),
        );
        let scene = Scene::Container(ContainerNode::new(vec![node]));
        assert_eq!(
            query(&scene, "/ball/external/pos").unwrap(),
            IntrospectValue::Float(0.25),
        );
    }

    #[test]
    fn immediate_driver_schema_discovery() {
        let scene = immediate_root(0.0);
        assert_eq!(
            query(&scene, "/external/$schema").unwrap(),
            IntrospectValue::Json(serde_json::json!([{ "path": "pos", "type": "float" }])),
        );
    }

    #[test]
    fn immediate_driver_unknown_path_propagates() {
        let scene = immediate_root(0.0);
        assert_eq!(
            query(&scene, "/external/ghost").unwrap_err(),
            QueryError::UnknownIntrospectPath,
        );
    }

    #[test]
    fn immediate_driver_opted_out_reports_opted_out() {
        let scene = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            OpaqueDriver,
            Rect::default(),
        ));
        assert_eq!(
            query(&scene, "/external/pos").unwrap_err(),
            QueryError::IntrospectionOptedOut,
        );
    }

    // ---- R1482 §5.12 §2 #7 — what the answer is, not just what it says ----

    #[test]
    fn r1482_a_retained_node_in_the_painted_frame_is_as_of_that_frame() {
        // The case with no witness before R1482: the value is whatever the
        // view fn built into a node it rebuilds every paint.
        let scene = container_with_nested_counted("c", 3);
        let (value, origin) = query_from(&scene, SceneSource::Paint, "/c/external/count").unwrap();
        assert_eq!(value, IntrospectValue::Int(3));
        assert_eq!(origin, AnswerOrigin::PaintFrame);
    }

    #[test]
    fn r1482_a_driver_in_the_painted_frame_is_live() {
        // The case R828 opened the fallback FOR: the handle is the `Rc` the
        // tick loop drives, so this answer is current simulation state — and
        // must not be reported the same way as the node above.
        let node = Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(PosDriver { pos: 0.25 }, Rect::default())
                .with_tag("ball"),
        );
        let scene = Scene::Container(ContainerNode::new(vec![node]));
        let (value, origin) = query_from(&scene, SceneSource::Paint, "/ball/external/pos").unwrap();
        assert_eq!(value, IntrospectValue::Float(0.25));
        assert_eq!(origin, AnswerOrigin::PaintDriver);
    }

    #[test]
    fn r1482_the_state_scene_answers_as_state_whatever_node_kind_holds_it() {
        // The state scene is not rebuilt per frame, so neither node kind
        // makes its answer as-of-a-frame — the rule `AnswerOrigin::of`
        // encodes, pinned against both kinds so a future edit cannot make
        // the state scene report a frame word for one of them.
        let retained = container_with_nested_counted("c", 3);
        assert_eq!(
            query_from(&retained, SceneSource::State, "/c/external/count")
                .unwrap()
                .1,
            AnswerOrigin::State,
        );
        let driver = Scene::Container(ContainerNode::new(vec![Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(PosDriver { pos: 0.25 }, Rect::default())
                .with_tag("ball"),
        )]));
        assert_eq!(
            query_from(&driver, SceneSource::State, "/ball/external/pos")
                .unwrap()
                .1,
            AnswerOrigin::State,
        );
    }

    #[test]
    fn r1482_schema_discovery_reports_the_origin_of_the_node_it_described() {
        // `$schema` takes the same two branches as a value read, so it must
        // report the same origin — otherwise a client discovering a contract
        // cannot tell whether the contract it found is the live one.
        let scene = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            PosDriver { pos: 0.0 },
            Rect::default(),
        ));
        assert_eq!(
            query_from(&scene, SceneSource::Paint, "/external/$schema")
                .unwrap()
                .1,
            AnswerOrigin::PaintDriver,
        );
    }

    #[test]
    fn r1482_query_is_the_origin_dropping_projection_of_query_from() {
        // `query` must stay exactly the value half of the same resolution —
        // if it ever grew its own walk, the two would drift.
        let scene = container_with_nested_counted("c", 42);
        for path in ["/c/external/count", "/c/external/$schema"] {
            assert_eq!(
                query(&scene, path),
                query_from(&scene, SceneSource::State, path)
                    .map(|(v, _)| v)
                    .map_err(|r| r.error),
                "path {path}",
            );
        }
        assert_eq!(
            query(&scene, "/ghost/external/count"),
            query_from(&scene, SceneSource::State, "/ghost/external/count")
                .map(|(v, _)| v)
                .map_err(|r| r.error),
        );
    }

    // ---- R1485 §5.12 §2 #7 — a refusal says which surface refused ----

    #[test]
    fn r1485_a_reached_surface_names_itself_when_it_declines() {
        // The three ways a walk can land on a surface that then says no —
        // the same three words a successful read reports, so a client can
        // match a refusal against the answers it already understands.
        let retained = container_with_nested_counted("c", 3);
        let refusal = query_from(&retained, SceneSource::State, "/c/external/nope")
            .expect_err("an undeclared path is refused");
        assert_eq!(refusal.error, QueryError::UnknownIntrospectPath);
        assert_eq!(refusal.refused_by, Some(AnswerOrigin::State));

        let refusal = query_from(&retained, SceneSource::Paint, "/c/external/nope")
            .expect_err("an undeclared path is refused");
        assert_eq!(refusal.refused_by, Some(AnswerOrigin::PaintFrame));

        let driver = Scene::Container(ContainerNode::new(vec![Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(PosDriver { pos: 0.25 }, Rect::default())
                .with_tag("ball"),
        )]));
        let refusal = query_from(&driver, SceneSource::Paint, "/ball/external/nope")
            .expect_err("an undeclared path is refused");
        assert_eq!(refusal.error, QueryError::UnknownIntrospectPath);
        assert_eq!(refusal.refused_by, Some(AnswerOrigin::PaintDriver));
    }

    #[test]
    fn r1485_the_same_surface_names_itself_in_both_outcomes() {
        // The property that makes the refusal word checkable rather than
        // decorative: a surface reports one identity, whether the slot it
        // was asked for exists or not. A refusal that named a different
        // surface than that surface's own answers would send a client
        // looking for the schema in the wrong place.
        let scenes = [
            (
                container_with_nested_counted("c", 3),
                "/c/external/",
                "count",
            ),
            (
                Scene::Container(ContainerNode::new(vec![Scene::ImmediateModeNode(
                    ImmediateModeNode::from_driver(PosDriver { pos: 0.25 }, Rect::default())
                        .with_tag("ball"),
                )])),
                "/ball/external/",
                "pos",
            ),
        ];
        for (scene, prefix, declared) in scenes {
            for source in [SceneSource::State, SceneSource::Paint] {
                let answered = query_from(&scene, source, &format!("{prefix}{declared}"))
                    .expect("the declared slot answers")
                    .1;
                let refused = query_from(&scene, source, &format!("{prefix}undeclared"))
                    .expect_err("the undeclared slot is refused")
                    .refused_by;
                assert_eq!(
                    Some(answered),
                    refused,
                    "{prefix} under {source:?} must name one surface in both outcomes",
                );
            }
        }
    }

    #[test]
    fn r1485_an_opted_out_node_is_reached_and_says_so() {
        // Distinct from "nothing is here": the address was right. Before
        // R1485 both arrived as a bare word with no way to tell a shut door
        // from a missing one.
        let scene = Scene::External(ExternalNode::new(Box::new(StubExternal)));
        let refusal =
            query_from(&scene, SceneSource::State, "/external/pos").expect_err("opted out");
        assert_eq!(refusal.error, QueryError::IntrospectionOptedOut);
        assert_eq!(refusal.refused_by, Some(AnswerOrigin::State));
    }

    #[test]
    fn r1485_a_refusal_that_reached_nothing_names_nothing() {
        // The other half, and the reason `refused_by` is an `Option` rather
        // than a fourth word: naming a surface here would be an invention.
        // Each of these ends the walk at a different stage — parse, shape,
        // and scene walk — so the absence is pinned at all three.
        let scene = container_with_nested_counted("c", 3);
        for (path, expected) in [
            ("/ghost/external/count", QueryError::NoExternalAtPath),
            ("/c/count", QueryError::UnsupportedPath),
            (
                "/window[]/c/external/count",
                QueryError::Path(PathError::EmptyWindowId),
            ),
        ] {
            let refusal =
                query_from(&scene, SceneSource::Paint, path).expect_err("must be refused");
            assert_eq!(refusal.error, expected, "path {path}");
            assert_eq!(refusal.refused_by, None, "path {path} names no surface");
        }
    }

    #[test]
    fn r1485_query_drops_the_refusing_surface_as_it_drops_the_origin() {
        // `query` is the projection for callers holding one scene; it must
        // stay exactly that on the failure channel too, so the two cannot
        // disagree about WHY a path failed.
        let scene = container_with_nested_counted("c", 3);
        for path in ["/c/external/nope", "/ghost/external/count", "/c/count"] {
            assert_eq!(
                query(&scene, path),
                query_from(&scene, SceneSource::State, path)
                    .map(|(v, _)| v)
                    .map_err(|r| r.error),
                "path {path}",
            );
        }
    }
}
