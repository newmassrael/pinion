//! ★★★★★ R1762 §5.38 §5.40 §2 #7 — **a control that is collapsed until you
//! open it, for any surface, not only a form row.**
//!
//! # What forced this module
//!
//! R1732 built the collapsed chooser — a box holding the word in effect, a
//! chevron, and a roster that appears over everything only while it is open —
//! and built it *inside* `config_form`, because a configuration form was the
//! one thing that needed it. Its geometry took a `RowBox`, its paint took a
//! `ConfigField`, and its roster's tags were composed from a configuration
//! path. None of that is about choosing.
//!
//! The second consumer arrived at R1762 and it is not a form: the analysis
//! tool's preferences page, whose behaviour reference draws two rows —
//! *Interface* and *Ring buffer size* — as a value and a chevron. Measured
//! before this module existed, reproducing them had exactly three routes and
//! two of them are ones this project has ruled out:
//!
//! * hand-roll a box with a word and an arrow in the screen, which is the class
//!   R1673 measured on a sibling screen (a switch drawn as a track with no knob)
//!   and which this shell already has one instance of in its own preset menu;
//! * paint a whole `config_form` to get one row of it, which puts a form's
//!   layout policy, its defect skins and its offered-key chips on a page that
//!   has none of those things;
//! * or lift the control out, which is this file.
//!
//! # What a caller brings, and what it does not
//!
//! A caller brings the rectangle, the word in effect, the [`Picker`] while one
//! is open, and the room the roster must stay inside. It does **not** bring a
//! layout policy or a document: the roster's own geometry is a function of the
//! control and the room, which is what makes the same control legible on a form
//! row and on a settings row without either of them knowing about the other.
//!
//! ⚠ The **room** is the caller's for the reason R1732 recorded and this module
//! keeps: a form is laid into a pane it cannot see the bottom of, so a roster
//! that decided its own direction against its own extent would open downward
//! off the end of a scrolled viewport. The rule is the same here, and a
//! settings page has the same problem with the page region it is laid in.

use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{Border, BoxStyle, TextStyle};
use pinion_core::voice::Silence;
use pinion_core::widgets::picker::Picker;
use pinion_core::{
    AlignItems, ColorRole, FlexDirection, JustifyContent, LayoutStyle, Scene, Size, Theme,
};

use crate::config_form::{form_run_style, framed, placed};

/// ★★★★★ R2119 §5.2 §5.11 — **where this control's painted addresses come
/// from**, parametric in the prefix its caller painted it under.
///
/// The twenty-second instalment of an address debt, and the second time the
/// owner of a family turned out to be the FRAMEWORK rather than the screen —
/// R2050 found the same one row over, in `config_form`. A roster's box and its
/// options are composed *here*, from a prefix the caller brings, so a consumer
/// that spells `"<its own prefix>.roster.<key>"` is the second speller of a
/// string this module writes, and a wrong letter there is silent in the worst
/// direction: the press lands on nothing and the screen simply does not choose.
///
/// # ★★★★★ The inverse is the point, not a convenience
///
/// An option's address is `option.<key>.<word>` and **the word may carry the
/// separator** — measured on the analysis tool's own capture-source roster,
/// whose words are `lo · 127.0.0.1:7447` and `file · session-2.capture`. So the
/// tail is split at the FIRST separator and only the first, and a reader that
/// takes the last segment answers `1:7447`. That is not a hypothetical: it is
/// what this module did until R2119, and it painted a roster whose second row
/// read `1:7447` while the value behind it was the whole address.
///
/// ⇒ the recovery belongs beside the composition, where it is one edit away
/// from the rule it inverts, rather than at each reader as a `rsplit`.
pub mod address {
    /// The part word an open roster's own box is addressed under.
    pub const ROSTER: &str = "roster";

    /// The part word each option in an open roster is addressed under.
    pub const OPTION: &str = "option";

    /// The address of the roster `key` opens, under the prefix it is painted
    /// with.
    #[must_use]
    pub fn roster(prefix: &str, key: &str) -> String {
        format!("{prefix}.{}", roster_suffix(key))
    }

    /// [`roster`] without the prefix — what a [`super::RosterBox`] is addressed
    /// by inside the surface that holds it.
    #[must_use]
    pub fn roster_suffix(key: &str) -> String {
        format!("{ROSTER}.{key}")
    }

    /// The address of the option `word` of the roster `key`.
    #[must_use]
    pub fn option(prefix: &str, key: &str, word: &str) -> String {
        format!("{prefix}.{}", option_suffix(key, word))
    }

    /// [`option`] without the prefix — the form [`super::RosterBox::options`]
    /// carries, because a laid-out roster does not know what it will be painted
    /// under.
    #[must_use]
    pub fn option_suffix(key: &str, word: &str) -> String {
        format!("{OPTION}.{key}.{word}")
    }

    /// The prefix every option of the roster `key` is addressed under.
    ///
    /// ★ A composer's *stem*, published for the one reader that legitimately
    /// needs it: a driver asking the paint for the whole family at once. What a
    /// caller naming ONE option gets is [`option`], never this — R2109.1's rule.
    #[must_use]
    pub fn option_prefix(prefix: &str, key: &str) -> String {
        format!("{prefix}.{OPTION}.{key}.")
    }

    /// The word an option's address names, given the roster it belongs to.
    ///
    /// ★★★★★ [`option`]'s inverse. The whole remainder is the word — see the
    /// module note: a word carrying the separator is the ordinary case, not the
    /// edge one, so this may not split again.
    #[must_use]
    pub fn option_word<'a>(prefix: &str, key: &str, tag: &'a str) -> Option<&'a str> {
        tag.strip_prefix(&option_prefix(prefix, key))
    }

    /// [`option_word`] against a suffix, for a reader inside the surface.
    #[must_use]
    pub fn option_word_of_suffix<'a>(key: &str, suffix: &'a str) -> Option<&'a str> {
        suffix.strip_prefix(&format!("{OPTION}.{key}."))
    }
}

/// The outline an open roster draws inside its own box, and so the inset its
/// options are laid within.
pub const ROSTER_FRAME: u32 = 1;

/// The gap between a collapsed control and the roster it opens.
pub const ROSTER_GAP: u32 = 2;

/// The width of the chevron seat at a collapsed control's trailing edge.
pub const CHEVRON_W: u32 = 22;

/// ★★★★★ R1732 — **where an open roster landed**, as its own layer.
///
/// A value a caller holds apart from whatever it laid the rest of its surface
/// into, because a roster is *over* the things below it and a hit test that
/// walked those in order would resolve a press on the roster to whatever it
/// happens to cover. Publishing it apart makes the layering a declared fact
/// instead of a consequence of iteration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterBox {
    /// What the roster belongs to — a configuration path, a settings key, or
    /// whatever else the caller addresses its control by.
    pub key: String,
    /// The whole roster's box, which is what the paint fills and outlines.
    pub rect: Rect,
    /// Each option, as the tag suffix it is pressed by and where it landed.
    pub options: Vec<(String, Rect)>,
    /// Whether the roster opened **upward** because there was no room below.
    ///
    /// Derived from the room, published because a test that has to re-derive it
    /// is a test that can agree with a wrong answer.
    pub above: bool,
}

/// ★★★★★ R1732 — lay an open roster under (or over) the control it belongs to.
///
/// Downward unless the whole roster would leave `room`, and upward when it
/// would — never half of each, because a roster that clipped would show a
/// reader fewer options than it has and say nothing.
#[must_use]
pub fn lay_roster(
    key: &str,
    control: Rect,
    picker: &Picker,
    room: Rect,
    option_h: u32,
) -> RosterBox {
    let n = u32::try_from(picker.len()).unwrap_or(u32::MAX);
    let height = n * option_h + ROSTER_FRAME * 2;
    let below = control.y + control.h + ROSTER_GAP;
    let room_bottom = room.y + room.h;
    let above = below + height > room_bottom && control.y >= height + ROSTER_GAP;
    let top = if above {
        control.y - height - ROSTER_GAP
    } else {
        below
    };
    let rect = Rect::new(control.x, top, control.w, height);
    let options = picker
        .options()
        .iter()
        .enumerate()
        .map(|(n, word)| {
            let n = u32::try_from(n).unwrap_or(u32::MAX);
            (
                address::option_suffix(key, word),
                Rect::new(
                    rect.x + ROSTER_FRAME,
                    rect.y + ROSTER_FRAME + n * option_h,
                    rect.w.saturating_sub(ROSTER_FRAME * 2),
                    option_h,
                ),
            )
        })
        .collect();
    RosterBox {
        key: key.to_owned(),
        rect,
        options,
        above,
    }
}

/// What a collapsed chooser's three parts are addressed by.
///
/// Given rather than composed here, because the two consumers address theirs
/// differently — a form by configuration path, a page by row key — and a rule
/// for composing them inside this module would be a third vocabulary neither
/// caller uses.
#[derive(Debug, Clone)]
pub struct ChooserTags {
    /// The control itself: the box a press lands on and the node a reader is
    /// told about.
    pub control: String,
    /// The word in effect, addressed so a specification can name it and a
    /// driver can read it.
    pub shown: String,
    /// The chevron seat.
    pub arrow: String,
}

/// ★★★★★ R1732 — a choice **collapsed**: the word it holds, and the chevron
/// that opens the rest.
///
/// `skin` is the caller's, because what a chooser looks like when the value
/// behind it is in trouble is a fact about the caller's domain: a form paints a
/// defect outline there, and a page with no defects paints its ordinary field
/// skin.
#[must_use]
pub fn view_collapsed(
    tags: &ChooserTags,
    word: &str,
    control: Rect,
    origin: (u32, u32),
    skin: BoxStyle,
    theme: &Theme,
) -> Scene {
    let shown = Rect::new(10, 0, control.w.saturating_sub(CHEVRON_W + 16), control.h);
    let seat = Rect::new(
        control.x + control.w.saturating_sub(CHEVRON_W),
        control.y,
        CHEVRON_W,
        control.h,
    );
    Scene::Container(
        ContainerNode::new(vec![
            // ★★★★ R1732 — the word the control holds, ADDRESSED. A run nothing
            // can name is a run a conformance check reads back as a missing
            // part and a driver cannot ask about. Its words are the control's
            // announced value, so it is folded into it rather than said twice.
            Scene::Text(
                TextNode::styled(
                    word.to_owned(),
                    shown,
                    run_style()
                        .with_size_px(12)
                        .with_fg(theme.resolve(ColorRole::OnSurface)),
                )
                .with_tag(tags.shown.clone())
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(shown.x, shown.y)
                        .with_size(Size::px(shown.w, shown.h))
                        .with_pointer_transparent(true)
                        .with_silence(Silence::part_of(tags.control.clone())),
                ),
            ),
            chevron(
                tags.arrow.clone(),
                tags.control.clone(),
                seat,
                (control.x, control.y),
                theme,
            ),
        ])
        .with_tag(tags.control.clone())
        .with_style(skin)
        .with_layout(placed(
            framed(LayoutStyle::new().with_focusable(true)),
            control,
            origin,
        )),
    )
}

/// The arrow at a collapsed control's trailing edge, and its **declared
/// silence**.
///
/// No pill skin, unlike a button: the reference draws the arrow inside the
/// field's own box rather than as a control beside it, and a bordered pill here
/// would read as a second control on a row that has one.
///
/// The silence is the load-bearing half. This rectangle is published, painted
/// and pressable, so the voice census asks about it — and the honest answer is
/// that its content is already in the chooser's announcement, which carries the
/// same open/closed state the arrow draws.
fn chevron(
    tag: String,
    control_tag: String,
    seat: Rect,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    // `placed` makes it pointer-transparent, which is what keeps a press
    // reaching the consumer's hit test over the published rectangle rather than
    // dying on a tag that has no `External`.
    //
    // ★★★★★ R1952 — the chevron is DRAWN. It was `U+25BE` in a 10px run until
    // the face this tree ships was asked whether it has that glyph, with
    // `Font::glyph_id_for`; it does not, and neither does any other face this
    // tree owns. A person opening the analysis shell's settings page or the
    // node lab's routing picker saw a `.notdef` box where the affordance
    // belongs — on a control whose whole job is to say *there is a list under
    // here*.
    Scene::Container(
        ContainerNode::new(vec![crate::indicator::inline(
            crate::indicator::Indicator::Selector,
            seat.h.min(seat.w),
            theme.resolve(ColorRole::OnSurfaceMuted),
            "the chevron that opens the roster; the chooser announces its state",
        )])
        .with_tag(tag)
        .with_layout(placed(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_silence(Silence::part_of(control_tag)),
            seat,
            origin,
        )),
    )
}

/// ★★★★★ R1732 — **an open roster, over what it covers.**
///
/// Painted last by its caller so it is on top, and from the [`RosterBox`] so
/// the rectangles a press is resolved against and the rectangles a reader sees
/// are the same value. The highlight comes from the [`Picker`] and the mark
/// from `chosen`: those are two different facts — where the reader is, and what
/// the surface holds — and a roster that drew one of them twice could not show
/// a reader moving away from the value.
#[must_use]
pub fn view_roster(
    tag_prefix: &str,
    roster: &RosterBox,
    picker: &Picker,
    chosen: &str,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    let rows: Vec<Scene> = roster
        .options
        .iter()
        .enumerate()
        .map(|(n, (suffix, rect))| {
            // ★★★★★ R2119 — the address's own inverse, not a `rsplit` here.
            //
            // A word may carry the separator — the analysis tool's capture
            // sources are `lo · 127.0.0.1:7447` and `file ·
            // session-2.capture` — so taking the LAST segment answers
            // `1:7447`, which is what this roster painted until this round
            // while the value behind it was whole. The tail after
            // `option.<key>.` is the word entire, and only the module that
            // composed it can say where that boundary is.
            //
            // ⚠ The fallback cannot be reached by any caller in this
            // workspace — a `RosterBox`'s suffixes are composed by
            // `lay_roster` from the very key carried beside them — so it is
            // asserted rather than merely tolerated: a hand-built roster whose
            // key and suffixes disagree is a defect in the caller, caught
            // wherever tests run, and drawing the address is the least
            // damaging thing to do in a painter that must not abort a frame.
            debug_assert!(
                address::option_word_of_suffix(&roster.key, suffix).is_some(),
                "the roster keyed {:?} carries the option suffix {suffix:?}, \
                 which was not composed from that key",
                roster.key,
            );
            let word = address::option_word_of_suffix(&roster.key, suffix).unwrap_or(suffix);
            let here = n == picker.at();
            let ink = if word == chosen {
                theme.resolve(ColorRole::Accent)
            } else {
                theme.resolve(ColorRole::OnSurface)
            };
            let mut node = ContainerNode::new(vec![Scene::Text(TextNode::styled(
                word.to_owned(),
                Rect::new(10, 0, rect.w.saturating_sub(20), rect.h),
                run_style().with_size_px(12).with_fg(ink),
            ))])
            .with_tag(format!("{tag_prefix}.{suffix}"))
            .with_layout(placed(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center),
                *rect,
                (roster.rect.x, roster.rect.y),
            ));
            if here {
                node = node.with_style(
                    BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh))
                        .with_corner_radius(6),
                );
            }
            Scene::Container(node)
        })
        .collect();
    Scene::Container(
        ContainerNode::new(rows)
            .with_tag(address::roster(tag_prefix, &roster.key))
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::Surface))
                    .with_corner_radius(8)
                    .with_border(Border::new(theme.resolve(ColorRole::Outline), ROSTER_FRAME)),
            )
            .with_layout(placed(framed(LayoutStyle::new()), roster.rect, origin)),
    )
}

/// The run style every part of a chooser draws with.
fn run_style() -> TextStyle {
    form_run_style()
}

#[cfg(test)]
mod tests {
    use super::{ChooserTags, address, lay_roster, view_collapsed, view_roster};
    use pinion_core::scene::Rect;
    use pinion_core::style::BoxStyle;
    use pinion_core::widgets::picker::Picker;
    use pinion_core::{ColorRole, Theme};

    fn theme() -> Theme {
        Theme::light()
    }

    fn picker() -> Picker {
        Picker::over(["256 MB", "512 MB", "1 GB"], "512 MB").expect("three words is a roster")
    }

    fn tags() -> ChooserTags {
        ChooserTags {
            control: "probe.control".to_owned(),
            shown: "probe.shown".to_owned(),
            arrow: "probe.arrow".to_owned(),
        }
    }

    /// ★★ R1674 — the collapsed control keeps its word and its arrow inside the
    /// box it strokes. The crate gate (`crate::frame_gate`), which asked the day
    /// this module gained a border of its own.
    #[test]
    fn r1762_a_collapsed_chooser_keeps_its_parts_inside_its_own_frame() {
        let marks = crate::frame_gate::assert_frame_contained("chooser collapsed", &mut |w, _h| {
            view_collapsed(
                &tags(),
                "512 MB",
                Rect::new(0, 0, w.min(208), 32),
                (0, 0),
                BoxStyle::filled(theme().resolve(ColorRole::Surface)).with_corner_radius(8),
                &theme(),
            )
        });
        assert!(marks > 0, "the gate examined {marks} mark(s)");
    }

    /// ★★★★★ R1762 — the word's own BOX stays inside the control's, which the
    /// ink gate above cannot say.
    ///
    /// Found by a counterfactual that passed: widening the word's rectangle past
    /// the control changed nothing, because `assert_frame_contained` measures
    /// **ink** leaving a box and a short word in a wider box still fits. That is
    /// the gate working correctly on a different question — so the question it
    /// does not ask is asked here, where a value long enough to fill its box
    /// would then be laid over the chevron rather than elided before it.
    #[test]
    fn r1762_the_word_a_chooser_shows_stays_clear_of_its_chevron() {
        let control = Rect::new(0, 0, 208, 32);
        let mut scene = view_collapsed(
            &tags(),
            "a capture source whose name is far too long for this box",
            control,
            (0, 0),
            BoxStyle::filled(theme().resolve(ColorRole::Surface)),
            &theme(),
        );
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, 400, 200);
        let mut shown = None;
        let mut arrow = None;
        scene.for_each_node(&mut |visit| {
            let Some(tag) = visit.node.tag() else { return };
            let Some(rect) = visit.absolute_rect() else {
                return;
            };
            if tag == "probe.shown" {
                shown = Some(rect);
            } else if tag == "probe.arrow" {
                arrow = Some(rect);
            }
        });
        let shown = shown.expect("the control draws the word it holds");
        let arrow = arrow.expect("the control draws the chevron that opens it");
        assert!(
            shown.x + shown.w <= arrow.x,
            "the word ends before the chevron begins: {shown:?} then {arrow:?}",
        );
        assert!(
            arrow.x + arrow.w <= control.x + control.w,
            "and the chevron ends inside the control: {arrow:?} in {control:?}",
        );
    }

    /// ★★ R1674 — and so does the open roster, at the sizes a popup is asked
    /// at: it is anchored rather than bound, so the question is whether its own
    /// options stay inside its own outline.
    #[test]
    fn r1762_an_open_roster_keeps_its_options_inside_its_own_frame() {
        let roster = lay_roster(
            "retention",
            Rect::new(0, 0, 208, 32),
            &picker(),
            Rect::new(0, 0, 400, 400),
            30,
        );
        // Two sizes, because one is the case R1656 measured: an assumption and
        // a defect are then the same number. A roster is anchored rather than
        // bound, so the sizes are the caller's — both are big enough that "the
        // popup is wider than the window" is not what is being asked.
        let marks = crate::frame_gate::assert_frame_contained_at(
            "chooser roster",
            &[(420, 260), (300, 200)],
            &mut |_w, _h| view_roster("probe", &roster, &picker(), "512 MB", (0, 0), &theme()),
        );
        assert!(marks > 0, "the gate examined {marks} mark(s)");
    }

    /// ★★★★★ R1762 — the roster opens DOWNWARD unless the whole of it would
    /// leave the room, and then upward — never half of each, because a roster
    /// that clipped would show a reader fewer options than it has and say
    /// nothing.
    #[test]
    fn r1762_a_roster_flips_rather_than_clipping() {
        let control = Rect::new(0, 300, 208, 32);
        let room = Rect::new(0, 0, 400, 400);
        let below = lay_roster("retention", Rect::new(0, 0, 208, 32), &picker(), room, 30);
        assert!(!below.above, "there is room below at the top of the room");
        assert!(below.rect.y > 32, "so it hangs under the control");

        let flipped = lay_roster("retention", control, &picker(), room, 30);
        assert!(
            flipped.above,
            "the whole roster does not fit under a control at the foot of the room"
        );
        assert!(
            flipped.rect.y + flipped.rect.h <= control.y,
            "so it sits entirely above it: {:?} vs {control:?}",
            flipped.rect
        );
        assert_eq!(
            flipped.options.len(),
            picker().len(),
            "and every option is in it either way",
        );
    }

    /// ★★★★★ R2119 — **an option WORD that carries the address separator is
    /// drawn whole**, and this is a repair rather than a precaution.
    ///
    /// Measured before the fix, on the analysis tool's own capture sources: the
    /// roster's second row read `1:7447` where the picker held
    /// `lo · 127.0.0.1:7447`, because the paint recovered the word by taking
    /// the LAST segment of the address it had just composed. The value behind
    /// the row was whole the entire time, so nothing that read the state could
    /// see it — only a reader looking at the screen.
    ///
    /// ⚠ The words here are dotted ON PURPOSE. A roster of `eth0`/`wlan0`
    /// passes with the defect in place, which is why it survived from R1732 to
    /// R2119 under two gates that both drew this control.
    #[test]
    fn r2119_a_dotted_option_word_is_drawn_whole() {
        let words = ["eth0", "lo \u{00B7} 127.0.0.1:7447"];
        let picker = Picker::over(words, "eth0").expect("two words is a roster");
        let roster = lay_roster(
            "interface",
            Rect::new(0, 0, 208, 32),
            &picker,
            Rect::new(0, 0, 400, 400),
            30,
        );
        let scene = view_roster("shell.settings", &roster, &picker, "eth0", (0, 0), &theme());
        let mut drawn: Vec<String> = Vec::new();
        collect_text(&scene, &mut drawn);
        assert_eq!(
            drawn,
            words.map(str::to_owned).to_vec(),
            "each option is drawn with the word the picker holds"
        );
    }

    /// ★★★★★ R2119 — the composition and its inverse are ONE pair, driven over
    /// words that carry the separator and keys that carry it too.
    ///
    /// A form addresses its rows by configuration PATH, so the key half is
    /// dotted on that consumer and the word half is dotted on this one. Either
    /// alone leaves a `rsplit`/`split_once` reader passing.
    #[test]
    fn r2119_a_roster_address_and_its_inverse_are_one_pair() {
        let prefix = "shell.settings";
        for key in ["interface", "net.timeout"] {
            for word in [
                "eth0",
                "lo \u{00B7} 127.0.0.1:7447",
                "file \u{00B7} a.capture",
            ] {
                let tag = address::option(prefix, key, word);
                assert_eq!(
                    address::option_word(prefix, key, &tag),
                    Some(word),
                    "{key}/{word} does not round-trip"
                );
                assert_eq!(
                    address::option_word_of_suffix(key, &address::option_suffix(key, word)),
                    Some(word),
                );
                assert!(
                    tag.starts_with(&address::option_prefix(prefix, key)),
                    "the option prefix is the stem its members hang off"
                );
            }
            assert_eq!(
                address::roster(prefix, key),
                format!("{prefix}.{}", address::roster_suffix(key))
            );
            // ★ A roster's own box is NOT one of its options, in either
            // direction: a reader that answered `Some` here would report the
            // box as a word nobody can choose.
            assert_eq!(
                address::option_word(prefix, key, &address::roster(prefix, key)),
                None
            );
        }
        // ★★ And a form's declaration is the same string, because it delegates.
        assert_eq!(
            crate::config_form::address::roster("form", "net.timeout"),
            address::roster("form", "net.timeout")
        );
    }

    fn collect_text(scene: &pinion_core::Scene, out: &mut Vec<String>) {
        match scene {
            pinion_core::Scene::Text(node) => out.push(node.content.clone()),
            pinion_core::Scene::Container(node) => {
                for child in &node.children {
                    collect_text(child, out);
                }
            }
            _ => {}
        }
    }
}
