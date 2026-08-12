//! R1554 §5.50 §5.39 §5.40 — the **group box**: a titled frame around a set of
//! controls, optionally gated by a checkbox in its own title. The toolkit
//! group box, HTML `<fieldset>` + `<legend>`, WAI-ARIA `role="group"`.
//!
//! # Why it did not exist before
//!
//! `grep -rn GroupBox` over 29 crates and 206 examples answered nothing, and the reason is not
//! that a frame with a label is hard to draw. It is that the half of group box
//! that matters — `setCheckable(true)`, where clearing the title checkbox makes the whole panel
//! inert — was **inexpressible**: the scene had no way to say "this subtree is
//! disabled", so a binding could only reach the look by threading a disabled
//! flag into every descendant widget's own state, and even then the Tab order,
//! the pointer router and the accessibility tree would each have needed their
//! own copy of the same bookkeeping.
//!
//! R1554's [`LayoutStyle::with_disabled`](pinion_core::style::LayoutStyle::with_disabled)
//! is that missing declaration, and this widget is its first consumer. One flag
//! on the content region and the cascade does the rest.
//!
//! # Geometry: the title sits ABOVE the frame, and that is a decision
//!
//! The toolkit draws the title *interrupting* the frame's top edge, the
//! label's background cutting a gap in the frame line. Reproducing that needs
//! the title to paint **after** the frame, and paint order in a §5.2 scene is
//! declaration order — which is also §5.39 **Tab order**. An overlapping title
//! therefore welds the two axes together, and they disagree: the gate must
//! come FIRST in the tab chain (it is what re-enables the rest) and LAST in
//! the paint chain.
//!
//! It was built the overlapping way first, and the binding's own test caught
//! it: Tab visited `opt_verbose`, `opt_trace`, then the gate. Rather than pick which axis to be
//! wrong on, the title moved above the frame — `AppKit`'s `NSBox` and GTK 4's `GtkFrame` label
//! convention — where nothing overlaps and both orders are the declaration
//! order. The *capability* the toolkit sets the floor for is a titled frame
//! that gates its contents; where the title sits is a shape choice, and this
//! shape has one fewer coupling in it.
//!
//! A [`flat`](GroupBoxStyle::flat) group box draws the top rule alone (the
//! toolkit's `PE_FrameGroupBox` reduced to its top line) — the convention for a group that
//! sections a form it does not need to box.

use pinion_core::Scene;
use pinion_core::availability::Unavailable;
use pinion_core::composite_tag::GroupBoxTag;
use pinion_core::mnemonic::MnemonicLabel;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size,
    SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::checkbox::CheckboxState;

use crate::checkbox::{CheckboxStyle, view_checkbox_box};

/// Where the title sits along the frame's top edge — the toolkit
/// `setAlignment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupBoxTitleAlign {
    /// Inset from the leading edge by [`GroupBoxStyle::title_indent`]. The toolkit's `AlignLeft` and its
    /// default.
    #[default]
    Start,
    /// Centred over the frame. The toolkit `AlignHCenter`.
    Center,
    /// Inset from the trailing edge. The toolkit `AlignRight`.
    End,
}

/// The gate a **checkable** group box carries in its title — the toolkit
/// `setCheckable(true)` plus `isChecked()`.
///
/// `None` where this appears as an `Option` is the toolkit's default non-checkable
/// group: a titled frame that gates nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupBoxCheck {
    /// Whether the gate is on. `false` is what makes the content region
    /// disabled — the whole point of the widget.
    pub checked: bool,
    /// The checkbox's own interaction posture, for the M3 state layer. The
    /// checkbox stays live while the contents are inert, so this is never
    /// derived from `checked`.
    pub interaction: CheckboxState,
}

/// Metrics + variant flags for [`view_group_box`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupBoxStyle {
    /// Title font size, px.
    pub title_font_size_px: u32,
    /// Inset of the title band from the frame's leading (or trailing) edge, px
    /// — the toolkit's title offset. Ignored for [`GroupBoxTitleAlign::Center`].
    pub title_indent: u32,
    /// Horizontal breathing room inside the title band, px: the gap it cuts in
    /// the frame line on each side of the text.
    pub title_padding_x: u32,
    /// Height of the title band, px. Half of it is the amount the frame is
    /// pushed down, so the frame line passes through the band's middle.
    pub title_height: u32,
    /// Gap between the checkbox and the title text, px.
    pub title_gap: u32,
    /// Frame corner radius, px.
    pub corner_radius: u32,
    /// Frame border width, px.
    pub border_width: u32,
    /// Padding inside the frame, around the content, px.
    pub content_padding: u32,
    /// Gap between the content's children, px.
    pub content_gap: u32,
    /// Draw the top rule alone instead of a frame — the toolkit `setFlat(true)`.
    pub flat: bool,
    /// Where the title sits.
    pub title_align: GroupBoxTitleAlign,
}

impl Default for GroupBoxStyle {
    /// M3-flavoured defaults close to the toolkit Fusion's group-box metrics.
    fn default() -> Self {
        Self {
            title_font_size_px: 14,
            title_indent: 12,
            title_padding_x: 6,
            title_height: 20,
            title_gap: 8,
            corner_radius: 6,
            border_width: 1,
            content_padding: 12,
            content_gap: 8,
            flat: false,
            title_align: GroupBoxTitleAlign::Start,
        }
    }
}

/// A titled frame around `content`, gated by `check` when it is `Some(..)`.
///
/// # Tags
///
/// Three, from the [`GroupBoxTag`] SSOT: `tag` (the frame — `role=group`),
/// `"{tag}_title"` (the title band, and the click target of a checkable
/// group's checkbox), `"{tag}_content"` (the region that carries the `disabled`
/// declaration).
///
/// # The gate
///
/// `Some(GroupBoxCheck { checked: false, .. })` puts
/// [`LayoutStyle::with_disabled`](pinion_core::style::LayoutStyle::with_disabled)
/// on the content region, and nothing else. Everything a user or an agent
/// observes from there — no Tab stop inside the region, a press landing on the
/// region rather than on the control under it, `aria-disabled` on every
/// descendant, faded ink, a `scene/disabled` row naming `"{tag}_content"` as
/// the cause — is the §5.39 cascade's, derived from that one flag.
///
/// The title band and the frame are deliberately **outside** the region: a
/// gate that disabled its own checkbox could not be turned back on. That is
/// the toolkit's behaviour too, and here it is a property of where the
/// declaration sits rather than a special case in the cascade.
///
/// # Mnemonic
///
/// `title` goes through [`TextNode::mnemonic_styled`], so `"&Advanced"` underlines the `A` and binds
/// <kbd>Alt</kbd>+`A` — the R1543 vocabulary, one declaration, and the binding
/// it produces targets the painted title band, which for a checkable group is
/// the checkbox. The toolkit's group box accepts the same `&` in its title and
/// does the same thing with it.
/// Why a checkable group's contents are inert, when they are.
///
/// [`None`] for a group with no checkbox, and for one whose box is ticked.
///
/// The detail names the CONDITION, which is what
/// [`UnavailableKind::Precondition`](pinion_core::availability::UnavailableKind::Precondition)
/// documents its detail to be — and it names it with the title as a reader sees
/// it, mnemonic marker resolved, so the sentence a listener hears matches the
/// word painted in the frame.
fn gate_reason(title: &str, check: Option<GroupBoxCheck>) -> Option<Unavailable> {
    matches!(check, Some(GroupBoxCheck { checked: false, .. })).then(|| {
        let shown = MnemonicLabel::parse(title).display;
        Unavailable::precondition(format!("{shown} is turned on"))
    })
}

#[must_use]
pub fn view_group_box(
    tag: &'static str,
    title: &str,
    check: Option<GroupBoxCheck>,
    theme: &Theme,
    style: &GroupBoxStyle,
    content: Vec<Scene>,
) -> Scene {
    let outline = theme.resolve(ColorRole::Outline);

    let content_region = Scene::Container(
        ContainerNode::new(content)
            .with_tag(GroupBoxTag::content(tag))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(style.content_gap)
                    .with_padding(Rect::new(
                        style.content_padding,
                        style.content_padding,
                        style.content_padding,
                        style.content_padding,
                    ))
                    .with_flex_grow(1.0)
                    // R1554 — THE declaration. One flag; the cascade derives
                    // the Tab order, the pointer refusal, `aria-disabled` and
                    // the ink from it.
                    //
                    // R1669 — and it says WHY, which is the whole reason this
                    // widget's own title carries a checkbox: the condition is
                    // in reach of the person reading the greyed panel, and it
                    // is one tick away. Stating it as a `Precondition` is what
                    // puts "turn <title> on" on `scene/disabled` and into the
                    // screen reader's announcement, where a bare flag left a
                    // listener with "dimmed" and no way to learn the remedy.
                    .with_availability(gate_reason(title, check)),
            ),
    );

    let framed = if style.flat {
        // The toolkit `setFlat(true)`: the top line only. A 1px rule above the content, in
        // a column so the content still flows beneath it.
        let rule = Scene::Box(
            BoxNode::new(Rect::default(), BoxStyle::filled(outline)).with_layout(
                LayoutStyle::new()
                    .with_size(Size::auto().with_height(SizeValue::Px(style.border_width.max(1))))
                    .with_flex_grow(0.0),
            ),
        );
        Scene::Container(
            ContainerNode::new(vec![rule, content_region]).with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_flex_grow(1.0),
            ),
        )
    } else {
        Scene::Container(
            ContainerNode::new(vec![content_region])
                .with_style(
                    BoxStyle::filled(Color::TRANSPARENT)
                        .with_border(Border::new(outline, style.border_width))
                        .with_corner_radius(style.corner_radius),
                )
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Column)
                        // ★★ R1673 — the frame's own pixels are RESERVED. The
                        // content region grew to the frame's full box, so it
                        // covered the outline on all four edges — the defect
                        // `containment` reports from the round a box began to
                        // own the border it strokes inside itself (R1672).
                        .with_padding(Rect::new(
                            style.border_width,
                            style.border_width,
                            style.border_width,
                            style.border_width,
                        ))
                        .with_flex_grow(1.0),
                ),
        )
    };

    // The title band is declared FIRST — see the module docs. It is the gate's
    // Tab stop and click target, and it must precede the members it governs.
    let title_band = title_band_node(tag, title, check, theme, style);

    Scene::Container(
        ContainerNode::new(vec![title_band, framed])
            .with_tag(tag)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(style.title_gap)
                    .with_flex_grow(1.0),
            ),
    )
}

/// How tall the title band has to be: **the tallest thing in it**.
///
/// ★★ R1673 — derived rather than picked, because three independent tokens
/// decided one height between them and nobody compared any pair of them:
/// `title_height`, `CheckboxStyle::box_size` for a checkable group, and the
/// line box of `title_font_size_px` for the legend that is always there.
///
/// Both of the last two were measured escaping. The checkbox was found by the
/// round's own re-measurement of a consumer; the LEGEND was found by the test
/// written to cover the checkbox, on its first run, in the arm that has no
/// checkbox at all — which is the argument for deriving from a list rather than
/// from the one member somebody happened to be looking at.
fn title_band_height(check: Option<GroupBoxCheck>, style: &GroupBoxStyle) -> u32 {
    let legend = pinion_core::containment::line_box(style.title_font_size_px);
    let control = match check {
        Some(_) => CheckboxStyle::default().box_size,
        None => 0,
    };
    style.title_height.max(legend).max(control)
}

/// The title band: a row holding the optional checkbox and the legend, above
/// the frame.
///
/// Declared before the frame so it precedes the members it gates in both
/// orders — see the module docs on why those two are the same order here.
fn title_band_node(
    tag: &'static str,
    title: &str,
    check: Option<GroupBoxCheck>,
    theme: &Theme,
    style: &GroupBoxStyle,
) -> Scene {
    // The legend is never dimmed by the gate — it labels the group, and the
    // group is live even when its contents are not. What DOES dim it is the
    // checkbox's own `Disabled` posture, which is a different fact (the whole
    // group is unavailable), so it is read from `interaction`.
    let title_disabled = matches!(
        check,
        Some(GroupBoxCheck {
            interaction: CheckboxState::Disabled,
            ..
        })
    );
    let title_color = if title_disabled {
        theme.resolve(ColorRole::OnSurfaceMuted)
    } else {
        theme.resolve(ColorRole::OnSurface)
    };
    let legend = Scene::Text(TextNode::mnemonic_styled(
        title,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.title_font_size_px)
            .with_fg(title_color),
    ));
    let mut children: Vec<Scene> = Vec::new();
    if let Some(check) = check {
        children.push(view_checkbox_box(
            check.checked,
            check.interaction,
            theme,
            &CheckboxStyle::default(),
        ));
    }
    children.push(legend);

    // Alignment is `justify-content` on a full-width band. With nothing
    // overlapping there is no gap to cut, so no arm needs its own fill or its
    // own positioning — the three alignments differ by one enum value.
    let (justify, lead, trail) = match style.title_align {
        GroupBoxTitleAlign::Start => (JustifyContent::Start, style.title_indent, 0),
        GroupBoxTitleAlign::Center => (JustifyContent::Center, 0, 0),
        GroupBoxTitleAlign::End => (JustifyContent::End, 0, style.title_indent),
    };

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(GroupBoxTag::title(tag))
            .with_layout({
                let mut l = LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(justify)
                    .with_gap(style.title_gap)
                    .with_padding(Rect::new(lead, 0, trail, 0))
                    // ★ R1673 — at least as tall as what it holds. The band is
                    // a token (20) and a checkable group puts a checkbox in it
                    // whose square is another token (24), so the checkbox stood
                    // two pixels above and below its own band. A title that
                    // carries a control is as tall as that control.
                    .with_size(
                        Size::auto().with_height(SizeValue::Px(title_band_height(check, style))),
                    )
                    .with_flex_grow(0.0);
                // A checkable group's title IS its checkbox: it is the Tab stop
                // and the click target. A plain group's title is a label, and
                // labels are not focus stops (the W3C decoration rule R1020's
                // enumeration already encodes).
                if check.is_some() {
                    l = l.with_focusable(true);
                }
                l
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::{GroupBoxCheck, GroupBoxStyle, GroupBoxTitleAlign, view_group_box};
    use crate::state_layer::{DISABLED, state_layer};
    use pinion_core::Scene;
    use pinion_core::composite_tag::GroupBoxTag;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect};
    use pinion_core::scene_disabled::{disabled_census, resolve_disabled};
    use pinion_core::style::{BoxStyle, Color, LayoutStyle};
    use pinion_core::theme::{ColorRole, Theme};
    use pinion_core::widgets::checkbox::CheckboxState;

    const TAG: &str = "advanced";
    const CONTROL: Color = Color::rgb(0x21, 0x96, 0xf3);

    fn content() -> Vec<Scene> {
        vec![Scene::Box(
            BoxNode::new(Rect::default(), BoxStyle::filled(CONTROL))
                .with_tag("threshold")
                .with_layout(LayoutStyle::new().with_focusable(true)),
        )]
    }

    /// The group inside a surface-filled root, which is what a binding paints —
    /// and what gives the cascade a backdrop to fade toward.
    fn app(check: Option<GroupBoxCheck>, theme: &Theme) -> Scene {
        let mut scene = Scene::Container(
            ContainerNode::new(vec![view_group_box(
                "advanced",
                "&Advanced",
                check,
                theme,
                &GroupBoxStyle::default(),
                content(),
            )])
            .with_tag("root")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface))),
        );
        resolve_disabled(&mut scene);
        scene
    }

    fn find<'a>(scene: &'a Scene, tag: &str) -> &'a Scene {
        fn walk<'a>(s: &'a Scene, tag: &str) -> Option<&'a Scene> {
            if s.tag() == Some(tag) {
                return Some(s);
            }
            match s {
                Scene::Container(c) => c.children.iter().find_map(|c| walk(c, tag)),
                Scene::Scroll(s) => walk(&s.content, tag),
                _ => None,
            }
        }
        walk(scene, tag).unwrap_or_else(|| panic!("no node tagged {tag}"))
    }

    fn fill_of(scene: &Scene) -> Color {
        match scene {
            Scene::Box(b) => b.style.fill,
            Scene::Container(c) => c.style.fill,
            other => panic!("no fill: {other:?}"),
        }
    }

    #[test]
    fn a_plain_group_gates_nothing() {
        let scene = app(None, &Theme::default());
        assert!(
            disabled_census(&scene).is_empty(),
            "the toolkit's setCheckable(false) default: a titled frame and no gate",
        );
        assert_eq!(fill_of(find(&scene, "threshold")), CONTROL, "full ink");
    }

    #[test]
    fn a_checked_group_gates_nothing_either() {
        let scene = app(
            Some(GroupBoxCheck {
                checked: true,
                interaction: CheckboxState::Idle,
            }),
            &Theme::default(),
        );
        assert!(disabled_census(&scene).is_empty());
    }

    #[test]
    fn clearing_the_check_makes_the_content_region_the_declarer() {
        let scene = app(
            Some(GroupBoxCheck {
                checked: false,
                interaction: CheckboxState::Idle,
            }),
            &Theme::default(),
        );
        let census = disabled_census(&scene);
        let content_tag = GroupBoxTag::content(TAG);
        let region = census
            .iter()
            .find(|d| d.tag == content_tag)
            .expect("the region is published");
        assert!(region.self_declared);
        assert!(region.declared_by.is_none(), "nothing above it is disabled");
        let member = census
            .iter()
            .find(|d| d.tag == "threshold")
            .expect("its member is published");
        assert_eq!(
            member.declared_by.as_deref(),
            Some(content_tag.as_str()),
            "an agent that finds `threshold` unresponsive learns what to act on",
        );
    }

    /// R1669 — the gate says WHY, and the reason names the condition a reader
    /// can act on: this group's own checkbox, spelled as the frame paints it.
    ///
    /// This was the ONE production declaration in the tree that stated no
    /// reason (measured: every other `with_disabled` site is a test fixture or
    /// a doc mention), and it is the one where the remedy is a single tick.
    #[test]
    fn r1669_a_gated_group_says_which_condition_would_open_it() {
        use pinion_core::availability::{Recourse, UnavailableKind};

        let scene = app(
            Some(GroupBoxCheck {
                checked: false,
                interaction: CheckboxState::Idle,
            }),
            &Theme::default(),
        );
        let census = disabled_census(&scene);
        let content_tag = GroupBoxTag::content(TAG);
        for row in &census {
            assert_eq!(
                row.reason.kind(),
                UnavailableKind::Precondition,
                "{} is inert as {:?}",
                row.tag,
                row.reason.kind(),
            );
            assert_eq!(
                row.reason.recourse(),
                Recourse::Satisfy,
                "the remedy is one tick and the recourse has to say so",
            );
            assert_eq!(
                row.reason.detail(),
                "Advanced is turned on",
                "the condition, with the mnemonic marker resolved as the frame paints it",
            );
        }
        assert!(
            census.iter().any(|d| d.tag == content_tag),
            "the declarer is in the census",
        );
        assert!(
            census.len() > 1,
            "and so is its member, carrying the SAME reason without a walk",
        );
    }

    /// R1669 — a group whose box is ticked, and one with no box at all, declare
    /// nothing. Asserted in both directions because a reason that leaked into
    /// the live case would grey a panel nobody gated.
    #[test]
    fn r1669_an_open_group_declares_no_reason() {
        for check in [
            None,
            Some(GroupBoxCheck {
                checked: true,
                interaction: CheckboxState::Idle,
            }),
        ] {
            let scene = app(check, &Theme::default());
            assert!(
                disabled_census(&scene).is_empty(),
                "{check:?} gates nothing and must declare nothing",
            );
        }
    }

    #[test]
    fn the_gate_cannot_disable_its_own_checkbox() {
        // A gate that greyed the control which re-enables it would be a dead
        // end. Here that is a property of WHERE the declaration sits, not a
        // special case anywhere.
        let scene = app(
            Some(GroupBoxCheck {
                checked: false,
                interaction: CheckboxState::Idle,
            }),
            &Theme::default(),
        );
        let title = GroupBoxTag::title(TAG);
        assert!(
            !find(&scene, &title).is_disabled(),
            "the title band — which IS the checkbox — stays live",
        );
        assert_eq!(
            scene.collect_focusable_tags(),
            vec![title],
            "and it is the group's only Tab stop while the contents are inert",
        );
    }

    #[test]
    fn a_gated_control_lands_on_the_same_ink_as_a_self_disabled_one() {
        // The reason the M3 token moved down to `pinion-core` this round. A
        // control greyed by its group and a control greyed by its own state
        // enum must not be two shades of disabled.
        let theme = Theme::default();
        let scene = app(
            Some(GroupBoxCheck {
                checked: false,
                interaction: CheckboxState::Idle,
            }),
            &theme,
        );
        assert_eq!(
            fill_of(find(&scene, "threshold")),
            state_layer(CONTROL, CheckboxState::Disabled, &theme),
            "the cascade's fade IS `state_layer`'s disabled arm, because both \
             read one token and lerp toward the same surface",
        );
        assert!(
            (DISABLED - 0.38).abs() < f32::EPSILON,
            "and the token is M3's 38%",
        );
    }

    #[test]
    fn the_title_carries_the_mnemonic_it_was_declared_with() {
        let scene = app(None, &Theme::default());
        let title = find(&scene, &GroupBoxTag::title(TAG));
        let Scene::Container(band) = title else {
            panic!("title band is a container")
        };
        let legend = band
            .children
            .iter()
            .find_map(|c| match c {
                Scene::Text(t) => Some(t),
                _ => None,
            })
            .expect("a legend");
        assert_eq!(legend.content, "Advanced", "the marker is resolved away");
        assert_eq!(
            legend.mnemonic.as_ref().map(|m| m.key),
            Some('A'),
            "the toolkit accepts the same `&` in a group-box title",
        );
    }

    #[test]
    fn a_flat_group_draws_a_rule_and_no_frame() {
        fn count(s: &Scene, n: &mut u32) {
            if let Scene::Container(c) = s {
                if c.style.border.is_some() {
                    *n += 1;
                }
                for child in &c.children {
                    count(child, n);
                }
            }
        }
        let theme = Theme::default();
        let style = GroupBoxStyle {
            flat: true,
            ..GroupBoxStyle::default()
        };
        let scene = view_group_box("g", "Section", None, &theme, &style, content());
        let mut borders = 0_u32;
        count(&scene, &mut borders);
        assert_eq!(
            borders, 0,
            "the toolkit setFlat(true) draws the top line alone"
        );
    }

    #[test]
    fn the_title_precedes_the_members_it_gates_in_declaration_order() {
        // Declaration order is BOTH paint order and Tab order, and the gate has
        // to be first in the tab chain — it is the control that re-enables the
        // rest. Asserted on the structure, not only on the enumeration, because
        // the enumeration would still pass if a future arm made the members
        // unfocusable for some other reason.
        let scene = app(
            Some(GroupBoxCheck {
                checked: true,
                interaction: CheckboxState::Idle,
            }),
            &Theme::default(),
        );
        let Scene::Container(group) = find(&scene, TAG) else {
            panic!("container")
        };
        assert_eq!(
            group.children[0].tag().map(str::to_owned),
            Some(GroupBoxTag::title(TAG)),
            "the title band is the group's first child",
        );
        assert_eq!(
            scene.collect_focusable_tags(),
            vec![GroupBoxTag::title(TAG), "threshold".to_owned()],
            "so Tab reaches the gate before what it governs",
        );
    }

    #[test]
    fn the_three_alignments_differ_by_one_value_and_nothing_else() {
        // The overlapping-title design needed a per-arm fill and a per-arm
        // absolute offset to cut the frame's gap. With the title above the frame
        // there is no gap, so every arm is the same node with a different
        // `justify-content` — which is the simplification worth pinning.
        let theme = Theme::default();
        let band = |align: GroupBoxTitleAlign| {
            let style = GroupBoxStyle {
                title_align: align,
                ..GroupBoxStyle::default()
            };
            let scene = view_group_box("g", "Section", None, &theme, &style, content());
            match find(&scene, &GroupBoxTag::title("g")) {
                Scene::Container(c) => (
                    c.layout.justify_content,
                    c.style.fill,
                    c.layout.absolute_position,
                ),
                other => panic!("not a container: {other:?}"),
            }
        };
        let (js, fs, ps) = band(GroupBoxTitleAlign::Start);
        let (jc, fc, pc) = band(GroupBoxTitleAlign::Center);
        let (je, fe, pe) = band(GroupBoxTitleAlign::End);
        assert_ne!(js, jc);
        assert_ne!(jc, je);
        for fill in [fs, fc, fe] {
            assert_eq!(fill, Color::TRANSPARENT, "no arm paints a surface patch");
        }
        for pos in [ps, pc, pe] {
            assert!(pos.is_none(), "no arm leaves the flow");
        }
    }

    #[test]
    fn an_unavailable_group_mutes_its_own_legend() {
        // Two different facts: the CONTENTS are gated off (`checked`), versus
        // the whole group is unavailable (the checkbox's own Disabled posture).
        // The legend follows the second, never the first.
        let theme = Theme::default();
        let gated = app(
            Some(GroupBoxCheck {
                checked: false,
                interaction: CheckboxState::Idle,
            }),
            &theme,
        );
        let unavailable = app(
            Some(GroupBoxCheck {
                checked: true,
                interaction: CheckboxState::Disabled,
            }),
            &theme,
        );
        let legend = |s: &Scene| -> Color {
            let Scene::Container(band) = find(s, &GroupBoxTag::title(TAG)) else {
                panic!("container")
            };
            band.children
                .iter()
                .find_map(|c| match c {
                    Scene::Text(t) => Some(t.style.fg_color),
                    _ => None,
                })
                .expect("legend")
        };
        assert_eq!(legend(&gated), theme.resolve(ColorRole::OnSurface));
        assert_eq!(
            legend(&unavailable),
            theme.resolve(ColorRole::OnSurfaceMuted),
        );
    }

    #[test]
    fn a_gated_region_absorbs_a_press_aimed_at_the_control_inside_it() {
        use pinion_core::style::LayoutStyle as L;
        // Laid out by hand: the test asserts routing, and `hit_test` reads
        // rects, so the rects are set rather than measured.
        let inner = Scene::Box(
            BoxNode::new(Rect::new(0, 0, 100, 40), BoxStyle::filled(CONTROL)).with_tag("threshold"),
        );
        let region = Scene::Container(
            ContainerNode::new(vec![inner])
                .with_tag(GroupBoxTag::content("g"))
                .with_layout(L::new().with_disabled(true)),
        );
        let Scene::Container(mut region_node) = region else {
            panic!("container")
        };
        region_node.rect = Rect::new(0, 0, 100, 40);
        let mut root = ContainerNode::new(vec![Scene::Container(region_node)]);
        root.rect = Rect::new(0, 0, 100, 40);
        let scene = Scene::Container(root);
        let hit = scene.hit_test(50, 20).expect("inside");
        assert_eq!(
            hit.segments,
            vec![GroupBoxTag::content("g")],
            "the press stops at the region — the toolkit hands such an event to the parent",
        );
    }

    /// ★★ R1673 — nothing this widget paints leaves the box that owns it, at
    /// every size and in both postures.
    ///
    /// A counterfactual is why this exists rather than the round's word for it:
    /// removing the frame reservation from the content region, and removing the
    /// title band's knowledge of the checkbox it holds, both left this crate's
    /// whole suite GREEN. The defects were real — measured on
    /// `hello-group-box`, the content region covered the outline on all four
    /// edges and the title checkbox stood two pixels above and below its band —
    /// and only a CONSUMER booting could see them, one screen at a time, which
    /// is the shape R1655 recorded for this same crate.
    ///
    /// The assertion is the framework's own
    /// [`pinion_core::test_fixtures::screen_ink::assert_contained_ink`], the one
    /// three screens run, so a widget and the screens holding it are judged by
    /// one rule rather than by two that can drift.
    #[test]
    fn r1673_nothing_a_group_box_paints_leaves_the_box_that_owns_it() {
        use pinion_core::test_fixtures::screen_ink::assert_contained_ink;
        use pinion_runtime::layout::compute_layout;

        let theme = Theme::light();
        for checked in [None, Some(true), Some(false)] {
            for (label, width, height) in [("opening", 420u32, 260u32), ("narrow", 180, 120)] {
                let check = checked.map(|checked| GroupBoxCheck {
                    checked,
                    interaction: CheckboxState::Idle,
                });
                let body = vec![Scene::Box(BoxNode::new(
                    Rect::new(0, 0, 40, 20),
                    BoxStyle::filled(theme.resolve(ColorRole::Accent)),
                ))];
                let mut scene = Scene::Container(
                    pinion_core::scene::ContainerNode::new(vec![view_group_box(
                        "advanced",
                        "&Advanced",
                        check,
                        &theme,
                        &GroupBoxStyle::default(),
                        body,
                    )])
                    .with_layout(
                        LayoutStyle::new().with_size(pinion_core::style::Size::px(width, height)),
                    ),
                );
                let mut cache = pinion_text::LayoutCache::new();
                compute_layout(&mut scene, &mut cache, width, height);
                let when = format!("{label}, check = {checked:?}");
                assert_contained_ink(&when, &scene, (width, height));
            }
        }
    }
}
