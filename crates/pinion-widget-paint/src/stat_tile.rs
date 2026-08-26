//! R1843 §5.2 §5.11 — a **KPI stat tile**: a label over a value, optionally a
//! delta, optionally a trailing figure the caller supplies, in a box whose every
//! word is *placed* by [`crate::caption`] rather than offset by hand.
//!
//! ## Why this is a crate and not a third copy
//!
//! Two sites in this tree already draw this shape, and neither could see the
//! other:
//!
//! * `examples/hello-stat-tiles` — label, value, delta and a
//!   `pinion_chart::Sparkline`, in its own `fn tile`.
//! * `examples/hello-analyzer-shell`'s latency card — three tiles, label over
//!   value, no trailing figure, built inline in `latency_body`.
//!
//! The analysis-tool census asks the dashboard for *KPI stat tiles with
//! sparklines*, which is the UNION of what those two each do half of. Written
//! at the third site it would have been a third hand-rolled tile; the count at
//! which this tree lifts an internal duplication into substrate is three, and
//! this is that lift.
//!
//! ## The sparkline is the CALLER's, and that is a dependency decision
//!
//! `pinion-widget-paint` does not depend on `pinion-chart`, and a tile that
//! embedded a sparkline would make it. So the trailing figure is a scene the
//! caller builds for a rectangle this module hands them
//! ([`StatTile::build_with`]): the two consumers that want a chart pass one, the
//! latency card passes nothing, and the widget crate keeps its dependency set.
//! It also means the trailing figure is not *restricted* to a sparkline — a
//! gauge, an icon strip or a second value fit the same slot.
//!
//! ## Every word is a child of its own box
//!
//! Each row is built with [`crate::caption::captioned`], so a row's word is a
//! CHILD of a box tagged for that row rather than a sibling of the tile. This
//! tree files a painted mark under its nearest tagged ancestor, so a word
//! painted beside its box is filed under whatever encloses both — the defect
//! [`crate::caption`] was written for. A tile that hand-positioned three runs
//! inside one container would reintroduce it three times over.
//!
//! Tags are `{tag}.label`, `{tag}.value`, `{tag}.delta` and `{tag}.trail`, each
//! carrying its caption at the framework's own
//! [`crate::caption::CAPTION_SUFFIX`].
//!
//! ## ★★★★★ A TILE IS ONE REGION TO A READER, AND R1846 IS WHERE IT SAID SO
//!
//! Tagging each word's own box is what the section above is for, and it has a
//! consequence the first version did not answer: **a tagged region this tree
//! cannot classify is `unvoiced`**, and the tile authored four of them per
//! tile — nine once their captions and a caller's figure are counted. Measured
//! at R1846 on the one screen that composes this tile, `scene/voice` reported
//! **27 undecided regions**, one third of a card that every other gate called
//! green.
//!
//! So each region now declares what it is:
//!
//! | region | declaration | why |
//! |---|---|---|
//! | `{tag}.label` | [`Silence::name_of`] | its text IS the tile's name — a voice here says it twice |
//! | `{tag}.value` | [`Silence::part_of`] | folded into the tile's value |
//! | `{tag}.delta` | [`Silence::part_of`] | folded into the tile's value |
//! | `{tag}.trail` | [`Silence::part_of`] | the tile's trailing figure, and this one covers the CALLER's scene |
//!
//! All four name the tile's own tag, and [`pinion_core::voice::SilenceKind`]
//! says a borrowed name and a folded part both reach the subtree — which is
//! how four declarations answer for nine regions, captions included.
//!
//! ⚠ **What this asks of a consumer, stated rather than assumed: the tile's
//! own tag must be a node that speaks.** A tile whose root has no
//! `AccessNode` now reports four `dangling` regions instead of nine `unvoiced`
//! ones — a NAMED defect where there had been an anonymous one, which is the
//! direction this project moves such things. It is not a new requirement; a
//! stat tile nothing announces was always unreadable.
//!
//! ## The placements are in the TILE's space
//!
//! A child laid out absolutely resolves against its container, so the rows are
//! built in the tile's own coordinates and [`Tile`]'s placements are in that
//! space. [`Tile::origin`] is the offset to the caller's space. Returning them
//! untranslated is deliberate: [`crate::caption::Placed`] is the rectangle the
//! run was actually built with, and moving one by arithmetic here would make it
//! a recomputation again, which is the thing that module exists to stop.

use pinion_core::Scene;
use pinion_core::containment::line_box;
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextOverflow, TextStyle};
use pinion_core::voice::Silence;

use crate::caption::{Align, Caption, Placed, Pointer, captioned};

// ★★★★★ R1843 — a hand-rolled `elide` lived here and is DELETED rather than
// kept beside the fix, and the reason is worth more than the code was.
//
// A caption too wide for its box is reported by the ink census as a mark
// painted outside it, and the tempting repair is to pick a shorter word. This
// round made that move THREE times in a row — each one satisfied the gate at
// the width in front of it and said nothing about the next width the sweep
// tries — then wrote an `elide` that trimmed the text automatically, which was
// the same mistake with a loop around it.
//
// ⚠⚠ None of them could ever have worked, and the numbers say why: `Caption`'s
// headless fallback is `chars * px / 2` while the census's stand-in is
// `chars * px`. **A factor of two.** Trimming against the first can never
// satisfy the second.
//
// What works is one line in `row` below — declaring that the paint is CONFINED
// (`TextOverflow`), which is what makes the census clamp its stand-in to the
// rectangle at all. Keeping a mechanism that cannot work, next to the one that
// does, is how the next reader comes to believe the wrong rule.

/// ★★★★★ R1843 — a row's height is DERIVED from its face, and there is no
/// setter for it.
///
/// The first draft carried three constants (12, 17, 12) and a
/// `with_row_heights` to override them. The running application refused it by
/// name: *"a text box authored at the font size rather than at its LINE box —
/// the shaper's line for a 12px face is 21px"*, and nine of this card's marks
/// were reported outside their boxes. A 12px label in a 12px box overflows **by
/// construction**, because ascent, descent and leading all sit outside the em.
///
/// So the numbers are gone rather than corrected, and the setter with them.
/// Correcting them would have left the same defect one edit away; deriving the
/// height from [`line_box`] makes a row too short for its own face
/// unrepresentable, which is the difference between fixing a value and removing
/// a way to be wrong.
fn row_height(style: &TextStyle) -> u32 {
    line_box(style.font_size_px.max(1))
}

/// A label over a value, with an optional delta and an optional trailing figure.
#[derive(Debug, Clone)]
pub struct StatTile {
    label: String,
    value: String,
    delta: Option<String>,
    label_style: TextStyle,
    value_style: TextStyle,
    delta_style: TextStyle,
    box_style: BoxStyle,
    pad_x: u32,
    pad_y: u32,
    gap: u32,
    trail_h: u32,
    align: Align,
}

impl StatTile {
    /// A tile showing `value` under `label`.
    ///
    /// Everything else is defaulted, so the smallest call is the two facts a
    /// stat tile is *about*. Alignment starts at [`Align::Start`], which is the
    /// framework default and therefore changes nothing for a caller who does
    /// not say.
    #[must_use]
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            delta: None,
            label_style: TextStyle::default(),
            value_style: TextStyle::default(),
            delta_style: TextStyle::default(),
            box_style: BoxStyle::default(),
            pad_x: 10,
            pad_y: 6,
            gap: 4,
            trail_h: 0,
            align: Align::Start,
        }
    }

    /// A third row, under the value — a change, a target, a unit.
    #[must_use]
    pub fn with_delta(mut self, delta: impl Into<String>) -> Self {
        self.delta = Some(delta.into());
        self
    }

    /// The label row's text style.
    #[must_use]
    pub fn with_label_style(mut self, style: TextStyle) -> Self {
        self.label_style = style;
        self
    }

    /// The value row's text style.
    #[must_use]
    pub fn with_value_style(mut self, style: TextStyle) -> Self {
        self.value_style = style;
        self
    }

    /// The delta row's text style.
    #[must_use]
    pub fn with_delta_style(mut self, style: TextStyle) -> Self {
        self.delta_style = style;
        self
    }

    /// The tile box's own fill, radius and border.
    #[must_use]
    pub fn with_box_style(mut self, style: BoxStyle) -> Self {
        self.box_style = style;
        self
    }

    /// Room kept clear inside the tile, on each side of each axis.
    #[must_use]
    pub const fn with_padding(mut self, pad_x: u32, pad_y: u32) -> Self {
        self.pad_x = pad_x;
        self.pad_y = pad_y;
        self
    }

    /// Room between one row and the next.
    #[must_use]
    pub const fn with_gap(mut self, gap: u32) -> Self {
        self.gap = gap;
        self
    }

    /// Reserve `height` at the bottom for a figure the caller draws.
    ///
    /// Reserving is separate from supplying because the caller cannot build the
    /// figure until they know its rectangle, and they cannot know its rectangle
    /// until the rows above it are laid out. [`StatTile::build_with`] closes
    /// that loop.
    #[must_use]
    pub const fn with_trail(mut self, height: u32) -> Self {
        self.trail_h = height;
        self
    }

    /// Where each row's word sits horizontally inside the tile.
    #[must_use]
    pub const fn with_align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// The narrowest this tile can be drawn without shortening any of its words.
    ///
    /// ★★★★★ R1843 — the instrument a caller needs and a magic number cannot
    /// be. A strip deciding how many tiles fit was comparing against a
    /// hand-picked floor, which says nothing about whether a tile's OWN words
    /// fit: at the maximised board five tiles each cleared that floor and
    /// `"Round trip ms"` still hung 19px past its label box. The tile is the
    /// thing that knows what it has to say, so it is the thing that can answer
    /// how much room that needs.
    ///
    /// ⚠ **Measured with the CENSUS's stand-in (`chars * px`), which is NOT
    /// the rule [`place`] uses**, and the difference is deliberate — see the
    /// comment in the body. `Caption`'s headless fallback is `chars * px / 2`,
    /// a factor of two smaller, and R1843 spent four attempts trimming against
    /// that one while the gate measured with this one. So a caller that
    /// honours this width satisfies the gate; it is a conservative CEILING on
    /// what `place` will ask for, not the same number.
    ///
    /// ⚠⚠ R1850 — this paragraph said "measured through `Caption::extent`, the
    /// same sizing rule `place` will use", and BOTH halves were false: there is
    /// no such item (which is the broken intra-doc link that took CI red for
    /// five rounds), and the body twelve lines below already said the opposite
    /// on purpose. A doc and a comment inside one function contradicting each
    /// other is the same class as a gate that copies a value the framework
    /// owns — two declarations of one fact, free to drift.
    ///
    /// [`place`]: crate::caption::place
    #[must_use]
    pub fn min_width(&self) -> u32 {
        // The floor a tile needs for its rows to be legible rather than a
        // column of ellipses: room for its widest row's characters at that
        // row's face, plus the padding either side.
        //
        // ⚠ Deliberately the CENSUS's stand-in (`chars * px`) and not
        // `Caption`'s headless fallback (`chars * px / 2`). The two are a
        // factor of two apart, and R1843 spent four attempts trimming against
        // the smaller one while the gate measured with the larger.
        let widest = |text: &str, style: &TextStyle| {
            u32::try_from(text.chars().count())
                .unwrap_or(u32::MAX)
                .saturating_mul(style.font_size_px.max(1))
        };
        let mut need =
            widest(&self.label, &self.label_style).max(widest(&self.value, &self.value_style));
        if let Some(delta) = self.delta.as_ref() {
            need = need.max(widest(delta, &self.delta_style));
        }
        need + self.pad_x * 2
    }

    /// The rectangle a trailing figure would be given, in the tile's own space.
    ///
    /// `None` when no trail was reserved, or when the rows above it have already
    /// used the height — a tile too short for its own figure draws the words and
    /// says so here rather than painting a figure of negative height.
    #[must_use]
    pub fn trail_rect(&self, rect: Rect) -> Option<Rect> {
        if self.trail_h == 0 {
            return None;
        }
        let top = self.rows_bottom();
        let inner_w = rect.w.saturating_sub(self.pad_x * 2);
        let bottom = rect.h.saturating_sub(self.pad_y);
        if inner_w == 0 || top + self.trail_h > bottom {
            return None;
        }
        Some(Rect::new(self.pad_x, top, inner_w, self.trail_h))
    }

    /// Build the tile, with no trailing figure.
    #[must_use]
    pub fn build(&self, tag: &str, rect: Rect) -> Tile {
        self.assemble(tag, rect, None)
    }

    /// Build the tile, asking `trail` for the trailing figure.
    ///
    /// `trail` is called with the rectangle from [`StatTile::trail_rect`], in
    /// the tile's own coordinate space, and only when there is one — so a caller
    /// never builds a figure that has nowhere to go.
    #[must_use]
    pub fn build_with(&self, tag: &str, rect: Rect, trail: impl FnOnce(Rect) -> Scene) -> Tile {
        let scene = self.trail_rect(rect).map(trail);
        self.assemble(tag, rect, scene)
    }

    /// Where the text rows end, measured from the tile's top edge.
    fn rows_bottom(&self) -> u32 {
        let mut y =
            self.pad_y + row_height(&self.label_style) + self.gap + row_height(&self.value_style);
        if self.delta.is_some() {
            y += self.gap + row_height(&self.delta_style);
        }
        y + self.gap
    }

    fn row(&self, tag: &str, text: &str, style: &TextStyle, rect: Rect) -> (Scene, Placed) {
        // Shortened to the row's own width, so the caption `place` measures is
        // one that fits — see [`elide`] for why the tile does this rather than
        // its callers picking shorter words.
        // ★★★★★ R1843 — the row's style BOUNDS ITS INK, and this one line is
        // what four other attempts were groping for.
        //
        // The ink census does not read a run's rectangle and does not shape
        // text. `stand_in_ink` stands in for the glyphs with `chars * px`, and
        // it clamps that to the run's rectangle **only when the style's
        // overflow confines the paint** — `TextOverflow::Visible`, the default,
        // does not. So a caption in the default style is measured as though
        // every character were a full em wide and reported as ink outside its
        // box, however narrow the rectangle it was placed in.
        //
        // ⚠ And the two estimates are a FACTOR OF TWO apart: `Caption`'s
        // headless fallback is `chars * px / 2`, the census's stand-in is
        // `chars * px`. Trimming against the first can never satisfy the
        // second, which is why shortening the words, eliding them, and asking
        // the tile for a minimum width all produced byte-identical failures.
        // The fix is not a smaller string; it is saying that the paint is
        // confined, which is what every other card on that screen already says
        // through its own clipping helper.
        let style = style.clone().with_overflow(TextOverflow::Ellipsis);
        let caption = Caption::new(text, style).align(self.align);
        // ★★★★★ R1843 — the run is CLAMPED to its row, and this is the one
        // thing in this module that overrides the shaper on purpose.
        //
        // `elide` above trims against whatever `place` will measure with — but
        // in this tree's paint path there is no metrics provider at scene-build
        // time, so that measurement is `Sized::Guessed`, and the guess
        // (`chars * px / 2`) UNDER-measures real glyphs. The census that reads
        // the finished scene does have real metrics. So a run sized by the
        // guess can be placed inside its box and still be reported outside it,
        // which is what `"Round trip ms"` did at 19px past its label box
        // through three separate attempts to shorten it.
        //
        // A tile's row exists to hold that row's word. Stating the row's own
        // width makes containment a PROPERTY of the tile rather than an
        // outcome of whether a provider happened to be installed. What it costs
        // is the overflow signal — a clamped caption always reports `Fits` —
        // and that is the honest trade to name: the tile guarantees the mark is
        // inside its box, and `elide` is what keeps the words readable when
        // there are real metrics to trim against.
        let caption = caption.stating((rect.w, rect.h));
        captioned(
            tag,
            rect,
            BoxStyle::default(),
            &caption,
            // The words do not take presses; the tile does, so a click anywhere
            // on it reaches one target rather than three depending on where the
            // glyphs happened to land.
            Pointer::Transparent,
        )
    }

    fn assemble(&self, tag: &str, rect: Rect, trail: Option<Scene>) -> Tile {
        let inner_w = rect.w.saturating_sub(self.pad_x * 2);
        let mut y = self.pad_y;

        let (label_scene, label) = self.row(
            &format!("{tag}.label"),
            &self.label,
            &self.label_style,
            Rect::new(self.pad_x, y, inner_w, row_height(&self.label_style)),
        );
        // ★★★★★ R1846 — the label's text is the tile's NAME, so a voice here
        // would say it twice. See [`Self::assemble`]'s own note below for why
        // every region this tile authors has to answer that question.
        let label_scene = label_scene.silenced(Silence::name_of(tag.to_owned()));
        y += row_height(&self.label_style) + self.gap;

        let (value_scene, value) = self.row(
            &format!("{tag}.value"),
            &self.value,
            &self.value_style,
            Rect::new(self.pad_x, y, inner_w, row_height(&self.value_style)),
        );
        let value_scene = value_scene.silenced(Silence::part_of(tag.to_owned()));
        y += row_height(&self.value_style);

        let mut children = vec![label_scene, value_scene];
        let delta = self.delta.as_ref().map(|text| {
            y += self.gap;
            let (scene, placed) = self.row(
                &format!("{tag}.delta"),
                text,
                &self.delta_style,
                Rect::new(self.pad_x, y, inner_w, row_height(&self.delta_style)),
            );
            children.push(scene.silenced(Silence::part_of(tag.to_owned())));
            placed
        });

        let trail_rect = trail.map(|scene| {
            let where_ = self
                .trail_rect(rect)
                .unwrap_or_else(|| Rect::new(self.pad_x, self.rows_bottom(), inner_w, 0));
            children.push(
                Scene::Container(
                    ContainerNode::new(vec![scene])
                        .with_tag(format!("{tag}.trail"))
                        .with_layout(
                            LayoutStyle::new()
                                .with_absolute_position(where_.x, where_.y)
                                .with_size(Size::px(where_.w, where_.h))
                                .with_pointer_transparent(true),
                        ),
                )
                // ★★★★★ R1846 — and this one COVERS the caller's figure, which
                // is a claim about the tile's contract rather than about the
                // figure. `build_with` hands a rectangle the tile reserved to a
                // closure; what comes back is the tile's trailing figure, folded
                // into the tile's own announcement the way the delta is. A
                // figure that has to speak for itself is not a tile's trail —
                // it is a chart, and it wants its own region.
                //
                // ⚠ Stated here rather than left to each caller because a
                // silence the crate does not declare is a silence NOBODY
                // declares: measured at R1846, `pinion_chart::Sparkline` paints
                // `spark` and `spark.line` with no voice of their own, so the
                // two regions inside every trail were undecided in the one
                // screen that composes them.
                .silenced(Silence::part_of(tag.to_owned())),
            );
            where_
        });

        let scene = Scene::Container(
            ContainerNode::new(children)
                .with_tag(tag.to_owned())
                .with_style(self.box_style.clone())
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(rect.x, rect.y)
                        .with_size(Size::px(rect.w, rect.h)),
                ),
        );

        Tile {
            scene,
            origin: (rect.x, rect.y),
            label,
            value,
            delta,
            trail: trail_rect,
        }
    }
}

/// A built tile, and where each of its words landed.
///
/// Not `Clone`: [`Scene`] is not, deliberately — a scene is built once and
/// handed on, and a tile that could be duplicated would be a second way to get
/// two nodes answering to one tag.
#[derive(Debug)]
pub struct Tile {
    scene: Scene,
    origin: (u32, u32),
    label: Placed,
    value: Placed,
    delta: Option<Placed>,
    trail: Option<Rect>,
}

impl Tile {
    /// The tile's scene, ready to drop into a parent's children.
    #[must_use]
    pub fn into_scene(self) -> Scene {
        self.scene
    }

    /// The tile's scene, without consuming the placements beside it.
    #[must_use]
    pub const fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The tile's top-left in the CALLER's space — the offset every placement
    /// below is relative to.
    #[must_use]
    pub const fn origin(&self) -> (u32, u32) {
        self.origin
    }

    /// Where the label landed, in the tile's own space.
    #[must_use]
    pub const fn label(&self) -> Placed {
        self.label
    }

    /// Where the value landed, in the tile's own space.
    #[must_use]
    pub const fn value(&self) -> Placed {
        self.value
    }

    /// Where the delta landed, in the tile's own space, if there is one.
    #[must_use]
    pub const fn delta(&self) -> Option<Placed> {
        self.delta
    }

    /// The rectangle the trailing figure was given, in the tile's own space.
    #[must_use]
    pub const fn trail(&self) -> Option<Rect> {
        self.trail
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caption::CAPTION_SUFFIX;
    use pinion_core::scene::BoxNode;
    use pinion_core::voice::SilenceKind;

    /// A tile big enough for three rows and a figure.
    ///
    /// ⚠ R1843 grew this from 110 tall. Row heights are derived from
    /// [`line_box`] rather than authored, so a fixture sized against the old
    /// constants stopped holding a figure the moment the rows became as tall as
    /// their faces actually need. The test caught it, which is what a fixture
    /// tied to real geometry is for.
    const ROOM: Rect = Rect::new(40, 90, 160, 200);

    fn tags_of(scene: &Scene) -> Vec<String> {
        match scene {
            Scene::Container(node) => node
                .children
                .iter()
                .filter_map(|child| match child {
                    Scene::Container(row) => row.tag.as_ref().map(ToString::to_string),
                    Scene::Text(run) => run.tag.as_ref().map(ToString::to_string),
                    _ => None,
                })
                .collect(),
            other => panic!("a tile is a container, got {other:?}"),
        }
    }

    fn child_named<'a>(scene: &'a Scene, tag: &str) -> &'a Scene {
        match scene {
            Scene::Container(node) => node
                .children
                .iter()
                .find(|child| match child {
                    Scene::Container(row) => row.tag.as_deref() == Some(tag),
                    Scene::Text(run) => run.tag.as_deref() == Some(tag),
                    _ => false,
                })
                .unwrap_or_else(|| panic!("no child tagged {tag} in {:?}", tags_of(scene))),
            other => panic!("not a container: {other:?}"),
        }
    }

    /// ★★★★★ The whole reason this is a crate rather than a third hand-rolled
    /// tile. A word painted as a SIBLING of its box is filed under whatever
    /// encloses both, which is the defect [`crate::caption`] was written for —
    /// so the assertion is not "the words are present" but "each word is inside
    /// a box that is its own".
    #[test]
    fn r1843_every_word_is_a_child_of_a_box_that_is_its_own() {
        let tile = StatTile::new("throughput", "1.4 Gb/s")
            .with_delta("+3%")
            .build("card.health.stat.0", ROOM);

        for row in ["label", "value", "delta"] {
            let tag = format!("card.health.stat.0.{row}");
            let box_ = child_named(tile.scene(), &tag);
            assert_eq!(
                tags_of(box_),
                vec![format!("{tag}{CAPTION_SUFFIX}")],
                "{row}'s run must be the sole child of {row}'s own box"
            );
        }
    }

    /// ★★★★★ R1843 — a press anywhere on a tile reaches ONE target, and this
    /// assertion exists because a counterfactual FOUND that nothing checked it.
    ///
    /// The tile's rows are pointer-transparent on purpose: a click should
    /// reach the tile rather than land on whichever of the label, the value or
    /// the delta happened to be under the cursor. Breaking that — making each
    /// word a target — still compiles, still paints, and the whole suite
    /// stayed green. A mechanism nothing can refute is one nobody is holding.
    #[test]
    fn r1843_a_press_on_any_word_reaches_the_tile_and_not_the_word() {
        let tile = StatTile::new("depth", "12")
            .with_delta("+1")
            .build("t", ROOM);

        for row in ["label", "value", "delta"] {
            match child_named(tile.scene(), &format!("t.{row}")) {
                Scene::Container(node) => assert!(
                    node.layout.pointer_transparent,
                    "{row} takes presses, so a click on that word never reaches the tile"
                ),
                other => panic!("a row is a container, got {other:?}"),
            }
        }
    }

    /// A delta is a row a caller ASKS for. Without it the tile is two rows, and
    /// nothing downstream should have to distinguish "no delta" from "an empty
    /// delta" — the second is a row a reader sees and cannot read.
    #[test]
    fn r1843_a_delta_row_exists_exactly_when_one_was_declared() {
        let bare = StatTile::new("loss", "0.02%").build("t", ROOM);
        let with = StatTile::new("loss", "0.02%")
            .with_delta("-0.01")
            .build("t", ROOM);

        assert!(bare.delta().is_none());
        assert!(!tags_of(bare.scene()).contains(&"t.delta".to_owned()));
        assert!(with.delta().is_some());
        assert!(tags_of(with.scene()).contains(&"t.delta".to_owned()));
    }

    /// ★ The trailing figure is reserved and then supplied, and the gap between
    /// the two is where a caller could be handed a rectangle that does not
    /// exist. A tile with no room must not ask: a closure that runs has already
    /// built a figure with nowhere to go.
    #[test]
    fn r1843_a_figure_is_asked_for_only_when_there_is_room_for_it() {
        let short = Rect::new(0, 0, 160, 30);
        let spec = StatTile::new("rate", "88/s").with_trail(24);

        assert!(
            spec.trail_rect(short).is_none(),
            "30px cannot hold rows + 24"
        );
        assert!(spec.trail_rect(ROOM).is_some());

        let mut asked = 0_u32;
        let tile = spec.build_with("t", short, |rect| {
            asked += 1;
            Scene::Box(BoxNode::new(rect, BoxStyle::default()))
        });
        assert_eq!(asked, 0, "the closure ran for a figure with nowhere to go");
        assert!(tile.trail().is_none());
    }

    /// The figure sits BELOW every word, and a delta pushes it further down.
    /// Stated as an inequality against the rows' own placements rather than
    /// against a constant, so it keeps holding when a caller restyles the rows.
    #[test]
    fn r1843_the_figure_sits_below_every_word() {
        for delta in [None, Some("+1")] {
            let mut spec = StatTile::new("depth", "12").with_trail(20);
            if let Some(text) = delta {
                spec = spec.with_delta(text);
            }
            let tile = spec.build_with("t", ROOM, |rect| {
                Scene::Box(BoxNode::new(rect, BoxStyle::default()))
            });

            let trail = tile.trail().expect("ROOM holds a figure");
            let lowest = tile
                .delta()
                .map_or_else(|| tile.value().holder(), Placed::holder);
            assert!(
                trail.y >= lowest.y + lowest.h,
                "figure at {} overlaps the last row ending at {}",
                trail.y,
                lowest.y + lowest.h
            );
        }
    }

    /// The placements are in the TILE's space, and `origin` is what relates
    /// them to the caller's. Asserted because the alternative — translating
    /// them here — would turn a rectangle the run was BUILT with back into a
    /// recomputation, and that is the whole defect `caption` exists to stop.
    #[test]
    fn r1843_placements_are_in_the_tiles_own_space() {
        let tile = StatTile::new("peers", "7").build("t", ROOM);

        assert_eq!(tile.origin(), (ROOM.x, ROOM.y));
        assert!(
            tile.label().holder().y < ROOM.y,
            "a placement at {} is in the caller's space, not the tile's",
            tile.label().holder().y
        );
    }

    /// ★★★★★ R1846 — **every region this tile authors declares what it is.**
    ///
    /// The gate the module note above is about. A tagged region this tree
    /// cannot classify is `unvoiced`, and tagging each word's own box — which
    /// is why this crate exists — creates four of them per tile before any
    /// caption or caller's figure is counted. Measured on the one screen that
    /// composes this tile, that was 27 undecided regions in one card, invisible
    /// to `cargo test` because a voice census runs over a running screen.
    ///
    /// ⚠ The assertion is the DECLARATION and not the census's verdict: whether
    /// a borrowed name reaches a caption is `pinion_core::voice`'s question and
    /// is answered by its own tests. What this crate can be held to is that it
    /// answers for each region it makes, and names its own tile when it does.
    #[test]
    fn r1846_every_region_the_tile_authors_declares_a_voice() {
        // `with_trail` is what reserves the figure's band — without it the tile
        // builds three rows and no `.trail`, and the count below is what says
        // so rather than the test quietly answering for one region fewer.
        let tile = StatTile::new("throughput", "1.4 Gb/s")
            .with_delta("+3%")
            .with_trail(20)
            .build_with("card.health.stat.0", ROOM, |rect| {
                Scene::Box(BoxNode::new(rect, BoxStyle::default()))
            });

        // Every row the tile builds, and what it must say about itself. The
        // label is the tile's NAME; the rest are folded into what the tile
        // announces.
        let expected = [
            ("label", SilenceKind::NameOf),
            ("value", SilenceKind::PartOf),
            ("delta", SilenceKind::PartOf),
            ("trail", SilenceKind::PartOf),
        ];
        let authored = tags_of(tile.scene());
        assert_eq!(
            authored.len(),
            expected.len(),
            "this test answers for every region the tile authors, and it built {authored:?}"
        );

        for (row, kind) in expected {
            let tag = format!("card.health.stat.0.{row}");
            let child = child_named(tile.scene(), &tag);
            let silence = child
                .layout_style()
                .and_then(|style| style.silence.as_ref())
                .unwrap_or_else(|| {
                    panic!("{tag} declares no voice, so a census calls it undecided")
                });
            assert_eq!(silence.kind(), kind, "{row} declares the wrong kind");
            assert_eq!(
                silence.detail(),
                "card.health.stat.0",
                "{row} must name the tile it is part of, so the redirect can be checked"
            );
        }
    }
}
