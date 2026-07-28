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
use pinion_core::style::{Display, FlexDirection, LayoutStyle, Size, SizeValue};

use crate::vn::model::VnStep;
use crate::vn::stage::VnStage;
use crate::vn::state::{VnMode, VnState};

/// Padding around the dialogue band, in pixels (2 cells at the default 8×16
/// metric). Applied on all four sides by [`dialogue_column`].
const MARGIN: u32 = 16;
/// One blank line's worth of vertical space, in pixels — the unit
/// [`spaced_row`] uses to open a block.
///
/// NOT a row height: rows are sized by the backend's text measure (a Vello row
/// measures ~24px at the default font, a TUI row 16px). R1345 §5.21 — the
/// pre-R1345 view used this as an authored row height, which never reached a
/// pixel.
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
    // Absolutely positioned, so out of flow: the dialogue column below places
    // itself against the root, not after the sprites.
    stage_nodes(state.stage(), &mut children);

    // ── the dialogue band ──
    let mut rows: Vec<Scene> = Vec::new();

    let step_no = usize::from(state.runtime().step) + 1;
    let step_count = state.step_count();
    rows.push(text_row(format!(
        "the-tide · VN · {} · 스텝 {step_no}/{step_count}",
        mode_label(state.mode())
    )));

    match state.current_step() {
        Some(VnStep::Line { speaker, .. }) => {
            if speaker.is_empty() {
                rows.push(spaced_row("(나레이션)".to_string()));
            } else {
                rows.push(spaced_row(format!("{speaker}:")));
            }
            // The revealed prefix, with a caret while still typing.
            let mut line = state.revealed_text();
            if !state.fully_revealed() {
                line.push('▌');
            }
            rows.push(text_row(format!("  {line}")));
            rows.push(spaced_row(
                "invoke tick <ms> 로 글자가 드러남 · advance 로 넘김".to_string(),
            ));
        }
        Some(VnStep::TimedChoice {
            prompt, options, ..
        }) => {
            rows.push(spaced_row(format!("? {prompt}")));
            rows.push(text_row(countdown_bar(state)));
            for (i, opt) in options.iter().enumerate() {
                rows.push(text_row(format!("  {}) {}", i + 1, opt.label)));
            }
            rows.push(spaced_row(
                "invoke choose <index> · 시간이 다하면 기본 선택으로 결정".to_string(),
            ));
        }
        None => {
            rows.push(spaced_row("— 끝 —".to_string()));
            if let Some(opt) = state.resolved_option() {
                let how = if state.resolution().is_some_and(|r| r.timed_out) {
                    "시간 초과"
                } else {
                    "선택"
                };
                rows.push(text_row(format!(
                    "결말: {} ({how}) → {}",
                    opt.label, opt.outcome
                )));
            } else {
                rows.push(text_row("결말: (선택 없음)".to_string()));
            }
        }
    }

    children.push(dialogue_column(rows));
    root(children)
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

/// One dialogue row. Carries no `rect`: the dialogue column places it and the
/// backend's text measure sizes it, so a long line wraps to as many rows as it
/// needs and the rows below move down.
///
/// R1345 §5.21 — this used to author `Rect::new(MARGIN, y, WIDTH, LINE)` from a
/// running `y` cursor. Those coordinates never reached a pixel: `compute_layout`
/// overwrites every `rect` (it is an OUTPUT, not an input), so the whole
/// dialogue block was painted at `x = 0, y = 0` — flush into the top-left of the
/// **stage artwork** — instead of in the dialogue band below it. The sibling
/// [`image_at`] already knew this and states its geometry as `LayoutStyle`; the
/// text rows did not.
fn text_row(content: impl Into<String>) -> Scene {
    Scene::Text(TextNode::new(content, Rect::default()))
}

/// A row that opens a new block: one blank line above it. The pre-R1345 view
/// spelled this as a bigger jump in its `y` cursor.
fn spaced_row(content: impl Into<String>) -> Scene {
    let mut row = text_row(content);
    if let Scene::Text(t) = &mut row {
        t.layout.margin.y = LINE;
    }
    row
}

/// The dialogue band: a padded column pinned **below the stage**, holding the
/// rows in flow.
///
/// Absolutely positioned for the same reason [`image_at`] is — the stage is a
/// fixed-geometry canvas and the dialogue sits at a known band under it — but
/// its *children* stay in flow, so a long line wraps and the rows below move
/// down, which an absolute `y` per row could not do.
///
/// Width is `Px(STAGE_W)`, so prose reflows to the **stage width**, not to the
/// window: this projection is a fixed 800px canvas (the sprites are placed
/// against `STAGE_W`), and a dialogue band that reflowed independently of the
/// art it sits under would drift out of register with it. A window-relative VN
/// stage is a real feature, but it is a whole-canvas change (sprite placement,
/// background fit, `SpritePos::center_x`) and not something to smuggle in here.
fn dialogue_column(rows: Vec<Scene>) -> Scene {
    let mut node = ContainerNode::default();
    node.layout = LayoutStyle::new().with_absolute_position(0, STAGE_H);
    node.layout.display = Display::Flex;
    node.layout.flex_direction = FlexDirection::Column;
    node.layout.size.width = SizeValue::Px(STAGE_W);
    node.layout.padding = Rect::new(MARGIN, MARGIN, MARGIN, MARGIN);
    node.children = rows;
    Scene::Container(node)
}

/// The projection root — the canvas the stage and the dialogue band both
/// position themselves absolutely against.
///
/// Authors NO size: `compute_layout` gives the root its viewport, so a size
/// here would be dead intent — the very thing this module's R1345 fix is about.
/// (Verified: an authored `Px` root size is overwritten by the viewport.)
fn root(children: Vec<Scene>) -> Scene {
    let mut node = ContainerNode::default();
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

    /// R1345 §5.21 — the dialogue lands BELOW the stage, not on top of it.
    ///
    /// The pre-R1345 view authored `Rect::new(MARGIN, y, WIDTH, LINE)` from a
    /// running `y` cursor starting at `STAGE_H + 16`. None of it reached a
    /// pixel: `compute_layout` overwrites `rect`, so every dialogue row painted
    /// from `(0, 0)` — flush into the top-left of the stage artwork — while the
    /// sibling `image_at` (which states its geometry as `LayoutStyle`) placed
    /// the art correctly. The tests here read the scene's TEXT, so they passed
    /// through the entire bug; this pins the GEOMETRY.
    #[test]
    fn r1345_dialogue_sits_below_the_stage_and_reflows() {
        use pinion_runtime::{LayoutCache, compute_layout};

        fn text_rects(s: &Scene, out: &mut Vec<Rect>) {
            match s {
                Scene::Text(t) => out.push(t.rect),
                Scene::Container(c) => c.children.iter().for_each(|ch| text_rects(ch, out)),
                _ => {}
            }
        }

        let owner = Owner::new();
        owner.run(|| {
            // R1470.1 — the wrapping line is deliberately ASCII, and that is a
            // correctness fix rather than a style choice. What this test pins is
            // FLOW (a wrapped row pushes its neighbours down); whether a given
            // run wraps at all depends on the text being shapeable, and a host
            // CJK face is NOT guaranteed — every CI runner here has none, and
            // R1448 made a font-less host a legal state on purpose. With the
            // dialogue in Hangul this assertion therefore passed on a developer
            // box and failed on CI, which is a host property masquerading as a
            // layout property (and a [[zero-flake-policy]] violation).
            //
            // Registering the repo's vendored NanumGothic into the LayoutCache
            // does NOT rescue it — measured, not assumed: the family registers
            // ("NanumGothic" comes back) but `register_font_data` makes a face
            // selectable BY NAME, and it does not enter automatic script
            // fallback, so `vn_scene`'s unnamed default style still shapes the
            // Hangul as .notdef and the line stays one row.
            //
            // Hangul metrics are pinned where the vendored face lives and can be
            // named: `pinion_text_font`'s `wrap` / `fallback` tests.
            let state = VnState::new(VnScript::new(vec![VnStep::line(
                "무녀",
                "Do not come back. The tide will take you, and this line is \
                 deliberately long enough that it cannot sit on a single \
                 measured row at the stage width, so it must fold onto several \
                 rows and push whatever follows it further down the column.",
            )]));
            // Reveal it: a VnState starts at `revealed_chars = 0`, so without
            // ticking the typewriter the row's content is literally "  ▌" and
            // the reflow this test claims to check never happens.
            let _ = state.tick(60_000);
            assert!(state.fully_revealed(), "the line is on screen to wrap");
            let mut scene = vn_scene(&state);
            let mut cache = LayoutCache::new();
            compute_layout(&mut scene, &mut cache, STAGE_W, 600);

            let mut rs = Vec::new();
            text_rects(&scene, &mut rs);
            assert!(rs.len() >= 3, "header + speaker + line: {rs:?}");
            for r in &rs {
                assert!(
                    r.y >= STAGE_H,
                    "no dialogue row may paint over the stage band (y < {STAGE_H}): {r:?}",
                );
                assert_eq!(r.x, MARGIN, "the column's left padding is real: {r:?}");
                assert!(r.w > 0 && r.h > 0, "every row is a real box: {r:?}");
            }
            for pair in rs.windows(2) {
                assert!(
                    pair[1].y >= pair[0].y + pair[0].h,
                    "rows stack without overlap even when one wraps: {:?} then {:?}",
                    pair[0],
                    pair[1],
                );
            }
            // The point of putting the rows in flow: a long line occupies
            // several lines and pushes its neighbours down, which an absolute
            // `y` per row could never do.
            assert!(
                rs.iter().any(|r| r.h > 32),
                "the long line must occupy more than one measured line: {rs:?}",
            );
        });
    }

    /// R1345 §5.21 — the sprites keep their solved stage geometry.
    ///
    /// `image_at` already authored `LayoutStyle`; the R1345 restructure (the
    /// dialogue moved into its own absolutely-positioned column) must not
    /// disturb the stage, whose art is placed by `SpritePos`.
    #[test]
    fn r1345_stage_art_keeps_its_absolute_geometry() {
        use pinion_runtime::{LayoutCache, compute_layout};

        fn images(s: &Scene, out: &mut Vec<(Rect, Option<String>)>) {
            match s {
                Scene::Image(i) => out.push((i.rect, i.tag.as_ref().map(ToString::to_string))),
                Scene::Container(c) => c.children.iter().for_each(|ch| images(ch, out)),
                _ => {}
            }
        }

        let owner = Owner::new();
        owner.run(|| {
            let state = VnState::new(VnScript::new(vec![VnStep::narration("밀물").with_stage(
                vec![
                    crate::vn::StageOp::Background {
                        source: "bg.png".to_string(),
                    },
                    crate::vn::StageOp::Show {
                        sprite: crate::vn::VnSprite::new(
                            "mudang",
                            "mudang.png",
                            crate::vn::SpritePos::Center,
                            1,
                        ),
                    },
                ],
            )]));
            let mut scene = vn_scene(&state);
            let mut cache = LayoutCache::new();
            compute_layout(&mut scene, &mut cache, STAGE_W, 600);

            let mut imgs = Vec::new();
            images(&scene, &mut imgs);
            assert_eq!(imgs.len(), 2, "background + one sprite: {imgs:?}");
            let bg = imgs
                .iter()
                .find(|(_, t)| t.as_deref() == Some("vn.background"))
                .expect("background tagged");
            assert_eq!(
                bg.0,
                Rect::new(0, 0, STAGE_W, STAGE_H),
                "the background fills the stage band",
            );
            let sprite = imgs
                .iter()
                .find(|(_, t)| t.as_deref() == Some("vn.sprite.mudang"))
                .expect("sprite tagged");
            assert!(
                sprite.0.y + sprite.0.h == STAGE_H,
                "the sprite's feet rest on the stage floor: {:?}",
                sprite.0,
            );
        });
    }
}
