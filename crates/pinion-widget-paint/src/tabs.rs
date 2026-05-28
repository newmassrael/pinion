//! R690 §5.16 §5.40 §5.50 — backend-agnostic `Tabs` paint composition.
//!
//! Phase B widget-catalog entry — the DCC / IDE / settings-dialog
//! tabbed-panel primitive. The `tabs` substrate produces a horizontal
//! tab-strip [`Scene`] from a flat label slice + the active index, so
//! the consuming binding wires hit-test + WAI-ARIA tab/tablist/tabpanel
//! semantics with one composite-tag axis (`{tag}#{index}`).
//!
//! ## Selection model is reused, not reinvented
//!
//! "Select 1 of N" is exactly [`RadioGroup`] semantics, so R690
//! reuses [`pinion_core::widgets::radio_group::RadioGroupExternal`]
//! for the selection statechart rather than authoring a new SCXML
//! machine (per [[abstraction-needs-second-consumer]] — the
//! settings-panel nav-rail set the same precedent). This module owns
//! only the *paint* axis; the binding owns the External + keyboard +
//! a11y walker. The single visible difference from a radio group at
//! the AT surface is the role triple (`tablist` / `tab` / `tabpanel`)
//! and the horizontal Arrow-Left/Right roving model.
//!
//! ## Naming
//!
//! Mirrors [`crate::tree_view`]: a [`TabsStyle`] carrier struct with
//! [`TabsStyle::m3_default`] defaults and a [`view_tabs`] fn producing
//! a [`Scene`] fragment. Each tab is tagged via [`composite_tab_tag`]
//! (`{strip_tag}#{index}`) — the same [[multi-external-substrate-extra-externals-pattern]]
//! shape the input router splits at `#`.
//!
//! ## Future axes (per [[abstraction-needs-second-consumer]])
//!
//! - **Scrollable / closeable / draggable tabs** (IDE editor tabs).
//!   Deferred until a real editor consumer surfaces the need.
//! - **`aria-controls` tab→panel addressing**. R690 renders a single
//!   panel for the active tab so the relationship is structurally
//!   implicit; multi-panel addressing lands additively when a
//!   consumer needs it.
//! - **Manual-activation focus state-layer**. R690's first consumer
//!   (`hello-tabs`) uses *automatic activation* (Arrow selects), so
//!   the focused tab and the selected tab coincide and the active
//!   indicator already marks both. A *manual-activation* consumer
//!   (Arrow moves a cursor, Enter commits) diverges focus from
//!   selection and would add a `focused_index` param + focus
//!   state-layer here — mirroring [`crate::tree_view::view_tree_focused`]
//!   — once that 2nd consumer materialises.
//! - **Mouse hover/press M3 state-layers per tab**, added when a
//!   consumer reads the per-tab interaction state.
//!
//! [`RadioGroup`]: pinion_core::widgets::radio_group::RadioGroup

use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::{Color, Scene};

/// R690 §5.50 — Material 3 `Tabs` strip dimensions. Mirrors the
/// [`crate::tree_view::TreeViewStyle`] carrier pattern so the widget
/// catalog presents a uniform `Style` surface.
///
/// Defaults track the M3 Primary-tabs spec: strip height 48 px
/// (touch target + WCAG 2.5.5); active-indicator bar 3 px thick with
/// a 3 px corner radius (the rounded M3 indicator); label font 14 px
/// (M3 `title-small`); 16 px horizontal padding per tab; 0 px gap
/// between tabs (M3 fixed tabs are contiguous, sharing one baseline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabsStyle {
    /// Strip height in logical pixels. M3 token = 48 px.
    pub tab_height: u32,
    /// Active-indicator bar thickness in logical pixels. M3 token =
    /// 3 px. The bar sits flush along the bottom edge of the active
    /// tab; inactive tabs reserve the same band painted transparent
    /// so labels stay vertically aligned across the strip.
    pub indicator_thickness: u32,
    /// Corner radius of the active-indicator bar. M3 rounds the
    /// indicator's top edge; a radius equal to the thickness reads as
    /// the canonical pill cap.
    pub indicator_radius: u32,
    /// Label font size in logical pixels. M3 `title-small` ≈ 14 px.
    pub font_size_px: u32,
    /// Horizontal padding inside each tab (leading + trailing), in
    /// logical pixels. M3 token = 16 px.
    pub tab_padding: u32,
    /// Gap between adjacent tabs in logical pixels. M3 fixed tabs are
    /// contiguous (0 px); a binding can widen this for a segmented
    /// look.
    pub tab_gap: u32,
}

impl TabsStyle {
    /// R690 §5.50 — Material 3 `Tabs` defaults (Primary tabs). See the
    /// struct docs for the per-field token anchors.
    #[must_use]
    pub const fn m3_default() -> Self {
        Self {
            tab_height: 48,
            indicator_thickness: 3,
            indicator_radius: 3,
            font_size_px: 14,
            tab_padding: 16,
            tab_gap: 0,
        }
    }
}

impl Default for TabsStyle {
    fn default() -> Self {
        Self::m3_default()
    }
}

/// R690 §5.16 §5.50 — compose a tab's composite tag — the
/// [[multi-external-substrate-extra-externals-pattern]] form
/// `{strip_tag}#{index}`. The input router splits this at `#` and
/// rewrites cursor hits on tab `i` into
/// `invoke("send", "<i>:<EventName>")` against the shared
/// [`RadioGroupExternal`](pinion_core::widgets::radio_group::RadioGroupExternal).
/// Public so the binding's a11y walker stamps the same `NodeId` the
/// hit-test target uses.
#[must_use]
pub fn composite_tab_tag(strip_tag: &str, index: usize) -> String {
    format!("{strip_tag}#{index}")
}

/// R690 §5.16 §5.50 — horizontal tab-strip paint.
///
/// # Arguments
///
/// - `tag` — strip container tag; the input router hit-tests this as
///   the `TabList` scope, and per-tab tags are the composite form
///   `{tag}#{index}` ([`composite_tab_tag`]).
/// - `labels` — one visible label per tab; index `i` becomes the tab
///   tagged `{tag}#{i}`. The label is also the tab's AT accessible
///   name (WAI-ARIA name-from-contents).
/// - `selected_index` — the active tab. `Some(i)` paints tab `i` with
///   the [`ColorRole::Accent`] label + active-indicator bar; every
///   other tab paints muted with a transparent indicator band.
///   `None` (no active tab) paints every tab muted.
/// - `theme` — current [`Theme`] palette.
/// - `style` — [`TabsStyle`] carrier; pass [`TabsStyle::m3_default`].
///
/// # Returns
///
/// A [`Scene::Container`] tagged `tag` laying out one tab container
/// per label left-to-right (`flex Row`, cross-axis `Stretch` so every
/// tab fills the strip height). Each tab is a `flex Column` of a
/// grow-to-fill label region above a fixed-height indicator bar.
#[must_use]
pub fn view_tabs(
    tag: &'static str,
    labels: &[&str],
    selected_index: Option<usize>,
    theme: &Theme,
    style: &TabsStyle,
) -> Scene {
    let mut tabs: Vec<Scene> = Vec::with_capacity(labels.len());
    for (i, label) in labels.iter().enumerate() {
        tabs.push(build_tab(tag, i, label, selected_index == Some(i), theme, style));
    }
    Scene::Container(
        ContainerNode::new(tabs)
            .with_tag(tag)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start)
                    .with_gap(style.tab_gap)
                    .with_size(Size::auto().with_height(SizeValue::Px(style.tab_height))),
            ),
    )
}

/// Compose one tab: a grow-to-fill centered label region above a
/// fixed-height active-indicator bar.
fn build_tab(
    strip_tag: &'static str,
    index: usize,
    label: &str,
    is_selected: bool,
    theme: &Theme,
    style: &TabsStyle,
) -> Scene {
    let label_color = if is_selected {
        theme.resolve(ColorRole::Accent)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    let indicator_color = if is_selected {
        theme.resolve(ColorRole::Accent)
    } else {
        Color::TRANSPARENT
    };

    let label_node = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.font_size_px)
            .with_fg(label_color),
    ));
    // Label region grows (`flex_grow = 1.0`) to fill the strip height
    // above the fixed indicator bar, centering the label vertically.
    let label_row = Scene::Container(
        ContainerNode::new(vec![label_node]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_flex_grow(1.0)
                .with_padding(Rect::new(style.tab_padding, 0, style.tab_padding, 0)),
        ),
    );
    // Indicator bar: width Auto so the tab Column's `AlignItems::Stretch`
    // extends it across the full tab width; height fixed at the M3
    // indicator thickness.
    let indicator = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(indicator_color).with_corner_radius(style.indicator_radius),
        )
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::auto().with_height(SizeValue::Px(style.indicator_thickness))),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![label_row, indicator])
            .with_tag(composite_tab_tag(strip_tag, index))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light_theme() -> Theme {
        Theme::light()
    }

    /// Walk `scene` collecting every tag on `Scene::Container` nodes.
    fn collect_tags(scene: &Scene) -> Vec<String> {
        let mut out = Vec::new();
        collect_tags_inner(scene, &mut out);
        out
    }

    fn collect_tags_inner(scene: &Scene, out: &mut Vec<String>) {
        if let Scene::Container(c) = scene {
            if let Some(tag) = &c.tag {
                out.push(tag.to_string());
            }
            for child in &c.children {
                collect_tags_inner(child, out);
            }
        }
    }

    fn count_tab_children(scene: &Scene) -> usize {
        match scene {
            Scene::Container(c) => c
                .children
                .iter()
                .filter(|s| matches!(s, Scene::Container(_)))
                .count(),
            _ => 0,
        }
    }

    /// Find the fill colour of the indicator [`Scene::Box`] inside a
    /// tab container (the only `Scene::Box` in a tab subtree).
    fn indicator_fill(tab: &Scene) -> Option<Color> {
        if let Scene::Box(b) = tab {
            return Some(b.style.fill);
        }
        if let Scene::Container(c) = tab {
            for child in &c.children {
                if let Some(found) = indicator_fill(child) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// Find the first label [`Scene::Text`] foreground colour in a tab.
    fn label_fg(tab: &Scene) -> Option<Color> {
        if let Scene::Text(t) = tab {
            return Some(t.style.fg_color);
        }
        if let Scene::Container(c) = tab {
            for child in &c.children {
                if let Some(found) = label_fg(child) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn tabs() -> [&'static str; 3] {
        ["General", "Appearance", "Advanced"]
    }

    #[test]
    fn r690_tabs_style_m3_default_constants() {
        let s = TabsStyle::m3_default();
        assert_eq!(s.tab_height, 48);
        assert_eq!(s.indicator_thickness, 3);
        assert_eq!(s.indicator_radius, 3);
        assert_eq!(s.font_size_px, 14);
        assert_eq!(s.tab_padding, 16);
        assert_eq!(s.tab_gap, 0);
    }

    #[test]
    fn r690_empty_labels_produces_root_only() {
        let scene = view_tabs("tabs", &[], Some(0), &light_theme(), &TabsStyle::m3_default());
        assert_eq!(count_tab_children(&scene), 0);
        assert_eq!(collect_tags(&scene), vec!["tabs".to_string()]);
    }

    #[test]
    fn r690_n_labels_produce_n_tabs_with_composite_tags() {
        let labels = tabs();
        let scene = view_tabs(
            "tabs",
            &labels,
            Some(0),
            &light_theme(),
            &TabsStyle::m3_default(),
        );
        assert_eq!(count_tab_children(&scene), 3);
        assert_eq!(
            collect_tags(&scene),
            vec![
                "tabs".to_string(),
                "tabs#0".to_string(),
                "tabs#1".to_string(),
                "tabs#2".to_string(),
            ]
        );
    }

    #[test]
    fn r690_composite_tab_tag_helper_matches_emit() {
        assert_eq!(composite_tab_tag("tabs", 2), "tabs#2");
        assert_eq!(composite_tab_tag("editor", 0), "editor#0");
    }

    #[test]
    fn r690_selected_tab_carries_accent_indicator_others_transparent() {
        let labels = tabs();
        let theme = light_theme();
        let scene = view_tabs("tabs", &labels, Some(1), &theme, &TabsStyle::m3_default());
        let Scene::Container(c) = &scene else {
            panic!("expected Container root");
        };
        let accent = theme.resolve(ColorRole::Accent);
        assert_eq!(indicator_fill(&c.children[0]), Some(Color::TRANSPARENT));
        assert_eq!(indicator_fill(&c.children[1]), Some(accent));
        assert_eq!(indicator_fill(&c.children[2]), Some(Color::TRANSPARENT));
    }

    #[test]
    fn r690_no_selection_paints_every_indicator_transparent() {
        let labels = tabs();
        let scene = view_tabs("tabs", &labels, None, &light_theme(), &TabsStyle::m3_default());
        let Scene::Container(c) = &scene else {
            panic!("expected Container root");
        };
        for child in &c.children {
            assert_eq!(indicator_fill(child), Some(Color::TRANSPARENT));
        }
    }

    #[test]
    fn r690_selected_tab_label_is_accent_inactive_is_muted() {
        let labels = tabs();
        let theme = light_theme();
        let scene = view_tabs("tabs", &labels, Some(2), &theme, &TabsStyle::m3_default());
        let Scene::Container(c) = &scene else {
            panic!("expected Container root");
        };
        assert_eq!(label_fg(&c.children[2]), Some(theme.resolve(ColorRole::Accent)));
        assert_eq!(
            label_fg(&c.children[0]),
            Some(theme.resolve(ColorRole::OnSurfaceMuted))
        );
    }

    #[test]
    fn r690_label_is_visible_text() {
        let labels = tabs();
        let scene = view_tabs("tabs", &labels, Some(0), &light_theme(), &TabsStyle::m3_default());
        let collected = {
            fn collect_text(scene: &Scene, out: &mut Vec<String>) {
                match scene {
                    Scene::Text(t) => out.push(t.content.clone()),
                    Scene::Container(c) => {
                        for child in &c.children {
                            collect_text(child, out);
                        }
                    }
                    _ => {}
                }
            }
            let mut out = Vec::new();
            collect_text(&scene, &mut out);
            out
        };
        assert_eq!(collected, vec!["General", "Appearance", "Advanced"]);
    }
}
