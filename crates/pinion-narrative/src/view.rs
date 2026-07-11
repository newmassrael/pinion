//! The projection: [`NarrativeState`] → a queryable pinion [`Scene`].
//!
//! This is the read-side render of the CQRS pair. The output is a plain
//! structured scene of `Text` rows (§2 #1 — no opaque paint), so it renders
//! identically on the GUI and TUI backends (§2 #6) and every field is
//! readable as data via `scene/query` (§2 #7) — an AI agent sees the story,
//! not pixels. Layout is deterministic top-down cell rows so the same
//! projection is stable across backends and across frames.

use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};

use crate::state::NarrativeState;

/// Left margin, in pixels (2 cells at the default 8×16 metric).
const MARGIN: u32 = 16;
/// Content row width, in pixels.
const WIDTH: u32 = 760;
/// Text row height, in pixels (one cell).
const LINE: u32 = 16;

/// Project the read-model into a retained scene.
#[must_use]
pub fn narrative_scene(state: &NarrativeState) -> Scene {
    let mut children: Vec<Scene> = Vec::new();
    let mut y: u32 = 16;

    if state.world().is_empty() {
        children.push(text_row("the-tide 서사 walk", MARGIN, y));
        y += 32;
        children.push(text_row("(리포트에 세계선이 없습니다)", MARGIN, y));
        return root(children, y + LINE + MARGIN);
    }

    let cursor = state.cursor();
    let world_no = usize::from(cursor.world) + 1;
    let world_count = state.world_count();
    let scene_count = state.scene_count();
    let scene_no = if scene_count == 0 {
        0
    } else {
        usize::from(cursor.scene) + 1
    };
    let branch_id = state
        .current_world_line()
        .map_or("-", |w| w.branch_id.as_str());

    // Header — telling / world-line / position.
    children.push(text_row(
        format!(
            "the-tide · {} · 세계선 {branch_id} ({world_no}/{world_count}) · 장면 {scene_no}/{scene_count}",
            state.world().telling,
        ),
        MARGIN,
        y,
    ));
    y += 32;

    // Title + intent of the current scene.
    if let Some(scene) = state.current_scene() {
        children.push(text_row(scene.title.clone(), MARGIN, y));
        y += 28;
        children.push(text_row(format!("의도: {}", scene.intent), MARGIN, y));
        y += 32;

        children.push(text_row("단서(disclosures):", MARGIN, y));
        y += 22;
        if scene.disclosures.is_empty() {
            children.push(text_row("  (없음)", MARGIN, y));
            y += 20;
        } else {
            for d in &scene.disclosures {
                children.push(text_row(
                    format!("  · [{}] {}  (@{})", d.mode, d.fact, d.first_at),
                    MARGIN,
                    y,
                ));
                y += 20;
            }
        }
    } else {
        children.push(text_row("(장면 없음)", MARGIN, y));
        y += 28;
    }

    y += 12;

    // World-line strip — the fork topology as a selectable row.
    let mut worlds = String::from("세계선: ");
    for (i, w) in state.world().worlds.iter().enumerate() {
        if i > 0 {
            worlds.push_str("   ");
        }
        worlds.push(if i == usize::from(cursor.world) {
            '▸'
        } else {
            '·'
        });
        worlds.push(' ');
        worlds.push_str(&w.branch_id);
    }
    children.push(text_row(worlds, MARGIN, y));
    y += 32;

    children.push(text_row("n/p 장면 · [ ] 세계선 · Esc 종료", MARGIN, y));

    root(children, y + LINE + MARGIN)
}

fn text_row(content: impl Into<String>, x: u32, y: u32) -> Scene {
    Scene::Text(TextNode::new(content, Rect::new(x, y, WIDTH, LINE)))
}

fn root(children: Vec<Scene>, height: u32) -> Scene {
    let mut node = ContainerNode::default();
    node.rect = Rect::new(0, 0, 800, height);
    node.children = children;
    Scene::Container(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Disclosure, PlayableWorld, SceneNode, WorldLine};
    use pinion_core::reactive::Owner;

    fn collect_text(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => {
                for child in &c.children {
                    collect_text(child, out);
                }
            }
            _ => {}
        }
    }

    fn sample() -> PlayableWorld {
        PlayableWorld {
            telling: "reader".to_string(),
            worlds: vec![WorldLine {
                branch_id: "main".to_string(),
                scenes: vec![SceneNode {
                    idx: 0,
                    title: "물때표".to_string(),
                    intent: "규칙 각인".to_string(),
                    disclosures: vec![Disclosure {
                        mode: "plant".to_string(),
                        fact: "만조엔 길이 사라진다".to_string(),
                        first_at: "ch01".to_string(),
                    }],
                }],
            }],
            ..PlayableWorld::default()
        }
    }

    #[test]
    fn projects_current_scene_as_readable_text() {
        let owner = Owner::new();
        owner.run(|| {
            let state = NarrativeState::new(sample());
            let scene = narrative_scene(&state);
            let mut text = Vec::new();
            collect_text(&scene, &mut text);
            let joined = text.join("\n");
            assert!(joined.contains("물때표"), "title present: {joined}");
            assert!(joined.contains("규칙 각인"), "intent present");
            assert!(
                joined.contains("만조엔 길이 사라진다"),
                "disclosure present"
            );
            assert!(joined.contains("장면 1/1"), "position present");
        });
    }

    #[test]
    fn empty_world_projects_a_valid_scene() {
        let owner = Owner::new();
        owner.run(|| {
            let state = NarrativeState::new(PlayableWorld::default());
            let scene = narrative_scene(&state);
            let mut text = Vec::new();
            collect_text(&scene, &mut text);
            assert!(text.iter().any(|t| t.contains("세계선이 없습니다")));
        });
    }
}
