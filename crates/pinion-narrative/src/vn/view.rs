//! The projection: [`VnState`] → a queryable pinion [`Scene`].
//!
//! The read-side render of the VN runner. The output is a plain structured
//! scene of `Text` rows (§2 #1 — no opaque paint), so it renders identically
//! on the GUI and TUI backends (§2 #6) and every field — the revealed
//! dialogue, the options, the countdown — is readable as data via
//! `scene/query` (§2 #7). The countdown is drawn as a block-character bar
//! *and* surfaced numerically (`remaining_ms`), so the visual is TUI-native
//! and the truth stays queryable.

use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};

use crate::vn::model::VnStep;
use crate::vn::state::{VnMode, VnState};

/// Left margin, in pixels (2 cells at the default 8×16 metric).
const MARGIN: u32 = 16;
/// Content row width, in pixels.
const WIDTH: u32 = 760;
/// Text row height, in pixels (one cell).
const LINE: u32 = 16;
/// Number of cells in the countdown bar.
const BAR_CELLS: usize = 20;

/// Project the runner into a retained scene.
#[must_use]
pub fn vn_scene(state: &VnState) -> Scene {
    let mut children: Vec<Scene> = Vec::new();
    let mut y: u32 = 16;

    let step_no = usize::from(state.runtime().step) + 1;
    let step_count = state.step_count();
    children.push(text_row(
        format!(
            "the-tide · VN · {} · 스텝 {step_no}/{step_count}",
            mode_label(state.mode())
        ),
        y,
    ));
    y += 30;

    match state.current_step() {
        Some(VnStep::Line { speaker, .. }) => {
            if speaker.is_empty() {
                children.push(text_row("(나레이션)".to_string(), y));
            } else {
                children.push(text_row(format!("{speaker}:"), y));
            }
            y += 24;
            // The revealed prefix, with a caret while still typing.
            let mut line = state.revealed_text();
            if !state.fully_revealed() {
                line.push('▌');
            }
            children.push(text_row(format!("  {line}"), y));
            y += 30;
            children.push(text_row(
                "invoke tick <ms> 로 글자가 드러남 · advance 로 넘김".to_string(),
                y,
            ));
        }
        Some(VnStep::TimedChoice {
            prompt, options, ..
        }) => {
            children.push(text_row(format!("? {prompt}"), y));
            y += 26;
            children.push(text_row(countdown_bar(state), y));
            y += 26;
            for (i, opt) in options.iter().enumerate() {
                children.push(text_row(format!("  {}) {}", i + 1, opt.label), y));
                y += 20;
            }
            y += 6;
            children.push(text_row(
                "invoke choose <index> · 시간이 다하면 기본 선택으로 결정".to_string(),
                y,
            ));
        }
        None => {
            children.push(text_row("— 끝 —".to_string(), y));
            y += 26;
            if let Some(opt) = state.resolved_option() {
                let how = if state.resolution().is_some_and(|r| r.timed_out) {
                    "시간 초과"
                } else {
                    "선택"
                };
                children.push(text_row(
                    format!("결말: {} ({how}) → {}", opt.label, opt.outcome),
                    y,
                ));
            } else {
                children.push(text_row("결말: (선택 없음)".to_string(), y));
            }
        }
    }

    y += LINE + MARGIN;
    root(children, y)
}

/// A block-character countdown bar plus the remaining time in seconds — a
/// visual that works on both backends, backed by the queryable
/// `remaining_ms`.
fn countdown_bar(state: &VnState) -> String {
    let timeout = state.timeout_ms();
    let remaining = state.remaining_ms();
    let filled = if timeout == 0 {
        BAR_CELLS
    } else {
        // ceil so a non-zero remaining always shows at least one cell.
        let num = usize::try_from(u64::from(remaining) * (BAR_CELLS as u64)).unwrap_or(0);
        let cells = num.div_ceil(usize::try_from(timeout).unwrap_or(1).max(1));
        cells.min(BAR_CELLS)
    };
    let bar: String = "█".repeat(filled) + &"░".repeat(BAR_CELLS - filled);
    let whole = remaining / 1000;
    let tenths = (remaining % 1000) / 100;
    format!("  남은 시간 [{bar}] {whole}.{tenths}s")
}

/// The Korean label for a runner mode (the header line's suffix).
const fn mode_label(mode: VnMode) -> &'static str {
    match mode {
        VnMode::Line => "대사",
        VnMode::Choice => "선택",
        VnMode::End => "끝",
    }
}

fn text_row(content: impl Into<String>, y: u32) -> Scene {
    Scene::Text(TextNode::new(content, Rect::new(MARGIN, y, WIDTH, LINE)))
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
    use crate::vn::model::{VnOption, VnScript, VnStep};
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

    fn joined(scene: &Scene) -> String {
        let mut text = Vec::new();
        collect_text(scene, &mut text);
        text.join("\n")
    }

    fn script() -> VnScript {
        VnScript::new(vec![
            VnStep::line("무녀", "돌아오지 마라"),
            VnStep::timed_choice(
                "부름이 들린다",
                vec![
                    VnOption::new("돌아본다", "answer"),
                    VnOption::new("버틴다", "endure"),
                ],
                4000,
                1,
            ),
        ])
    }

    #[test]
    fn line_projects_speaker_and_revealed_prefix_with_caret() {
        let owner = Owner::new();
        owner.run(|| {
            let state = VnState::new(script());
            assert!(!state.tick(100)); // reveal 4 of 8 chars
            let out = joined(&vn_scene(&state));
            assert!(out.contains("무녀:"), "speaker shown: {out}");
            assert!(
                out.contains("돌아오지▌"),
                "typewriter caret while typing: {out}"
            );
            assert!(out.contains("대사"), "mode label");
        });
    }

    #[test]
    fn choice_projects_prompt_options_and_countdown_bar() {
        let owner = Owner::new();
        owner.run(|| {
            let state = VnState::new(script());
            assert!(state.goto(1));
            let out = joined(&vn_scene(&state));
            assert!(out.contains("부름이 들린다"), "prompt: {out}");
            assert!(out.contains("1) 돌아본다"), "option 1");
            assert!(out.contains("2) 버틴다"), "option 2");
            assert!(out.contains("남은 시간"), "countdown row");
            assert!(out.contains("4.0s"), "full countdown at entry: {out}");
            assert!(out.contains('█'), "bar filled at entry");
        });
    }

    #[test]
    fn end_projects_the_outcome() {
        let owner = Owner::new();
        owner.run(|| {
            let state = VnState::new(script());
            assert!(state.goto(1));
            state.choose(0).unwrap();
            let out = joined(&vn_scene(&state));
            assert!(out.contains("— 끝 —"), "end marker: {out}");
            assert!(out.contains("결말: 돌아본다"), "outcome label: {out}");
            assert!(out.contains("answer"), "outcome tag: {out}");
        });
    }
}
