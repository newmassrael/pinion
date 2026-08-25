//! R1574 §5.38 §5.39 — the **Material 3 Switch** painter: a rounded pill track
//! with a knob that sits at one end or the other.
//!
//! # Why this file did not exist until now
//!
//! `pinion-widget-paint` has a painter per catalog widget — `button`,
//! `checkbox`, `radio_composite`, `slider`, `tabs`, `disclosure` — and no
//! `switch`. So every binding whose widget is a `role = Switch` painted its own
//! track and knob: **twelve of them**, measured at R1570.1 —
//! `hello-toggle`, `hello-theme`, `hello-elevation`, `hello-gradient`,
//! `hello-image`, `hello-path`, `hello-pdf-export`, `hello-richtext`,
//! `hello-richtext-background`, `hello-richtext-blocks`,
//! `hello-richtext-cells`, `hello-richtext-list`. Four times the rule-of-three
//! threshold, on the framework's most ordinary control.
//!
//! # What the absence cost, and how it showed up
//!
//! Not as a duplicated *drawing*. As a duplicated **declaration**. When R1570.1
//! made "a declared interactive role is a keyboard focus stop" true of the
//! tree, `hello-checkbox` needed one line — `CheckboxStyle::focusable`, on the
//! painter that owns its tagged node — while ten of this class had to repeat
//! `.with_focusable(true)` in the binding, because there was no painter to put
//! it on. That is [[r1532-column-declares-its-painter]]'s rule one axis over:
//! **an absent extension point shows up as workaround code**, and here the
//! workaround was a declaration that has to be remembered twelve times.
//!
//! The next cross-cutting declaration would have cost the same twelve edits.
//!
//! # The shape
//!
//! [`view_switch`] owns the tagged, focusable track container and the knob
//! inside it, which is precisely the part every consumer had to get right and
//! the part a cross-cutting round has to reach. What it deliberately does
//! **not** own is the row around it: a caption's text, its position relative to
//! the track, and any status line are per-binding layout, and folding them in
//! would make the painter refuse the bindings whose switch sits in a toolbar or
//! a settings row rather than beside a label.
//!
//! Colour comes from [`crate::state_layer::state_layer`], the one definition
//! the checkbox, the slider and the table rows already share, so a switch's
//! hover weight cannot drift from a checkbox's.
//!
//! # Against the toolkit 6.11
//!
//! The toolkit has **no switch widget at all**. check box is the checkbox, and
//! a toolkit application that wants a switch either subclasses
//! abstract button and paints it (the recipe in the toolkit's own forums) or
//! uses the toolkit's declarative language's `Switch`, which is a different
//! toolkit. So this is not parity — it is the floor the toolkit does not have,
//! and the twelve hand-rolled consumers here are the same tax a toolkit
//! codebase pays, made visible because they are all in one tree.

use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::voice::Silence;
use pinion_core::widgets::interaction::InteractionState;

/// R1574 §5.38 — the Switch's geometry and its interaction declarations.
///
/// Defaults are [`Self::m3`]'s, which are the numbers the twelve pre-lift
/// bindings had all independently chosen — 64×32 track, 24×24 knob, 4 px
/// inset — so adopting the painter is a behaviour-preserving change for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchStyle {
    /// Track width in logical pixels (M3 `Switch` 52 dp rounded to the 64 px
    /// this tree's bindings settled on).
    pub track_w: u32,
    /// Track height in logical pixels.
    pub track_h: u32,
    /// Track corner radius; at `track_h / 2` the track is a pill.
    pub track_radius: u32,
    /// Inset between the track's edge and the knob, in logical pixels. The
    /// knob's travel is `track_w - 2 * track_pad - knob_size`.
    pub track_pad: u32,
    /// Knob side length in logical pixels.
    pub knob_size: u32,
    /// Knob corner radius; at `knob_size / 2` the knob is a circle.
    pub knob_radius: u32,
    /// R1574 §5.39 — keyboard focus stop. When `true`, [`view_switch`] marks the
    /// tagged track `.with_focusable(true)` so the scene-derived §5.39
    /// enumeration collects it as a Tab stop.
    ///
    /// Default `true`, and on this painter the default is the whole point:
    /// **this field is why the file exists**. R1570.1 had to add
    /// `.with_focusable(true)` by hand in ten bindings of this class because
    /// there was no painter to declare it on once (see the module doc). A
    /// binding that genuinely wants a non-focusable switch — one inside a
    /// composite that owns the Tab stop itself — clears it here.
    pub focusable: bool,
    /// R1837 §5.13 — whether the tagged track lets a press through to whatever
    /// is under it.
    ///
    /// Default `false`, which is what a switch that IS the pointer target
    /// wants: the tag it carries is the `scene/click` address, and the router
    /// resolving a press to it is the whole arrangement.
    ///
    /// ★★★★★ A switch drawn INSIDE another control needs the opposite, for
    /// R1649.1's reason: the router resolves a press to the deepest TAGGED node
    /// under the cursor and then looks for that tag's `External`, so a tagged
    /// node that is an ADDRESS rather than a primitive swallows the press and
    /// forwards nothing — that is how a whole screen was found dead to a real
    /// mouse while 118 scripted assertions passed. The configuration form is
    /// exactly that case: its boolean control publishes its geometry and the
    /// consumer's hit test reads it, so the track has to be an address a census
    /// can see rather than a target a press stops at.
    ///
    /// ★ And this field is a second answer to the question the module doc opens
    /// with. A form that could not declare it here would either hand-roll a
    /// thirteenth track or leave the track untagged — and an untagged track is
    /// invisible to the conformance census that classifies a row by what its
    /// control actually draws.
    pub pointer_transparent: bool,
}

impl SwitchStyle {
    /// The Material 3 Switch metrics, and the ones the pre-lift bindings used.
    #[must_use]
    pub const fn m3() -> Self {
        Self {
            track_w: 64,
            track_h: 32,
            track_radius: 16,
            track_pad: 4,
            knob_size: 24,
            knob_radius: 12,
            focusable: true,
            pointer_transparent: false,
        }
    }
}

impl Default for SwitchStyle {
    fn default() -> Self {
        Self::m3()
    }
}

/// R1574 §5.38 — the track fill for `(state, on)`: M3's Switch role mapping.
///
/// `on` sits on [`ColorRole::Accent`], `off` on
/// [`ColorRole::SurfaceContainerHighest`], and the interaction overlay is
/// [`state_layer`](crate::state_layer::state_layer) — the same definition the
/// checkbox box and the slider track resolve through, so the three cannot drift
/// apart at the same interaction state.
#[must_use]
pub fn switch_track_for<S: InteractionState + Copy>(theme: &Theme, state: S, on: bool) -> Color {
    let base = if on {
        theme.resolve(ColorRole::Accent)
    } else {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    };
    crate::state_layer::state_layer(base, state, theme)
}

/// R1574 §5.38 — the knob fill for `(state, on)`: M3's Switch thumb mapping.
///
/// [`ColorRole::OnAccent`] when on (so the thumb reads against the accent
/// track), [`ColorRole::Outline`] when off. A disabled thumb fades toward
/// [`ColorRole::OnSurfaceMuted`] rather than toward `Surface`, which is
/// deliberately **not** `state_layer`'s target: a thumb that faded toward the
/// surface would vanish into its own track, and a switch whose thumb cannot be
/// found reads as a track with no state at all. A divergent consumer per
/// [`crate::state_layer`]'s own doctrine, referencing that module's
/// [`DISABLED`](crate::state_layer::DISABLED) token so the weight still lives in
/// one place.
#[must_use]
pub fn switch_knob_for<S: InteractionState + Copy>(theme: &Theme, state: S, on: bool) -> Color {
    let base = if on {
        theme.resolve(ColorRole::OnAccent)
    } else {
        theme.resolve(ColorRole::Outline)
    };
    if state.is_disabled() {
        base.lerp(
            theme.resolve(ColorRole::OnSurfaceMuted),
            crate::state_layer::DISABLED,
        )
    } else {
        base
    }
}

/// R1574 §5.38 §5.39 §5.40 — the switch: a tagged, focusable pill track with the
/// knob justified to one end.
///
/// `tag` goes on the **track**, which is what makes the track the pointer
/// target, the focus stop and the `scene/click` address in one node — the
/// property twelve bindings each had to arrange by hand.
///
/// `aria_label` is the accessible name. It is required rather than optional
/// because a switch's visible caption is almost always a *sibling* of the track
/// (a settings row puts the label on the left and the control on the right), so
/// the scene-walk name derivation cannot reach it and an unnamed switch
/// announces as an operable control with no indication of what it operates.
/// Passing `""` is legal and means "the caption is inside this subtree" — a
/// deliberate opt-out rather than an oversight.
///
/// The knob's position is [`JustifyContent`], not an offset: `Start` for off,
/// `End` for on. That leaves the travel to the layout pass, so a binding that
/// changes [`SwitchStyle::track_w`] gets the right throw without arithmetic.
///
/// ★★★★★ R1837 — `silence` is for a switch that does **not** announce itself,
/// and it is required to be explicit rather than defaulted. A tagged, painted
/// region that says nothing is `unvoiced` to the voice census, which is the
/// whole apparatus R1691 built; a switch drawn inside a control that already
/// announces the checkbox has to declare where a reader receives it instead of
/// simply going quiet. `None` means this switch speaks for itself, which is
/// what `aria_label` is then for. It is a parameter and not a
/// [`SwitchStyle`] field because the relay names another node's tag, which is
/// built per row and cannot be a `'static` constant — and [`Silence`] is not
/// `Copy`, which that struct is.
#[must_use]
pub fn view_switch<S: InteractionState + Copy>(
    tag: impl Into<std::borrow::Cow<'static, str>>,
    state: S,
    on: bool,
    theme: &Theme,
    style: &SwitchStyle,
    aria_label: &str,
    silence: Option<Silence>,
) -> Scene {
    let knob = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(switch_knob_for(theme, state, on))
                .with_corner_radius(style.knob_radius),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(style.knob_size, style.knob_size))),
    );
    let quiet = match silence {
        Some(silence) => LayoutStyle::new().with_silence(silence),
        None => LayoutStyle::new(),
    };
    let mut track = ContainerNode::new(vec![knob])
        .with_tag(tag)
        .with_style(
            BoxStyle::filled(switch_track_for(theme, state, on))
                .with_corner_radius(style.track_radius),
        )
        .with_layout(
            quiet
                .with_focusable(style.focusable)
                .with_pointer_transparent(style.pointer_transparent)
                .flex(FlexDirection::Row)
                .with_justify(if on {
                    JustifyContent::End
                } else {
                    JustifyContent::Start
                })
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(style.track_w, style.track_h))
                .with_padding(Rect::new(
                    style.track_pad,
                    style.track_pad,
                    style.track_pad,
                    style.track_pad,
                )),
        );
    if !aria_label.is_empty() {
        track = track.with_aria_label(aria_label.to_owned());
    }
    Scene::Container(track)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::widgets::toggle::ToggleState;

    fn theme() -> Theme {
        Theme::default()
    }

    /// The tagged node's own properties — the ones twelve bindings each had to
    /// arrange, and the reason this painter exists.
    fn track_of(scene: &Scene) -> &ContainerNode {
        match scene {
            Scene::Container(c) => c,
            other => panic!("a switch paints a Container track, got {other:?}"),
        }
    }

    #[test]
    fn r1574_the_tag_and_the_focus_stop_are_on_the_same_node() {
        // The whole point of the lift: whoever owns the tag owns the focus
        // declaration, so a cross-cutting round edits one place. R1570.1 had to
        // edit ten bindings because these two facts lived apart.
        let scene = view_switch(
            "sw",
            ToggleState::Idle,
            false,
            &theme(),
            &SwitchStyle::m3(),
            "Dark mode",
            None,
        );
        let track = track_of(&scene);
        assert_eq!(track.tag.as_deref(), Some("sw"));
        assert!(
            track.layout.focusable,
            "the tagged track IS the Tab stop — a switch whose role is \
             announced as operable and which no keyboard can reach is the R1570 \
             defect",
        );
        assert_eq!(track.aria_label.as_deref(), Some("Dark mode"));
    }

    /// ★★★★★ R1837 — a switch drawn INSIDE another control lets the press
    /// through, and the default does not.
    ///
    /// Both halves are asserted, because either alone is the wrong behaviour
    /// somewhere. A switch that IS the pointer target must stop the press — its
    /// tag is the `scene/click` address and the router resolving to it is the
    /// whole arrangement. A switch that is one affordance inside a control must
    /// not: the router resolves a press to the deepest TAGGED node and then
    /// looks for that tag's `External`, so an address that swallows a press
    /// leaves the control under it dead to a real mouse while every scripted
    /// assertion stays green. That is R1649.1's defect, and it was found by a
    /// hand after 118 of those assertions passed.
    #[test]
    fn r1837_a_switch_inside_a_control_lets_the_press_through() {
        let style = SwitchStyle::m3();
        let target = view_switch("sw", ToggleState::Idle, false, &theme(), &style, "x", None);
        assert!(
            !track_of(&target).layout.pointer_transparent,
            "a switch that is the pointer target keeps the press it is the \
             address for",
        );
        let inside = SwitchStyle {
            pointer_transparent: true,
            ..style
        };
        let nested = view_switch(
            "sw",
            ToggleState::Idle,
            false,
            &theme(),
            &inside,
            "",
            Some(Silence::part_of("row.control")),
        );
        assert!(
            track_of(&nested).layout.pointer_transparent,
            "a switch inside another control forwards the press to it",
        );
        // ★★★★★ And it declares where a reader receives it. A tagged, painted
        // region that says nothing is `unvoiced`; the tag cannot be dropped
        // (a census classifies the control by it), so the silence is what makes
        // keeping it legitimate.
        assert_eq!(
            track_of(&nested)
                .layout
                .silence
                .as_ref()
                .and_then(Silence::relay_target),
            Some("row.control"),
            "a quiet switch names the node a reader hears instead",
        );
        assert_eq!(
            track_of(&nested).tag.as_deref(),
            Some("sw"),
            "and it keeps its tag — a census that classifies a control by what \
             it draws cannot see an untagged track",
        );
    }

    #[test]
    fn r1574_a_binding_can_decline_the_focus_stop_and_the_name() {
        let style = SwitchStyle {
            focusable: false,
            ..SwitchStyle::m3()
        };
        let scene = view_switch("sw", ToggleState::Idle, false, &theme(), &style, "", None);
        let track = track_of(&scene);
        assert!(!track.layout.focusable, "a composite may own the Tab stop");
        assert_eq!(
            track.aria_label, None,
            "an empty label is an opt-out, not an empty name — a node with \
             `aria-label: \"\"` announces as nameless, which is worse than \
             letting the scene walk find the caption",
        );
    }

    #[test]
    fn r1574_the_knob_travels_by_justification_not_by_arithmetic() {
        let style = SwitchStyle::m3();
        let off = view_switch("sw", ToggleState::Idle, false, &theme(), &style, "x", None);
        let on = view_switch("sw", ToggleState::Idle, true, &theme(), &style, "x", None);
        assert_eq!(track_of(&off).layout.justify_content, JustifyContent::Start);
        assert_eq!(track_of(&on).layout.justify_content, JustifyContent::End);
        // And the geometry is the style's, so a wider track throws further with
        // no per-binding arithmetic.
        let wide = SwitchStyle {
            track_w: 96,
            ..style
        };
        let scene = view_switch("sw", ToggleState::Idle, true, &theme(), &wide, "x", None);
        assert_eq!(
            track_of(&scene).layout.size,
            Size::px(96, style.track_h),
            "the track takes its width from the style",
        );
    }

    #[test]
    fn r1574_on_and_off_differ_in_both_the_track_and_the_knob() {
        let t = theme();
        let (on_track, off_track) = (
            switch_track_for(&t, ToggleState::Idle, true),
            switch_track_for(&t, ToggleState::Idle, false),
        );
        let (on_knob, off_knob) = (
            switch_knob_for(&t, ToggleState::Idle, true),
            switch_knob_for(&t, ToggleState::Idle, false),
        );
        assert_ne!(on_track, off_track, "the track encodes the value");
        assert_ne!(
            on_knob, off_knob,
            "and so does the knob — a switch that changed only its track would \
             be unreadable in a monochrome palette",
        );
    }

    #[test]
    fn r1574_the_track_overlay_is_the_shared_state_layer() {
        // Not a re-implementation: the assertion is that this painter's hover
        // weight IS the one the checkbox resolves through, so the two cannot
        // drift at the same interaction state.
        let t = theme();
        for on in [false, true] {
            for state in [
                ToggleState::Idle,
                ToggleState::Hover,
                ToggleState::Pressed,
                ToggleState::Disabled,
            ] {
                let base = if on {
                    t.resolve(ColorRole::Accent)
                } else {
                    t.resolve(ColorRole::SurfaceContainerHighest)
                };
                assert_eq!(
                    switch_track_for(&t, state, on),
                    crate::state_layer::state_layer(base, state, &t),
                    "{state:?} on={on} resolves through the shared overlay",
                );
            }
        }
    }

    #[test]
    fn r1574_a_disabled_knob_fades_toward_the_muted_role_not_into_its_track() {
        // The one deliberate divergence from `state_layer`, asserted so it is a
        // decision rather than an accident: fading the thumb toward `Surface`
        // would let it vanish into the track it sits on.
        let t = theme();
        for on in [false, true] {
            let knob = switch_knob_for(&t, ToggleState::Disabled, on);
            let track = switch_track_for(&t, ToggleState::Disabled, on);
            assert_ne!(
                knob, track,
                "a disabled switch still shows WHERE its thumb is (on={on})",
            );
            assert_ne!(
                knob,
                switch_knob_for(&t, ToggleState::Idle, on),
                "and it does read as disabled",
            );
        }
    }
}
