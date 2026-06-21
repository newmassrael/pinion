// R985 §5.16 §5.38 §5.40 — cascading submenus demo. Proper-noun
// identifiers (MenuBar, WAI-ARIA, MenuBarExternal, …) are intentional.
#![allow(clippy::doc_markdown)]

//! # hello-menu-nested — cascading submenus (R985)
//!
//! The R985 follow-on to `hello-menu`: a command-class menubar whose
//! dropdowns nest. `File > Open Recent > Older` cascades two submenu
//! levels deep; `View > Appearance` one. A submenu parent row paints a
//! trailing chevron and, in the AT tree, carries `aria-haspopup="menu"` +
//! `aria-expanded` and (when open) owns the nested [`AriaRole::Menu`].
//!
//! Everything drives the one
//! [`MenuBarExternal`](pinion_core::widgets::menu::MenuBarExternal):
//!
//! * `invoke("send", "t<m>:PointerUp")` toggles a top title.
//! * `invoke("send", "i<path>:PointerUp")` activates an item by its
//!   descent path relative to the open dropdown — `i1` is the top item 1,
//!   `i1.2` is item 2 of submenu 1; activating a submenu *opens* it, a
//!   leaf fires the `menu.command` intent (payload the full absolute path).
//! * `invoke("key", "<W3CKeyName>")` drives the WAI-ARIA §3.16 cascade
//!   keyboard model (Arrow Right opens a submenu / moves to the next top
//!   menu on a leaf; Arrow Left / Escape closes one submenu level).
//!
//! The open cascade is queryable via `scene/snapshot` on the per-level
//! `menu_dropdown` / `menu_sub<d>` nodes and `scene/access` for the AT
//! tree (§2 invariants #2 / #7).

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, AccessState, AriaRole, MenuItemCell, SubmenuCell,
    WidgetA11y, menu_item_nodes,
};
use pinion_core::external::{External, ExternalIntrospect, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::menu::{MenuBarExternal, MenuItem, parse_path, path_text};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::menu::{
    MenuItemView, MenuLevel, MenuStyle, composite_item_path_tag, composite_title_tag,
    view_menu_bar, view_menu_cascade,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloMenuNestedRenderer, HelloMenuNestedRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 360;
const THEME_TAG: &str = "app";
const BAR_TAG: &str = "menu";
/// R715 §5.16 — the transparent click-outside dismiss barrier; its outside
/// `PointerUp` routes (R51.42) to the one `MenuBarExternal`, closing the
/// whole cascade.
const BARRIER_TAG: &str = "menu#barrier";
const BODY_FONT_PX: u32 = 15;
const FOOTER_FONT_PX: u32 = 12;
/// R985 — the maximum cascade depth this *demo binding's* `MenuState`
/// snapshot carries (top dropdown + nested submenus). The core `MenuBar` is
/// uncapped (`open_path: Vec<usize>`); this cap exists only because
/// [`pinion_core::WidgetCore::State`] is `Copy`, so the snapshot needs a
/// fixed-size collection. R985.1 (session-review): set well past any
/// conceivable real menu nesting (16 ≫ the demo's 2 deep, and no real menu
/// nests anywhere near this) so the `read_state` cap is never reached in
/// practice — mirroring the R805.1 `u64`-bitmap "≫ any real menu" rationale
/// rather than a tight magic cap that could silently truncate live data.
const MAX_OPEN_DEPTH: usize = 16;

/// R985 — per-open-level container tags (index `d` is the popup at cascade
/// depth `d`): level 0 is the top dropdown, deeper levels its submenus.
/// The a11y `Menu` nodes + snapshot anchors. Sized one past the demo's
/// deepest chain so a future deeper menu only widens this array.
const LEVEL_TAGS: [&str; 4] = ["menu_dropdown", "menu_sub1", "menu_sub2", "menu_sub3"];

/// Top-level menu titles; index `m` becomes the title tagged `menu#t<m>`.
const MENU_TITLES: [&str; 3] = ["File", "Edit", "View"];

/// R985 — one entry in the example's recursive menu model. A
/// [`Self::Submenu`] carries a nested `&'static [Item]` list so the whole
/// tree is a compile-time constant.
#[derive(Clone, Copy)]
enum Item {
    /// A one-shot command (`menuitem`).
    Command(&'static str),
    /// A toggle (`menuitemcheckbox`) with its boot checked state.
    Checkbox(&'static str, bool),
    /// A non-interactive divider (`separator`).
    Separator,
    /// A nested submenu (`menuitem` + `aria-haspopup="menu"`).
    Submenu(&'static str, &'static [Item]),
}

impl Item {
    /// Project to the `MenuBarExternal` model entry (boot state), recursing
    /// into nested submenus.
    fn to_model(self) -> MenuItem {
        match self {
            Item::Command(_) => MenuItem::command(),
            Item::Checkbox(_, checked) => MenuItem::checkbox(checked),
            Item::Separator => MenuItem::separator(),
            Item::Submenu(_, kids) => {
                MenuItem::submenu(kids.iter().map(|it| it.to_model()).collect())
            }
        }
    }

    /// Project to the paint descriptor; `checked` is the live read-back
    /// state for a checkbox row.
    fn to_view(self, checked: bool) -> MenuItemView<'static> {
        match self {
            Item::Command(l) => MenuItemView::command(l),
            Item::Checkbox(l, _) => MenuItemView::checkbox(l, checked),
            Item::Separator => MenuItemView::separator(),
            Item::Submenu(l, _) => MenuItemView::submenu(l),
        }
    }

    /// The label (empty for a separator) — the a11y submenu-name source.
    const fn label(self) -> &'static str {
        match self {
            Item::Command(l) | Item::Checkbox(l, _) | Item::Submenu(l, _) => l,
            Item::Separator => "",
        }
    }

    const fn is_separator(self) -> bool {
        matches!(self, Item::Separator)
    }

    const fn is_submenu(self) -> bool {
        matches!(self, Item::Submenu(..))
    }

    const fn is_checkbox(self) -> bool {
        matches!(self, Item::Checkbox(..))
    }
}

/// `File`'s deepest level (`Open Recent > Older`).
const FILE_OLDER: &[Item] = &[Item::Command("2024.log"), Item::Command("2023.log")];
/// `File`'s `Open Recent` submenu — two recent files + a nested `Older`.
const FILE_RECENT: &[Item] = &[
    Item::Command("report.txt"),
    Item::Command("notes.md"),
    Item::Submenu("Older", FILE_OLDER),
];
/// `View`'s `Appearance` submenu.
const VIEW_APPEARANCE: &[Item] = &[Item::Command("Light"), Item::Command("Dark")];

/// Per-menu item lists. `File` nests two levels (`Open Recent > Older`);
/// `View` carries a top-level checkbox + an `Appearance` submenu; `Edit`
/// stays flat.
const MENUS: [&[Item]; 3] = [
    &[
        Item::Command("New"),
        Item::Submenu("Open Recent", FILE_RECENT),
        Item::Separator,
        Item::Command("Save"),
    ],
    &[Item::Command("Undo"), Item::Command("Redo")],
    &[
        Item::Checkbox("Show Grid", true),
        Item::Submenu("Appearance", VIEW_APPEARANCE),
    ],
];

/// Cached projection of the cascading menubar. `Copy` so the shell hands
/// the snapshot into the paint closure without lifetime gymnastics; the
/// open submenu descent is a fixed-cap array (`MAX_OPEN_DEPTH`) so the
/// struct stays `Copy` without an allocation.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
struct MenuState {
    /// Open top dropdown index, or `None` when closed.
    open: Option<usize>,
    /// The open submenu item indices below the top dropdown, descending.
    open_path: [usize; MAX_OPEN_DEPTH],
    /// How many entries of `open_path` are live.
    open_depth: usize,
    /// Highlighted item within the *deepest* open menu, or `None`.
    active: Option<usize>,
    /// Keyboard-focused top-level title (cursor while closed).
    bar_focus: usize,
    /// Live checked bitmap of the *top* open menu's items (bit `i` = item
    /// `i` checked). R985.1 (session-review) — DELIBERATE demo scope: this
    /// binding's [`MENUS`] places checkboxes only at the top level, so a
    /// single top-menu bitmap suffices to reflect runtime toggles. The core
    /// wire DOES support nested checkboxes (`checked.<menu>.<item>.<sub>` is
    /// readable / writable); a binding whose model nests a checkbox would
    /// snapshot per visible level instead. Not a framework gap — a scoped
    /// projection for this demo's model.
    checked: u64,
}

impl MenuState {
    /// The live open submenu descent.
    fn open_path(&self) -> &[usize] {
        &self.open_path[..self.open_depth.min(MAX_OPEN_DEPTH)]
    }

    /// Whether the top open menu's item `i` is checked.
    const fn item_checked(self, i: usize) -> bool {
        i < 64 && (self.checked >> i) & 1 != 0
    }
}

/// Walk the model along the open path, returning `(items, active)` for each
/// open level (level 0 = the top dropdown). For a parent level `active` is
/// the open-child index; for the deepest level it is the highlighted item.
fn open_levels(state: &MenuState) -> Vec<(&'static [Item], Option<usize>)> {
    let mut levels: Vec<(&'static [Item], Option<usize>)> = Vec::new();
    let Some(m) = state.open else {
        return levels;
    };
    let path = state.open_path();
    let mut items: &'static [Item] = MENUS[m];
    for d in 0..=path.len() {
        let active = if d < path.len() {
            Some(path[d])
        } else {
            state.active
        };
        levels.push((items, active));
        if d < path.len() {
            match items.get(path[d]) {
                Some(Item::Submenu(_, kids)) => items = kids,
                _ => break,
            }
        }
    }
    levels
}

/// The item composite tag for level-`d` item `i`: the descent path is
/// `open_path[..d]` then `i` (matching the paint's relative tags).
fn item_tag(path: &[usize], d: usize, i: usize) -> String {
    let mut rel = path[..d.min(path.len())].to_vec();
    rel.push(i);
    composite_item_path_tag(BAR_TAG, &rel)
}

/// view-fn (§6.3): pure sync `MenuState -> Scene`. Paints the title strip,
/// the body copy, and — when open — the click-outside barrier + the
/// [`view_menu_cascade`] popup chain (top dropdown + one nested popup per
/// open submenu), placed **last** so they paint over the content.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: MenuState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = MenuStyle::m3_default();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let menubar = view_menu_bar(BAR_TAG, &MENU_TITLES, state.open, &theme, &style);

    let body = Scene::Text(TextNode::styled(
        "File > Open Recent > Older nests two levels; View > Appearance one.",
        Rect::default(),
        TextStyle::new()
            .with_size_px(BODY_FONT_PX)
            .with_fg(on_surface),
    ));
    let footer = Scene::Text(TextNode::styled(
        "\u{2192} open submenu   \u{2190}/Esc close one level   Enter activate",
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
        children.push(dismiss_barrier(
            BARRIER_TAG,
            (0, style.bar_height),
            (WIN_W, WIN_H - style.bar_height),
        ));
        // Project each open level to paint descriptors; only the top level
        // carries checkboxes (live state read back from the External).
        let levels_model = open_levels(&state);
        let level_views: Vec<Vec<MenuItemView>> = levels_model
            .iter()
            .enumerate()
            .map(|(d, (items, _))| {
                items
                    .iter()
                    .enumerate()
                    .map(|(i, it)| it.to_view(d == 0 && state.item_checked(i)))
                    .collect()
            })
            .collect();
        let levels: Vec<MenuLevel> = level_views
            .iter()
            .zip(levels_model.iter())
            .map(|(views, (_, active))| MenuLevel {
                items: views.as_slice(),
                active: *active,
            })
            .collect();
        children.extend(view_menu_cascade(
            BAR_TAG,
            &LEVEL_TAGS,
            m,
            &levels,
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
        // R985 — the open submenu descent ("1.2" -> [1, 2]), decoded via the
        // core wire codec (R985.1: one parse home, not a re-rolled split).
        if let Some(IntrospectValue::Text(s)) = intro.query("open_path") {
            if let Some(descent) = parse_path(&s) {
                for &idx in descent.iter().take(MAX_OPEN_DEPTH) {
                    out.open_path[out.open_depth] = idx;
                    out.open_depth += 1;
                }
            }
        }
        // Snapshot the top open menu's live checked state.
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
        "pinion hello-menu-nested (R985 §5.16 §5.40)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        Self::forward_key_to_external(scene, focused, key)
    }

    fn fmt_state_log(state: &MenuState) -> String {
        let open = state
            .open
            .map_or_else(|| "-".to_string(), |m| m.to_string());
        format!(
            "open={open} path=[{}] active={:?}",
            path_text(state.open_path()),
            state.active
        )
    }
}

impl WidgetA11y for MenuView {
    /// R985 §5.40 — WAI-ARIA §3.16 menubar + cascading menu tree. The
    /// menubar root + per-title menuitems are bespoke (the two-level
    /// skeleton diverges from a one-level context menu); each open level is
    /// the lifted [`menu_item_nodes`] unit, with the open submenu parent
    /// carrying `aria-haspopup` / `aria-expanded` and owning the next
    /// level's `Menu` node.
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
            let title_focused = group_focused && state.open.is_none() && state.bar_focus == m;
            let mut title = AccessNode::new(composite_title_tag(BAR_TAG, m), AriaRole::MenuItem)
                .with_state(AccessState {
                    focused: title_focused,
                    ..AccessState::default()
                })
                .with_position_in_set(u32::try_from(m + 1).unwrap_or(u32::MAX))
                .with_size_of_set(title_setsize);
            if state.open == Some(m) {
                title = title.with_child(LEVEL_TAGS[0]);
            }
            nodes.push(title);
        }

        let Some(m) = state.open else {
            return nodes;
        };
        let levels = open_levels(state);
        let path = state.open_path();
        for (d, (items, _level_active)) in levels.iter().enumerate() {
            let Some(&level_tag) = LEVEL_TAGS.get(d) else {
                break;
            };
            let level_name = if d == 0 {
                MENU_TITLES[m]
            } else {
                // The parent submenu's label names this nested menu.
                levels[d - 1].0[path[d - 1]].label()
            };
            // Separators are presentational — dropped before the slice so
            // posinset / setsize count real items only (per R805).
            let real: Vec<usize> = (0..items.len())
                .filter(|&i| !items[i].is_separator())
                .collect();
            let tags: Vec<String> = real.iter().map(|&i| item_tag(path, d, i)).collect();
            let child_tags: Vec<&'static str> = real
                .iter()
                .map(|&i| {
                    if d < path.len() && path[d] == i {
                        LEVEL_TAGS.get(d + 1).copied().unwrap_or("")
                    } else {
                        ""
                    }
                })
                .collect();
            let cells: Vec<MenuItemCell<'_>> = real
                .iter()
                .enumerate()
                .map(|(pos, &i)| {
                    let it = items[i];
                    let expanded = d < path.len() && path[d] == i;
                    MenuItemCell {
                        tag: &tags[pos],
                        label: None,
                        checked: (d == 0 && it.is_checkbox()).then(|| state.item_checked(i)),
                        disabled: false,
                        // Only the deepest level's active item is AT-focused.
                        focused: group_focused && d == path.len() && state.active == Some(i),
                        // A submenu parent carries aria-haspopup; the open one
                        // is aria-expanded and owns the next level's menu node.
                        popup: it.is_submenu().then(|| SubmenuCell {
                            expanded,
                            owns: (expanded && !child_tags[pos].is_empty())
                                .then_some(child_tags[pos]),
                        }),
                    }
                })
                .collect();
            nodes.extend(menu_item_nodes(level_tag, level_name, &cells));
        }
        nodes
    }

    fn access_focus_target(state: &MenuState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused != Some(<Self as WidgetCore>::tag()) {
            return focused.map(AccessFocus::atomic);
        }
        let path = state.open_path();
        let descendant = match state.open {
            Some(m) => match state.active {
                // The deepest active item: descent = open_path then active.
                Some(i) => item_tag(path, path.len(), i),
                None if path.is_empty() => composite_title_tag(BAR_TAG, m),
                // Open submenu but nothing highlighted: the deepest parent.
                None => composite_item_path_tag(BAR_TAG, path),
            },
            None => composite_title_tag(BAR_TAG, state.bar_focus),
        };
        Some(AccessFocus::composite(
            <Self as WidgetCore>::tag(),
            descendant,
        ))
    }

    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Some((kind, rest)) = split_sub_tag(sub_tag) else {
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
                // PointerUp is the activation edge: a title toggles, an item
                // opens (submenu) or fires (leaf).
                let _ = intro.invoke(
                    "send",
                    IntrospectValue::Text(format!("{kind}{rest}:PointerUp")),
                );
                true
            }
            AccessAction::Focus => {
                if kind == 't' {
                    if let Ok(i) = rest.parse::<i64>() {
                        let _ = intro.intervene("bar_focus", IntrospectValue::Int(i));
                    }
                } else if let Some(mut p) = parse_path(rest) {
                    // Point the cursor: open_path = prefix, active = last —
                    // decode/encode via the core wire codec (R985.1).
                    if let Some(last) = p.pop() {
                        let _ = intro.intervene("open_path", IntrospectValue::Text(path_text(&p)));
                        if let Ok(i) = i64::try_from(last) {
                            let _ = intro.intervene("active", IntrospectValue::Int(i));
                        }
                    }
                }
                true
            }
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for MenuView {
    type Renderer = HelloMenuNestedRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// Read an optional-index introspect slot (`open` / `active`).
fn query_index(intro: &dyn ExternalIntrospect, path: &str) -> Option<usize> {
    match intro.query(path) {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    }
}

/// Split a composite sub-tag into its kind char (`t` / `i`) and the rest of
/// the string (a single index for `t`, a dotted descent path for `i`).
fn split_sub_tag(sub_tag: &str) -> Option<(char, &str)> {
    let mut chars = sub_tag.chars();
    let kind = chars.next()?;
    if kind != 't' && kind != 'i' {
        return None;
    }
    Some((kind, chars.as_str()))
}

fn main() {
    pinion_shell::run::<MenuView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_state(m: usize, path: &[usize], active: Option<usize>) -> MenuState {
        let mut s = MenuState {
            open: Some(m),
            active,
            ..MenuState::default()
        };
        for (k, &idx) in path.iter().enumerate() {
            s.open_path[k] = idx;
        }
        s.open_depth = path.len();
        s
    }

    #[test]
    fn open_levels_walks_the_cascade() {
        // File > Open Recent (1) > Older (2): three open levels.
        let s = open_state(0, &[1, 2], Some(0));
        let levels = open_levels(&s);
        assert_eq!(levels.len(), 3, "top + Open Recent + Older");
        assert_eq!(levels[0].1, Some(1), "level 0 open child = Open Recent");
        assert_eq!(levels[1].1, Some(2), "level 1 open child = Older");
        assert_eq!(levels[2].1, Some(0), "deepest level active");
        // The deepest level is Older's items.
        assert!(matches!(levels[2].0[0], Item::Command("2024.log")));
    }

    #[test]
    fn access_node_marks_submenu_parent() {
        let s = open_state(0, &[], Some(1));
        let nodes = MenuView::access_node(&s, Some(BAR_TAG));
        // The Open Recent parent (top item 1) advertises its popup.
        let parent = nodes
            .iter()
            .find(|n| n.tag == "menu#i1")
            .expect("submenu parent node");
        assert_eq!(parent.has_popup, Some(pinion_a11y::HasPopup::Menu));
        assert_eq!(
            parent.expanded,
            Some(false),
            "collapsed while open_path is empty"
        );
    }

    #[test]
    fn access_node_open_submenu_owns_child() {
        let s = open_state(0, &[1], Some(0));
        let nodes = MenuView::access_node(&s, Some(BAR_TAG));
        let parent = nodes
            .iter()
            .find(|n| n.tag == "menu#i1")
            .expect("submenu parent node");
        assert_eq!(
            parent.expanded,
            Some(true),
            "expanded while its submenu is open"
        );
        assert!(
            parent.children.contains(&"menu_sub1".to_string()),
            "owns the nested menu node"
        );
    }
}
