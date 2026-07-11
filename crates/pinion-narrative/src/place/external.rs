//! The AI-first read surface over a solved [`PlaceLayout`].
//!
//! A read-only §5.15 `External` so an agent can read the map's solved
//! geometry over RPC (§2 #2 / #7): how many places, how they connect, and
//! where each solved to. The map is a projection of authored relations, so
//! there is nothing to `intervene` on here — the geometry is derived, not
//! authored (re-solving means re-reading the graph, upstream).

use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, RepaintOwner, ThreadOwnership,
};
use serde::Serialize;

use crate::place::layout::PlaceLayout;

/// Read-only `External` exposing a solved place-map's geometry.
#[derive(Debug)]
pub struct PlaceMapExternal {
    layout: PlaceLayout,
}

impl PlaceMapExternal {
    /// Wrap a solved layout for introspection.
    #[must_use]
    pub fn new(layout: PlaceLayout) -> Self {
        Self { layout }
    }

    /// The solved layout being exposed.
    #[must_use]
    pub fn layout(&self) -> &PlaceLayout {
        &self.layout
    }
}

/// The per-node geometry shape emitted by the `nodes` query.
#[derive(Serialize)]
struct NodeGeom<'a> {
    id: &'a str,
    label: &'a str,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    cell_x: i32,
    cell_y: i32,
    contained_by: Option<&'a str>,
}

impl External for PlaceMapExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for PlaceMapExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("node_count", "int"),
            ("edge_count", "int"),
            ("nodes", "json"),
            ("edges", "json"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "node_count" => Some(IntrospectValue::Int(int_of(self.layout.nodes.len()))),
            "edge_count" => Some(IntrospectValue::Int(int_of(self.layout.edges.len()))),
            "nodes" => {
                let geoms: Vec<NodeGeom> = self
                    .layout
                    .nodes
                    .iter()
                    .map(|n| NodeGeom {
                        id: &n.id,
                        label: &n.label,
                        x: n.rect.x,
                        y: n.rect.y,
                        w: n.rect.w,
                        h: n.rect.h,
                        cell_x: n.cell.0,
                        cell_y: n.cell.1,
                        contained_by: n.contained_by.map(|pi| self.layout.nodes[pi].id.as_str()),
                    })
                    .collect();
                Some(json(&geoms))
            }
            "edges" => Some(json(&self.layout.edges)),
            _ => None,
        }
    }

    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        // The map is derived geometry — nothing is authored here.
        Err(InterveneError::UnknownPath)
    }
}

fn json<T: Serialize>(value: &T) -> IntrospectValue {
    IntrospectValue::Json(serde_json::to_value(value).unwrap_or(serde_json::Value::Null))
}

fn int_of(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::place::layout::solve_layout;
    use crate::place::model::{Adjacency, Direction, Place, PlaceGraph};

    fn sample_external() -> PlaceMapExternal {
        let graph = PlaceGraph {
            places: vec![
                Place {
                    id: "village".to_string(),
                    label: "마을".to_string(),
                    contained_by: None,
                },
                Place {
                    id: "shrine".to_string(),
                    label: "굿당".to_string(),
                    contained_by: Some("village".to_string()),
                },
                Place {
                    id: "mudflat".to_string(),
                    label: "갯벌".to_string(),
                    contained_by: None,
                },
            ],
            adjacencies: vec![Adjacency {
                from: "village".to_string(),
                to: "mudflat".to_string(),
                direction: Some(Direction::East),
            }],
        };
        PlaceMapExternal::new(solve_layout(&graph))
    }

    #[test]
    fn reports_counts() {
        let ext = sample_external();
        assert!(matches!(
            ext.query("node_count"),
            Some(IntrospectValue::Int(3))
        ));
        assert!(matches!(
            ext.query("edge_count"),
            Some(IntrospectValue::Int(1))
        ));
        assert!(ext.query("unknown").is_none());
    }

    #[test]
    fn nodes_query_carries_geometry_and_containment() {
        let ext = sample_external();
        match ext.query("nodes") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                assert_eq!(items.len(), 3);
                let shrine = items
                    .iter()
                    .find(|v| v["id"] == "shrine")
                    .expect("shrine present");
                assert_eq!(shrine["contained_by"], "village");
                assert!(shrine["w"].as_u64().unwrap() > 0);
            }
            other => panic!("expected Json array, got {other:?}"),
        }
    }

    #[test]
    fn map_is_read_only() {
        let mut ext = sample_external();
        assert!(matches!(
            ext.intervene("node_count", IntrospectValue::Int(9)),
            Err(InterveneError::UnknownPath)
        ));
    }
}
