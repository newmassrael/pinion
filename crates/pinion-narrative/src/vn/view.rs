//! The projection: [`VnState`] → a queryable pinion [`Scene`].
//!
//! The read-side render of the VN runner. Structured, no opaque paint (§2 #1):
//! the dialogue is `Text` rows and the stage is `Scene::Image` nodes whose
//! `source` is an asset *reference* (never pixels), so it renders identically
//! on the GUI and TUI backends (§2 #6) and every field — the revealed
//! dialogue, the options, the countdown, and *which* image is on stage where —
//! is readable as data via `scene/query` (§2 #7). The countdown is drawn as a
//! block-character bar *and* surfaced numerically (`remaining_ms`), so the
//! visual is TUI-native and the truth stays queryable.

use pinion_core::scene::{ContainerNode, ImageNode, Rect, Scene, TextNode};
use pinion_core::style::{LayoutStyle, Size};

use crate::vn::model::VnStep;
use crate::vn::stage::VnStage;
use crate::vn::state::{VnMode, VnState};

/// Left margin, in pixels (2 cells at the default 8×16 metric).
const MARGIN: u32 = 16;
/// Content row width, in pixels.
const WIDTH: u32 = 760;
/// Text row height, in pixels (one cell).
const LINE: u32 = 16;
/// Number of cells in the countdown bar.
const BAR_CELLS: usize = 20;
/// Stage width, in pixels (the background fills it; sprite x positions read
/// [`SpritePos::center_x`](crate::vn::SpritePos::center_x) of it).
const STAGE_W: u32 = 800;
/// Stage height, in pixels (the visual band above the dialogue box).
const STAGE_H: u32 = 240;
/// Sprite box width / height, in pixels.
const SPRITE_W: u32 = 160;
const SPRITE_H: u32 = 200;

/// Project the runner into a retained scene: the stage (background + sprites)
/// above the dialogue box.
#[must_use]
pub fn vn_scene(state: &VnState) -> Scene {
    let mut children: Vec<Scene> = Vec::new();

    // ── the visual stage (§2 #1 Image nodes, §2 #7 queryable references) ──
    stage_nodes(state.stage(), &mut children);

    let mut y: u32 = STAGE_H + 16;

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

/// Project the stage into `Scene::Image` nodes: the background (full stage
/// rect) then each sprite (already sorted back-to-front by layer), positioned
/// by [`SpritePos`](crate::vn::SpritePos). Each image's `source` is an asset
/// reference the backend resolves — the scene carries *which* image and
/// *where*, never pixels (§2 #1 / #7). Sprites are tagged with their id so an
/// AI can path-route to a specific character.
fn stage_nodes(stage: &VnStage, out: &mut Vec<Scene>) {
    let data = stage.data();
    if let Some(background) = data.background {
        out.push(image_at(
            background,
            "vn.background".to_string(),
            0,
            0,
            STAGE_W,
            STAGE_H,
        ));
    }
    for sprite in data.sprites {
        let x = sprite.at.center_x(STAGE_W).saturating_sub(SPRITE_W / 2);
        let y = STAGE_H.saturating_sub(SPRITE_H); // feet rest on the stage floor
        out.push(image_at(
            sprite.source,
            format!("vn.sprite.{}", sprite.id),
            x,
            y,
            SPRITE_W,
            SPRITE_H,
        ));
    }
}

/// An `Image` node **absolutely positioned + fixed-size** so the paint layout
/// honours `(x, y, w, h)` instead of collapsing the node to the flow default.
/// The `Rect` mirrors the layout (pre-layout intent); the `LayoutStyle`
/// absolute-position override is what actually reaches the painted pixels
/// (`ImageNode`'s bare `Rect` is otherwise overwritten by the taffy pass).
fn image_at(source: String, tag: String, x: u32, y: u32, w: u32, h: u32) -> Scene {
    let layout = LayoutStyle::new()
        .with_size(Size::px(w, h))
        .with_absolute_position(x, y);
    Scene::Image(
        ImageNode::new(source, Rect::new(x, y, w, h))
            .with_layout(layout)
            .with_tag(tag),
    )
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

    fn collect_images(scene: &Scene, out: &mut Vec<(String, i64)>) {
        match scene {
            // (source, tag-marker via x) — record source + x for position checks.
            Scene::Image(n) => out.push((n.source.clone(), i64::from(n.rect.x))),
            Scene::Container(c) => {
                for child in &c.children {
                    collect_images(child, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn stage_projects_background_and_positioned_sprites_as_images() {
        use crate::vn::stage::{SpritePos, VnSprite};
        let owner = Owner::new();
        owner.run(|| {
            let state = VnState::new(script());
            state.stage().set_background("tideflat");
            state
                .stage()
                .show(VnSprite::new("mudang", "mudang_calm", SpritePos::Left, 1));
            state
                .stage()
                .show(VnSprite::new("child", "child", SpritePos::Right, 0));
            let mut imgs = Vec::new();
            collect_images(&vn_scene(&state), &mut imgs);
            // Background + 2 sprites, all as Image nodes carrying source refs.
            let sources: Vec<&str> = imgs.iter().map(|(s, _)| s.as_str()).collect();
            assert!(
                sources.contains(&"tideflat"),
                "background image: {sources:?}"
            );
            assert!(sources.contains(&"mudang_calm"), "sprite source");
            // The left sprite paints further left than the right one.
            let x = |src: &str| {
                imgs.iter()
                    .find(|(s, _)| s == src)
                    .map(|(_, x)| *x)
                    .unwrap()
            };
            assert!(x("mudang_calm") < x("child"), "left is left of right");
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
