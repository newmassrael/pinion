//! R2020 §5.38 §5.50 — the **badge**: a small read-out chip beside the thing it
//! is about, and the one decision that makes it either a label or a report.
//!
//! # Why this is a crate module and not a private helper
//!
//! It was a private helper — `config_form`'s `badge()` — and the shape it
//! painted was one shape for every use. Measured against the behaviour canon
//! this project reproduces, that is not what a badge is: the canon draws a
//! *read-out* (a value's declared type, where the value came from) as an
//! outlined chip, and a *state* (`HOT`, `RESTART`, `Drop`, `First`) as a chip
//! FILLED with that state's own low-emphasis tone. Counted: **three of its four
//! screens** draw one (the lab 4 sites, the capture viewer 7, the dashboard 1;
//! the shell 0 — it tints by message KIND, not by state), and **seven of its
//! cards in all**. The two forms carry different information — one says *here is
//! a fact about this row*, the other says *this row is in this state*.
//!
//! The standing direction for this framework is that the deliverable is an API
//! an application composes with rather than a screen: a status badge belongs to
//! any surface that reports state, and leaving it inside a settings form meant
//! the next screen that wanted one would copy it. [`BadgeTone`] is the
//! decision, [`view_badge`] is the paint, and the geometry is shared so the two
//! forms sit on one row without one of them being a different size.
//!
//! # What the filled form forced
//!
//! ★★★★★ A filled state badge needs a ground to fill and a foreground legible
//! on it, which is [`StateTone::container`] and [`StateTone::on_container`].
//! Before R2020 the vocabulary had that pair for `error` alone — so the same
//! badge could be painted for a wrong row and not for a caution, a right or an
//! informational one. This module is the consumer that forced the tier to
//! become uniform, which is why the pair is asked of the STATE here rather than
//! spelled per call: a caller cannot reach for one state's container and
//! another's foreground.
//!
//! # What is deliberately not here
//!
//! The canon's chips have a 2 px corner where these have 4, and no border where
//! the outlined form keeps one. The corner is not reproduced: it is a pixel-tier
//! difference on a mark whose census entry is about the mark existing, and
//! moving it would move rectangles under gates that measure boxes for a reason
//! unrelated to this round ⇒ the pixel tier is
//! [[debt-the-canon-census-counts-markup-and-not-pixels]]. The BORDER is
//! reproduced — a filled badge has none — because the fill is what separates it
//! from the row and a second separation would read as a heavier mark than the
//! canon's.

use pinion_core::Scene;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, TextOverflow,
    TextStyle,
};
use pinion_core::theme::{ColorRole, StateTone, Theme};
use pinion_core::voice::Silence;

/// A badge's horizontal padding, one side.
///
/// Named because a caller laying a badge out beside flexible content has to add
/// it back into that content's width budget (R1656 measured what happens when
/// it does not: the badge is painted past the row's own right edge, with the
/// row's box right, the badge's box right, and only their sum wrong).
pub const BADGE_PAD: u32 = 6;

/// A badge's vertical padding, one side.
pub const BADGE_PAD_Y: u32 = 2;

/// The type size a badge's word is set at — small, because a badge is read
/// beside something rather than instead of it.
pub const BADGE_TEXT_PX: u32 = 9;

/// **What a badge is saying**, which is what decides how it is painted.
///
/// Two arms and not a colour parameter: *outlined read-out* and *filled state*
/// are the two things the canon draws, and a caller handed a free colour can
/// build a third that means nothing — or, worse, pair one state's ground with
/// another's ink. The arms carry roles rather than [`Color`]s for the same
/// reason: a badge painted from a value cannot follow a theme change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    /// A **read-out**: a fact about the thing beside it that is not a state —
    /// a value's declared type, where a value came from, what a row is instead
    /// of configuration. Drawn on the raised surface tier and outlined in
    /// `ink`, which is also the word's colour.
    Neutral {
        /// The word's colour, and the outline's.
        ink: ColorRole,
    },
    /// A **state** the thing beside it is in. Drawn on that state's container
    /// with the foreground that container carries, and NOT outlined.
    State(StateTone),
}

impl BadgeTone {
    /// The ground this badge is painted on.
    #[must_use]
    pub const fn ground(self) -> ColorRole {
        match self {
            Self::Neutral { .. } => ColorRole::SurfaceContainerHigh,
            Self::State(tone) => tone.container(),
        }
    }

    /// The colour of the word.
    #[must_use]
    pub const fn ink(self) -> ColorRole {
        match self {
            Self::Neutral { ink } => ink,
            Self::State(tone) => tone.on_container(),
        }
    }

    /// The outline, when there is one.
    ///
    /// ★ A filled badge has none, and that is the canon's shape rather than a
    /// saving: the tint already separates the chip from the row, so an outline
    /// on top of it draws a second boundary around one mark.
    #[must_use]
    pub const fn outline(self) -> Option<ColorRole> {
        match self {
            Self::Neutral { ink } => Some(ink),
            Self::State(_) => None,
        }
    }
}

/// The base style a badge's word is set in.
///
/// `EllipsisMiddle`, the same policy `config_form::form_run_style` carries: a
/// badge's word is placed in an exact rectangle, and a word that outgrows it
/// overhangs the row rather than being cut.
///
/// ★★★★★ R2020 — **this is spelled twice in this crate and the second spelling
/// is safe, which is a conclusion the round reached the wrong way round first.**
///
/// The closing audit noticed the duplication — this module copied the form's
/// style, so a policy that had one spelling now has two, which is the class the
/// rest of this round is about — and argued from R1656's comment (*the badges
/// refuse to shrink, the key is allowed to shrink, and the shaper then elides
/// IT*) that on a badge the policy could never fire, so the repair was to drop
/// it. **Driven, that is false.** Removing it took three of the node lab's
/// gates red at once: `r1654` reported **18 runs with an exact box and no
/// policy for outgrowing it** — `restart`, `hot`, `bool`, `address[]`, `from
/// the role` among them — and `r1653` reported those same 18 marks painted
/// OUTSIDE their boxes, overhanging by 4 to 57 px.
///
/// R1656 is about the FLEX deficit, which the key absorbs; this is about the
/// run's own rectangle, which the badge is given exactly. Two different boxes,
/// and reading one comment answered for the other.
///
/// ⇒ the duplication stands, and what makes it safe is not that it is one line:
/// **`r1654` is a gate over every painted run in the screen**, so either
/// spelling losing the policy is red. A rule with a gate does not need a single
/// spelling — which is the actual distinction between this and the hand-written
/// role lists this round derived away, none of which had one.
fn badge_run_style() -> TextStyle {
    TextStyle::new().with_overflow(TextOverflow::EllipsisMiddle)
}

/// Paint a badge.
///
/// `tag` addresses the chip and declares its [`Silence`]. ★★ R1691 — a tagged
/// badge declares its own silence because its words are usually already
/// announced whole as the name of the description region its row publishes, so
/// a node here would read them out twice on every focus move. That duplicate is
/// what the reference toolkit produces by construction for every label bound to
/// a field, and it offers nothing to suppress it.
///
/// ★ R1655 — the chip is pointer-transparent. A tagged node that is an ADDRESS
/// rather than a primitive becomes the §5.35 router's hit target: the router
/// looks the tag up as an `External`, finds none, and forwards NOTHING. Wherever
/// such a badge was painted the surface under it was dead to a real mouse while
/// every wire-driven assertion about it passed.
///
/// ★ R1656 — it refuses to shrink. A badge is a read-out; what gives way beside
/// it is the flexible content, which elides. R1536 measured a 10 px mark painted
/// at 6 px when a decoration was allowed to absorb a layout deficit.
#[must_use]
pub fn view_badge(
    text: &str,
    tone: BadgeTone,
    theme: &Theme,
    tag: Option<(String, Silence)>,
) -> Scene {
    let ink: Color = theme.resolve(tone.ink());
    let label = Scene::Text(TextNode::styled(
        text.to_owned(),
        Rect::default(),
        badge_run_style().with_size_px(BADGE_TEXT_PX).with_fg(ink),
    ));
    let mut box_style = BoxStyle::filled(theme.resolve(tone.ground())).with_corner_radius(4);
    if let Some(outline) = tone.outline() {
        box_style = box_style.with_border(Border::new(theme.resolve(outline), 1));
    }
    let mut node = ContainerNode::new(vec![label])
        .with_style(box_style)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_padding(Rect::new(BADGE_PAD, BADGE_PAD_Y, BADGE_PAD, BADGE_PAD_Y))
                .with_flex_shrink(0.0)
                .with_pointer_transparent(true),
        );
    if let Some((tag, silence)) = tag {
        node = node.with_tag(tag);
        node.layout.silence = Some(silence);
    }
    Scene::Container(node)
}

#[cfg(test)]
mod tests {
    use super::{BadgeTone, view_badge};
    use pinion_core::Scene;
    use pinion_core::style::Color;
    use pinion_core::theme::{ColorRole, StateTone, Theme};

    /// The ground, the word's colour, and the outline if there is one — which
    /// is the whole of what a badge shows.
    fn painted(scene: &Scene) -> (Color, Color, Option<Color>) {
        let Scene::Container(node) = scene else {
            panic!("a badge is a container");
        };
        let Some(Scene::Text(label)) = node.children.first() else {
            panic!("a badge paints its word");
        };
        (
            node.style.fill,
            label.style.fg_color,
            node.style.border.map(|border| border.color),
        )
    }

    /// ★★★★★ R2020 — **a state badge is painted from ONE state's pair.**
    ///
    /// The defect this module exists to make unrepresentable is a chip whose
    /// ground comes from one state and whose word comes from another — which is
    /// what a `(fill, ink)` pair of free colours invites, and what every caller
    /// of the private helper this replaced had to get right by hand.
    ///
    /// It is asserted over every state rather than for one, because the tier
    /// this round made uniform is the claim: a state added to the vocabulary
    /// and given a container pair passes this, and one given three of its four
    /// roles cannot compile.
    #[test]
    fn r2020_a_state_badge_wears_that_states_container_pair() {
        let mut judged = 0_usize;
        for (word, theme) in [("light", Theme::light()), ("dark", Theme::dark())] {
            for tone in StateTone::ALL {
                let scene = view_badge("READY", BadgeTone::State(tone), &theme, None);
                let (fill, ink, border) = painted(&scene);
                assert_eq!(
                    fill,
                    theme.resolve(tone.container()),
                    "{word}: a `{}` badge is filled with something other than its \
                     own container",
                    tone.word()
                );
                assert_eq!(
                    ink,
                    theme.resolve(tone.on_container()),
                    "{word}: a `{}` badge's word is not the ink its ground carries",
                    tone.word()
                );
                assert_eq!(
                    border,
                    None,
                    "{word}: a filled `{}` badge draws a second boundary around \
                     itself where the canon draws none",
                    tone.word()
                );
                judged += 1;
            }
        }
        assert_eq!(
            judged,
            2 * StateTone::ARMS,
            "every state in both palettes is the population this is about"
        );
        println!("[r2020] {judged} state badge(s) painted from their own pair");
    }

    /// A read-out keeps the outlined form, because the two are different
    /// claims and a screen that painted them alike would be saying one thing.
    #[test]
    fn r2020_a_neutral_badge_is_outlined_and_not_filled_with_a_state() {
        let theme = Theme::light();
        let scene = view_badge(
            "locator[]",
            BadgeTone::Neutral {
                ink: ColorRole::OnSurfaceMuted,
            },
            &theme,
            None,
        );
        let (fill, ink, border) = painted(&scene);
        assert_eq!(fill, theme.resolve(ColorRole::SurfaceContainerHigh));
        assert_eq!(ink, theme.resolve(ColorRole::OnSurfaceMuted));
        assert_eq!(
            border,
            Some(theme.resolve(ColorRole::OnSurfaceMuted)),
            "a read-out is separated from the row by its outline, having no fill \
             of its own to do it"
        );
        for tone in StateTone::ALL {
            assert_ne!(
                fill,
                theme.resolve(tone.container()),
                "a read-out that happens to land on `{}`'s ground would report a \
                 state nobody claimed",
                tone.word()
            );
        }
    }

    /// ★★ R1674 — a badge's word stays inside the box the badge draws. The
    /// crate gate ([`crate::frame_gate`]).
    ///
    /// Both forms, because they are not the same shape: only the neutral one
    /// strokes an outline, so a gate that ran the filled arm alone would never
    /// see a border — and the border is the two pixels a word can be pushed
    /// past. Every state as well, because their words differ in length and the
    /// box is content-sized.
    #[test]
    fn r1674_a_badge_keeps_its_word_inside_its_own_box() {
        let theme = Theme::light();
        let mut forms: Vec<(String, BadgeTone)> = vec![(
            "neutral".to_owned(),
            BadgeTone::Neutral {
                ink: ColorRole::OnSurfaceMuted,
            },
        )];
        for tone in StateTone::ALL {
            forms.push((tone.word().to_owned(), BadgeTone::State(tone)));
        }
        for (word, tone) in forms {
            crate::frame_gate::assert_frame_contained(&format!("badge {word}"), &mut |_w, _h| {
                view_badge("out of range", tone, &theme, None)
            });
        }
    }
}
