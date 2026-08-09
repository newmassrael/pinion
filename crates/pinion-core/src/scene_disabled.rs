//! R1554 §5.39 §5.35 §5.40 — the **disabled cascade**: the one interaction
//! property in the §5.21 layout sidecar that is INHERITED, and the pass that
//! resolves it over a produced paint scene.
//!
//! ## What was missing
//!
//! [`LayoutStyle`](crate::style::LayoutStyle) carried four interaction
//! declarations before this round — `pointer_transparent` (R705), `focusable`
//! (R1020), `drop_target` (R1080), `cursor` (R1196) — and every one of them
//! describes the node that carries it and nothing else. The toolkit's
//! `setEnabled` is the one that does not: a disabled widget makes
//! its whole subtree non-interactive, which is why group box can gate a
//! panel of controls from one checkbox in its title and `<fieldset disabled>`
//! can gate a form. pinion had no way to state it, and consequently no group
//! container at all (`grep -rn GroupBox` over 29 crates and 206 examples
//! answered nothing).
//!
//! A binding *could* have marked every descendant instead. What it could not
//! do is keep that marking true as the subtree changed, tell the accessibility
//! tree, keep the Tab order and the pointer router in step with both, or fade
//! the ink — four consequences, each decided somewhere the binding does not
//! reach. So the declaration is one bool and the consequences are derived.
//!
//! ## The cascade is a derivation, not a write into the descendants
//!
//! The toolkit implements the same inheritance by **mutating** it: `setEnabled(false)` runs
//! `setEnabled_helper` recursively, setting `WA_Disabled` on every descendant widget, and `setEnabled(true)` walks them
//! again taking it back except where `WA_ForceDisabled` says the child disabled itself. That
//! is N copies of one fact, kept in step by remembering to re-run the helper —
//! on reparenting, most of all.
//!
//! Here the derived half ([`LayoutStyle::resolved_disabled`]) is recomputed
//! from the declarations on **every** produced paint scene, in both directions,
//! by [`resolve_disabled`]. `V::view` rebuilds the tree from scratch each frame
//! (R26), so nothing survives a frame to be stale, and no builder can write the
//! derived half — the same posture R1518 took for the accessibility focus flag
//! and R682's `paint_hash` for its structural hash.
//!
//! It runs at exactly one place:
//! [`settle_to_fixed_point`](../../pinion_runtime/fn.settle_to_fixed_point.html),
//! which every paint-scene producer in both backends already funnels through
//! (its own doc records why: five copies of that loop had drifted). So the
//! terminal and the window resolve one cascade, and a producer cannot forget
//! it because there is nowhere left to forget it in.
//!
//! ## The ink
//!
//! A region that is inert but painted as though it were live is worse than
//! either, so the cascade fades it. The fade is the Material 3
//! [`DISABLED`] fraction toward the
//! node's **backdrop** — the nearest opaque fill above it — which is the same
//! token, in the same direction, that
//! `pinion_widget_paint::state_layer::state_layer` already applies to a widget
//! whose own state enum is `Disabled`. A control inside a disabled group and a
//! control disabled by itself therefore land on the same ink, and that is why
//! the token moved down to `pinion_core` this round rather than being restated.
//!
//! Resolving to concrete channels (rather than reducing alpha, which would be
//! the smaller change) is what carries the fade into the terminal: a
//! `TermCell` has no alpha and `pinion_tui`'s `color_to_tui` drops it (§2 #6).
//!
//! Four node kinds carry content the cascade cannot fade — an `Image`'s
//! pixels, an `External`'s backend surface, an `ImmediateModeNode`'s driver
//! output, a `TextGrid`'s buffer. The toolkit cannot grey a GL widget either.
//! What the cascade does instead of guessing is **publish** it: the census
//! reports each disabled node's [`DisabledInk`], so "declared disabled, ink
//! unchanged" is a fact an agent reads rather than a surprise it discovers.
//!
//! [`LayoutStyle::resolved_disabled`]: crate::style::LayoutStyle::resolved_disabled

use crate::Scene;
use crate::style::{BoxStyle, Color, PathStyle, TextStyle};
use crate::widgets::interaction::DISABLED;

/// The colour a disabled region fades toward when nothing above it declares an
/// opaque fill — mid grey, the direction every toolkit's disabled ink takes.
///
/// A backdrop is normally found: an application's root container carries the
/// window background, and a group frame carries its own surface. This is the
/// answer for the tree that declares none, and it is a real answer rather than
/// a skip: a lerp toward mid grey reduces contrast for light ink and for dark
/// ink alike, so the region reads as inert either way. Leaving such a node
/// untouched would be the one outcome worth avoiding — inert content painted
/// exactly as live content.
pub const BACKDROP_FALLBACK: Color = Color::rgb(0x80, 0x80, 0x80);

/// What [`resolve_disabled`] was able to do to a disabled node's ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisabledInk {
    /// The node's declared colours were faded toward its backdrop by
    /// [`DISABLED`] — `Box`, `Text`, `Path`, `Container` and `Scroll`.
    Faded,
    /// The node paints content the cascade does not author: an [`Scene::Image`]
    /// texture, an [`Scene::External`] backend surface, an
    /// [`Scene::ImmediateModeNode`] driver's output, or a [`Scene::TextGrid`]
    /// buffer. It is inert and announced disabled; its pixels are unchanged.
    ///
    /// Published rather than silently skipped — see the module docs.
    OpaqueContent,
}

impl DisabledInk {
    /// The lowercase wire spelling, for the `scene/disabled` RPC surface.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            DisabledInk::Faded => "faded",
            DisabledInk::OpaqueContent => "opaque_content",
        }
    }
}

/// One disabled node in a resolved paint scene, and *why* it is disabled.
///
/// The `why` is the half the toolkit has no accessor for. `isEnabled()` answers a bool;
/// `isEnabledTo(ancestor)` answers a bool about an ancestor the caller must already have picked;
/// `testAttribute(WA_ForceDisabled)` distinguishes self from inherited but names nobody. Which ancestor
/// greyed a control is, in the toolkit, a `parentWidget()` walk in a debugger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisabledNode {
    /// The disabled node's own tag. Untagged disabled nodes are not reported —
    /// they cannot be addressed, so a row for one would name nothing.
    pub tag: String,
    /// The node carries its own [`disabled`](crate::style::LayoutStyle::disabled)
    /// declaration — the toolkit's `WA_ForceDisabled`. True and [`Self::declared_by`] `Some` together mean a
    /// self-disabled node that also sits inside a disabled region, so
    /// re-enabling the region leaves this one disabled.
    pub self_declared: bool,
    /// The nearest **strict ancestor** that declared the region this node is
    /// in, or [`None`] when the node's own declaration is the only one.
    pub declared_by: Option<String>,
    /// What the cascade did to this node's ink.
    pub ink: DisabledInk,
}

/// Resolve the inherited disabled property over a produced paint scene: record
/// [`resolved_disabled`](crate::style::LayoutStyle::resolved_disabled) on every
/// node, and fade the ink of each node the pass newly resolves as disabled.
///
/// Idempotent. The fade fires on the `false -> true` transition of the derived
/// field, so laying one scene out twice cannot fade it twice — which matters
/// because the settle loop is permitted to run several layout passes, and a
/// relative lerp applied twice is a different colour.
///
/// Cost is one walk of the tree with two `Color`s carried down, and it touches
/// style only inside a disabled region. A tree that declares nothing disabled
/// pays the walk and writes `false` where `false` already was.
pub fn resolve_disabled(scene: &mut Scene) {
    cascade(scene, false, BACKDROP_FALLBACK);
}

fn cascade(scene: &mut Scene, inherited: bool, backdrop: Color) {
    // R1554 — the backdrop for THIS node is what is behind it, so it is read
    // from the ancestors before the node's own fill can join it. A node's own
    // fill is what its CHILDREN sit on.
    let effective = inherited || scene.declares_disabled();
    let newly = if let Some(layout) = scene.layout_style_mut() {
        let was = layout.resolved_disabled;
        layout.resolved_disabled = effective;
        effective && !was
    } else {
        // `Scene::Effect` carries no sidecar and paints nothing.
        false
    };
    if newly {
        fade_node(scene, backdrop);
    }
    let child_backdrop = opaque_fill(scene).unwrap_or(backdrop);
    match scene {
        Scene::Container(c) => {
            for child in &mut c.children {
                cascade(child, effective, child_backdrop);
            }
        }
        Scene::Scroll(s) => cascade(&mut s.content, effective, child_backdrop),
        Scene::Box(_)
        | Scene::Text(_)
        | Scene::Path(_)
        | Scene::Image(_)
        | Scene::External(_)
        | Scene::Effect(_)
        | Scene::ImmediateModeNode(_)
        | Scene::TextGrid(_) => {}
    }
}

/// The node's own fill when it is fully opaque — the backdrop its descendants
/// composite against. A translucent or absent fill lets the ancestors' backdrop
/// through, which is what the painter does too.
fn opaque_fill(scene: &Scene) -> Option<Color> {
    let fill = match scene {
        Scene::Box(n) => n.style.fill,
        Scene::Container(n) => n.style.fill,
        // R1554 — a grid's cells composite over its palette's default
        // background, which is the surface a descendant would sit on.
        Scene::TextGrid(n) => n.palette.default_bg(),
        Scene::Text(_)
        | Scene::Path(_)
        | Scene::Image(_)
        | Scene::External(_)
        | Scene::Effect(_)
        | Scene::ImmediateModeNode(_)
        | Scene::Scroll(_) => return None,
    };
    (fill.a == u8::MAX).then_some(fill)
}

/// What [`resolve_disabled`] can do to one node's ink, and the classification
/// the census publishes for it.
fn fade_node(scene: &mut Scene, backdrop: Color) -> DisabledInk {
    match scene {
        Scene::Box(n) => {
            fade_box_style(&mut n.style, backdrop);
            DisabledInk::Faded
        }
        Scene::Container(n) => {
            fade_box_style(&mut n.style, backdrop);
            DisabledInk::Faded
        }
        Scene::Text(n) => {
            fade_text_style(&mut n.style, backdrop);
            for run in &mut n.runs {
                fade_text_style(&mut run.style, backdrop);
            }
            DisabledInk::Faded
        }
        Scene::Path(n) => {
            fade_path_style(&mut n.style, backdrop);
            DisabledInk::Faded
        }
        // A scroll container has no ink of its own (its viewport is a clip);
        // its content is faded by the walk that descends into it.
        Scene::Scroll(_) => DisabledInk::Faded,
        // See `DisabledInk::OpaqueContent` — content the cascade does not
        // author. `Effect` paints nothing at all and joins them rather than
        // claiming a fade it did not perform.
        Scene::Image(_)
        | Scene::External(_)
        | Scene::ImmediateModeNode(_)
        | Scene::TextGrid(_)
        | Scene::Effect(_) => DisabledInk::OpaqueContent,
    }
}

/// Fade one colour toward `backdrop` by the M3 [`DISABLED`] fraction.
///
/// A fully transparent colour stays transparent: nothing is painted, so there
/// is nothing to dim, and lerping would materialise ink the binding did not
/// declare (the alpha short-circuit both painters already take).
fn fade(color: Color, backdrop: Color) -> Color {
    if color.a == 0 {
        color
    } else {
        color.lerp(backdrop, DISABLED)
    }
}

// The three fade fns below DESTRUCTURE their style struct rather than reaching
// for the fields they know about. A colour added to any of them then fails to
// compile until someone states what the disabled cascade does with it — the
// completeness discipline R1550's `Footprint` impls use, and the reason a
// facet cannot silently escape the fade the way it could escape a hand-listed
// field set.

fn fade_box_style(style: &mut BoxStyle, backdrop: Color) {
    let BoxStyle {
        fill,
        border,
        corner_radius: _,
        gradient,
        shadows,
    } = style;
    *fill = fade(*fill, backdrop);
    if let Some(border) = border {
        border.color = fade(border.color, backdrop);
    }
    if let Some(gradient) = gradient {
        for stop in &mut gradient.stops {
            stop.color = fade(stop.color, backdrop);
        }
    }
    for shadow in shadows.iter_mut() {
        // A shadow is depth, and M3 removes elevation from a disabled surface
        // outright rather than dimming it. Fading the shadow colour toward the
        // backdrop is that removal expressed in the one channel available here.
        shadow.color = fade(shadow.color, backdrop);
    }
}

fn fade_text_style(style: &mut TextStyle, backdrop: Color) {
    let TextStyle {
        font_family: _,
        font_size_px: _,
        fg_color,
        bg_color,
        font_weight: _,
        font_style: _,
        line_height: _,
        letter_spacing: _,
        text_align: _,
        text_indent: _,
        decoration,
        overflow: _,
    } = style;
    *fg_color = fade(*fg_color, backdrop);
    if let Some(bg) = bg_color {
        *bg = fade(*bg, backdrop);
    }
    // R1546's run background and R1540's underline colour both carry ink, so
    // both fade. The underline's FORM (curly / dotted) is meaning, not ink —
    // an error squiggle under disabled text is still an error squiggle.
    if let Some(uc) = &mut decoration.underline_color {
        *uc = fade(*uc, backdrop);
    }
}

fn fade_path_style(style: &mut PathStyle, backdrop: Color) {
    let PathStyle {
        stroke,
        fill,
        gradient,
    } = style;
    if let Some(stroke) = stroke {
        stroke.color = fade(stroke.color, backdrop);
    }
    if let Some(fill) = fill {
        *fill = fade(*fill, backdrop);
    }
    if let Some(gradient) = gradient {
        for stop in &mut gradient.stops {
            stop.color = fade(stop.color, backdrop);
        }
    }
}

/// Enumerate every **tagged** disabled node in a resolved paint scene, with the
/// ancestor that disabled it.
///
/// Reports one flat row per node rather than also publishing the grouping by
/// declaring ancestor: the grouping is a `group_by` over
/// [`DisabledNode::declared_by`], and two published forms of one fact are two
/// things that can disagree.
///
/// Call it on a scene [`resolve_disabled`] has run over. On a raw view scene
/// the declarations are still there and the walk still finds them — the
/// [`ink`](DisabledNode::ink) column is the part that describes work the
/// cascade has done.
#[must_use]
pub fn disabled_census(scene: &Scene) -> Vec<DisabledNode> {
    let mut out = Vec::new();
    census(scene, None, &mut out);
    out
}

fn census(scene: &Scene, declared_by: Option<&str>, out: &mut Vec<DisabledNode>) {
    let self_declared = scene.declares_disabled();
    if (self_declared || declared_by.is_some())
        && let Some(tag) = scene.tag()
    {
        out.push(DisabledNode {
            tag: tag.to_owned(),
            self_declared,
            declared_by: declared_by.map(str::to_owned),
            ink: ink_of(scene),
        });
    }
    // The nearest declaring ancestor FOR THE CHILDREN: this node when it
    // declares (whatever it inherited), else whatever was passed down.
    let child_declarer = if self_declared {
        scene.tag().or(declared_by)
    } else {
        declared_by
    };
    match scene {
        Scene::Container(c) => {
            for child in &c.children {
                census(child, child_declarer, out);
            }
        }
        Scene::Scroll(s) => census(&s.content, child_declarer, out),
        Scene::Box(_)
        | Scene::Text(_)
        | Scene::Path(_)
        | Scene::Image(_)
        | Scene::External(_)
        | Scene::Effect(_)
        | Scene::ImmediateModeNode(_)
        | Scene::TextGrid(_) => {}
    }
}

/// The [`DisabledInk`] classification for a node, without touching it — the
/// read-only peer of [`fade_node`]'s return value. The two agree because a test
/// asserts they do over every node kind
/// (`the_census_classification_matches_what_the_fade_did`); two lists that must
/// match is the shape R1547 found a second implementation hiding in.
fn ink_of(scene: &Scene) -> DisabledInk {
    match scene {
        Scene::Box(_)
        | Scene::Container(_)
        | Scene::Text(_)
        | Scene::Path(_)
        | Scene::Scroll(_) => DisabledInk::Faded,
        Scene::Image(_)
        | Scene::External(_)
        | Scene::ImmediateModeNode(_)
        | Scene::TextGrid(_)
        | Scene::Effect(_) => DisabledInk::OpaqueContent,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BACKDROP_FALLBACK, DisabledInk, disabled_census, fade_node, ink_of, resolve_disabled,
    };
    use crate::Scene;
    use crate::scene::{BoxNode, ContainerNode, Rect, TextNode};
    use crate::style::{BoxStyle, Color, LayoutStyle, TextStyle};
    use crate::widgets::interaction::DISABLED;

    const INK: Color = Color::rgb(0x10, 0x20, 0x30);
    const SURFACE: Color = Color::rgb(0xf0, 0xf0, 0xf0);

    fn text(tag: &'static str) -> Scene {
        Scene::Text(
            TextNode::styled(tag, Rect::default(), TextStyle::new().with_fg(INK)).with_tag(tag),
        )
    }

    fn filled(tag: &'static str, fill: Color, children: Vec<Scene>) -> ContainerNode {
        ContainerNode::new(children)
            .with_tag(tag)
            .with_style(BoxStyle::filled(fill))
    }

    fn fg_of(scene: &Scene) -> Color {
        match scene {
            Scene::Text(t) => t.style.fg_color,
            other => panic!("not a text node: {other:?}"),
        }
    }

    /// Find a tagged node anywhere in the tree — the tests address by tag so
    /// they do not restate the tree's shape as index paths (R1553's lifted
    /// `scene_probe`, same reason).
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

    // ----- the derivation -----

    #[test]
    fn a_declared_region_resolves_on_the_declarer_and_every_descendant() {
        let mut scene = Scene::Container(filled(
            "root",
            SURFACE,
            vec![
                text("live"),
                Scene::Container(
                    filled("group", SURFACE, vec![text("inner")])
                        .with_layout(LayoutStyle::new().with_disabled(true)),
                ),
            ],
        ));
        resolve_disabled(&mut scene);
        assert!(find(&scene, "group").is_disabled(), "the declarer");
        assert!(find(&scene, "inner").is_disabled(), "its descendant");
        assert!(
            !find(&scene, "live").is_disabled(),
            "a sibling outside the region is untouched",
        );
        assert!(!scene.is_disabled(), "and so is the root above it");
    }

    #[test]
    fn the_derivation_is_written_in_both_directions() {
        // The cascade must CLEAR a stale derived flag, not only set it. A
        // pass that only ever set it would look correct on every fresh view
        // scene and be wrong on any tree handed in twice.
        let mut scene = Scene::Container(ContainerNode::new(vec![text("a")]));
        // Forge the derived half the way nothing in production can.
        if let Some(l) = find_mut(&mut scene, "a").layout_style_mut() {
            l.resolved_disabled = true;
        }
        resolve_disabled(&mut scene);
        assert!(
            !find(&scene, "a").is_disabled(),
            "no declaration anywhere, so the derived flag is cleared",
        );
    }

    fn find_mut<'a>(scene: &'a mut Scene, tag: &str) -> &'a mut Scene {
        fn walk<'a>(s: &'a mut Scene, tag: &str) -> Option<&'a mut Scene> {
            if s.tag() == Some(tag) {
                return Some(s);
            }
            match s {
                Scene::Container(c) => c.children.iter_mut().find_map(|c| walk(c, tag)),
                Scene::Scroll(s) => walk(&mut s.content, tag),
                _ => None,
            }
        }
        walk(scene, tag).unwrap_or_else(|| panic!("no node tagged {tag}"))
    }

    // ----- the ink -----

    #[test]
    fn the_fade_lands_on_the_m3_disabled_ink_over_the_nearest_opaque_backdrop() {
        let mut scene = Scene::Container(filled(
            "root",
            SURFACE,
            vec![Scene::Container(
                ContainerNode::new(vec![text("inner")])
                    .with_tag("group")
                    .with_layout(LayoutStyle::new().with_disabled(true)),
            )],
        ));
        resolve_disabled(&mut scene);
        assert_eq!(
            fg_of(find(&scene, "inner")),
            INK.lerp(SURFACE, DISABLED),
            "the same token, direction and backdrop a self-disabled widget's \
             state layer uses — which is why the token is shared, not restated",
        );
    }

    #[test]
    fn a_tree_with_no_opaque_backdrop_still_fades() {
        // The one outcome worth avoiding is inert content painted exactly as
        // live content, so the fallback is a real answer rather than a skip.
        let mut scene = Scene::Container(
            ContainerNode::new(vec![text("inner")])
                .with_tag("group")
                .with_layout(LayoutStyle::new().with_disabled(true)),
        );
        resolve_disabled(&mut scene);
        assert_eq!(
            fg_of(find(&scene, "inner")),
            INK.lerp(BACKDROP_FALLBACK, DISABLED)
        );
    }

    #[test]
    fn the_nearest_opaque_fill_is_the_backdrop_not_the_outermost() {
        let mut scene = Scene::Container(filled(
            "root",
            SURFACE,
            vec![Scene::Container(
                filled("group", Color::rgb(0x00, 0x00, 0x00), vec![text("inner")])
                    .with_layout(LayoutStyle::new().with_disabled(true)),
            )],
        ));
        resolve_disabled(&mut scene);
        // `group` itself faded first (it is in its own region), and its
        // children composite over the faded surface — which is what the
        // painter does.
        let group_fill = match find(&scene, "group") {
            Scene::Container(c) => c.style.fill,
            other => panic!("not a container: {other:?}"),
        };
        assert_eq!(
            fg_of(find(&scene, "inner")),
            INK.lerp(group_fill, DISABLED),
            "the backdrop is what is actually behind the node",
        );
    }

    #[test]
    fn the_fade_is_idempotent() {
        // The settle loop may run several layout passes, and a relative lerp
        // applied twice is a different colour. The derived flag's transition is
        // what gates it.
        let mut scene = Scene::Container(filled(
            "root",
            SURFACE,
            vec![Scene::Container(
                ContainerNode::new(vec![text("inner")])
                    .with_tag("group")
                    .with_layout(LayoutStyle::new().with_disabled(true)),
            )],
        ));
        resolve_disabled(&mut scene);
        let once = fg_of(find(&scene, "inner"));
        resolve_disabled(&mut scene);
        resolve_disabled(&mut scene);
        assert_eq!(fg_of(find(&scene, "inner")), once, "three runs, one fade");
    }

    #[test]
    fn a_transparent_colour_stays_transparent() {
        // Lerping it would materialise ink the binding never declared — the
        // alpha short-circuit both painters already take.
        let mut node = Scene::Box(BoxNode::new(
            Rect::default(),
            BoxStyle::filled(Color::TRANSPARENT),
        ));
        assert_eq!(fade_node(&mut node, SURFACE), DisabledInk::Faded);
        match node {
            Scene::Box(b) => assert_eq!(b.style.fill, Color::TRANSPARENT),
            other => panic!("not a box: {other:?}"),
        }
    }

    #[test]
    fn every_style_run_fades_not_only_the_base_style() {
        // A syntax-highlighted or find-matched line carries its colours in
        // `runs`; fading only `style` would leave the highlighted spans at full
        // contrast inside a greyed region.
        use crate::scene::StyleRun;
        let accent = Color::rgb(0xff, 0x00, 0x00);
        let node = TextNode::styled("abcdef", Rect::default(), TextStyle::new().with_fg(INK))
            .with_runs(vec![StyleRun::new(0, 3, TextStyle::new().with_fg(accent))]);
        let mut scene = Scene::Container(filled(
            "root",
            SURFACE,
            vec![Scene::Container(
                ContainerNode::new(vec![Scene::Text(node)])
                    .with_tag("group")
                    .with_layout(LayoutStyle::new().with_disabled(true)),
            )],
        ));
        resolve_disabled(&mut scene);
        let Scene::Container(group) = find(&scene, "group") else {
            panic!("not a container")
        };
        let Scene::Text(t) = &group.children[0] else {
            panic!("not text")
        };
        assert_eq!(t.runs[0].style.fg_color, accent.lerp(SURFACE, DISABLED));
    }

    // ----- the census -----

    #[test]
    fn the_census_names_the_nearest_declaring_ancestor() {
        let mut scene = Scene::Container(filled(
            "root",
            SURFACE,
            vec![Scene::Container(
                filled(
                    "outer",
                    SURFACE,
                    vec![Scene::Container(
                        filled("middle", SURFACE, vec![text("deep")])
                            .with_layout(LayoutStyle::new().with_disabled(true)),
                    )],
                )
                .with_layout(LayoutStyle::new().with_disabled(true)),
            )],
        ));
        resolve_disabled(&mut scene);
        let census = disabled_census(&scene);
        let deep = census.iter().find(|d| d.tag == "deep").expect("present");
        assert_eq!(deep.declared_by.as_deref(), Some("middle"));
        assert!(!deep.self_declared);
        let middle = census.iter().find(|d| d.tag == "middle").expect("present");
        assert!(middle.self_declared, "it carries its own declaration");
        assert_eq!(
            middle.declared_by.as_deref(),
            Some("outer"),
            "AND sits in one — the toolkit's WA_ForceDisabled case, both halves reported",
        );
    }

    #[test]
    fn an_untagged_node_is_absent_from_the_census_not_reported_nameless() {
        let mut scene = Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::new("x", Rect::default()))])
                .with_layout(LayoutStyle::new().with_disabled(true)),
        );
        resolve_disabled(&mut scene);
        assert!(
            disabled_census(&scene).is_empty(),
            "nothing here is addressable, so there is nothing to publish",
        );
    }

    #[test]
    fn the_census_classification_matches_what_the_fade_did() {
        // `ink_of` and `fade_node` are two lists over the same ten node kinds,
        // and a mismatch would publish a fade that never happened. Run over
        // R1516's `SceneNodeKind::ALL` census rather than a hand-listed set, so
        // an eleventh kind cannot join with only one of the two lists updated.
        for kind in crate::scene::SceneNodeKind::ALL {
            let mut node = crate::test_fixtures::scene_of_kind(kind);
            let expected = ink_of(&node);
            assert_eq!(
                fade_node(&mut node, SURFACE),
                expected,
                "classification and action disagree for {}",
                kind.name(),
            );
        }
    }
}
