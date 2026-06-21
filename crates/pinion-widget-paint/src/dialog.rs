//! R693 §5.16 §5.40 §5.50 — backend-agnostic modal `Dialog` paint
//! composition.
//!
//! Phase B widget-catalog entry — the confirm / form / alert dialog
//! every pro DCC / IDE / CAD tool ships, and a direct step toward the
//! northern-star "Unreal-class editor self-hosted in pinion" (every
//! editor command that needs confirmation routes through one).
//!
//! ## Chrome, not behaviour
//!
//! This module owns only the *paint* axis: a full-window scrim
//! (backdrop) with a centred elevated panel carrying a title, a message,
//! and an action-button row. The modal *behaviour* — the focus trap,
//! auto-focus on open, restore on close, Escape-to-dismiss — lives in
//! the [`pinion_runtime::FocusManager`] modal-scope substrate
//! (`push_modal_scope` / `pop_modal_scope`) the binding drives through
//! [`pinion_core::modal_scope_request`]. This mirrors the HTML
//! `<dialog>` split: the element provides chrome + modality, the author
//! provides the form controls.
//!
//! ## Author-provided actions
//!
//! The dialog's action buttons (OK / Cancel / …) are **real** focusable
//! [`pinion_core::external::External`] widgets the binding composes and
//! renders (via [`crate::button::view_button`]); [`view_dialog`] only
//! lays the pre-rendered button scenes out in a trailing row. Keeping
//! them as real externals — not sub-tags painted by this module — is
//! what makes the Tab trap visible: each button mirrors its own focus
//! via [`External::on_focus_change`](pinion_core::external::External)
//! and paints its own focus ring, exactly as a multi-field form does
//! (the todomvc multi-tab-stop precedent), so Tab cycling inside the
//! trap moves a real, painted focus ring.
//!
//! ## Input blocking (scrim)
//!
//! The scrim is the overlay's outermost [`Scene::Container`], tagged so
//! the input router resolves backdrop hits to it. Placed **last** in the
//! root child list (paints on top) and sized to the full window, it is
//! the topmost hit target everywhere except over the panel's controls
//! (`Scene::hit_test` descends topmost-last), so background widgets
//! never receive a click while the dialog is up. The scrim tag carries
//! no `External`, so a backdrop click is swallowed — the WAI-ARIA modal
//! default (no light-dismiss; Escape is the dismissal path).
//!
//! ## Future axes (per [[abstraction-needs-second-consumer]])
//!
//! - **Light-dismiss** (backdrop click closes) — opt-in, lands when a
//!   consumer wants it (register the scrim tag as an `External`).
//! - **Elevation shadow** — M3 dialogs sit at elevation level 3; pinion
//!   has no shadow primitive yet, so the panel reads as elevated through
//!   the [`ColorRole::SurfaceContainerHigh`] tier alone.
//! - **Content body** (R788) — landed: [`DialogContent::body`] carries an
//!   author-composed [`Scene`] between the message and the action row (a
//!   form, a list, the own-rendered file browser). Icon / divider remain
//!   additive panel slots.

use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::{Color, Scene};

/// R693 §5.50 — Material 3 modal `Dialog` dimensions + scrim weight.
/// Mirrors the [`crate::toolbar::ToolbarStyle`] / [`crate::menu::MenuStyle`]
/// carrier pattern so the widget catalog presents a uniform `Style`
/// surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogStyle {
    /// Scrim (backdrop) opacity over black, 0–255. Defaults to the
    /// shared [`crate::scrim::M3_SCRIM_ALPHA`]; see that constant for the
    /// M3 token rationale.
    pub scrim_alpha: u8,
    /// Panel width in logical pixels (M3 dialog min-width ≈ 280, max
    /// ≈ 560; 360 is a comfortable confirm-dialog default).
    pub panel_width: u32,
    /// Panel corner radius (M3 dialog ≈ 28 px).
    pub panel_radius: u32,
    /// Panel inner padding (each edge; M3 dialog ≈ 24 px).
    pub panel_padding: u32,
    /// Vertical gap between the title, message, and action row.
    pub panel_gap: u32,
    /// Title font size (M3 `headline-small` ≈ 24 px; 22 = `title-large`).
    pub title_font_px: u32,
    /// Message font size (M3 `body-medium` ≈ 14 px).
    pub message_font_px: u32,
    /// Gap between adjacent action buttons.
    pub action_gap: u32,
    /// Material elevation level the panel casts its drop-shadow at
    /// (R711 §5.50; MD3 dialog = Level 3). `0` = flat.
    pub elevation: u8,
}

impl DialogStyle {
    /// R693 §5.50 — Material 3 modal-dialog defaults. See the struct
    /// docs for the per-field token anchors.
    #[must_use]
    pub const fn m3_default() -> Self {
        Self {
            scrim_alpha: crate::scrim::M3_SCRIM_ALPHA,
            panel_width: 360,
            panel_radius: 28,
            panel_padding: 24,
            panel_gap: 16,
            title_font_px: 22,
            message_font_px: 14,
            action_gap: 8,
            elevation: crate::elevation::DIALOG_LEVEL,
        }
    }

    /// The scrim fill: black at [`Self::scrim_alpha`]. Delegates to the
    /// shared [`crate::scrim::scrim_fill`] so the derivation has one home.
    #[must_use]
    pub const fn scrim_color(self) -> Color {
        crate::scrim::scrim_fill(self.scrim_alpha)
    }
}

impl Default for DialogStyle {
    fn default() -> Self {
        Self::m3_default()
    }
}

/// R693 §5.50 — the dialog panel's content. Groups the title + message
/// (+ the R788 [`body`](Self::body) slot) so [`view_dialog`] stays within
/// the 7-argument convention the other `view_*` substrate fns hold; an
/// empty `&str` / `None` omits that node. Extensible: future panel slots
/// (icon, supporting text) join here.
///
/// Not `Copy`/`Clone`: the optional [`body`](Self::body) owns a [`Scene`]
/// (which is deliberately non-`Clone` — it can own `External` handles), so
/// a `DialogContent` is moved into [`view_dialog`] once.
#[derive(Debug)]
pub struct DialogContent<'a> {
    /// Dialog heading (`headline`). Empty omits the title node.
    pub title: &'a str,
    /// Body text. Empty omits the message node.
    pub message: &'a str,
    /// R788 — optional content body between the message and the action
    /// row: an author-composed [`Scene`] (a form, a list, a file browser
    /// pane). `None` omits the slot, so a plain confirm dialog passes
    /// `body: None` exactly as before. This is the "scrollable content"
    /// panel slot the module docs flagged as a future axis, landed by its
    /// first consumer (R788's modal file-open dialog embeds the
    /// own-rendered file browser here).
    pub body: Option<Scene>,
}

/// R693 §5.16 §5.50 — compose a modal dialog overlay: a full-window
/// scrim centring an elevated panel (title + message + trailing action
/// row).
///
/// # Arguments
///
/// - `scrim_tag` — the backdrop container tag; the router resolves
///   background clicks to it (swallowed — no light-dismiss by default).
/// - `panel_tag` — the panel container tag (queryable via
///   `scene/snapshot`, §2 invariant #7).
/// - `content` — the title + message + optional [`body`](DialogContent::body)
///   ([`DialogContent`]); an empty field / `None` body omits that node.
/// - `actions` — pre-rendered action-button scenes (real focusable
///   [`External`](pinion_core::external::External) widgets), laid out
///   left-to-right in a trailing row.
/// - `viewport` — `(width, height)` window dimensions, so the scrim
///   fills the viewport and the panel centres within it.
/// - `theme` / `style` — palette + [`DialogStyle`] carrier.
///
/// # Returns
///
/// A [`Scene::Container`] tagged `scrim_tag`, absolutely positioned at
/// the origin and sized to the window, centring the panel. Place it
/// **last** in the root child list so it paints over (and hit-tests
/// above) the rest of the scene.
#[must_use]
pub fn view_dialog(
    scrim_tag: &'static str,
    panel_tag: &'static str,
    content: DialogContent<'_>,
    actions: Vec<Scene>,
    viewport: (u32, u32),
    theme: &Theme,
    style: &DialogStyle,
) -> Scene {
    let mut panel_children: Vec<Scene> = Vec::with_capacity(3);
    if !content.title.is_empty() {
        panel_children.push(Scene::Text(TextNode::styled(
            content.title,
            Rect::default(),
            TextStyle::new()
                .with_size_px(style.title_font_px)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )));
    }
    if !content.message.is_empty() {
        panel_children.push(Scene::Text(TextNode::styled(
            content.message,
            Rect::default(),
            TextStyle::new()
                .with_size_px(style.message_font_px)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )));
    }
    // R788 — the optional author-composed body, between the message and the
    // trailing action row (omitted when `None`).
    if let Some(body) = content.body {
        panel_children.push(body);
    }
    panel_children.push(Scene::Container(
        ContainerNode::new(actions).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::End)
                .with_gap(style.action_gap),
        ),
    ));

    let panel = Scene::Container(
        ContainerNode::new(panel_children)
            .with_tag(panel_tag)
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh))
                    .with_corner_radius(style.panel_radius)
                    .with_shadows(crate::elevation::elevation(style.elevation)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start)
                    .with_gap(style.panel_gap)
                    .with_size(Size::auto().with_width(SizeValue::Px(style.panel_width)))
                    .with_padding(Rect::new(
                        style.panel_padding,
                        style.panel_padding,
                        style.panel_padding,
                        style.panel_padding,
                    )),
            ),
    );

    // R703 — shared scrim backdrop; the dialog centres its panel.
    crate::scrim::scrim_backdrop(
        scrim_tag,
        style.scrim_color(),
        viewport,
        FlexDirection::Column,
        AlignItems::Center,
        JustifyContent::Center,
        panel,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;
    use pinion_core::widgets::button::ButtonExternal;

    fn theme() -> Theme {
        Theme::light()
    }

    fn action(tag: &'static str) -> Scene {
        // Stand-in for a real action button: a tagged External the
        // dialog lays out (the binding renders `view_button`, but the
        // chrome only needs a tagged node to position).
        Scene::External(ExternalNode::new(Box::new(ButtonExternal::new())).with_tag(tag))
    }

    fn dialog() -> Scene {
        view_dialog(
            "dialog_scrim",
            "dialog_panel",
            DialogContent {
                title: "Discard changes?",
                message: "Your edits will be lost.",
                body: None,
            },
            vec![action("dialog_cancel"), action("dialog_ok")],
            (520, 320),
            &theme(),
            &DialogStyle::m3_default(),
        )
    }

    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        if let Scene::Container(c) = scene {
            if c.tag.as_deref() == Some(tag) {
                return Some(c);
            }
            for child in &c.children {
                if let Some(found) = find_container(child, tag) {
                    return Some(found);
                }
            }
        }
        None
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

    fn external_tags(scene: &Scene) -> Vec<String> {
        fn walk(scene: &Scene, out: &mut Vec<String>) {
            match scene {
                Scene::External(n) => {
                    if let Some(tag) = &n.tag {
                        out.push(tag.to_string());
                    }
                }
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

    #[test]
    fn r693_dialog_style_m3_defaults() {
        let s = DialogStyle::m3_default();
        assert_eq!(s.scrim_alpha, 0x66);
        assert_eq!(s.panel_width, 360);
        assert_eq!(s.panel_radius, 28);
        assert_eq!(s.panel_padding, 24);
        assert_eq!(s.panel_gap, 16);
        assert_eq!(s.title_font_px, 22);
        assert_eq!(s.message_font_px, 14);
        assert_eq!(s.action_gap, 8);
    }

    #[test]
    fn r693_scrim_color_is_black_at_alpha() {
        let s = DialogStyle::m3_default();
        assert_eq!(s.scrim_color(), Color::rgba(0, 0, 0, 0x66));
    }

    #[test]
    fn r693_scrim_fills_the_window() {
        let scene = dialog();
        let scrim = find_container(&scene, "dialog_scrim").expect("scrim node");
        assert_eq!(
            scrim.layout.size,
            Size::px(520, 320),
            "scrim covers the viewport"
        );
        assert_eq!(
            scrim.layout.absolute_position,
            Some((0, 0)),
            "anchored at origin"
        );
        assert_eq!(scrim.style.fill, Color::rgba(0, 0, 0, 0x66));
    }

    #[test]
    fn r693_panel_centred_and_elevated() {
        let t = theme();
        let scene = dialog();
        let scrim = find_container(&scene, "dialog_scrim").expect("scrim node");
        assert_eq!(scrim.layout.justify_content, JustifyContent::Center);
        assert_eq!(scrim.layout.align_items, AlignItems::Center);
        let panel = find_container(&scene, "dialog_panel").expect("panel node");
        assert_eq!(panel.style.fill, t.resolve(ColorRole::SurfaceContainerHigh));
        assert_eq!(panel.style.corner_radius, 28);
        assert_eq!(panel.layout.size.width, SizeValue::Px(360));
        // R711 — the panel literally casts the MD3 Level-3 drop-shadow.
        assert_eq!(
            panel.style.shadows,
            crate::elevation::elevation(crate::elevation::DIALOG_LEVEL),
            "dialog panel carries the shared MD3 L3 elevation shadow",
        );
    }

    #[test]
    fn r693_title_and_message_present() {
        let scene = dialog();
        let text = all_text(&scene);
        assert!(text.contains(&"Discard changes?".to_string()));
        assert!(text.contains(&"Your edits will be lost.".to_string()));
    }

    #[test]
    fn r693_action_buttons_laid_out_in_order() {
        let scene = dialog();
        assert_eq!(
            external_tags(&scene),
            vec!["dialog_cancel".to_string(), "dialog_ok".to_string()],
            "actions laid out left-to-right in the passed order"
        );
    }

    #[test]
    fn r693_empty_title_omits_node() {
        let scene = view_dialog(
            "dialog_scrim",
            "dialog_panel",
            DialogContent {
                title: "",
                message: "Body only.",
                body: None,
            },
            vec![action("ok")],
            (400, 300),
            &theme(),
            &DialogStyle::m3_default(),
        );
        let text = all_text(&scene);
        assert_eq!(
            text,
            vec!["Body only.".to_string()],
            "no empty title text node"
        );
    }

    #[test]
    fn r788_body_slot_sits_between_message_and_actions() {
        // The author-composed body (here a tagged container) is laid out
        // after the message and before the trailing action row.
        let body = Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                "browser pane",
                Rect::default(),
                TextStyle::new(),
            ))])
            .with_tag("dialog_body"),
        );
        let scene = view_dialog(
            "dialog_scrim",
            "dialog_panel",
            DialogContent {
                title: "Open file",
                message: "Pick one.",
                body: Some(body),
            },
            vec![action("dialog_cancel"), action("dialog_ok")],
            (640, 480),
            &theme(),
            &DialogStyle::m3_default(),
        );
        // The body node is present inside the panel.
        let panel = find_container(&scene, "dialog_panel").expect("panel node");
        let positions: Vec<&str> = panel
            .children
            .iter()
            .map(|c| match c {
                Scene::Text(t) if t.content == "Open file" => "title",
                Scene::Text(t) if t.content == "Pick one." => "message",
                Scene::Container(c) if c.tag.as_deref() == Some("dialog_body") => "body",
                Scene::Container(_) => "actions",
                _ => "other",
            })
            .collect();
        assert_eq!(
            positions,
            vec!["title", "message", "body", "actions"],
            "body slot sits between the message and the action row",
        );
        // The body's own content survives into the tree.
        assert!(
            all_text(&scene).contains(&"browser pane".to_string()),
            "body content rendered"
        );
    }

    #[test]
    fn r788_no_body_keeps_message_then_actions() {
        // `body: None` is the unchanged plain-confirm shape.
        let scene = dialog();
        let panel = find_container(&scene, "dialog_panel").expect("panel node");
        // title, message, action-row only — three children, no body.
        assert_eq!(panel.children.len(), 3, "no body slot when None");
    }
}
