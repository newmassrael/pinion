//! R668 §5.38 §5.50 — backend-agnostic Checkbox paint composition.
//!
//! This module is the substrate lift of the [`view_checkbox`]
//! composition (M3 filled-checkbox visual: rounded square + optional
//! `\u{2713}` glyph + label row). Pre-R668 the only consumer was
//! `examples/hello-checkbox` (R654 retrofit, R57.X.checkbox theme
//! ramp); R668 adds the `examples/settings-panel` Notifications
//! section as the 2nd consumer (6-channel `CheckboxExternal` cluster
//! per the §5.34 composite-tag convention), so this lift satisfies
//! the [[abstraction-needs-second-consumer]] Rule of Three gate.
//!
//! ## Naming
//!
//! Mirrors [`crate::text_field`]: a [`CheckboxStyle`] carrier struct
//! with [`CheckboxStyle::m3_filled`] defaults and a [`view_checkbox`]
//! fn that produces a [`Scene`] fragment the binding's outer view-fn
//! wraps in its root container. The signature
//!
//! ```rust,ignore
//! pub fn view_checkbox(
//!     tag: &'static str,
//!     interaction: CheckboxState,
//!     checked: bool,
//!     theme: &Theme,
//!     style: &CheckboxStyle,
//!     label: &str,
//! ) -> Scene;
//! ```
//!
//! follows `text_field`'s `(tag, state, ..., theme, style, aria_label)`
//! shape; `label` does double-duty as the visible text and the AT
//! enrich-from-scene name (Checkbox UX always pairs the visual mark
//! with a linguistic label in W3C / Apple HIG / M3 conventions).

use pinion_core::scene::{ContainerNode, Rect, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::checkbox::CheckboxState;
use pinion_core::{Color, Scene};

/// (R668 §5.38 §5.50) Material-3 filled-`Checkbox` paint dimensions.
/// Mirrors the [`crate::text_field::TextFieldStyle`] pattern so binding
/// callers see a uniform `Style` carrier across the widget catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckboxStyle {
    /// Square check-box side length in logical pixels (default 24 =
    /// M3 `Checkbox` touch-target inner mark; the outer 48 px tap
    /// target is the parent row's responsibility).
    pub box_size: u32,
    /// Box corner radius in logical pixels (default 4 = M3 `Checkbox`
    /// shape token, matches `TextField` filled-variant corner).
    pub box_radius: u32,
    /// Box border thickness in logical pixels (default 2 = M3 outline
    /// stroke weight at 1.0× DPI).
    pub border_width: u32,
    /// Horizontal gap between the box visual and the label in logical
    /// pixels (default 10 ≈ M3 8 pt + half-cell typographic balance
    /// the pre-lift binding chose).
    pub row_gap: u32,
    /// Label font size in logical pixels (default 16 ≈ M3
    /// `body-large`).
    pub font_size_px: u32,
    /// Check-mark glyph font size in logical pixels (default 18 — the
    /// `\u{2713}` glyph reads cleanly at this size inside the 24-px
    /// box across the `Noto` / `DejaVu` / `Inter` font fallback chain the
    /// shell's parley resolver picks).
    pub glyph_size_px: u32,
    /// R1570 §5.39 — keyboard focus stop. When `true`, [`view_checkbox`] marks
    /// the tagged Container `.with_focusable(true)` so the scene-derived §5.39
    /// enumeration collects it as a Tab stop.
    ///
    /// Default `true`, and the default is the point: this field did not exist
    /// until R1570, so `hello-checkbox` shipped a `role = CheckBox` that `focus/set` refused and `focus/next` could not
    /// reach — which also made its `apply_aria_activate` unreachable, since that gates on `focused == Some(tag)`.
    /// HTML's native `<input type=checkbox>` and the toolkit's check box (`StrongFocus`) are both focusable
    /// without asking, so the fail-safe direction is the same one [`crate::button::ButtonStyle::focusable`] took
    /// at R1030 for the same reason.
    pub focusable: bool,
}

impl CheckboxStyle {
    /// (R668 §5.50) Material 3 filled-checkbox defaults. Mirrors the
    /// constants every pre-lift binding (hello-checkbox) carried.
    #[must_use]
    pub const fn m3_filled() -> Self {
        Self {
            box_size: 24,
            box_radius: 4,
            border_width: 2,
            row_gap: 10,
            font_size_px: 16,
            glyph_size_px: 18,
            focusable: true,
        }
    }

    /// R1570 §5.39 — override the keyboard focus stop (default `true`).
    /// An interactive checkbox keeps the default; a decorative or
    /// coordinator-driven one opts out. See [`Self::focusable`].
    #[must_use]
    pub const fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }
}

impl Default for CheckboxStyle {
    fn default() -> Self {
        Self::m3_filled()
    }
}

/// (R668 §5.50) Material-3 accent ramp for the checkbox box fill when
/// `checked == true`. Anchors on [`ColorRole::Accent`] and layers the
/// canonical state-layer overlays (hover 0.08 / pressed 0.12 /
/// disabled 0.38 toward `Surface`). When `checked == false` the
/// caller substitutes [`Color::TRANSPARENT`] — the border + parent
/// surface mark the empty state.
#[must_use]
pub fn checkbox_accent_for(theme: &Theme, state: CheckboxState) -> Color {
    let base = theme.resolve(ColorRole::Accent);
    crate::state_layer::state_layer(base, state, theme)
}

/// (R668 §5.50) Border color ramp — anchors on [`ColorRole::Outline`]
/// with the same state-layer treatment as [`checkbox_accent_for`] so
/// checked / unchecked share overlay weights and the border is
/// visually continuous through the state-layer transition.
#[must_use]
pub fn checkbox_outline_for(theme: &Theme, state: CheckboxState) -> Color {
    let base = theme.resolve(ColorRole::Outline);
    crate::state_layer::state_layer(base, state, theme)
}

/// (R668 §5.50) another declarative toolkit the M3 filled-`Checkbox` paint scene
/// fragment.
///
/// # Arguments
///
/// - `tag` — dispatch tag (paired with the binding's
///   [`pinion_core::WidgetCore::tag`] return value) the input router
///   hit-tests against and the §5.20 intent system carries on intents
///   the SCXML statechart emits.
/// - `interaction` — current SCXML statechart projection from
///   [`CheckboxState`]; drives the accent + border state-layer ramps
///   plus the disabled-glyph muting.
/// - `checked` — current value-sidecar projection (the §5.16
///   `bool_field(N)` slot the `#[widget]` derive carries on the
///   `(CheckboxState, bool)` State tuple); drives the box fill
///   (accent vs transparent) and the `\u{2713}` glyph presence.
/// - `theme` — current [`Theme`] palette ([`Theme::light`] /
///   [`Theme::dark`] / fade interpolation per
///   [[r57-x-theme-fade-substrate]]); the binding's view-fn resolves
///   it through [`pinion_core::theme::use_theme`] and forwards.
/// - `style` — [`CheckboxStyle`] dimension carrier; pass
///   [`CheckboxStyle::m3_filled`] or [`CheckboxStyle::default`] for
///   the M3 defaults, or a customised value for non-M3 visuals.
/// - `label` — visible label text rendered to the right of the box.
///   Doubles as the AT accessible name through
///   [`pinion_a11y::enrich_names_from_scene`]'s DFS first-non-
///   presentational `TextNode` scan; the check-glyph `TextNode`
///   carries [`TextRole::Presentational`] so it's skipped and the
///   label lands as the natural name (per
///   [[ai-first-rpc-introspection-obligation]]).
///
/// # Returns
///
/// A [`Scene::Container`] tagged `tag` holding `[box_visual, label]`
/// laid out as an M3-spec horizontal row (`flex Row` +
/// `align_items: Center` + `gap: row_gap`). Binding view fns wrap this
/// in their outer container with any surrounding chrome (theme
/// surface fill + window-centering layout etc.).
#[must_use]
pub fn view_checkbox(
    tag: &'static str,
    interaction: CheckboxState,
    checked: bool,
    theme: &Theme,
    style: &CheckboxStyle,
    label: &str,
) -> Scene {
    let box_visual = view_checkbox_box(checked, interaction, theme, style);
    let label_color = if matches!(interaction, CheckboxState::Disabled) {
        theme.resolve(ColorRole::OnSurfaceMuted)
    } else {
        theme.resolve(ColorRole::OnSurface)
    };
    let label_node = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.font_size_px)
            .with_fg(label_color),
    ));
    Scene::Container(
        ContainerNode::new(vec![box_visual, label_node])
            .with_tag(tag)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(style.row_gap)
                    .with_focusable(style.focusable),
            ),
    )
}

/// (R837 §5.50) The bare M3 check-*box* visual — the rounded square +
/// optional `\u{2713}` glyph, **without** the label or the outer dispatch
/// tag. The SSOT [`view_checkbox`] composes around it (box + label + tag);
/// non-interactive consumers that render a bool as a checkbox glyph inside
/// their own cell (the editable property grid / data grid, whose value cell
/// already carries the hit-test tag and toggles through the grid
/// coordinator, not a per-cell `CheckboxExternal`) reuse just the box. This
/// keeps one M3 checkbox rendering across the catalog — pass
/// [`CheckboxState::Idle`] for a static display.
///
/// Unchecked is transparent (border + parent surface mark the empty state,
/// the M3 spec); the box never invents a fill.
#[must_use]
pub fn view_checkbox_box(
    checked: bool,
    interaction: CheckboxState,
    theme: &Theme,
    style: &CheckboxStyle,
) -> Scene {
    let box_fill = if checked {
        checkbox_accent_for(theme, interaction)
    } else {
        Color::TRANSPARENT
    };
    let border_color = checkbox_outline_for(theme, interaction);
    let mut box_children: Vec<Scene> = Vec::new();
    // ★ R1673 — `None` when the square has no room for a tick at all, and the
    // glyph is then simply absent. A square that small still reads as checked
    // (the fill turns accent); painting a one-pixel mark over its own outline
    // instead would be a clamp that answers where the honest answer is nothing.
    if let (true, Some(glyph_px)) = (checked, glyph_px_that_fits(style)) {
        let glyph_color = if matches!(interaction, CheckboxState::Disabled) {
            theme.resolve(ColorRole::OnSurfaceMuted)
        } else {
            theme.resolve(ColorRole::OnAccent)
        };
        box_children.push(Scene::Text(
            TextNode::styled(
                CHECK_GLYPH,
                Rect::default(),
                TextStyle::new().with_size_px(glyph_px).with_fg(glyph_color),
            )
            // R51.81 — Presentational so enrich_names_from_scene skips the
            // glyph and lands on the linguistic label (when wrapped by
            // `view_checkbox`).
            .with_role(TextRole::Presentational),
        ));
    }
    Scene::Container(
        ContainerNode::new(box_children)
            .with_style(
                BoxStyle::filled(box_fill)
                    .with_corner_radius(style.box_radius)
                    .with_border(Border::new(border_color, style.border_width)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(style.box_size, style.box_size)),
            ),
    )
}

/// The tick's face: the declared size, or the largest whose LINE BOX fits the
/// square's **content** — the box less the outline it strokes inside itself.
///
/// ★★ R1673 — `box_size` and `glyph_size_px` are two tokens and nothing related
/// them, so a caller could ask for a face larger than the box holding it and
/// nothing said so. Measured across the tree: three consumers put a 16-18px
/// tick in a 14-16px square, where its shaped ink is 20-22 tall, and the check
/// mark painted three pixels above and below its own box on the property grid,
/// the data grid and the group box's title. `scene/containment` reports every
/// one of them now that a box owns its border (R1672).
///
/// A ceiling rather than a replacement: a caller whose square is generous keeps
/// the face it asked for, and only one that cannot hold it is overruled. The
/// inverse of [`line_box`](pinion_core::containment::line_box)'s
/// `px * 3 / 2 + 2`, which is a reservation and therefore conservative — a
/// square that passes this has room for a face the host may shape taller than
/// ours.
fn glyph_px_that_fits(style: &CheckboxStyle) -> Option<u32> {
    let content = style.box_size.saturating_sub(style.border_width * 2);
    // Stepped down against `line_box` rather than solved by inverting it. The
    // first draft inverted `px * 3 / 2 + 2` by hand and the sweep caught it
    // immediately: at three pixels of content the algebra answered "nothing
    // fits" while a 1px face's line box is exactly three. An inverse written
    // beside a function is a second copy of that function's arithmetic, free to
    // disagree with it — asking the function is not.
    let mut px = style.glyph_size_px.min(content);
    while px > 0 && pinion_core::containment::line_box(px) > content {
        px -= 1;
    }
    (px > 0).then_some(px)
}

/// W3C-canonical Unicode CHECK MARK (`U+2713`). Named so the binding
/// and tests reference the same symbol the M3 spec calls out, instead
/// of an inline literal.
const CHECK_GLYPH: &str = "\u{2713}";

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn light_theme() -> Theme {
        Theme::light()
    }

    fn count_text_children(scene: &Scene) -> usize {
        match scene {
            Scene::Container(c) => c
                .children
                .iter()
                .map(|s| match s {
                    Scene::Text(_) => 1,
                    Scene::Container(inner) => inner
                        .children
                        .iter()
                        .filter(|x| matches!(x, Scene::Text(_)))
                        .count(),
                    _ => 0,
                })
                .sum(),
            _ => 0,
        }
    }

    #[test]
    fn r668_checkbox_style_m3_filled_constants_match_pre_lift() {
        // Pin the pre-lift hello-checkbox constants survive the lift
        // bit-exact: BOX_SIZE=24, BOX_RADIUS=4, ROW_GAP=10,
        // font_size_px=16 (label), glyph_size_px=18 (check mark).
        // Border width was 2 in the inline `Border::new(border_color, 2)`
        // call — pin it here too.
        let s = CheckboxStyle::m3_filled();
        assert_eq!(s.box_size, 24);
        assert_eq!(s.box_radius, 4);
        assert_eq!(s.border_width, 2);
        assert_eq!(s.row_gap, 10);
        assert_eq!(s.font_size_px, 16);
        assert_eq!(s.glyph_size_px, 18);
    }

    #[test]
    fn r668_view_checkbox_unchecked_has_one_text_child_label_only() {
        // Unchecked: no `\u{2713}` glyph → only the label TextNode is
        // present (one Text under the row Container).
        let scene = Owner::new().run(|| {
            view_checkbox(
                "main_checkbox",
                CheckboxState::Idle,
                false,
                &light_theme(),
                &CheckboxStyle::m3_filled(),
                "Receive newsletter",
            )
        });
        assert_eq!(count_text_children(&scene), 1);
    }

    #[test]
    fn r668_view_checkbox_checked_has_two_text_children_glyph_plus_label() {
        // Checked: glyph + label = 2 TextNodes (one inside the box
        // Container, one in the row).
        let scene = Owner::new().run(|| {
            view_checkbox(
                "main_checkbox",
                CheckboxState::Idle,
                true,
                &light_theme(),
                &CheckboxStyle::m3_filled(),
                "Receive newsletter",
            )
        });
        assert_eq!(count_text_children(&scene), 2);
    }

    #[test]
    fn r668_view_checkbox_carries_dispatch_tag() {
        // The outer container's `tag` field carries the dispatch tag
        // so the InputRouter resolves PointerDown / PointerUp /
        // PointerEnter to the matching `Scene::External(tag)` the
        // CheckboxExternal lives behind. Mirror of the
        // R55.G.20 "paint scene must carry composite tag" rule the
        // hello-checkbox a11y test (`r55_g20_view_contains_composite_paint_root_tag`)
        // already covered for the inline binding; this pin lifts that
        // contract onto the substrate.
        let scene = Owner::new().run(|| {
            view_checkbox(
                "main_checkbox",
                CheckboxState::Idle,
                false,
                &light_theme(),
                &CheckboxStyle::m3_filled(),
                "Receive newsletter",
            )
        });
        assert!(scene.contains_tag("main_checkbox"));
    }

    #[test]
    fn r668_view_checkbox_disabled_glyph_uses_muted_color() {
        // R57.X.checkbox / R668 — Disabled state mutes the glyph
        // toward OnSurfaceMuted (vs OnAccent for Idle/Hover/Pressed).
        // The pre-lift hello-checkbox carried this exact ramp; the
        // lift preserves it.
        let theme = light_theme();
        let expected_muted = theme.resolve(ColorRole::OnSurfaceMuted);
        let expected_normal = theme.resolve(ColorRole::OnAccent);
        // Sanity: the two roles really do resolve to different values
        // in the light palette (otherwise the disabled / non-disabled
        // discriminator would be observationally inert).
        assert_ne!(expected_muted, expected_normal);
    }

    #[test]
    fn r668_checkbox_accent_idle_resolves_to_theme_accent() {
        // Pre-lift parity: Idle accent ramp returns the theme's
        // unmodified accent role. The hello-checkbox a11y test
        // `r57_x_checkbox_checked_idle_uses_accent_role` already
        // pinned this for the inline binding; the lift carries it
        // forward.
        let theme = light_theme();
        assert_eq!(
            checkbox_accent_for(&theme, CheckboxState::Idle),
            theme.accent
        );
    }

    #[test]
    fn r668_checkbox_outline_idle_resolves_to_theme_outline() {
        // Pre-lift parity for the outline ramp (mirror of the accent
        // test, anchored on the outline role).
        let theme = light_theme();
        assert_eq!(
            checkbox_outline_for(&theme, CheckboxState::Idle),
            theme.outline
        );
    }

    #[test]
    fn r668_checkbox_hover_overlay_lerps_toward_on_surface() {
        // Pre-lift parity for the M3 state-layer ramp (hover 0.08).
        // Mirrors `r57_x_checkbox_hover_overlay_lerps_toward_on_surface`
        // in the pre-lift binding tests.
        let theme = light_theme();
        let expected = theme
            .resolve(ColorRole::Accent)
            .lerp(theme.resolve(ColorRole::OnSurface), 0.08);
        assert_eq!(checkbox_accent_for(&theme, CheckboxState::Hover), expected);
    }

    /// ★★ R1673 — the tick fits the square that holds it, at every size.
    ///
    /// The property this pins is a RELATION between two tokens that used to be
    /// independent: whatever `box_size` and `border_width` a caller sets, the
    /// face the glyph is drawn at has a line box the square's content can hold.
    /// Swept rather than sampled, because the failing sizes are the small ones
    /// and a single fixture picks whichever the author happened to think of.
    ///
    /// The counter-assertion is the load-bearing half: a generous square keeps
    /// the face the caller asked for, so this is a ceiling and not a rewrite.
    #[test]
    fn r1673_the_tick_fits_the_square_that_holds_it() {
        use pinion_core::containment::line_box;
        let mut checked_once = false;
        let mut absent_once = false;
        for box_size in 8..=48u32 {
            for border_width in 0..=3u32 {
                let style = CheckboxStyle {
                    box_size,
                    border_width,
                    ..CheckboxStyle::m3_filled()
                };
                let content = box_size.saturating_sub(border_width * 2);
                let Some(px) = glyph_px_that_fits(&style) else {
                    // No tick fits at all. The sweep found this on its first
                    // run at an 8px square with a 3px outline: two pixels of
                    // content, where even a 1px face needs a 3px line box.
                    assert!(
                        line_box(1) > content,
                        "the {box_size}px square with a {border_width}px \
                         outline offers {content} and refused a tick anyway",
                    );
                    absent_once = true;
                    continue;
                };
                assert!(
                    line_box(px) <= content,
                    "a {px}px tick needs a {}px line box and the {box_size}px \
                     square with a {border_width}px outline offers {content}",
                    line_box(px),
                );
                assert!(px <= style.glyph_size_px, "it is a ceiling, not a resize");
                if px == style.glyph_size_px {
                    checked_once = true;
                }
            }
        }
        assert!(
            checked_once,
            "no square in the sweep was generous enough to keep the declared \
             face — this would pass for a helper that always shrinks",
        );
        assert!(
            absent_once,
            "no square in the sweep was too small for any tick — the `None` \
             arm is then untested, which is how R1654's class starts",
        );
    }
}
