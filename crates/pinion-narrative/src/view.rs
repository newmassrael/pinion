//! The projection: [`NarrativeState`] → a queryable pinion [`Scene`].
//!
//! This is the read-side render of the CQRS pair. The output is a plain
//! structured scene of `Text` rows (§2 #1 — no opaque paint), so it renders
//! identically on the GUI and TUI backends (§2 #6) and every field is
//! readable as data via `scene/query` (§2 #7) — an AI agent sees the story,
//! not pixels.
//!
//! ## R1344 §5.21 §5.41 — intent lives in `LayoutStyle`, not in `rect`
//!
//! This view used to author absolute `rect`s (a running `y` cursor,
//! `Rect::new(MARGIN, y, WIDTH, LINE)`). That predated the TUI layout pass and
//! could not survive it: `compute_layout` **overwrites** every `rect`, so on the
//! Vello backend those coordinates were already dead code, while the TUI (which
//! ran no layout) was the only backend that honoured them. The two backends read
//! disjoint halves of the same scene — §2 #6 in name only.
//!
//! Now the projection is a **column flex**: the rows' order and spacing are
//! authored in `LayoutStyle`, and `rect` is a pure output on both backends. This
//! also makes prose *reflow* — a row longer than the terminal wraps to as many
//! cell rows as it needs and the next row moves down, where an absolute `y`
//! would have let the wrapped tail overlap its neighbour.

use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{Display, FlexDirection, SizeValue};

use crate::state::NarrativeState;

/// Padding around the content, in pixels (2 cells at the default 8×16 metric).
const MARGIN: u32 = 16;
/// Blank-line gap between sections, in pixels (one cell).
const SECTION_GAP: u32 = 16;

/// Project the read-model into a retained scene.
#[must_use]
pub fn narrative_scene(state: &NarrativeState) -> Scene {
    let mut children: Vec<Scene> = Vec::new();

    if state.world().is_empty() {
        children.push(text_row("the-tide 서사 walk"));
        children.push(section(text_row("(리포트에 세계선이 없습니다)")));
        return root(children);
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
    children.push(text_row(format!(
        "the-tide · {} · 세계선 {branch_id} ({world_no}/{world_count}) · 장면 {scene_no}/{scene_count}",
        state.world().telling,
    )));

    // Title + intent of the current scene.
    if let Some(scene) = state.current_scene() {
        children.push(section(text_row(scene.title.clone())));
        children.push(text_row(format!("의도: {}", scene.intent)));

        children.push(section(text_row("단서(disclosures):")));
        if scene.disclosures.is_empty() {
            children.push(text_row("  (없음)"));
        } else {
            for d in &scene.disclosures {
                children.push(text_row(format!(
                    "  · [{}] {}  (@{})",
                    d.mode, d.fact, d.first_at
                )));
            }
        }
    } else {
        children.push(section(text_row("(장면 없음)")));
    }

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
    children.push(section(text_row(worlds)));

    children.push(section(text_row("n/p 장면 · [ ] 세계선 · Esc 종료")));

    root(children)
}

/// One content row. Carries no `rect`: the column flex places it and the
/// backend's text measure sizes it (cells on the TUI, parley advances on
/// Vello), so a long row wraps to as many rows as its own backend needs.
fn text_row(content: impl Into<String>) -> Scene {
    Scene::Text(TextNode::new(content, Rect::default()))
}

/// Open a new section: `row` preceded by a one-cell blank gap. The
/// pre-R1344 view spelled this as a bigger jump in its `y` cursor.
fn section(row: Scene) -> Scene {
    let mut row = row;
    if let Scene::Text(t) = &mut row {
        t.layout.margin.y = SECTION_GAP;
    }
    row
}

/// The projection root: a padded column that fills its viewport, so the
/// content reflows to the terminal / window it is rendered into instead of
/// pinning to a hardcoded 800px page.
fn root(children: Vec<Scene>) -> Scene {
    let mut node = ContainerNode::default();
    node.layout.display = Display::Flex;
    node.layout.flex_direction = FlexDirection::Column;
    node.layout.size.width = SizeValue::Percent(100);
    node.layout.padding = Rect::new(MARGIN, MARGIN, MARGIN, MARGIN);
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

    /// R1344 §5.21 §5.41 — the column reflows; prose is not truncated.
    ///
    /// The pre-R1344 view authored absolute `rect`s with a running `y`
    /// cursor. Those were dead code on Vello (layout overwrites `rect`) and
    /// authoritative only on the TUI, which ran no layout — the disjoint
    /// halves this round closed. The existing tests here read the scene's
    /// TEXT, so they passed under either model; this pins the GEOMETRY.
    fn rects(s: &Scene, out: &mut Vec<Rect>) {
        match s {
            Scene::Text(t) => out.push(t.rect),
            Scene::Container(c) => c.children.iter().for_each(|ch| rects(ch, out)),
            _ => {}
        }
    }

    #[test]
    fn r1344_rows_stack_in_a_padded_column_and_prose_reflows() {
        use pinion_runtime::{LayoutCache, compute_layout_with_text_measure};

        /// A cell-ish measure model: 8×16 cells, greedy character wrap.
        ///
        /// Deliberately a local double, not `pinion_tui`'s real `CellTextLayout`
        /// — this crate must not dev-dep a backend (pinion-tui pulls ratatui /
        /// crossterm / tokio) and the test only needs the column's structure to
        /// be pinnable, not exact break points.
        struct CellIsh;
        impl pinion_runtime::layout::TextMeasure for CellIsh {
            fn measure_text(
                &self,
                content: &str,
                _s: &pinion_core::style::TextStyle,
                _r: &[pinion_core::scene::StyleRun],
                max_width: Option<u32>,
                _c: bool,
            ) -> Option<pinion_runtime::TextBox> {
                let cols = u32::try_from(content.chars().count()).unwrap_or(u32::MAX);
                let budget = max_width.map_or(u32::MAX, |px| (px / 8).max(1));
                let rows = cols.div_ceil(budget).max(1);
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "cell counts × 8/16 px are exact integers in f32"
                )]
                let measured = pinion_runtime::TextBox {
                    width: (cols.min(budget) * 8) as f32,
                    height: (rows * 16) as f32,
                    line_count: rows,
                    // A fixture measure over a uniform cell grid: it has no
                    // font metrics to report a baseline from, and on a uniform
                    // grid top-alignment already puts first rows level.
                    baseline: None,
                };
                Some(measured)
            }
        }

        let owner = Owner::new();
        owner.run(|| {
            let state = NarrativeState::new(sample());
            let mut scene = narrative_scene(&state);
            let mut cache = LayoutCache::new();
            let _ =
                compute_layout_with_text_measure(&mut scene, &mut cache, 320, 640, Some(&CellIsh));

            let mut rs = Vec::new();
            rects(&scene, &mut rs);
            assert!(rs.len() >= 4, "several rows: {rs:?}");
            for r in &rs {
                assert_eq!(r.x, MARGIN, "the column's left padding is real: {r:?}");
            }
            for pair in rs.windows(2) {
                assert!(
                    pair[1].y >= pair[0].y + pair[0].h,
                    "rows stack without overlap even when one wraps: {:?} then {:?}",
                    pair[0],
                    pair[1],
                );
            }
            assert!(
                rs.iter().any(|r| r.h > 16),
                "at least one row reflowed onto multiple lines in a 40-cell column: {rs:?}",
            );
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
