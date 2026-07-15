//! The AI-first read surface over a solved [`PlaceLayout`].
//!
//! The map's solved geometry is exposed over RPC (§2 #2 / #7) as a
//! **query-only** introspection node. Rather than hand-roll an `External`,
//! [`PlaceLayout`] implements the [`QuerySource`] contract and a binding
//! wraps it in pinion-core's `QueryOnlyIntrospect` — the shared read-only
//! introspection substrate (RPC-only, `intervene` refused). The map is
//! derived geometry, so there is nothing to author here; re-solving means
//! re-reading the graph upstream.

use pinion_core::external::{IntrospectSchema, IntrospectValue, QuerySource, SchemaField, int_of};
use serde::Serialize;

use crate::place::layout::PlaceLayout;

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

impl QuerySource for PlaceLayout {
    fn introspect_schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("node_count", "int"),
                    SchemaField::new("edge_count", "int"),
                    SchemaField::new("nodes", "json"),
                    SchemaField::new("edges", "json"),
                ]
            },
        )
    }

    fn introspect_query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "node_count" => Some(IntrospectValue::Int(int_of(self.nodes.len()))),
            "edge_count" => Some(IntrospectValue::Int(int_of(self.edges.len()))),
            "nodes" => {
                let geoms: Vec<NodeGeom> = self
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
                        contained_by: n.contained_by.map(|pi| self.nodes[pi].id.as_str()),
                    })
                    .collect();
                Some(IntrospectValue::json(&geoms))
            }
            "edges" => Some(IntrospectValue::json(&self.edges)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::place::layout::solve_layout;
    use crate::place::model::{Adjacency, Direction, Place, PlaceGraph};
    use pinion_core::external::QueryOnlyIntrospect;
    use pinion_core::external::{External, InterveneError};
    use std::rc::Rc;

    fn sample_layout() -> PlaceLayout {
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
        solve_layout(&graph)
    }

    #[test]
    fn query_source_reports_counts() {
        let layout = sample_layout();
        assert!(matches!(
            layout.introspect_query("node_count"),
            Some(IntrospectValue::Int(3))
        ));
        assert!(matches!(
            layout.introspect_query("edge_count"),
            Some(IntrospectValue::Int(1))
        ));
        assert!(layout.introspect_query("unknown").is_none());
    }

    #[test]
    fn nodes_query_carries_geometry_and_containment() {
        let layout = sample_layout();
        match layout.introspect_query("nodes") {
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
    fn wrapped_in_query_only_introspect_it_is_read_only() {
        let node = QueryOnlyIntrospect::new(Rc::new(sample_layout()));
        let intro = node.introspect().expect("introspectable");
        assert!(matches!(
            intro.query("node_count"),
            Some(IntrospectValue::Int(3))
        ));
        // A declared path refuses writes; an undeclared one is unknown.
        let mut node = node;
        let intro = node.introspect_mut().expect("introspectable");
        assert!(matches!(
            intro.intervene("node_count", IntrospectValue::Int(9)),
            Err(InterveneError::ReadOnly)
        ));
        assert!(matches!(
            intro.intervene("nope", IntrospectValue::Int(9)),
            Err(InterveneError::UnknownPath)
        ));
    }
}
