//! AT projection of a solved place-map.
//!
//! A 2D spatial map has no natural linear reading order for a screen
//! reader, so the accessible fallback is a `List` of the places by label —
//! the same shape the narrative walk uses. Geometry lives in the scene /
//! the [`PlaceMapExternal`](crate::place::PlaceMapExternal) for agents that
//! want coordinates.

use pinion_a11y::{AccessNode, AriaRole};

use crate::place::layout::PlaceLayout;

/// Build the AT node list for a solved place-map: a `List` container plus
/// one `ListItem` per place.
#[must_use]
pub fn place_map_access_nodes(
    container_tag: &str,
    layout: &PlaceLayout,
    focused: Option<&str>,
) -> Vec<AccessNode> {
    let container_focused = focused == Some(container_tag);
    let mut nodes =
        vec![AccessNode::new(container_tag, AriaRole::List).with_name("the-tide 장소 지도")];
    for (i, node) in layout.nodes.iter().enumerate() {
        nodes.push(
            AccessNode::new(format!("{container_tag}.place.{i}"), AriaRole::ListItem)
                .with_name(node.label.clone())
                .with_focused(container_focused && i == 0),
        );
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::place::layout::solve_layout;
    use crate::place::model::{Place, PlaceGraph};

    #[test]
    fn emits_a_list_of_places() {
        let graph = PlaceGraph {
            places: vec![
                Place {
                    id: "a".to_string(),
                    label: "갯벌".to_string(),
                    contained_by: None,
                },
                Place {
                    id: "b".to_string(),
                    label: "굿당".to_string(),
                    contained_by: None,
                },
            ],
            adjacencies: vec![],
        };
        let layout = solve_layout(&graph);
        let nodes = place_map_access_nodes("map", &layout, Some("map"));
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].role, AriaRole::List);
        assert_eq!(nodes[1].role, AriaRole::ListItem);
        assert_eq!(nodes[1].name.as_deref(), Some("갯벌"));
    }
}
