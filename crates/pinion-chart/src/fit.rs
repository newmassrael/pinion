//! R1633 — axis labels that do not collide, and a report of what it cost.
//!
//! An axis puts a label at every tick. Once the ticks are closer together than
//! the labels are wide, the labels overlap and the axis becomes unreadable —
//! which is the state a dense dashboard is in most of the time, because its
//! category axes are endpoint names and its time axes are scrubbed to a window.
//!
//! # The two references do two different things, and neither is enough
//!
//! **The toolkit draws every label and then hides the ones that collide** —
//! and only on one of its two axes. Measured at 6.11:
//!
//! * `verticalaxis.cpp` keeps a running `height` (the top of the last shown
//!   label) and hides any label that would reach past it. That is a real
//!   greedy overlap pass.
//! * `horizontalaxis.cpp` hides a colliding label **only if its text has
//!   already been elided all the way down to `"..."`**
//!   (`pos().x() < last_label_max_x && toPlainText() == ellipsis`). An ordinary
//!   label that overlaps its neighbour is drawn anyway.
//! * Its newer graphs module has no overlap pass at all: it makes one text item
//!   per category, elides each to fit its own slot, and shows every one of them.
//!
//! So on the axis where the problem actually bites — the horizontal one,
//! whose labels are wide — the toolkit's answer is to truncate each label to
//! `...` and then hide the truncated ones. Both references' passes are also
//! **silent**: `setVisible(false)` leaves nothing to ask.
//!
//! **The DCC picks the step instead.** `get_min_line_distance_x` measures the
//! label at each end of the visible range, takes the wider, adds six pixels of
//! padding, and hands that to the grid-step chooser as a minimum spacing. No
//! label is ever dropped, because none is ever placed too close. That is the
//! better mechanism — and it only works on an axis with a **ladder** to climb.
//! Its editors have no categorical axis, so it never meets the case where the
//! ticks *are* the data.
//!
//! # What this does
//!
//! Both, chosen by what the axis is, because the two axis kinds genuinely
//! differ:
//!
//! * A **ladder** axis — linear, log, time — is [`FitRule::Coarsened`]: the
//!   tick target is lowered until the labels clear each other, so the axis
//!   simply has fewer ticks and **no label is dropped**. The DCC's mechanism,
//!   generalised past a linear frame axis.
//! * A **category** axis has no ladder: its ticks are its slots and its slots
//!   are the data, so the only move is to label some of them
//!   ([`FitRule::Strided`]). The first and last are **pinned**, because they
//!   are the two that say where the axis begins and ends — the toolkit's
//!   first-wins scan drops whichever end it reaches last.
//!
//! And in both cases the ticks themselves are untouched: [`Fitted::ticks`] is
//! every tick the axis has (the grid), [`Fitted::labelled`] is the subset that
//! gets a label. Thinning the grid with the labels would take away the slot
//! separators a category axis is read by.
//!
//! # Past both references
//!
//! * **It says what it did.** [`Fitted::omitted`] names the ticks whose labels
//!   were not drawn and [`Fitted::rule`] says by what rule, and both reach the
//!   scene on [`pinion_core::derivation`]. Neither reference publishes either:
//!   one calls `setVisible(false)`, the other never knows a label was at risk.
//! * **It says when it failed.** [`Fitted::crowding`] is how many pixels the
//!   tightest surviving pair still overlaps by, which is not zero when even two
//!   labels will not fit. A pass that quietly gives up and a pass that
//!   succeeded look the same in both references.
//! * **The measurement is the REAL one where there is one.** Both references
//!   measure the laid-out string, and so does this: [`Along::extent_px`] asks
//!   [`pinion_core::measured_text_extent`] — the same §5.36 seam a view fn
//!   sizes a content-fitted column with — **in the style the label is about to
//!   be painted in**, so the fit and the paint cannot disagree about the face.
//!   Headless there is no provider and the answer is `None`, and only then does
//!   the [`Along::Width`] model apply: character count times the widest
//!   character's advance, which is an over-estimate and therefore thins more
//!   than needed rather than overlapping. Its own module doc names that model
//!   as the thing the seam retires, so it is the fallback here and not the
//!   plan. The DCC measures only the labels at the two *ends* of the range,
//!   which is an under-estimate the moment a longer label falls between them —
//!   this measures every label it is about to draw.
//! * **One derivation for both axes.** The toolkit's two axes are two
//!   implementations with different behaviour; the difference here is one value
//!   ([`Along`]), because a horizontal axis's labels take room by their width
//!   and a vertical axis's by their line height.

use pinion_core::measured_text_extent;
use pinion_core::style::TextStyle;

use crate::scale::ValueScale;
use crate::ticks::TickFormat;

/// How much room one label takes **along the axis it labels** (R1633).
///
/// The whole difference between a horizontal and a vertical axis, as a value
/// rather than as two code paths. A vertical axis is the easy one and this is
/// why: every label needs the same height, so its fit is a property of the tick
/// spacing alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Along {
    /// A **horizontal** axis: labels sit side by side, so each needs its own
    /// width — which is why this is the axis the references struggle with.
    ///
    /// The number is the **fallback**: when no shell has seeded a
    /// [`TextMetrics`](pinion_core::TextMetrics) provider the width is modelled
    /// as the label's character count times `advance_px`, the advance of the
    /// widest character at the label's size. For a numeric or time label that
    /// model is *exact* rather than approximate, because the digits of a UI
    /// font are tabular; for a category name it is an upper bound, which is the
    /// safe direction.
    Width {
        /// The widest character's advance, in pixels — used only when nothing
        /// can measure.
        advance_px: u32,
    },
    /// A **vertical** axis: labels stack, so each needs one line's height and
    /// they all need the same.
    Height {
        /// One line's height, in pixels — again the fallback.
        line_px: u32,
    },
}

impl Along {
    /// How many pixels `label` occupies along the axis, in the style it will be
    /// painted in.
    ///
    /// Measured through [`pinion_core::measured_text_extent`] when a shell has
    /// seeded a provider, and modelled from this value's own number when
    /// nothing has — which is the deterministic headless answer rather than a
    /// made-up one, exactly the fallback that seam's contract calls for.
    ///
    /// `style` is the label's own, so the measurement and the paint cannot
    /// disagree about the face, the size or the family. That is the whole
    /// reason the style is threaded down here instead of a size being passed.
    #[must_use]
    pub fn extent_px(self, label: &str, style: &TextStyle) -> u32 {
        let measured = measured_text_extent(label, style, None);
        match self {
            Self::Width { advance_px } => measured.map_or_else(
                || u32::try_from(label.chars().count()).unwrap_or(u32::MAX) * advance_px,
                pinion_core::TextExtent::width,
            ),
            Self::Height { line_px } => measured.map_or(line_px, pinion_core::TextExtent::height),
        }
    }
}

/// The room one axis's labels have, and how they take it up (R1633).
///
/// One value rather than two parameters travelling together, because every step
/// of a fit needs both: the orientation says *what* to measure and the style
/// says *in what face*, and a call that got them from different places would
/// measure a label it is not about to paint.
#[derive(Debug, Clone, PartialEq)]
pub struct Room {
    along: Along,
    style: TextStyle,
}

impl Room {
    /// The room a label of `style` has on an axis it takes up space `along`.
    #[must_use]
    pub const fn new(along: Along, style: TextStyle) -> Self {
        Self { along, style }
    }

    /// How many pixels `label` occupies along this axis.
    #[must_use]
    pub fn extent_px(&self, label: &str) -> u32 {
        self.along.extent_px(label, &self.style)
    }
}

/// How the labels were made to fit (R1633).
///
/// Always published, even as [`Self::Fits`], because "nothing was dropped" is
/// itself the answer a reader needs before trusting an axis they are reading
/// values off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitRule {
    /// Every label the axis asked for fits. Nothing was changed.
    Fits,
    /// A **ladder** axis: the tick target was lowered to the count named here
    /// until the labels cleared each other. No label was dropped — the axis has
    /// fewer ticks.
    Coarsened {
        /// The target the pass settled on.
        target: usize,
    },
    /// A **category** axis: there is no step to widen, so every `stride`-th
    /// slot is labelled, with the first and last pinned.
    Strided {
        /// The gap between labelled slots, in slots. Two means every other.
        stride: usize,
    },
}

impl FitRule {
    /// The wire spelling, and the word a client matches on.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fits => "fits",
            Self::Coarsened { .. } => "coarsened",
            Self::Strided { .. } => "strided",
        }
    }
}

/// An axis's ticks after a label-fit pass (R1633).
#[derive(Debug, Clone, PartialEq)]
pub struct Fitted {
    ticks: Vec<f64>,
    labelled: Vec<f64>,
    rule: FitRule,
    crowding: u32,
}

impl Fitted {
    /// An axis with no ticks at all — for a chart whose axis is labelled
    /// somewhere other than by its own ticks.
    pub(crate) fn empty() -> Self {
        Self {
            ticks: Vec::new(),
            labelled: Vec::new(),
            rule: FitRule::Fits,
            crowding: 0,
        }
    }

    /// Every tick the axis has — what the **grid** is drawn from.
    #[must_use]
    pub fn ticks(&self) -> &[f64] {
        &self.ticks
    }

    /// The ticks that get a **label**. A subset of [`Self::ticks`], in the same
    /// order.
    #[must_use]
    pub fn labelled(&self) -> &[f64] {
        &self.labelled
    }

    /// The ticks whose label was **not** drawn.
    ///
    /// Empty for a ladder axis however much it coarsened, because coarsening
    /// removes the tick rather than its label — which is the difference between
    /// "the axis is less detailed" and "the axis is hiding something", and the
    /// reason the two rules are not one.
    #[must_use]
    pub fn omitted(&self) -> Vec<f64> {
        self.ticks
            .iter()
            .filter(|t| !self.labelled.contains(t))
            .copied()
            .collect()
    }

    /// How the fit was reached.
    #[must_use]
    pub const fn rule(&self) -> FitRule {
        self.rule
    }

    /// How many pixels the tightest surviving pair of labels still overlaps by,
    /// or zero when they clear each other.
    ///
    /// Non-zero means the pass **could not** succeed: even the coarsest ladder
    /// or the widest stride leaves two labels touching, so the caller has to
    /// rotate, elide, or give the axis more room. Both references reach the
    /// same state and neither says so.
    #[must_use]
    pub const fn crowding(&self) -> u32 {
        self.crowding
    }

    /// Whether every drawn label clears its neighbours.
    #[must_use]
    pub const fn is_clear(&self) -> bool {
        self.crowding == 0
    }
}

/// The slot indices a category fit kept a label for (R1633).
///
/// A category tick IS its slot index as a value, so this is the projection back
/// — the form the charts that draw one label per slot need, since they iterate
/// slots rather than ticks.
pub(crate) fn labelled_indices(fitted: &Fitted) -> std::collections::BTreeSet<usize> {
    // `to_usize` is the crate's one value-to-slot conversion, with the
    // "already clamped by the caller" contract these values meet by
    // construction: a category tick IS `index_value(slot)`.
    fitted
        .labelled()
        .iter()
        .filter(|t| t.is_finite() && **t >= 0.0)
        .map(|t| crate::scale::to_usize(t.round()))
        .collect()
}

/// The pixel gap kept between two labels.
///
/// Two pixels rather than none, because a label that ends exactly where the
/// next begins reads as one word. The DCC pads by six around a much larger
/// default spacing; this is added to a width that is already an over-estimate.
const GAP_PX: u32 = 2;

/// The fewest ticks a ladder axis will coarsen to.
///
/// Two, because an axis labelled at one end only cannot be read as a scale at
/// all — and reaching this floor without fitting is exactly the state
/// [`Fitted::crowding`] exists to report.
const FLOOR: usize = 2;

/// Fit the labels of one axis: coarsen a ladder, stride a category, and report
/// what that cost (R1633).
///
/// `generate` answers the ticks for a candidate target and `format` answers how
/// a tick is labelled at that target — both are the axis's own, passed in
/// rather than reached for, so this function is the fit and nothing else.
pub(crate) fn fit(
    scale: &ValueScale,
    target: usize,
    room: &Room,
    generate: impl Fn(&ValueScale, usize) -> Vec<f64>,
    format: impl Fn(&ValueScale, &[f64]) -> TickFormat,
) -> Fitted {
    let categorical = matches!(scale, ValueScale::Category(_));
    if categorical {
        return stride_to_fit(scale, generate(scale, target), room, &format);
    }
    coarsen_to_fit(scale, target, room, &generate, &format)
}

/// A **ladder** axis: lower the target until the labels clear.
fn coarsen_to_fit(
    scale: &ValueScale,
    target: usize,
    room: &Room,
    generate: &impl Fn(&ValueScale, usize) -> Vec<f64>,
    format: &impl Fn(&ValueScale, &[f64]) -> TickFormat,
) -> Fitted {
    let mut last = Vec::new();
    let mut crowding = 0;
    for candidate in (FLOOR..=target.max(FLOOR)).rev() {
        let ticks = generate(scale, candidate);
        crowding = overlap(scale, &ticks, room, &format(scale, &ticks));
        if crowding == 0 {
            return Fitted {
                labelled: ticks.clone(),
                ticks,
                rule: if candidate == target {
                    FitRule::Fits
                } else {
                    FitRule::Coarsened { target: candidate }
                },
                crowding: 0,
            };
        }
        last = ticks;
    }
    // The floor did not fit either. Answer it rather than pretending, and keep
    // the labels: two crowded labels still say more than none.
    Fitted {
        labelled: last.clone(),
        ticks: last,
        rule: FitRule::Coarsened { target: FLOOR },
        crowding,
    }
}

/// A **category** axis: keep every `stride`-th slot, first and last pinned.
fn stride_to_fit(
    scale: &ValueScale,
    ticks: Vec<f64>,
    room: &Room,
    format: &impl Fn(&ValueScale, &[f64]) -> TickFormat,
) -> Fitted {
    let text = format(scale, &ticks);
    let count = ticks.len();
    if count < 2 {
        return Fitted {
            labelled: ticks.clone(),
            ticks,
            rule: FitRule::Fits,
            crowding: 0,
        };
    }
    let mut crowding = 0;
    for stride in 1..count {
        let kept = strided(&ticks, stride);
        crowding = overlap(scale, &kept, room, &text);
        if crowding == 0 {
            return Fitted {
                rule: if stride == 1 {
                    FitRule::Fits
                } else {
                    FitRule::Strided { stride }
                },
                labelled: kept,
                ticks,
                crowding: 0,
            };
        }
    }
    // Not even the two ends clear each other.
    let kept = strided(&ticks, count - 1);
    Fitted {
        rule: FitRule::Strided { stride: count - 1 },
        labelled: kept,
        ticks,
        crowding,
    }
}

/// Every `stride`-th tick, with the **last** pinned.
///
/// The pin is why this is not a plain step: an axis whose final label is missing
/// does not say where it ends, and a reader cannot tell a thinned axis from one
/// whose data stops early. When the pinned last would land inside a stride of
/// the previous kept tick, that previous one goes instead — so the pin never
/// creates the collision the stride was chosen to avoid.
fn strided(ticks: &[f64], stride: usize) -> Vec<f64> {
    let count = ticks.len();
    let mut kept: Vec<f64> = ticks.iter().step_by(stride).copied().collect();
    let last = ticks[count - 1];
    if kept.last().copied() != Some(last) {
        if (count - 1) % stride != 0 && (count - 1) - ((count - 1) / stride) * stride < stride {
            kept.pop();
        }
        kept.push(last);
    }
    kept
}

/// How many pixels the tightest pair of labels overlaps by, or zero.
fn overlap(scale: &ValueScale, ticks: &[f64], room: &Room, format: &TickFormat) -> u32 {
    let mut placed: Vec<(f32, u32)> = ticks
        .iter()
        .filter_map(|&t| {
            scale
                .map(t)
                .map(|px| (px, room.extent_px(&format.label(t))))
        })
        .collect();
    placed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut worst = 0;
    for pair in placed.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        #[allow(clippy::cast_precision_loss)]
        let needed = (left.1 + right.1) as f32 / 2.0 + GAP_PX as f32;
        let clear = right.0 - left.0;
        if clear < needed {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let short = (needed - clear).ceil() as u32;
            worst = worst.max(short);
        }
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Categories;
    use crate::plot::{axis_format, axis_ticks};
    use crate::scale::{CategoryScale, LinearScale};

    /// A linear axis over `0..=100` in `px` pixels.
    fn linear(px: f32) -> ValueScale {
        ValueScale::Linear(LinearScale::new((0.0, 100.0), (0.0, px)))
    }

    /// A category axis over `n` slots named `cat-0`.. in `px` pixels.
    fn categories(n: usize, px: f32) -> ValueScale {
        let names: Vec<String> = (0..n).map(|i| format!("cat-{i}")).collect();
        #[allow(clippy::cast_precision_loss)]
        let hi = (n - 1) as f64;
        ValueScale::Category(CategoryScale::new(
            Categories::new(names),
            (0.0, hi),
            (0.0, px),
        ))
    }

    /// A room whose face is the default label style — so a test states the
    /// advance it is reasoning about and nothing else varies.
    fn room(along: Along) -> Room {
        Room::new(along, crate::ChartStyle::default().label_text_style())
    }

    /// The labels a fit actually draws.
    fn drawn(scale: &ValueScale, fitted: &Fitted) -> Vec<String> {
        let format = axis_format(scale, fitted.ticks());
        fitted.labelled().iter().map(|&t| format.label(t)).collect()
    }

    /// ★ A **ladder** axis coarsens rather than dropping: the labels that
    /// survive are the whole tick set, and the axis simply has fewer ticks.
    ///
    /// The counterfactual is in the same test — the identical axis with room
    /// keeps its six ticks — so "it coarsened" is not something it does to
    /// every axis it is given.
    #[test]
    fn r1633_a_ladder_axis_coarsens_and_drops_no_label() {
        let along = room(Along::Width { advance_px: 7 });
        let roomy = axis_ticks(&linear(600.0), 6, &along);
        assert_eq!(roomy.rule(), FitRule::Fits);
        assert!(roomy.omitted().is_empty());
        assert_eq!(roomy.ticks().len(), roomy.labelled().len());

        let cramped = axis_ticks(&linear(70.0), 6, &along);
        assert!(
            matches!(cramped.rule(), FitRule::Coarsened { .. }),
            "the same axis in an eighth of the room coarsens: {:?}",
            cramped.rule()
        );
        assert!(
            cramped.ticks().len() < roomy.ticks().len(),
            "and it has fewer TICKS, not fewer labels: {:?} vs {:?}",
            cramped.ticks(),
            roomy.ticks()
        );
        assert!(
            cramped.omitted().is_empty(),
            "★ nothing was hidden — that is the difference between coarsening \
             and thinning, and why the two rules are not one"
        );
        assert!(cramped.is_clear(), "and the survivors clear each other");
    }

    /// ★ A **category** axis strides, because its ticks are its data: the grid
    /// keeps every slot and only the labels thin.
    #[test]
    fn r1633_a_category_axis_strides_and_keeps_every_slot_ticked() {
        let along = room(Along::Width { advance_px: 7 });
        let scale = categories(20, 300.0);
        let fitted = axis_ticks(&scale, 6, &along);

        assert_eq!(fitted.ticks().len(), 20, "every slot still ticks");
        assert!(
            matches!(fitted.rule(), FitRule::Strided { .. }),
            "and the labels stride: {:?}",
            fitted.rule()
        );
        assert!(
            fitted.labelled().len() < 20,
            "which means fewer of them: {:?}",
            fitted.labelled()
        );
        assert_eq!(
            fitted.omitted().len(),
            20 - fitted.labelled().len(),
            "and the ones that were dropped are NAMED"
        );
        assert!(fitted.is_clear());
    }

    /// ★ The **first and last** slots keep their labels, whatever the stride.
    ///
    /// The reference's greedy first-wins scan drops whichever end it reaches
    /// last, so the axis stops saying where it ends. Asserted across a range of
    /// widths so it is a property of the rule rather than of one fixture — with
    /// a guard that the range really does produce more than one stride.
    #[test]
    fn r1633_a_stride_pins_both_ends() {
        let along = room(Along::Width { advance_px: 7 });
        let mut strides = std::collections::BTreeSet::new();
        for px in [120.0, 200.0, 320.0, 500.0, 900.0_f32] {
            let scale = categories(17, px);
            let fitted = axis_ticks(&scale, 6, &along);
            let labels = drawn(&scale, &fitted);
            assert_eq!(
                labels.first().map(String::as_str),
                Some("cat-0"),
                "{px}px: the first slot is labelled"
            );
            assert_eq!(
                labels.last().map(String::as_str),
                Some("cat-16"),
                "{px}px: and so is the last — the one the reference loses"
            );
            assert!(fitted.is_clear(), "{px}px: and nothing overlaps");
            if let FitRule::Strided { stride } = fitted.rule() {
                strides.insert(stride);
            } else {
                strides.insert(1);
            }
        }
        assert!(
            strides.len() > 1,
            "the fixture must reach more than one stride or it proves nothing: {strides:?}"
        );
    }

    /// ★ When even the coarsest answer does not fit, it **says so** rather than
    /// pretending — the state both references reach silently.
    #[test]
    fn r1633_a_crowded_axis_reports_how_short_it_is() {
        let along = room(Along::Width { advance_px: 20 });
        let fitted = axis_ticks(&linear(40.0), 6, &along);
        assert!(
            !fitted.is_clear(),
            "two twenty-pixel-per-character labels cannot share forty pixels"
        );
        assert!(
            fitted.crowding() > 0,
            "and the shortfall is a MEASUREMENT, not a flag: {}",
            fitted.crowding()
        );
        assert!(
            !fitted.labelled().is_empty(),
            "the labels are still drawn — two crowded labels say more than none"
        );
    }

    /// ★ A **vertical** axis is the same derivation with one value changed, and
    /// its fit depends on the tick spacing alone because every label is one
    /// line tall.
    #[test]
    fn r1633_a_vertical_axis_fits_by_line_height() {
        let tall = axis_ticks(&linear(400.0), 8, &room(Along::Height { line_px: 15 }));
        assert_eq!(tall.rule(), FitRule::Fits);

        let short = axis_ticks(&linear(60.0), 8, &room(Along::Height { line_px: 15 }));
        assert!(
            matches!(short.rule(), FitRule::Coarsened { .. }),
            "sixty pixels does not hold eight fifteen-pixel lines: {:?}",
            short.rule()
        );
        // ★ And the WIDTH of the labels is irrelevant to it: the same axis with
        // labels ten times wider fits exactly the same, which a pass that
        // measured width on both axes would get wrong.
        let same = axis_ticks(&linear(60.0), 8, &room(Along::Height { line_px: 15 }));
        assert_eq!(same.ticks(), short.ticks());
    }

    /// ★ The fit **measures** where a shell has seeded a provider, and only
    /// models where nothing can.
    ///
    /// The whole claim of this round's measurement half, and the one that
    /// cannot be checked by looking at the fit's own numbers: the same axis is
    /// fitted twice, once headless and once under a provider whose answer
    /// deliberately DISAGREES with the model, and the two outcomes differ. A
    /// pass that ignored the seam would answer identically both times.
    ///
    /// Both references measure the real face — the toolkit's
    /// `horizontalAdvance`, the DCC's `BLF_width` — so measuring is parity
    /// rather than an extra; what is past them is that the fallback is
    /// *declared* instead of being a silent zero.
    #[test]
    fn r1633_the_fit_measures_where_it_can_and_models_where_it_cannot() {
        use pinion_core::{Owner, TEXT_METRICS, TextExtent, TextMetrics};
        use std::rc::Rc;

        /// Four pixels per character whatever the size — narrower than the
        /// model's default advance, so a measured axis fits MORE labels than a
        /// modelled one. The disagreement is the point.
        #[derive(Debug)]
        struct Narrow;
        impl TextMetrics for Narrow {
            fn measure(
                &self,
                text: &str,
                style: &pinion_core::style::TextStyle,
                _max: Option<u32>,
            ) -> Option<TextExtent> {
                let width = u32::try_from(text.chars().count()).unwrap_or(0) * 4;
                Some(TextExtent::new(width, style.font_size_px))
            }
        }

        let scale = categories(24, 300.0);
        let along = room(Along::Width { advance_px: 12 });

        // Headless: no provider, so the model applies and its wide advance
        // strides hard.
        let modelled = axis_ticks(&scale, 6, &along);
        assert!(matches!(modelled.rule(), FitRule::Strided { .. }));

        // Under a provider that says the labels are a third as wide, the same
        // axis keeps more of them.
        let owner = Owner::new();
        TEXT_METRICS.provide(&owner, Rc::new(Narrow));
        let measured = owner.run(|| axis_ticks(&scale, 6, &along));

        assert!(
            measured.labelled().len() > modelled.labelled().len(),
            "★ the seam was consulted: measured {} labels vs modelled {}",
            measured.labelled().len(),
            modelled.labelled().len()
        );
        assert!(measured.is_clear() && modelled.is_clear());
        // ...and the fallback is still exactly the model, which is what makes
        // a headless answer deterministic rather than absent.
        assert_eq!(
            along.extent_px("abcd"),
            4 * 12,
            "outside the owner, the model answers"
        );
        assert_eq!(
            owner.run(|| along.extent_px("abcd")),
            4 * 4,
            "and inside it, the provider does"
        );
    }

    /// The advance model is an **upper bound**: a label's extent never
    /// under-states, which is the direction that cannot produce an overlap.
    #[test]
    fn r1633_the_extent_model_never_understates() {
        let along = room(Along::Width { advance_px: 6 });
        assert_eq!(along.extent_px("12"), 12);
        assert_eq!(along.extent_px(""), 0);
        assert_eq!(
            along.extent_px("가나"),
            12,
            "counted in characters, so a wide script is under-counted by the \
             MODEL and over-counted by the advance a consumer measures for it"
        );
        assert_eq!(
            room(Along::Height { line_px: 15 }).extent_px("anything at all"),
            15,
            "a line is a line"
        );
    }
}
