// R691 §5.16 — example bindings tolerate looser doc-markdown lints
// than substrate crates; the architectural narrative carries many
// proper-noun identifiers (MenuBar, WAI-ARIA, MenuBarExternal, …).
#![allow(clippy::doc_markdown)]

//! `hello-menu` — R691 §5.16 §5.40 §5.50 first consumer of the
//! `pinion_widget_paint::menu` substrate.
//!
//! ## Why this binding exists
//!
//! Phase B widget-catalog entry. A classic editor menubar
//! (`File` / `Edit` / `View`) with floating command dropdowns — the
//! primitive every pro DCC / IDE / CAD tool ships, and a direct step
//! toward the northern-star "Unreal-class editor self-hosted in
//! pinion".
//!
//! ## Command-class, not selection
//!
//! A menu item is a one-shot command (WAI-ARIA `menuitem`): it carries
//! no `aria-selected` / `aria-checked`, and activating it fires a
//! command + dismisses the menu. So the binding drives selection-free
//! state through a command-class
//! [`MenuBarExternal`](pinion_core::widgets::menu::MenuBarExternal) —
//! **not** the [`RadioGroupExternal`](pinion_core::widgets::radio_group::RadioGroupExternal)
//! the R690 Tabs widget reuses (a tab strip *is* "select 1 of N", a
//! menubar is not). Item activation emits a `"command"` intent whose
//! payload is `"<menu>.<item>"`; the runtime prefixes the scene tag so
//! AI clients see `menu.command`.
//!
//! ## Keyboard model (WAI-ARIA 1.2 §3.5 Menubar)
//!
//! The whole keyboard model lives in `MenuBarExternal::apply_key`
//! (reached via `invoke("key", <name>)`), so the human keyboard, the
//! AT action layer, and an RPC headless client converge on one
//! statechart (§2 invariant #2). When the menubar owns focus:
//!
//! - **closed** — Arrow Left/Right move between titles (wrap),
//!   Home/End jump to first/last, Arrow Down / Enter / Space open the
//!   focused menu (first item highlighted).
//! - **open** — Arrow Up/Down move the active item (wrap), Home/End
//!   jump, Arrow Right/Left switch to the adjacent menu, Enter/Space
//!   activate the active item (command + close), Escape closes.
//!
//! ## Dismiss paths
//!
//! Escape, re-clicking the open title, activating an item, and (R715)
//! **clicking outside** the open dropdown all dismiss it. The
//! click-outside path is the shared light-dismiss barrier
//! ([`dismiss_barrier`]): a transparent, non-modal click-catcher painted
//! over the area below the title strip and tagged `menu#barrier`, so its
//! outside `PointerUp` routes to the one [`MenuBarExternal`] (R51.42),
//! which closes the menu. The barrier leaves the title strip clickable
//! above it, so a click on a sibling title switches menus instead of
//! dismissing. ComboBox (R714) is the barrier's other consumer.
//!
//! ## AI clients
//!
//! `query("open")` / `query("active")` / `query("bar_focus")` read the
//! structure; `invoke("send", "t<m>:PointerUp")` toggles a title,
//! `invoke("send", "i<i>:PointerUp")` activates an item, and
//! `invoke("key", "<W3CKeyName>")` drives the full keyboard model. The
//! dropdown is queryable via `scene/snapshot` on the `menu_dropdown`
//! node (§2 invariant #7).

use pinion_a11y::{
    menu_item_nodes, AccessAction, AccessFocus, AccessNode, AccessState, AriaRole, MenuItemCell,
    WidgetA11y,
};
use pinion_core::external::{External, ExternalIntrospect, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::menu::{MenuBarExternal, MenuItem};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::menu::{
    composite_item_tag, composite_title_tag, view_menu_bar, view_menu_dropdown, MenuItemView,
    MenuStyle,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloMenuRenderer, HelloMenuRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 320;
const THEME_TAG: &str = "app";
const BAR_TAG: &str = "menu";
const DROPDOWN_TAG: &str = "menu_dropdown";
/// R715 §5.16 — the transparent click-outside dismiss barrier. The
/// R51.42 composite form `<bar>#barrier` routes its outside `PointerUp`
/// to the one [`MenuBarExternal`] (tag `menu`), whose barrier arm closes
/// the open dropdown.
const BARRIER_TAG: &str = "menu#barrier";
const BODY_FONT_PX: u32 = 15;
const FOOTER_FONT_PX: u32 = 12;

/// Top-level menu titles. Index `m` becomes the title tagged
/// `menu#t<m>`; the label is each title's WAI-ARIA name-from-contents.
const MENU_TITLES: [&str; 3] = ["File", "Edit", "View"];

/// R805 — one dropdown entry in the example's menu model: the WAI-ARIA
/// item taxonomy this binding exercises. The `View` menu carries
/// checkbox toggles, `Edit` a disabled command + a separator.
#[derive(Clone, Copy)]
enum Item {
    /// A one-shot command (`menuitem`).
    Command(&'static str),
    /// A toggle (`menuitemcheckbox`) with its boot checked state.
    Checkbox(&'static str, bool),
    /// A non-interactive divider (`separator`).
    Separator,
    /// A disabled command (greyed, skipped, inert).
    DisabledCommand(&'static str),
}

impl Item {
    /// Project to the `MenuBarExternal` model entry (boot state).
    fn to_model(self) -> MenuItem {
        match self {
            Item::Command(_) => MenuItem::command(),
            Item::Checkbox(_, checked) => MenuItem::checkbox(checked),
            Item::Separator => MenuItem::separator(),
            Item::DisabledCommand(_) => MenuItem::command().disabled(),
        }
    }

    /// Project to the paint descriptor, taking the *live* checked state
    /// (read back from the External) for a checkbox.
    fn to_view(self, checked: bool) -> MenuItemView<'static> {
        match self {
            Item::Command(l) => MenuItemView::command(l),
            Item::Checkbox(l, _) => MenuItemView::checkbox(l, checked),
            Item::Separator => MenuItemView::separator(),
            Item::DisabledCommand(l) => MenuItemView::command(l).disabled(),
        }
    }

    const fn is_separator(self) -> bool {
        matches!(self, Item::Separator)
    }
}

/// Per-menu item lists. `MENUS[m][i]` is item `i` of menu `m` (tag
/// `menu#i<i>` while open). `File` is all commands; `Edit` shows a
/// disabled `Redo` + a separator before the clipboard group; `View`
/// shows two checkbox toggles + a separator before the zoom group.
const MENUS: [&[Item]; 3] = [
    &[
        Item::Command("New"),
        Item::Command("Open"),
        Item::Command("Save"),
        Item::Command("Quit"),
    ],
    &[
        Item::Command("Undo"),
        Item::DisabledCommand("Redo"),
        Item::Separator,
        Item::Command("Cut"),
        Item::Command("Copy"),
        Item::Command("Paste"),
    ],
    &[
        Item::Checkbox("Show Grid", true),
        Item::Checkbox("Show Rulers", false),
        Item::Separator,
        Item::Command("Zoom In"),
        Item::Command("Zoom Out"),
        Item::Command("Reset Zoom"),
    ],
];

/// Cached projection of the menubar: the open dropdown, the active
/// (highlighted) item within it, the keyboard-focused top-level title,
/// and the live checked state of the open menu's items. `Copy` so the
/// shell hands the snapshot into the paint closure without lifetime
/// gymnastics.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
struct MenuState {
    /// Open dropdown menu index, or `None` when closed.
    open: Option<usize>,
    /// Highlighted item within the open menu (WAI-ARIA active
    /// descendant), or `None`.
    active: Option<usize>,
    /// Keyboard-focused top-level title (the cursor that Arrow
    /// Left/Right moves while closed).
    bar_focus: usize,
    /// Live checked state of the open menu's items as a bitmap (bit `i`
    /// = item `i` checked; `0` when closed). Read back from the External
    /// so the paint reflects runtime toggles. A `u64` keeps `MenuState`
    /// `Copy` without an arbitrary item cap (64 ≫ any real menu; the
    /// R805.1 audit replaced a magic-8 `[bool; N]` that silently
    /// truncated).
    checked: u64,
}

impl MenuState {
    /// Whether the open menu's item `i` is checked (bit `i` of the
    /// snapshot bitmap). The single read accessor the paint + a11y share.
    const fn item_checked(self, i: usize) -> bool {
        i < 64 && (self.checked >> i) & 1 != 0
    }
}

/// view-fn (§6.3): pure sync mapping `MenuState -> Scene`. The
/// [`view_menu_bar`] substrate paints the title strip (tagged `menu`,
/// per-title `menu#t<m>`); when a menu is open, [`view_menu_dropdown`]
/// paints an absolutely-positioned floating command list (tagged
/// `menu_dropdown`, per-item `menu#i<i>`) placed **last** so it paints
/// over the body content, behind which the R715 [`dismiss_barrier`]
/// (tagged `menu#barrier`) catches the click-outside that closes it.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: MenuState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = MenuStyle::m3_default();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let menubar = view_menu_bar(BAR_TAG, &MENU_TITLES, state.open, &theme, &style);

    let body = Scene::Text(TextNode::styled(
        "Open a menu, hover or arrow to a command, then click or press Enter.",
        Rect::default(),
        TextStyle::new()
            .with_size_px(BODY_FONT_PX)
            .with_fg(on_surface),
    ));
    let footer = Scene::Text(TextNode::styled(
        "\u{2190}/\u{2192} title   \u{2193} open   \u{2191}/\u{2193} item   Esc close",
        Rect::default(),
        TextStyle::new()
            .with_size_px(FOOTER_FONT_PX)
            .with_fg(on_surface_muted),
    ));
    let content = Scene::Container(
        ContainerNode::new(vec![body, footer]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_justify(JustifyContent::Start)
                .with_flex_grow(1.0)
                .with_gap(10)
                .with_padding(Rect::new(16, 16, 16, 16)),
        ),
    );

    let mut children = vec![menubar, content];
    if let Some(m) = state.open {
        // R715 §5.16 — transparent click-outside dismiss barrier over
        // the area *below* the title strip (origin `(0, bar_height)`),
        // so the titles stay clickable above it and a sibling-title
        // click switches menus instead of dismissing. Pushed before the
        // dropdown so the dropdown hit-tests above the barrier; an
        // outside click lands on `menu#barrier` -> close.
        children.push(dismiss_barrier(
            BARRIER_TAG,
            (0, style.bar_height),
            (WIN_W, WIN_H - style.bar_height),
        ));
        // Placed last so the absolutely-positioned dropdown paints
        // over the content container (and the barrier) below the bar.
        // Each model item projects to a paint descriptor, with checkbox
        // rows carrying the *live* checked state read back from the
        // External (so a runtime toggle repaints the checkmark).
        let items: Vec<MenuItemView> = MENUS[m]
            .iter()
            .enumerate()
            .map(|(i, it)| it.to_view(state.item_checked(i)))
            .collect();
        children.push(view_menu_dropdown(
            BAR_TAG,
            DROPDOWN_TAG,
            m,
            &items,
            state.active,
            &theme,
            &style,
        ));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Stretch),
            ),
    )
}

struct MenuView;

impl WidgetCore for MenuView {
    type State = MenuState;
    // Every state change flows through `apply_key` (`invoke("key", …)`)
    // and the InputRouter composite `send` dispatch, so no
    // keybinding-channel events flow through `event_name`. `()` keeps
    // the trait's `Copy` bound satisfied without an unused variant
    // (mirrors hello-tabs / the radio-group precedent).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let menus: Vec<Vec<MenuItem>> = MENUS
            .iter()
            .map(|items| items.iter().map(|it| it.to_model()).collect())
            .collect();
        Box::new(MenuBarExternal::with_items(menus))
    }

    fn tag() -> &'static str {
        BAR_TAG
    }

    fn read_state(scene: &Scene) -> MenuState {
        let mut out = MenuState::default();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        out.open = query_index(intro, "open");
        out.active = query_index(intro, "active");
        out.bar_focus = match intro.query("bar_focus") {
            Some(IntrospectValue::Int(i)) => usize::try_from(i).unwrap_or(0),
            _ => 0,
        };
        // R805 — snapshot the open menu's live checked state so the paint
        // reflects runtime toggles (closed -> all false by default).
        if let Some(m) = out.open {
            let count = MENUS[m].len().min(64);
            for i in 0..count {
                if matches!(
                    intro.query(&format!("checked.{m}.{i}")),
                    Some(IntrospectValue::Bool(true))
                ) {
                    out.checked |= 1 << i;
                }
            }
        }
        out
    }

    fn view(state: MenuState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-menu (R691 §5.16 §5.40 §5.50)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        // Single source of truth for the keymap lives in the External's
        // `apply_key` (WAI-ARIA §3.5 menubar model), reached below.
        None
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // The whole WAI-ARIA §3.5 keyboard model lives in the External; the
        // menubar is one tab stop, so forwarding the focused key to its
        // `"key"` wire is the shared command-menu pattern (R772.1; unhandled
        // keys like Tab fall through).
        Self::forward_key_to_external(scene, focused, key)
    }

    fn fmt_state_log(state: &MenuState) -> String {
        let open = state
            .open
            .map_or_else(|| "-".to_string(), |m| m.to_string());
        match state.active {
            Some(a) => format!("open={open} active={a} bar_focus={}", state.bar_focus),
            None => format!("open={open} bar_focus={}", state.bar_focus),
        }
    }
}

impl WidgetA11y for MenuView {
    /// R691 §5.40 — WAI-ARIA 1.2 §3.5 menubar / menu / menuitem tree.
    /// Always emits the [`AriaRole::MenuBar`] root + one
    /// [`AriaRole::MenuItem`] per top-level title. When a menu is open
    /// it additionally emits an [`AriaRole::Menu`] dropdown (claimed as
    /// a child of the open title) + one [`AriaRole::MenuItem`] per
    /// command, the active one carrying the AT focus flag.
    ///
    /// Per-title and per-item names are derived by
    /// `enrich_names_from_scene` from each label `TextNode`; the
    /// container names (the menubar + the open menu) have no single
    /// paint equivalent so they stay explicit overrides.
    fn access_node(state: &MenuState, focused: Option<&str>) -> Vec<AccessNode> {
        let group_focused = focused == Some(<Self as WidgetCore>::tag());
        let n = MENU_TITLES.len();
        let mut nodes: Vec<AccessNode> = Vec::new();

        let mut menubar = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::MenuBar)
            .with_name("Application menu");
        for m in 0..n {
            menubar = menubar.with_child(composite_title_tag(BAR_TAG, m));
        }
        nodes.push(menubar);

        let title_setsize = u32::try_from(n).unwrap_or(u32::MAX);
        for m in 0..n {
            // A closed-and-focused title shows the keyboard cursor; an
            // open title delegates the AT focus to the active item.
            let title_focused = group_focused && state.open.is_none() && state.bar_focus == m;
            let mut title = AccessNode::new(composite_title_tag(BAR_TAG, m), AriaRole::MenuItem)
                .with_state(AccessState {
                    focused: title_focused,
                    ..AccessState::default()
                })
                .with_position_in_set(u32::try_from(m + 1).unwrap_or(u32::MAX))
                .with_size_of_set(title_setsize);
            if state.open == Some(m) {
                // The open title owns its dropdown (WAI-ARIA submenu
                // ownership); aria-haspopup / aria-expanded are future
                // additive axes.
                title = title.with_child(DROPDOWN_TAG);
            }
            nodes.push(title);
        }

        if let Some(m) = state.open {
            // R817 §5.40 — the open dropdown is the lifted `menu_item_nodes`
            // unit (the MenuBar + title MenuItems above stay bespoke: the
            // two-level menu-bar skeleton diverges from the one-level
            // context menu, so only the inner menu+items is shared). R805 —
            // separators are presentational, dropped before building the
            // slice so the items' posinset / setsize count real items only;
            // a checkbox item maps to `checked: Some(..)` (= menuitemcheckbox
            // + aria-checked), a disabled command to `disabled`.
            let items = MENUS[m];
            let menu_items: Vec<usize> =
                (0..items.len()).filter(|&i| !items[i].is_separator()).collect();
            let tags: Vec<String> =
                menu_items.iter().map(|&i| composite_item_tag(BAR_TAG, i)).collect();
            let cells: Vec<MenuItemCell<'_>> = menu_items
                .iter()
                .enumerate()
                .map(|(pos, &i)| MenuItemCell {
                    tag: &tags[pos],
                    label: None,
                    checked: matches!(items[i], Item::Checkbox(..))
                        .then(|| state.item_checked(i)),
                    disabled: matches!(items[i], Item::DisabledCommand(..)),
                    focused: group_focused && state.active == Some(i),
                    ..MenuItemCell::default()
                })
                .collect();
            nodes.extend(menu_item_nodes(DROPDOWN_TAG, MENU_TITLES[m], &cells));
        }
        nodes
    }

    /// R51.71 §5.40 — composite focus model. While the menubar owns
    /// focus, the parent (`menu`) stays focused and the active
    /// descendant points at the open menu's active item (or the open
    /// title when nothing is highlighted yet), falling back to the
    /// keyboard-focused title when closed.
    fn access_focus_target(state: &MenuState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused != Some(<Self as WidgetCore>::tag()) {
            return focused.map(AccessFocus::atomic);
        }
        let descendant = match state.open {
            Some(m) => match state.active {
                Some(i) => composite_item_tag(BAR_TAG, i),
                None => composite_title_tag(BAR_TAG, m),
            },
            None => composite_title_tag(BAR_TAG, state.bar_focus),
        };
        Some(AccessFocus::composite(<Self as WidgetCore>::tag(), descendant))
    }

    /// R51.70 §5.40 — composite child action dispatch. `Click` /
    /// `Default` on a title (`menu#t<m>`) toggles it; on an item
    /// (`menu#i<i>`) activates it (command + close). `Focus` records
    /// the AT-side cursor (`bar_focus` for a title, `active` for an
    /// item) without committing.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Some((kind, idx)) = parse_sub_tag(sub_tag) else {
            return false;
        };
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match action {
            AccessAction::Click | AccessAction::Default => {
                // PointerUp is the activation edge for both a title
                // (toggle) and an item (command + close).
                let _ = intro.invoke(
                    "send",
                    IntrospectValue::Text(format!("{kind}{idx}:PointerUp")),
                );
                true
            }
            AccessAction::Focus => {
                let slot = if kind == 't' { "bar_focus" } else { "active" };
                if let Ok(i) = i64::try_from(idx) {
                    let _ = intro.intervene(slot, IntrospectValue::Int(i));
                }
                true
            }
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for MenuView {
    type Renderer = HelloMenuRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// Read an optional-index introspect slot (`open` / `active`):
/// `Int(i)` → `Some(i)`, `Null` (or anything else) → `None`.
fn query_index(intro: &dyn ExternalIntrospect, path: &str) -> Option<usize> {
    match intro.query(path) {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    }
}

/// Parse a composite sub-tag (`t<m>` title / `i<i>` item) into its
/// kind char + index. Returns `None` for an unknown kind or a
/// non-numeric index.
fn parse_sub_tag(sub_tag: &str) -> Option<(char, usize)> {
    let mut chars = sub_tag.chars();
    let kind = chars.next()?;
    if kind != 't' && kind != 'i' {
        return None;
    }
    let idx: usize = chars.as_str().parse().ok()?;
    Some((kind, idx))
}

fn main() {
    pinion_shell::run::<MenuView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn closed() -> MenuState {
        MenuState::default()
    }

    fn open_menu(m: usize, active: Option<usize>) -> MenuState {
        MenuState {
            open: Some(m),
            active,
            bar_focus: m,
            ..MenuState::default()
        }
    }

    #[test]
    fn r691_closed_emits_menubar_plus_titles() {
        let nodes = MenuView::access_node(&closed(), None);
        assert_eq!(nodes.len(), 1 + MENU_TITLES.len());
        assert_eq!(nodes[0].role, AriaRole::MenuBar);
        assert_eq!(nodes[0].name.as_deref(), Some("Application menu"));
        for m in 0..MENU_TITLES.len() {
            assert_eq!(nodes[m + 1].role, AriaRole::MenuItem);
        }
    }

    #[test]
    fn r691_menubar_lists_all_titles_in_order() {
        let nodes = MenuView::access_node(&closed(), None);
        assert_eq!(nodes[0].children.len(), MENU_TITLES.len());
        for m in 0..MENU_TITLES.len() {
            assert_eq!(nodes[0].children[m], format!("menu#t{m}"));
        }
    }

    #[test]
    fn r691_open_emits_menu_and_items() {
        let nodes = MenuView::access_node(&open_menu(1, Some(2)), None);
        // 1 menubar + 3 titles + 1 menu + 5 edit items.
        assert_eq!(nodes.len(), 1 + 3 + 1 + 5);
        let menu = nodes
            .iter()
            .find(|n| n.role == AriaRole::Menu)
            .expect("open menu emits a Menu node");
        assert_eq!(menu.tag, DROPDOWN_TAG);
        assert_eq!(menu.name.as_deref(), Some("Edit"));
        assert_eq!(menu.children.len(), 5);
    }

    #[test]
    fn r691_open_title_owns_dropdown() {
        let nodes = MenuView::access_node(&open_menu(0, None), None);
        let title0 = nodes
            .iter()
            .find(|n| n.tag == "menu#t0")
            .expect("title 0 node");
        assert!(title0.children.iter().any(|c| c == DROPDOWN_TAG));
    }

    #[test]
    fn r691_items_carry_posinset_setsize() {
        // R805 — View = [checkbox, checkbox, separator, command, command,
        // command]. The separator is omitted from the AT item tree, so the
        // 5 remaining items carry posinset 1..=5 / setsize 5 (both the
        // `MenuItemCheckbox` and `MenuItem` roles count as menu items).
        let nodes = MenuView::access_node(&open_menu(2, None), None);
        let items: Vec<&AccessNode> = nodes
            .iter()
            .filter(|n| {
                n.tag.starts_with("menu#i")
                    && matches!(n.role, AriaRole::MenuItem | AriaRole::MenuItemCheckbox)
            })
            .collect();
        assert_eq!(items.len(), 5);
        for (pos, item) in items.iter().enumerate() {
            assert_eq!(item.position_in_set, Some(u32::try_from(pos + 1).unwrap()));
            assert_eq!(item.size_of_set, Some(5));
        }
    }

    #[test]
    fn r805_view_checkbox_items_carry_role_and_aria_checked() {
        let mut state = open_menu(2, None);
        state.checked |= 1; // bit 0 = Show Grid checked, Show Rulers (1) not.
        let nodes = MenuView::access_node(&state, None);
        let grid = nodes.iter().find(|n| n.tag == "menu#i0").unwrap();
        assert_eq!(grid.role, AriaRole::MenuItemCheckbox);
        assert_eq!(grid.state.checked, Some(true));
        let rulers = nodes.iter().find(|n| n.tag == "menu#i1").unwrap();
        assert_eq!(rulers.role, AriaRole::MenuItemCheckbox);
        assert_eq!(rulers.state.checked, Some(false));
    }

    #[test]
    fn r805_disabled_command_carries_aria_disabled() {
        // Edit item 1 = DisabledCommand("Redo"); item 0 = Command("Undo").
        let nodes = MenuView::access_node(&open_menu(1, None), None);
        let redo = nodes.iter().find(|n| n.tag == "menu#i1").unwrap();
        assert_eq!(redo.role, AriaRole::MenuItem);
        assert!(redo.state.disabled, "disabled command carries aria-disabled");
        let undo = nodes.iter().find(|n| n.tag == "menu#i0").unwrap();
        assert!(!undo.state.disabled, "enabled command is not disabled");
    }

    #[test]
    fn r805_separator_omitted_from_item_tree() {
        // Edit item 2 = Separator -> no AccessNode, not a Menu child.
        let nodes = MenuView::access_node(&open_menu(1, None), None);
        assert!(nodes.iter().all(|n| n.tag != "menu#i2"), "separator has no AT node");
        let menu = nodes.iter().find(|n| n.role == AriaRole::Menu).unwrap();
        assert_eq!(menu.children.len(), 5, "5 menu items, separator excluded");
        assert!(menu.children.iter().all(|c| c != "menu#i2"));
    }

    #[test]
    fn r691_focused_closed_title_marks_bar_focus() {
        let state = MenuState {
            open: None,
            active: None,
            bar_focus: 2,
            ..MenuState::default()
        };
        let nodes = MenuView::access_node(&state, Some("menu"));
        assert!(nodes[3].state.focused, "bar_focus title 2 is focused");
        assert!(!nodes[1].state.focused);
    }

    #[test]
    fn r691_focused_open_marks_active_item() {
        let nodes = MenuView::access_node(&open_menu(0, Some(1)), Some("menu"));
        let item1 = nodes
            .iter()
            .find(|n| n.tag == "menu#i1")
            .expect("item 1 node");
        assert!(item1.state.focused);
        // No closed-title focus while open.
        assert!(!nodes[1].state.focused);
    }

    #[test]
    fn r691_titles_enriched_from_paint() {
        let state = closed();
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let mut nodes = MenuView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[1].name.as_deref(), Some("File"));
        assert_eq!(nodes[2].name.as_deref(), Some("Edit"));
        assert_eq!(nodes[3].name.as_deref(), Some("View"));
    }

    #[test]
    fn r691_items_enriched_from_paint_when_open() {
        let state = open_menu(0, None);
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let mut nodes = MenuView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        let item0 = nodes.iter().find(|n| n.tag == "menu#i0").unwrap();
        assert_eq!(item0.name.as_deref(), Some("New"));
    }

    #[test]
    fn r691_focus_target_closed_points_to_bar_focus_title() {
        let state = MenuState {
            open: None,
            active: None,
            bar_focus: 1,
            ..MenuState::default()
        };
        let target = MenuView::access_focus_target(&state, Some("menu"))
            .expect("focused menubar returns Some");
        assert_eq!(target.focus_tag, "menu");
        assert_eq!(target.active_descendant.as_deref(), Some("menu#t1"));
    }

    #[test]
    fn r691_focus_target_open_points_to_active_item() {
        let target = MenuView::access_focus_target(&open_menu(0, Some(2)), Some("menu"))
            .expect("focused menubar returns Some");
        assert_eq!(target.active_descendant.as_deref(), Some("menu#i2"));
    }

    #[test]
    fn r691_focus_target_none_when_unfocused() {
        assert!(MenuView::access_focus_target(&closed(), None).is_none());
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn scene() -> Scene {
        let menus: Vec<Vec<MenuItem>> = MENUS
            .iter()
            .map(|items| items.iter().map(|it| it.to_model()).collect())
            .collect();
        Scene::External(
            ExternalNode::new(Box::new(MenuBarExternal::with_items(menus))).with_tag(BAR_TAG),
        )
    }

    fn open_of(scene: &Scene) -> Option<i64> {
        let Scene::External(node) = scene else {
            return None;
        };
        match node.handle.introspect()?.query("open") {
            Some(IntrospectValue::Int(i)) => Some(i),
            _ => None,
        }
    }

    fn press(s: &mut Scene, key: &str) -> bool {
        MenuView::apply_key(s, Some(BAR_TAG), key, pinion_core::Modifiers::empty())
    }

    #[test]
    fn r691_arrow_down_opens_focused_menu() {
        let mut s = scene();
        assert!(press(&mut s, "ArrowDown"));
        assert_eq!(open_of(&s), Some(0));
    }

    #[test]
    fn r691_escape_closes() {
        let mut s = scene();
        press(&mut s, "ArrowDown");
        assert!(press(&mut s, "Escape"));
        assert_eq!(open_of(&s), None);
    }

    #[test]
    fn r691_arrow_right_switches_open_menu() {
        let mut s = scene();
        press(&mut s, "ArrowDown"); // open menu 0
        press(&mut s, "ArrowRight"); // -> menu 1
        assert_eq!(open_of(&s), Some(1));
    }

    #[test]
    fn r691_unfocused_bar_swallows_keys() {
        let mut s = scene();
        assert!(!MenuView::apply_key(
            &mut s,
            Some("other"),
            "ArrowDown",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(open_of(&s), None);
    }

    #[test]
    fn r691_tab_is_not_handled() {
        let mut s = scene();
        assert!(!press(&mut s, "Tab"));
    }

    #[test]
    fn r691_access_child_invoke_click_title_opens() {
        let mut s = scene();
        assert!(MenuView::access_child_invoke(
            &mut s,
            BAR_TAG,
            "t1",
            AccessAction::Click
        ));
        assert_eq!(open_of(&s), Some(1));
    }

    #[test]
    fn r691_access_child_invoke_click_item_activates() {
        let mut s = scene();
        // Open menu 0 first.
        MenuView::access_child_invoke(&mut s, BAR_TAG, "t0", AccessAction::Click);
        assert_eq!(open_of(&s), Some(0));
        // Click item 2 -> command + close.
        assert!(MenuView::access_child_invoke(
            &mut s,
            BAR_TAG,
            "i2",
            AccessAction::Click
        ));
        assert_eq!(open_of(&s), None);
        let Scene::External(node) = &mut s else {
            panic!("external");
        };
        let mut got = Vec::new();
        node.handle.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tag_str(), "command");
        assert_eq!(got[0].payload, IntrospectValue::Text("0.2".to_string()));
    }

    #[test]
    fn r691_access_child_invoke_bad_sub_tag_declines() {
        let mut s = scene();
        assert!(!MenuView::access_child_invoke(
            &mut s,
            BAR_TAG,
            "x9",
            AccessAction::Click
        ));
    }

    #[test]
    fn r691_view_contains_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<MenuView>(
            MenuState::default(),
            &Frame::default(),
        );
    }

    #[test]
    fn r715_closed_view_omits_dismiss_barrier() {
        let closed = MenuState::default();
        let scene = pinion_core::Owner::new().run(|| view(closed, &Frame::new()));
        assert!(
            !scene.contains_tag(BARRIER_TAG),
            "closed: no click-outside barrier painted"
        );
        assert!(!scene.contains_tag(DROPDOWN_TAG), "closed: no dropdown painted");
    }

    #[test]
    fn r715_open_view_paints_dismiss_barrier_below_titles() {
        let open = MenuState {
            open: Some(0),
            active: None,
            bar_focus: 0,
            ..MenuState::default()
        };
        let scene = pinion_core::Owner::new().run(|| view(open, &Frame::new()));
        assert!(
            scene.contains_tag(BARRIER_TAG),
            "open: transparent click-outside barrier painted"
        );
        assert!(scene.contains_tag(DROPDOWN_TAG), "open: dropdown painted above barrier");
        // The barrier leaves the title strip clickable: it is anchored at
        // y = bar_height, not the window origin.
        let bar_h = MenuStyle::m3_default().bar_height;
        let Scene::Container(root) = &scene else {
            panic!("root is a container");
        };
        let barrier = root
            .children
            .iter()
            .find_map(|c| match c {
                Scene::Container(n) if n.tag.as_deref() == Some(BARRIER_TAG) => Some(n),
                _ => None,
            })
            .expect("barrier present when open");
        assert_eq!(
            barrier.layout.absolute_position,
            Some((0, bar_h)),
            "barrier starts below the title strip so titles stay clickable"
        );
    }
}
