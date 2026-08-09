//! R692 §5.16 §5.40 §5.50 — backend-agnostic `Toolbar` paint
//! composition.
//!
//! Phase B widget-catalog entry — the editor format / command strip.
//! One paint fn, [`view_toolbar`], renders the always-visible
//! horizontal control strip; there is no floating overlay (unlike
//! [`crate::menu`]'s dropdown), so a toolbar needs no
//! `absolute_position` layer.
//!
//! ## Two control classes
//!
//! Controls are command buttons (one-shot) or toggle buttons
//! (`aria-pressed`), driven by a command-class
//! [`ToolbarExternal`](pinion_core::widgets::toolbar::ToolbarExternal).
//! This module owns only the *paint* axis; the binding owns the
//! External + keyboard + a11y walker (mirrors [`crate::menu`] /
//! [`crate::tabs`]).
//!
//! ## Visible state
//!
//! - **Toggle pressed** — a tonal fill ([`ColorRole::Accent`] washed
//!   over [`ColorRole::Surface`] at [`PRESSED_STATE_LAYER`]), the M3
//!   "selected" container read with the palette pinion ships (the M3
//!   `secondaryContainer` token is a future palette axis).
//! - **Roving focus** — the keyboard cursor control draws a
//!   [`ColorRole::Accent`] focus-ring border (WCAG 2.4.7 focus
//!   indicator), independent of the pressed fill so a focused command
//!   shows the ring with no fill and a focused-pressed toggle shows
//!   both.
//! - Command + unpressed-toggle controls paint a transparent fill.
//!
//! ## Composite tags
//!
//! Controls are tagged [`composite_item_tag`] (`{bar}#{index}`). The
//! input router splits at `#` and rewrites cursor hits into
//! `invoke("send", "{i}:<Event>")` against the single shared
//! `ToolbarExternal` ([[multi-external-substrate-extra-externals-pattern]]).
//! Toolbar controls are uniform, so the sub-tag is the bare index (no
//! `t` / `i` discriminator the [`crate::menu`] title-vs-item split
//! needs).
//!
//! ## Future axes (per [[abstraction-needs-second-consumer]])
//!
//! - **Hover state-layer** — pointer hover is router-tracked; a hover
//!   overlay lands when the paint pipeline forwards hover state into
//!   the view fn (today only the keyboard roving cursor + the toggle
//!   pressed bits are visible).
//! - **Icon glyphs** in place of text labels, **min-width buttons**
//!   (M3 button min-width), **separators / section groups**, and an
//!   **overflow menu** for narrow strips.

use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue,
    TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::{Color, Scene};

/// R692 §5.50 — tonal-fill weight for a pressed (selected) toggle
/// control: [`ColorRole::Accent`] lerped over [`ColorRole::Surface`].
/// A clearly-"on" wash (vs the lighter 8 % hover token a
/// [`crate::menu`] active item uses) since a toggle's pressed state is
/// persistent, not transient.
pub const PRESSED_STATE_LAYER: f32 = 0.20;

/// R692 §5.50 — Material 3 `Toolbar` dimensions. Mirrors the
/// [`crate::menu::MenuStyle`] / [`crate::tabs::TabsStyle`] carrier
/// pattern so the widget catalog presents a uniform `Style` surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarStyle {
    /// Strip height in logical pixels (M3 docked toolbar ≈ 48 px).
    pub bar_height: u32,
    /// Control height inside the strip (`bar_height` minus the strip
    /// padding top + bottom).
    pub item_height: u32,
    /// Control label font size (M3 `label-large` ≈ 14 px).
    pub item_font_px: u32,
    /// Leading + trailing inset of a control label.
    pub item_padding: u32,
    /// Gap between adjacent controls.
    pub gap: u32,
    /// Inner padding of the strip container (each edge).
    pub bar_padding: u32,
    /// Control corner radius (M3 button ≈ 8 px).
    pub item_radius: u32,
    /// Focus-ring border thickness for the roving cursor control.
    pub focus_ring_width: u32,
    /// (R1020 §5.39) Keyboard focus stop. When `true`, `view_toolbar`
    /// marks the strip Container `.with_focusable(true)` so the
    /// scene-derived §5.39 enumeration collects the strip as a single Tab
    /// stop (the controls rove internally). Default `true` (R1030 fail-safe,
    /// web native-element model); opt out with `.with_focusable(false)` for a
    /// non-interactive affordance strip — e.g. a text editor's formatting bar,
    /// whose focus belongs to the text field — so the strip stays out of the
    /// Tab order.
    pub focusable: bool,
}

impl ToolbarStyle {
    /// R692 §5.50 — Material 3 `Toolbar` defaults. See the struct docs
    /// for the per-field token anchors.
    #[must_use]
    pub const fn m3_default() -> Self {
        Self {
            bar_height: 48,
            item_height: 36,
            item_font_px: 14,
            item_padding: 12,
            gap: 4,
            bar_padding: 6,
            item_radius: 8,
            focus_ring_width: 2,
            focusable: true,
        }
    }

    /// (R1020 §5.39) Mark the toolbar a single keyboard focus stop
    /// (default `true`). See [`Self::focusable`].
    #[must_use]
    pub const fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }
}

impl Default for ToolbarStyle {
    fn default() -> Self {
        Self::m3_default()
    }
}

/// R692 §5.16 §5.50 — compose a control's composite tag
/// (`{bar_tag}#{index}`). The router splits at `#`; the bare index
/// routes the pointer event to `ToolbarExternal`'s control path.
#[must_use]
pub fn composite_item_tag(bar_tag: &str, index: usize) -> String {
    format!("{bar_tag}#{index}")
}

/// R692 §5.16 §5.50 — horizontal toolbar control strip.
///
/// # Arguments
///
/// - `tag` — strip container tag; the router hit-tests this as the
///   `Toolbar` scope and per-control tags are [`composite_item_tag`].
/// - `labels` — one label per control; index `i` becomes the control
///   tagged `{tag}#{i}`.
/// - `pressed` — per-control pressed bits (a command control's bit is
///   always `false`). `pressed[i] == true` paints the tonal fill.
/// - `focus` — the roving cursor's control index.
/// - `group_focused` — whether the toolbar owns the shell focus; only
///   then does the `focus` control draw its focus ring.
/// - `theme` / `style` — palette + [`ToolbarStyle`] carrier.
///
/// # Returns
///
/// A [`Scene::Container`] tagged `tag` laying out one control per label
/// left-to-right. `labels.len()` should match `pressed.len()`; extra
/// `pressed` entries are ignored and missing ones default to unpressed.
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "each control axis (labels / pressed / disabled / roving focus) is an orthogonal paint input"
)]
pub fn view_toolbar(
    tag: &'static str,
    labels: &[&str],
    pressed: &[bool],
    disabled: &[bool],
    focus: usize,
    group_focused: bool,
    theme: &Theme,
    style: &ToolbarStyle,
) -> Scene {
    let mut controls: Vec<Scene> = Vec::with_capacity(labels.len());
    for (i, label) in labels.iter().enumerate() {
        let is_pressed = pressed.get(i).copied().unwrap_or(false);
        // A short / empty `disabled` slice leaves the missing indices enabled,
        // mirroring how `pressed` defaults to `false` (R989 reflective disabled
        // axis — the caller recomputes the mask each frame, e.g. a contextual
        // selection toolbar that greys "Delete" while nothing is selected).
        let is_disabled = disabled.get(i).copied().unwrap_or(false);
        let is_focused = group_focused && focus == i;
        controls.push(build_control(
            tag,
            i,
            label,
            is_pressed,
            is_disabled,
            is_focused,
            theme,
            style,
        ));
    }
    Scene::Container(
        ContainerNode::new(controls)
            .with_tag(tag)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    // (R1020 §5.39) When the binding opts in, the toolbar is a
                    // single keyboard focus stop (WAI-ARIA roving tabindex — the
                    // strip is the Tab stop, the controls rove internally); a
                    // non-interactive affordance strip leaves it out.
                    .with_focusable(style.focusable)
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Start)
                    .with_gap(style.gap)
                    .with_size(Size::auto().with_height(SizeValue::Px(style.bar_height)))
                    .with_padding(Rect::new(
                        style.bar_padding,
                        style.bar_padding,
                        style.bar_padding,
                        style.bar_padding,
                    )),
            ),
    )
}

/// another declarative toolkit one control: a centered label, tonal-filled
/// when pressed, focus-ringed when it is the roving cursor under group focus.
/// A `is_disabled` control greys its label ([`ColorRole::OnSurfaceMuted`]) and shows no pressed fill — it stays
/// focusable (the roving cursor may rest on it) but its activation is a no-op
/// the binding's reducer gates (R989 focusable-but-not-operable model; see the
/// `toolbar` module docs).
#[expect(
    clippy::too_many_arguments,
    reason = "control identity + the three reflective state bits + theme/style are distinct inputs"
)]
fn build_control(
    bar_tag: &str,
    index: usize,
    label: &str,
    is_pressed: bool,
    is_disabled: bool,
    is_focused: bool,
    theme: &Theme,
    style: &ToolbarStyle,
) -> Scene {
    let fill = if is_pressed && !is_disabled {
        theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), PRESSED_STATE_LAYER)
    } else {
        Color::TRANSPARENT
    };
    let mut box_style = BoxStyle::filled(fill).with_corner_radius(style.item_radius);
    if is_focused {
        box_style = box_style.with_border(Border::new(
            theme.resolve(ColorRole::Accent),
            style.focus_ring_width,
        ));
    }
    let label_role = if is_disabled {
        ColorRole::OnSurfaceMuted
    } else {
        ColorRole::OnSurface
    };
    let label_node = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.item_font_px)
            .with_fg(theme.resolve(label_role)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label_node])
            .with_tag(composite_item_tag(bar_tag, index))
            .with_style(box_style)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::auto().with_height(SizeValue::Px(style.item_height)))
                    .with_padding(Rect::new(style.item_padding, 0, style.item_padding, 0)),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::light()
    }

    const LABELS: [&str; 5] = ["B", "I", "U", "Undo", "Redo"];

    fn collect_tags(scene: &Scene) -> Vec<String> {
        fn walk(scene: &Scene, out: &mut Vec<String>) {
            if let Scene::Container(c) = scene {
                if let Some(tag) = &c.tag {
                    out.push(tag.to_string());
                }
                for child in &c.children {
                    walk(child, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    fn all_text(scene: &Scene) -> Vec<String> {
        fn walk(scene: &Scene, out: &mut Vec<String>) {
            match scene {
                Scene::Text(t) => out.push(t.content.clone()),
                Scene::Container(c) => {
                    for child in &c.children {
                        walk(child, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    /// Background fill of the container tagged `tag`, or `None`.
    fn tag_fill(scene: &Scene, tag: &str) -> Option<Color> {
        if let Scene::Container(c) = scene {
            if c.tag.as_deref() == Some(tag) {
                return Some(c.style.fill);
            }
            for child in &c.children {
                if let Some(f) = tag_fill(child, tag) {
                    return Some(f);
                }
            }
        }
        None
    }

    /// Border of the container tagged `tag`, or `None` (no such tag /
    /// no border).
    fn tag_border(scene: &Scene, tag: &str) -> Option<Border> {
        if let Scene::Container(c) = scene {
            if c.tag.as_deref() == Some(tag) {
                return c.style.border;
            }
            for child in &c.children {
                if let Some(b) = tag_border(child, tag) {
                    return Some(b);
                }
            }
        }
        None
    }

    /// Foreground colour of the label `TextNode` inside the control tagged
    /// `tag` (R989 disabled-dimming check).
    fn tag_label_fg(scene: &Scene, tag: &str) -> Option<Color> {
        fn text_fg(scene: &Scene) -> Option<Color> {
            match scene {
                Scene::Text(t) => Some(t.style.fg_color),
                Scene::Container(c) => c.children.iter().find_map(text_fg),
                _ => None,
            }
        }
        if let Scene::Container(c) = scene {
            if c.tag.as_deref() == Some(tag) {
                return text_fg(scene);
            }
            for child in &c.children {
                if let Some(f) = tag_label_fg(child, tag) {
                    return Some(f);
                }
            }
        }
        None
    }

    #[test]
    fn r989_disabled_control_greys_label_and_drops_fill() {
        let t = theme();
        // Control 0 disabled (and would-be pressed), control 1 enabled +
        // pressed: the disabled one greys its label and shows no fill even
        // though its pressed bit is set; the enabled one fills normally.
        let pressed = [true, true, false, false, false];
        let disabled = [true, false, false, false, false];
        let scene = view_toolbar(
            "toolbar",
            &LABELS,
            &pressed,
            &disabled,
            0,
            false,
            &t,
            &ToolbarStyle::m3_default(),
        );
        assert_eq!(
            tag_label_fg(&scene, "toolbar#0"),
            Some(t.resolve(ColorRole::OnSurfaceMuted)),
            "disabled control greys its label"
        );
        assert_eq!(
            tag_fill(&scene, "toolbar#0"),
            Some(Color::TRANSPARENT),
            "disabled control shows no pressed fill"
        );
        assert_eq!(
            tag_label_fg(&scene, "toolbar#1"),
            Some(t.resolve(ColorRole::OnSurface)),
            "enabled control keeps the normal label colour"
        );
        let pressed_fill = t
            .resolve(ColorRole::Surface)
            .lerp(t.resolve(ColorRole::Accent), PRESSED_STATE_LAYER);
        assert_eq!(
            tag_fill(&scene, "toolbar#1"),
            Some(pressed_fill),
            "enabled pressed control fills normally"
        );
    }

    #[test]
    fn r989_empty_disabled_slice_leaves_all_enabled() {
        let t = theme();
        let pressed = [false; 5];
        let scene = view_toolbar(
            "toolbar",
            &LABELS,
            &pressed,
            &[],
            0,
            false,
            &t,
            &ToolbarStyle::m3_default(),
        );
        for i in 0..LABELS.len() {
            assert_eq!(
                tag_label_fg(&scene, &composite_item_tag("toolbar", i)),
                Some(t.resolve(ColorRole::OnSurface)),
                "an empty disabled slice leaves every control enabled"
            );
        }
    }

    #[test]
    fn r692_toolbar_style_m3_default_constants() {
        let s = ToolbarStyle::m3_default();
        assert_eq!(s.bar_height, 48);
        assert_eq!(s.item_height, 36);
        assert_eq!(s.item_font_px, 14);
        assert_eq!(s.item_padding, 12);
        assert_eq!(s.gap, 4);
        assert_eq!(s.bar_padding, 6);
        assert_eq!(s.item_radius, 8);
        assert_eq!(s.focus_ring_width, 2);
    }

    #[test]
    fn r692_composite_tag_helper() {
        assert_eq!(composite_item_tag("toolbar", 0), "toolbar#0");
        assert_eq!(composite_item_tag("toolbar", 3), "toolbar#3");
    }

    #[test]
    fn r692_toolbar_tags_and_labels() {
        let pressed = [false; 5];
        let scene = view_toolbar(
            "toolbar",
            &LABELS,
            &pressed,
            &[],
            0,
            false,
            &theme(),
            &ToolbarStyle::m3_default(),
        );
        assert_eq!(
            collect_tags(&scene),
            vec![
                "toolbar".to_string(),
                "toolbar#0".to_string(),
                "toolbar#1".to_string(),
                "toolbar#2".to_string(),
                "toolbar#3".to_string(),
                "toolbar#4".to_string(),
            ]
        );
        assert_eq!(all_text(&scene), vec!["B", "I", "U", "Undo", "Redo"]);
    }

    #[test]
    fn r692_pressed_toggle_filled_others_transparent() {
        let t = theme();
        let pressed = [true, false, true, false, false];
        let scene = view_toolbar(
            "toolbar",
            &LABELS,
            &pressed,
            &[],
            0,
            false,
            &t,
            &ToolbarStyle::m3_default(),
        );
        let expected = t
            .resolve(ColorRole::Surface)
            .lerp(t.resolve(ColorRole::Accent), PRESSED_STATE_LAYER);
        assert_eq!(
            tag_fill(&scene, "toolbar#0"),
            Some(expected),
            "pressed toggle filled"
        );
        assert_eq!(
            tag_fill(&scene, "toolbar#1"),
            Some(Color::TRANSPARENT),
            "unpressed transparent"
        );
        assert_eq!(tag_fill(&scene, "toolbar#2"), Some(expected));
        assert_eq!(
            tag_fill(&scene, "toolbar#3"),
            Some(Color::TRANSPARENT),
            "command transparent"
        );
    }

    #[test]
    fn r692_focus_ring_only_on_focused_control_when_group_focused() {
        let t = theme();
        let pressed = [false; 5];
        // Focus index 2, group focused → only control 2 has the ring.
        let scene = view_toolbar(
            "toolbar",
            &LABELS,
            &pressed,
            &[],
            2,
            true,
            &t,
            &ToolbarStyle::m3_default(),
        );
        assert_eq!(
            tag_border(&scene, "toolbar#2"),
            Some(Border::new(t.resolve(ColorRole::Accent), 2)),
            "focused control draws the accent ring"
        );
        assert_eq!(
            tag_border(&scene, "toolbar#0"),
            None,
            "non-focused has no ring"
        );
        assert_eq!(tag_border(&scene, "toolbar#4"), None);
    }

    #[test]
    fn r692_no_focus_ring_when_group_unfocused() {
        let pressed = [false; 5];
        let scene = view_toolbar(
            "toolbar",
            &LABELS,
            &pressed,
            &[],
            2,
            false,
            &theme(),
            &ToolbarStyle::m3_default(),
        );
        assert_eq!(
            tag_border(&scene, "toolbar#2"),
            None,
            "no ring when the toolbar does not own focus"
        );
    }

    #[test]
    fn r692_focused_and_pressed_shows_both_fill_and_ring() {
        let t = theme();
        let pressed = [true, false, false, false, false];
        let scene = view_toolbar(
            "toolbar",
            &LABELS,
            &pressed,
            &[],
            0,
            true,
            &t,
            &ToolbarStyle::m3_default(),
        );
        let expected_fill = t
            .resolve(ColorRole::Surface)
            .lerp(t.resolve(ColorRole::Accent), PRESSED_STATE_LAYER);
        assert_eq!(tag_fill(&scene, "toolbar#0"), Some(expected_fill));
        assert_eq!(
            tag_border(&scene, "toolbar#0"),
            Some(Border::new(t.resolve(ColorRole::Accent), 2))
        );
    }
}
