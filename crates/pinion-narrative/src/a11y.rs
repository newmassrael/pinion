//! AT projection: the scene walk as a WAI-ARIA `List` of `ListItem`s.
//!
//! The same read-model, exposed to assistive tech. A read-only ordered
//! walk of scenes is a `List` whose items are the scenes; the current
//! cursor is the selected + focused item. This is the accessibility peer
//! of [`narrative_scene`](crate::narrative_scene) — the story readable by
//! screen readers, not just sighted users.

use pinion_a11y::{AccessNode, AriaRole};

use crate::state::NarrativeState;

/// Build the AT node list for the current walk position.
///
/// `container_tag` is the widget tag the binding paints under (so the
/// container node's tag matches the focusable widget). `focused` is the
/// shell's currently focused tag; when it is the container, the selected
/// scene item is also marked focused (`aria-activedescendant`).
#[must_use]
pub fn narrative_access_nodes(
    container_tag: &str,
    state: &NarrativeState,
    focused: Option<&str>,
) -> Vec<AccessNode> {
    let cursor = state.cursor();
    let container_focused = focused == Some(container_tag);
    let branch = state
        .current_world_line()
        .map_or("-", |w| w.branch_id.as_str());

    let mut nodes = vec![
        AccessNode::new(container_tag, AriaRole::List)
            .with_name(format!("the-tide 서사 walk — 세계선 {branch}")),
    ];

    if let Some(world_line) = state.current_world_line() {
        for (i, scene) in world_line.scenes.iter().enumerate() {
            let selected = i == usize::from(cursor.scene);
            nodes.push(
                AccessNode::new(format!("{container_tag}.scene.{i}"), AriaRole::ListItem)
                    .with_name(scene.title.clone())
                    .with_selected(selected)
                    .with_focused(container_focused && selected),
            );
        }
    }

    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PlayableWorld, SceneNode, WorldLine};
    use pinion_core::reactive::Owner;

    fn sample() -> PlayableWorld {
        PlayableWorld {
            worlds: vec![WorldLine {
                branch_id: "main".to_string(),
                scenes: vec![
                    SceneNode {
                        title: "a".to_string(),
                        ..SceneNode::default()
                    },
                    SceneNode {
                        title: "b".to_string(),
                        ..SceneNode::default()
                    },
                ],
            }],
            ..PlayableWorld::default()
        }
    }

    #[test]
    fn emits_list_container_and_scene_items() {
        let owner = Owner::new();
        owner.run(|| {
            let state = NarrativeState::new(sample());
            assert!(state.next_scene());
            let nodes = narrative_access_nodes("walk", &state, Some("walk"));
            assert_eq!(nodes.len(), 3);
            assert_eq!(nodes[0].role, AriaRole::List);
            assert_eq!(nodes[1].role, AriaRole::ListItem);
            assert_eq!(nodes[1].selected, Some(false));
            assert_eq!(nodes[2].selected, Some(true), "cursor is on scene 1");
            assert!(nodes[2].state.focused, "selected item is active descendant");
        });
    }
}
