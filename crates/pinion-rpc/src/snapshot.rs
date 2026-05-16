//! `scene/snapshot` RPC method dispatch (§5.12 method 4 of 7, R16 slice 13).
//!
//! Captures the scene-root primitive's discriminator and, for
//! `Scene::External` that opts in to §5.15 item 8 introspection, dumps
//! every `(path, value)` pair declared by `ExternalIntrospect::schema`.
//!
//! v0 scope:
//!   * path: `/[window[id]/]` only — no scene-path tail, since v0 has
//!     no addressable sub-tree shape.
//!   * dump granularity: scene root only. `Scene::Container` traversal
//!     waits on §5.3 DSL settling the addressable path syntax.
//!   * fallback for future `Scene` variants ([`SnapshotNode::Unknown`])
//!     keeps the dispatcher forward-compatible with `non_exhaustive`
//!     additions in pinion-core.

use pinion_core::external::IntrospectValue;
use pinion_core::Scene;

use crate::path::{self, PathError};

/// One scene-root primitive's snapshot shape.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotNode {
    Box,
    Text,
    Path,
    Image,
    Container,
    Effect,
    External(ExternalSnapshot),
    /// Catch-all for `Scene` variants added in a later pinion-core
    /// version that this dispatcher predates.
    Unknown,
}

/// `External` payload of [`SnapshotNode::External`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalSnapshot {
    /// `Some(fields)` when the `External` opted in to §5.15 item 8 and
    /// reported a schema; `None` otherwise. Order matches the schema's
    /// declared field order.
    pub introspect: Option<Vec<(String, IntrospectValue)>>,
}

/// Reasons [`snapshot`] can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    /// Window-prefix parsing failed.
    Path(PathError),
    /// Scene path is not v0-supported (must be empty after window
    /// prefix).
    UnsupportedPath,
}

impl From<PathError> for SnapshotError {
    fn from(err: PathError) -> Self {
        SnapshotError::Path(err)
    }
}

/// Build a snapshot of `scene` at the resolved window root.
///
/// # Errors
///
/// Returns [`SnapshotError`] when the window prefix is malformed or the
/// scene path carries an unsupported tail.
pub fn snapshot(scene: &Scene, raw_path: &str) -> Result<SnapshotNode, SnapshotError> {
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;

    // v0: scene path must be empty. Tails like /external/count belong
    // to scene/query, not scene/snapshot.
    if !resolved.scene_path.is_empty() {
        return Err(SnapshotError::UnsupportedPath);
    }

    Ok(snapshot_root(scene))
}

fn snapshot_root(scene: &Scene) -> SnapshotNode {
    match scene {
        Scene::Box(_) => SnapshotNode::Box,
        Scene::Text(_) => SnapshotNode::Text,
        Scene::Path(_) => SnapshotNode::Path,
        Scene::Image(_) => SnapshotNode::Image,
        Scene::Container(_) => SnapshotNode::Container,
        Scene::Effect(_) => SnapshotNode::Effect,
        Scene::External(node) => {
            let introspect = node.handle.introspect().map(|intro| {
                intro
                    .schema()
                    .fields
                    .iter()
                    .filter_map(|(name, _ty)| intro.query(name).map(|v| ((*name).to_string(), v)))
                    .collect()
            });
            SnapshotNode::External(ExternalSnapshot { introspect })
        }
        // `Scene` is non_exhaustive; future variants surface as Unknown
        // until this dispatcher is updated for them.
        _ => SnapshotNode::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};
    use pinion_core::Color;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    #[test]
    fn box_root_snapshots_as_box() {
        let scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        assert_eq!(snapshot(&scene, "").unwrap(), SnapshotNode::Box);
    }

    #[test]
    fn counted_external_dumps_introspect_fields() {
        let scene = counted_scene(42);
        let snap = snapshot(&scene, "").unwrap();
        match snap {
            SnapshotNode::External(ExternalSnapshot {
                introspect: Some(fields),
            }) => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].0, "count");
                assert_eq!(fields[0].1, IntrospectValue::Int(42));
            }
            other => panic!("expected External with introspect Some, got {other:?}"),
        }
    }

    #[test]
    fn stub_external_introspect_is_none() {
        let scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let snap = snapshot(&scene, "").unwrap();
        assert_eq!(
            snap,
            SnapshotNode::External(ExternalSnapshot { introspect: None }),
        );
    }

    #[test]
    fn window_prefix_short_circuits() {
        let scene = counted_scene(0);
        let snap = snapshot(&scene, "/window[main]").unwrap();
        match snap {
            SnapshotNode::External(ExternalSnapshot {
                introspect: Some(fields),
            }) => assert_eq!(fields[0].1, IntrospectValue::Int(0)),
            other => panic!("expected External, got {other:?}"),
        }
    }

    #[test]
    fn scene_path_tail_is_unsupported() {
        let scene = counted_scene(0);
        let err = snapshot(&scene, "/external/count").unwrap_err();
        assert_eq!(err, SnapshotError::UnsupportedPath);
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        let scene = counted_scene(0);
        let err = snapshot(&scene, "/window[main").unwrap_err();
        assert_eq!(err, SnapshotError::Path(PathError::MalformedPrefix));
    }
}
