// R832 §5.40 §5.50 §5.38 — own-rendered application menu bar.
//
// Lint scope note (mirrors hello-menu): WAI-ARIA / Material proper nouns
// appear in doc prose (MenuBar, MenuBarExternal, …).
#![allow(rustdoc::private_intra_doc_links)]
//! R832 — the R805 [`MenuBar`](pinion_core::widgets::menu) substrate
//! composed as **application chrome**: a persistent File / Edit / View
//! menu bar whose commands drive a real document model, not a standalone
//! menu-widget demo (that is [`hello-menu`]). This is the first
//! menu-DRIVEN pinion application — the Qt / Flutter-class "application
//! menu bar" Phase-B milestone.
//!
//! ## Composition
//!
//! The menu bar itself is pure composition over the R805 substrate: one
//! [`MenuBarExternal`] owns the whole WAI-ARIA §3.5 keyboard + pointer
//! model, [`view_menu_bar`] paints the title strip, and when a menu is
//! open [`view_menu_dropdown`] paints the floating list over a
//! [`dismiss_barrier`]. The application layer this round adds on top:
//!
//! - A [`DocModel`] reactive holder (document `content` + `dirty` flag)
//!   in [`Owner::cache`], read by the view fn and mutated by the
//!   [`WidgetCore::update`] reducer.
//! - File / Edit **command** items route the R805 `"menu.command"` intent
//!   (payload `"<menu>.<item>"`) into the reducer, which applies the
//!   document command (New / Open Sample / Save / Append Line / Clear).
//! - View **checkbox** items (`Show Status Bar`, `Uppercase`) keep their
//!   state in the `MenuBarExternal` (R805) and the view reads it back to
//!   toggle the status line / transform the rendered text — no reducer
//!   round-trip.
//! - A [`QueryOnlyIntrospect`] node (tag `doc`) exposes the document
//!   model to the §5.12 RPC plane so an AI client verifies command
//!   effects with `scene/query "/doc/external/{content_len,line_count,
//!   dirty}"` — the AI-first introspection obligation applied to app
//!   state, reusing the R810.1 substrate.
//!
//! ## AI clients
//!
//! `invoke("send", "t<m>:PointerUp")` toggles a title, `"i<i>:PointerUp"`
//! activates an item; the document effect is then observable through the
//! `doc` query node and the menu checkbox state through the `menu`
//! external — the whole click → open → select → command → app-state-change
//! loop is RPC-driven and RPC-verifiable.

use std::fmt::Write as _;
use std::rc::Rc;

use pinion_a11y::{
    menu_item_nodes, AccessAction, AccessFocus, AccessNode, AccessState, AriaRole, MenuItemCell,
    WidgetA11y,
};
use pinion_core::external::{
    External, ExternalIntrospect, IntrospectSchema, IntrospectValue, QueryOnlyIntrospect,
    QuerySource,
};
use pinion_core::intent::Intent;
use pinion_core::reactive::{batch, Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::menu::{MenuBarExternal, MenuItem};
use pinion_core::{intent_tag, Command, Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::menu::{
    composite_item_tag, composite_title_tag, view_menu_bar, view_menu_dropdown, MenuItemView,
    MenuStyle,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloMenuAppRenderer, HelloMenuAppRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 360;
const THEME_TAG: &str = "app";
const BAR_TAG: &str = "menu";
const DROPDOWN_TAG: &str = "menu_dropdown";
/// R715 §5.16 — the transparent click-outside dismiss barrier; the R51.42
/// composite form `<bar>#barrier` routes its outside `PointerUp` to the
/// one [`MenuBarExternal`] (tag `menu`), whose barrier arm closes the open
/// dropdown.
const BARRIER_TAG: &str = "menu#barrier";
/// `Owner::cache` key + RPC tag for the document introspection node.
const DOC_TAG: &str = "doc";
const BODY_FONT_PX: u32 = 15;
const STATUS_FONT_PX: u32 = 12;

/// R805 §5.20 — the `"menu.command"` intent the `MenuBarExternal` emits on
/// item activation (payload [`IntrospectValue::Text`] `"<menu>.<item>"`).
const COMMAND_INTENT_TAG: &str = intent_tag!("menu", "command");

/// Top-level menu titles. Index `m` becomes the title tagged `menu#t<m>`.
const MENU_TITLES: [&str; 3] = ["File", "Edit", "View"];

/// Document loaded by File -> Open Sample (3 lines).
const SAMPLE_DOC: &str =
    "The quick brown fox\njumps over the lazy dog\nand keeps right on running.";

/// One dropdown entry in this application's menu model.
#[derive(Clone, Copy)]
enum Item {
    /// A one-shot command (`menuitem`) carrying its `(menu, item)` route.
    Command(&'static str),
    /// A toggle (`menuitemcheckbox`) with its boot checked state.
    Checkbox(&'static str, bool),
    /// A non-interactive divider (`separator`).
    Separator,
}

impl Item {
    /// Project to the `MenuBarExternal` model entry (boot state).
    fn to_model(self) -> MenuItem {
        match self {
            Item::Command(_) => MenuItem::command(),
            Item::Checkbox(_, checked) => MenuItem::checkbox(checked),
            Item::Separator => MenuItem::separator(),
        }
    }

    /// Project to the paint descriptor, taking the live checked state for
    /// a checkbox.
    fn to_view(self, checked: bool) -> MenuItemView<'static> {
        match self {
            Item::Command(l) => MenuItemView::command(l),
            Item::Checkbox(l, _) => MenuItemView::checkbox(l, checked),
            Item::Separator => MenuItemView::separator(),
        }
    }

    const fn is_separator(self) -> bool {
        matches!(self, Item::Separator)
    }
}

/// Per-menu item lists. `MENUS[m][i]` is item `i` of menu `m`. File holds
/// document commands (with a separator before Save); Edit holds two
/// editing commands; View holds two checkbox toggles the view reads back.
const MENUS: [&[Item]; 3] = [
    &[
        Item::Command("New"),
        Item::Command("Open Sample"),
        Item::Separator,
        Item::Command("Save"),
    ],
    &[Item::Command("Append Line"), Item::Command("Clear")],
    &[
        Item::Checkbox("Show Status Bar", true),
        Item::Checkbox("Uppercase", false),
    ],
];

// View-menu checkbox item indices (read back as persistent view toggles).
const VIEW_MENU: usize = 2;
const SHOW_STATUS_ITEM: usize = 0;
const UPPERCASE_ITEM: usize = 1;

/// R832 §5.38 — the document model: the application state the menu bar
/// drives. A reactive holder shared (one `Rc`) by the view fn (reads), the
/// [`WidgetCore::update`] reducer (mutates), and the [`QueryOnlyIntrospect`]
/// node (RPC reads) — the [[reactive-holder-for-shared-external-view-state]]
/// pattern.
#[derive(Debug)]
struct DocModel {
    /// The document text (display-only this round; commands replace /
    /// append whole lines).
    content: Signal<String>,
    /// `true` once an editing command (Append Line / Clear) has run since
    /// the last New / Open / Save.
    dirty: Signal<bool>,
}

impl DocModel {
    fn new() -> Self {
        Self {
            content: Signal::new(String::new()),
            dirty: Signal::new(false),
        }
    }
}

impl QuerySource for DocModel {
    fn introspect_schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("content_len", "int"),
            ("line_count", "int"),
            ("dirty", "bool"),
            ("empty", "bool"),
        ])
    }

    fn introspect_query(&self, path: &str) -> Option<IntrospectValue> {
        let content = self.content.get();
        match path {
            "content_len" => Some(IntrospectValue::Int(int_of(content.chars().count()))),
            "line_count" => Some(IntrospectValue::Int(int_of(line_count(&content)))),
            "dirty" => Some(IntrospectValue::Bool(self.dirty.get())),
            "empty" => Some(IntrospectValue::Bool(content.is_empty())),
            _ => None,
        }
    }
}

/// `Owner::cache`-keyed document model (one SSOT for view + reducer +
/// introspect node).
fn use_doc() -> Rc<DocModel> {
    let owner = Owner::current().expect("use_doc requires an active Owner scope");
    owner.cache("menu_app.doc", DocModel::new)
}

/// Line count of a document: `0` for empty, else the number of `\n`-split
/// lines.
fn line_count(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        content.lines().count()
    }
}

/// `usize -> i64` for an introspect `Int` slot, saturating (counts never
/// realistically exceed `i64::MAX`).
fn int_of(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// Apply a File / Edit document command addressed as `(menu, item)`.
/// View-menu checkbox items (`menu == VIEW_MENU`) and separators never
/// reach here with a document effect — they are no-ops.
fn apply_doc_command(menu: usize, item: usize) {
    let doc = use_doc();
    match (menu, item) {
        // File -> New / Open Sample / Save (item 2 is the separator).
        (0, 0) => batch(|| {
            doc.content.set(String::new());
            doc.dirty.set(false);
        }),
        (0, 1) => batch(|| {
            doc.content.set(SAMPLE_DOC.to_owned());
            doc.dirty.set(false);
        }),
        (0, 3) => doc.dirty.set(false),
        // Edit -> Append Line / Clear.
        (1, 0) => {
            let mut content = doc.content.get();
            let next = line_count(&content) + 1;
            if !content.is_empty() {
                content.push('\n');
            }
            let _ = write!(content, "Line {next}");
            batch(|| {
                doc.content.set(content);
                doc.dirty.set(true);
            });
        }
        (1, 1) => batch(|| {
            doc.content.set(String::new());
            doc.dirty.set(true);
        }),
        _ => {}
    }
}

/// Cached projection of the menubar + the persistent view toggles. `Copy`
/// so the shell hands the snapshot into the paint closure without lifetime
/// gymnastics.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
struct MenuState {
    /// Open dropdown menu index, or `None` when closed.
    open: Option<usize>,
    /// Highlighted item within the open menu (WAI-ARIA active descendant).
    active: Option<usize>,
    /// Keyboard-focused top-level title.
    bar_focus: usize,
    /// Live checked bitmap of the OPEN menu's items (bit `i` = item `i`).
    checked: u64,
    /// Persistent `View -> Show Status Bar` toggle (read every frame, not
    /// only while the View menu is open).
    show_status: bool,
    /// Persistent `View -> Uppercase` toggle.
    uppercase: bool,
}

impl MenuState {
    /// Whether the open menu's item `i` is checked (bit `i` of the bitmap).
    const fn item_checked(self, i: usize) -> bool {
        i < 64 && (self.checked >> i) & 1 != 0
    }
}

/// Read an optional-index introspect slot: `Int(i)` -> `Some(i)`.
fn query_index(intro: &dyn ExternalIntrospect, path: &str) -> Option<usize> {
    match intro.query(path) {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    }
}

/// Read a `checked.<m>.<i>` boolean slot off the menu external.
fn query_checked(intro: &dyn ExternalIntrospect, menu: usize, item: usize) -> bool {
    matches!(
        intro.query(&format!("checked.{menu}.{item}")),
        Some(IntrospectValue::Bool(true))
    )
}

/// Parse a composite sub-tag (`t<m>` title / `i<i>` item) into its kind
/// char + index.
fn parse_sub_tag(sub_tag: &str) -> Option<(char, usize)> {
    let mut chars = sub_tag.chars();
    let kind = chars.next()?;
    if kind != 't' && kind != 'i' {
        return None;
    }
    let idx: usize = chars.as_str().parse().ok()?;
    Some((kind, idx))
}

/// Parse the `"<menu>.<item>"` command payload into `(menu, item)`.
fn parse_command_payload(payload: &str) -> Option<(usize, usize)> {
    let (m, i) = payload.split_once('.')?;
    Some((m.parse().ok()?, i.parse().ok()?))
}

/// Build the document content area: one Text row per line (uppercased
/// when the View toggle is on) or an empty-document placeholder, plus the
/// status line when `Show Status Bar` is on.
fn view_content_area(content: &str, state: MenuState, dirty: bool, theme: &Theme) -> Scene {
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let mut body_rows: Vec<Scene> = Vec::new();
    if content.is_empty() {
        body_rows.push(Scene::Text(TextNode::styled(
            "(empty document - use File > Open Sample or Edit > Append Line)",
            Rect::default(),
            TextStyle::new().with_size_px(BODY_FONT_PX).with_fg(on_surface_muted),
        )));
    } else {
        let shown = if state.uppercase { content.to_uppercase() } else { content.to_owned() };
        for line in shown.lines() {
            body_rows.push(Scene::Text(TextNode::styled(
                line,
                Rect::default(),
                TextStyle::new().with_size_px(BODY_FONT_PX).with_fg(on_surface),
            )));
        }
    }
    let body = Scene::Container(
        ContainerNode::new(body_rows).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_justify(JustifyContent::Start)
                .with_flex_grow(1.0)
                .with_gap(4)
                .with_padding(Rect::new(16, 16, 16, 16)),
        ),
    );

    let mut content_children = vec![body];
    if state.show_status {
        let status = format!(
            "lines: {}   chars: {}   {}",
            line_count(content),
            content.chars().count(),
            if dirty { "[modified]" } else { "[saved]" },
        );
        content_children.push(Scene::Text(TextNode::styled(
            &status,
            Rect::default(),
            TextStyle::new().with_size_px(STATUS_FONT_PX).with_fg(on_surface_muted),
        )));
    }
    Scene::Container(
        ContainerNode::new(content_children).with_tag("content").with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Stretch)
                .with_justify(JustifyContent::Start)
                .with_flex_grow(1.0)
                .with_gap(8)
                .with_padding(Rect::new(8, 8, 8, 8)),
        ),
    )
}

/// view-fn (§6.3): pure sync mapping `MenuState -> Scene`, reading the
/// document model from [`Owner::cache`].
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: MenuState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = MenuStyle::m3_default();
    let doc = use_doc();
    let content = doc.content.get();
    let dirty = doc.dirty.get();

    let menubar = view_menu_bar(BAR_TAG, &MENU_TITLES, state.open, &theme, &style);
    let content_area = view_content_area(&content, state, dirty, &theme);

    let mut children = vec![menubar, content_area];
    if let Some(m) = state.open {
        children.push(dismiss_barrier(
            BARRIER_TAG,
            (0, style.bar_height),
            (WIN_W, WIN_H - style.bar_height),
        ));
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

struct AppMenuView;

impl WidgetCore for AppMenuView {
    type State = MenuState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let menus: Vec<Vec<MenuItem>> = MENUS
            .iter()
            .map(|items| items.iter().map(|it| it.to_model()).collect())
            .collect();
        Box::new(MenuBarExternal::with_items(menus))
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // R832 §5.12 — expose the document model to the RPC plane so an AI
        // client verifies command effects. Shares the `Owner::cache`
        // `Rc<DocModel>` the view + reducer use (one SSOT).
        vec![ExtraExternal::new(
            DOC_TAG,
            Box::new(QueryOnlyIntrospect::new(use_doc())),
        )]
    }

    fn tag() -> &'static str {
        BAR_TAG
    }

    fn read_state(scene: &Scene) -> MenuState {
        // The boot state scene is a `Container([menu, doc])` (the doc
        // introspect node is an extra external), so resolve the primary
        // `MenuBarExternal` by tag rather than expecting a bare
        // `Scene::External` ([[multi-external-substrate-extra-externals-pattern]]).
        let mut out = MenuState::default();
        let Some(node) = scene.find_external_with_tag(BAR_TAG) else {
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
        // Persistent View toggles: read every frame (not only while the
        // View menu is open) since they drive the always-visible chrome.
        out.show_status = query_checked(intro, VIEW_MENU, SHOW_STATUS_ITEM);
        out.uppercase = query_checked(intro, VIEW_MENU, UPPERCASE_ITEM);
        // Open menu's live checked bitmap for the dropdown paint.
        if let Some(m) = out.open {
            let count = MENUS[m].len().min(64);
            for i in 0..count {
                if query_checked(intro, m, i) {
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
        "pinion hello-menu-app (R832 application menu bar)"
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
        // Container-aware key forward: the boot state scene is a
        // multi-external `Container([menu, doc])`, so resolve the primary
        // `MenuBarExternal` by tag (the default `forward_key_to_external`
        // assumes a bare `Scene::External`). Same idiom as the IME walk +
        // settings-panel's `apply_key_nav`. The whole WAI-ARIA §3.5
        // keyboard model lives in the External's `"key"` wire.
        if focused != Some(<Self as WidgetCore>::tag()) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(BAR_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        matches!(
            intro.invoke("key", IntrospectValue::Text(key.to_owned())),
            Ok(IntrospectValue::Bool(true))
        )
    }

    /// R832 §5.20 — the application reducer: a File / Edit `"menu.command"`
    /// applies the addressed document command (mutating the
    /// [`Owner::cache`] [`DocModel`]). View-menu checkbox commands carry no
    /// document effect (the `MenuBarExternal` owns their checked state; the
    /// view reads it back), so they fall through as no-ops.
    fn update(_state: MenuState, intent: &Intent) -> Vec<Command> {
        if intent.tag.as_ref() == COMMAND_INTENT_TAG {
            if let IntrospectValue::Text(payload) = &intent.payload {
                if let Some((m, i)) = parse_command_payload(payload) {
                    apply_doc_command(m, i);
                }
            }
        }
        Vec::new()
    }
}

impl WidgetA11y for AppMenuView {
    /// R691 §5.40 — WAI-ARIA 1.2 §3.5 menubar / menu / menuitem tree.
    /// Mirrors [`hello-menu`]: the `MenuBar` root + one title per menu, and
    /// when a menu is open the lifted [`menu_item_nodes`] dropdown unit
    /// (R817) claimed as a child of the open title.
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
                title = title.with_child(DROPDOWN_TAG);
            }
            nodes.push(title);
        }

        if let Some(m) = state.open {
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
                    checked: matches!(items[i], Item::Checkbox(..)).then(|| state.item_checked(i)),
                    disabled: false,
                    focused: group_focused && state.active == Some(i),
                })
                .collect();
            nodes.extend(menu_item_nodes(DROPDOWN_TAG, MENU_TITLES[m], &cells));
        }
        nodes
    }

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

impl WidgetView for AppMenuView {
    type Renderer = HelloMenuAppRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<AppMenuView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_command_payload_splits_menu_and_item() {
        assert_eq!(parse_command_payload("0.1"), Some((0, 1)));
        assert_eq!(parse_command_payload("2.0"), Some((2, 0)));
        assert_eq!(parse_command_payload("0.3"), Some((0, 3)));
        assert_eq!(parse_command_payload("nope"), None);
        assert_eq!(parse_command_payload("1."), None);
    }

    #[test]
    fn line_count_is_zero_for_empty_else_lines() {
        assert_eq!(line_count(""), 0);
        assert_eq!(line_count("one"), 1);
        assert_eq!(line_count("a\nb\nc"), 3);
        assert_eq!(line_count(SAMPLE_DOC), 3);
    }

    #[test]
    fn parse_sub_tag_accepts_title_and_item_kinds() {
        assert_eq!(parse_sub_tag("t0"), Some(('t', 0)));
        assert_eq!(parse_sub_tag("i3"), Some(('i', 3)));
        assert_eq!(parse_sub_tag("x1"), None);
        assert_eq!(parse_sub_tag("tz"), None);
    }

    #[test]
    fn access_node_closed_emits_menubar_plus_titles() {
        let nodes = AppMenuView::access_node(&MenuState::default(), None);
        assert_eq!(nodes.len(), 1 + MENU_TITLES.len());
        assert_eq!(nodes[0].role, AriaRole::MenuBar);
        for title in &nodes[1..] {
            assert_eq!(title.role, AriaRole::MenuItem);
        }
    }

    #[test]
    fn access_node_open_view_menu_emits_two_checkboxes() {
        let state = MenuState {
            open: Some(VIEW_MENU),
            active: Some(0),
            bar_focus: VIEW_MENU,
            ..MenuState::default()
        };
        let nodes = AppMenuView::access_node(&state, Some(BAR_TAG));
        let checkboxes = nodes
            .iter()
            .filter(|n| n.role == AriaRole::MenuItemCheckbox)
            .count();
        assert_eq!(checkboxes, 2, "View menu has two checkbox items");
    }
}
