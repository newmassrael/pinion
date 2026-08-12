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

use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, PathStyle, Size,
    Stroke, StrokeCap, TextOverflow, TextStyle,
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
///   presentational `TextNode` scan. ★ R1674 — and it is now the only
///   `TextNode` a checkbox has: the tick became a stroked
///   [`Scene::Path`], so the name derivation reaches the label
///   because there is nothing else to reach, rather than because the
///   glyph was marked presentational for it to step over (per
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
            .with_fg(label_color)
            // ★ R1674 — what happens when the label does not fit is STATED.
            // Found by the crate's frame gate on its first run: a label longer
            // than its share of the row painted 110px past the checkbox at a
            // 180px window, over the outline and over whatever was beside it,
            // and the default `Visible` is what let it. R1654 built these arms
            // for exactly this and no painter here had taken one.
            .with_overflow(TextOverflow::Ellipsis),
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
    if let (true, Some(tick)) = (checked, tick_path(style)) {
        let tick_color = if matches!(interaction, CheckboxState::Disabled) {
            theme.resolve(ColorRole::OnSurfaceMuted)
        } else {
            theme.resolve(ColorRole::OnAccent)
        };
        box_children.push(Scene::Path(
            PathNode::new(
                Rect::default(),
                tick.commands,
                PathStyle::stroked(
                    Stroke::new(tick_color, tick.stroke_px).with_cap(StrokeCap::Round),
                ),
            )
            .with_layout(LayoutStyle::new().with_size(Size::px(tick.side, tick.side))),
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

/// The check mark, as a stroked polyline sized to the square that holds it.
///
/// ★★★ R1674 — this used to be a text glyph, and the change is the payment of a
/// debt R1673 opened while fixing a different one.
///
/// R1673 found three consumers putting a 16-18px tick face in a 14-16px square,
/// where the shaped ink is 20-22 tall and painted three pixels above and below
/// its own outline. It repaired that by deriving the face from what the square
/// could hold — stepping down against
/// [`line_box`](pinion_core::containment::line_box) — and the derivation cost
/// more than it had to: `line_box` is a *reservation*, generous by ~35% because
/// it must cover a face's ascent, descent and leading. A check mark needs none
/// of those. It is not a letter sitting on a baseline; it is a shape. The M3
/// default tick went 18px -> 12px, and every checkbox in the tree shrank.
///
/// A path has no ascent. The mark is drawn inside the square's content box, so
/// it fits by construction with no derivation at all, and three things follow:
///
/// * the size token means what it says again — `glyph_size_px` is the tick's
///   side, capped only by a square genuinely too small to hold it;
/// * the commonest glyph in the catalog stops depending on the host's fonts,
///   which is a direct contribution to [[zero-flake-policy]] — the previous
///   form's ink was whatever face the machine happened to resolve `U+2713` to;
/// * the reference draws its check mark as a path too, which is why this class
///   of defect does not exist there.
///
/// The stroke is inset by half its width on every side. A round join reaches
/// exactly half the stroke width past the polyline (R1669 measured the miter
/// case going further, `half / sin(theta/2)`, which is why the cap and join are
/// not left to the rasterizer's default here), so the painted ink is inside the
/// node's own rectangle rather than merely near it.
fn tick_path(style: &CheckboxStyle) -> Option<Tick> {
    let content = style.box_size.saturating_sub(style.border_width * 2);
    let side = style.glyph_size_px.min(content);
    // A stroke needs a pixel of width and a pixel either side of it to be a
    // mark rather than a smudge; below that the honest answer is no tick, the
    // same `None` R1673 chose and for the same reason — a square that small
    // still reads as checked, because its fill turns accent.
    if side < 3 {
        return None;
    }
    let stroke_px = (side / 8).max(1);
    let half = stroke_px.div_ceil(2);
    let usable = side.saturating_sub(half * 2);
    #[allow(
        clippy::cast_precision_loss,
        reason = "a checkbox side is tens of pixels"
    )]
    let (origin, span) = (half as f32, usable as f32);
    // Proportions of the M3 check mark: the pen drops to just past halfway,
    // turns at a third of the width, and rises to the top right.
    let at = |fx: f32, fy: f32| PathPoint::new(origin + span * fx, origin + span * fy);
    Some(Tick {
        side,
        stroke_px,
        commands: vec![
            PathCommand::MoveTo(at(0.0, 0.52)),
            PathCommand::LineTo(at(0.34, 1.0)),
            PathCommand::LineTo(at(1.0, 0.0)),
        ],
    })
}

/// A check mark ready to be placed: its square, its pen width, and its stroke.
struct Tick {
    /// The side of the square the polyline is drawn in.
    side: u32,
    /// The pen width, derived from `side`.
    stroke_px: u32,
    /// The polyline, in the node's own coordinates.
    commands: Vec<PathCommand>,
}

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

    /// ★ R1674 — checked carries the SAME one text node as unchecked: the
    /// label.
    ///
    /// This test was `..._has_two_text_children_glyph_plus_label` from R668
    /// until the tick became a path. The count going 2 -> 1 is the change, and
    /// it is asserted here rather than deleted because the number is the
    /// evidence that the check mark left the text channel: the box's ink no
    /// longer depends on which face the host resolves `U+2713` to. The
    /// remaining one is the linguistic label, which is text because it is
    /// words.
    #[test]
    fn r668_view_checkbox_checked_has_one_text_child_the_label() {
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
        assert_eq!(count_text_children(&scene), 1);
        // And the tick is still THERE — a count that fell because the mark
        // vanished would read identically to one that fell because the mark
        // moved channels.
        let Scene::Container(row) = &scene else {
            panic!("the row is a container")
        };
        let Scene::Container(square) = &row.children[0] else {
            panic!("the square is a container")
        };
        assert!(
            matches!(square.children.as_slice(), [Scene::Path(_)]),
            "the checked square holds the tick, as a path",
        );
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

    /// ★★ R1673/R1674 — the tick fits the square that holds it, at every size,
    /// and the STROKED ink fits too.
    ///
    /// R1673 pinned this as a relation between two tokens that used to be
    /// independent: whatever `box_size` and `border_width` a caller sets, the
    /// tick has to fit the square's content. R1674 changed *how* — the mark is
    /// a path now, not a glyph — and this is the same property restated against
    /// the new answer, plus the one a polyline adds: a stroke has WIDTH, and a
    /// polyline that fits can still paint outside its node once the pen is
    /// taken into account. R1669 measured that exact class on a sparkline.
    ///
    /// Swept rather than sampled, because the failing sizes are the small ones
    /// and a single fixture picks whichever the author happened to think of.
    ///
    /// The counter-assertions are the load-bearing half: some square in the
    /// sweep must keep the face the caller asked for (or this would pass for a
    /// helper that always shrinks) and some must be too small for any tick (or
    /// the `None` arm is untested, which is how R1654's class starts).
    #[test]
    fn r1673_the_tick_fits_the_square_that_holds_it() {
        let mut kept_declared = false;
        let mut absent_once = false;
        for box_size in 8..=48u32 {
            for border_width in 0..=3u32 {
                let style = CheckboxStyle {
                    box_size,
                    border_width,
                    ..CheckboxStyle::m3_filled()
                };
                let content = box_size.saturating_sub(border_width * 2);
                let Some(tick) = tick_path(&style) else {
                    assert!(
                        content < 3,
                        "the {box_size}px square with a {border_width}px \
                         outline offers {content} and refused a tick anyway",
                    );
                    absent_once = true;
                    continue;
                };
                assert!(
                    tick.side <= content,
                    "a {}px tick in a {box_size}px square with a \
                     {border_width}px outline, which offers {content}",
                    tick.side,
                );
                assert!(
                    tick.side <= style.glyph_size_px,
                    "it is a ceiling, not a resize",
                );
                if tick.side == style.glyph_size_px {
                    kept_declared = true;
                }

                // ★ The pen, which a glyph did not have. Every command point,
                // grown by half the stroke width in each direction — the reach
                // of a round join and a round cap — must be inside the node.
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a checkbox side is tens of pixels"
                )]
                let (half, side) = (tick.stroke_px.div_ceil(2) as f32, tick.side as f32);
                for command in &tick.commands {
                    let point = match command {
                        PathCommand::MoveTo(p) | PathCommand::LineTo(p) => *p,
                        other => panic!("the tick is a polyline, not {other:?}"),
                    };
                    assert!(
                        point.x - half >= 0.0
                            && point.y - half >= 0.0
                            && point.x + half <= side
                            && point.y + half <= side,
                        "a {}px pen at {point:?} reaches outside the {side}px tick",
                        tick.stroke_px,
                    );
                }
            }
        }
        assert!(
            kept_declared,
            "no square in the sweep was generous enough to keep the declared \
             tick — this would pass for a helper that always shrinks",
        );
        assert!(
            absent_once,
            "no square in the sweep was too small for any tick — the `None` \
             arm is then untested, which is how R1654's class starts",
        );
    }

    /// ★★ R1674 — the tick is a PATH, and the checked box carries no text.
    ///
    /// Stated as an assertion because it is the whole of what the change buys
    /// and it is invisible otherwise: a check mark drawn from the host's fonts
    /// renders as whatever face resolves `U+2713`, which is a rendering that
    /// varies by machine in the most-used widget in the catalog. The two
    /// text-child counts below are the pre-R1674 tests restated — a checked box
    /// used to have two text children and now has one, because the tick left
    /// the text channel entirely.
    #[test]
    fn r1674_the_tick_is_a_path_and_carries_no_font() {
        let theme = light_theme();
        let style = CheckboxStyle::m3_filled();
        let checked = view_checkbox_box(true, CheckboxState::Idle, &theme, &style);
        let Scene::Container(c) = &checked else {
            panic!("the box is a container")
        };
        assert_eq!(c.children.len(), 1, "a checked box holds exactly the tick");
        let Scene::Path(tick) = &c.children[0] else {
            panic!("the tick is a path, not {:?}", c.children[0])
        };
        assert!(
            tick.style.stroke.is_some_and(|s| s.width > 0),
            "a stroked mark, so nothing depends on a fill rule either",
        );
        assert_eq!(
            tick.commands.len(),
            3,
            "move, down-stroke, up-stroke — the mark a person reads as a check",
        );
        let mut text_nodes = 0usize;
        checked.for_each_node(&mut |visit| {
            if matches!(visit.node, Scene::Text(_)) {
                text_nodes += 1;
            }
        });
        assert_eq!(
            text_nodes, 0,
            "no text node anywhere in a checked box: the tick no longer asks \
             the host which font it has",
        );

        let unchecked = view_checkbox_box(false, CheckboxState::Idle, &theme, &style);
        let Scene::Container(c) = &unchecked else {
            panic!("the box is a container")
        };
        assert!(c.children.is_empty(), "an unchecked box holds nothing");
    }

    /// ★★ R1674 — nothing this painter draws lands on the square it strokes,
    /// at either size and in every posture.
    ///
    /// The crate gate ([`crate::frame_gate`]); see there for why every bordered
    /// painter owes it and why the population is parsed rather than listed.
    #[test]
    fn r1674_the_checkbox_keeps_its_ink_inside_its_square() {
        let theme = light_theme();
        for checked in [false, true] {
            for interaction in [
                CheckboxState::Idle,
                CheckboxState::Hover,
                CheckboxState::Pressed,
                CheckboxState::Disabled,
            ] {
                crate::frame_gate::assert_frame_contained(
                    &format!("checkbox checked={checked} {interaction:?}"),
                    &mut |_w, _h| {
                        view_checkbox(
                            "chk",
                            interaction,
                            checked,
                            &theme,
                            &CheckboxStyle::default(),
                            "Enable telemetry",
                        )
                    },
                );
            }
        }
    }
}
