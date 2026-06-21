//! AccessKit builder for a **WAI-ARIA `menu` holding `menuitem`s**
//! (R691 / R805 §5.40, lifted R817): a labelled [`AriaRole::Menu`]
//! container whose children are [`AriaRole::MenuItem`] /
//! [`AriaRole::MenuItemCheckbox`] nodes, each carrying its focus / disabled
//! posture and (for a checkbox item) `aria-checked`.
//!
//! This is the shared *inner* a11y unit behind two consumers — the open
//! dropdown of `hello-menu` (a menu bar) and the whole popup of
//! `hello-contextmenu`. Both build the identical `menu` + `menuitem`
//! topology by hand, so it is lifted here as one source of truth (the R758
//! a11y-axis "divergence-is-a-bug" rule).
//!
//! ## Scope: the inner unit only
//!
//! `hello-menu` and `hello-contextmenu` diverge in *nesting depth* —
//! `hello-menu` wraps its dropdowns in a [`AriaRole::MenuBar`] of
//! per-title [`AriaRole::MenuItem`]s (a two-level structure), while
//! `hello-contextmenu` is a single one-level menu. Only the inner
//! `menu` + `menuitem` unit is common, so only that is lifted; the menu
//! bar skeleton stays inline in `hello-menu` (lifting the divergent outer
//! level would be over-merge, per [[abstraction-needs-second-consumer]]).
//!
//! Mirrors [`listbox_option_nodes`](crate::listbox_option_nodes) /
//! [`tablist_tab_nodes`](crate::tablist_tab_nodes): a [`MenuItemCell`]
//! slice + container tag / name, returning `[menu, item_0, ...]`.
//! `aria-posinset` / `aria-setsize` are derived from each item's index in
//! the slice — so the caller passes only the *real* items (a separator is
//! presentational and is dropped before building the slice, per R805).
//!
//! The item role follows [`MenuItemCell::checked`]: `Some(_)` is a
//! `menuitemcheckbox` carrying `aria-checked`; `None` is a plain
//! `menuitem`. R985 — a submenu parent ([`MenuItemCell::has_popup`]) is a
//! plain `menuitem` that additionally carries `aria-haspopup="menu"`,
//! `aria-expanded`, and (when open) a child reference to the nested
//! [`AriaRole::Menu`] it owns. The binding builds each *open* level by
//! calling [`menu_item_nodes`] once per level and stitching the parent's
//! `owns` child reference to the nested menu's container tag.

use crate::node::{AccessNode, AccessState};
use crate::role::{AriaRole, HasPopup};

/// One item within a [`menu_item_nodes`] menu.
#[derive(Clone, Copy, Debug, Default)]
pub struct MenuItemCell<'a> {
    /// The item's `External` tag (typically a `{primary}#{i}` composite).
    pub tag: &'a str,
    /// Explicit accessible name (surviving scene enrichment), or `None` to
    /// rely on name-from-contents enrichment of the painted label.
    pub label: Option<&'a str>,
    /// `Some(_)` makes this a `menuitemcheckbox` carrying `aria-checked`;
    /// `None` is a plain command `menuitem`.
    pub checked: Option<bool>,
    /// `true` lowers `aria-disabled` (a non-operable command item).
    pub disabled: bool,
    /// `true` when this item is the roving active descendant (the menu owns
    /// focus and this is the active index).
    pub focused: bool,
    /// R985 — `Some` for a submenu parent (a `menuitem` carrying
    /// `aria-haspopup="menu"`); `None` for a plain command / checkbox item.
    /// The [`SubmenuCell`] payload carries the open state + owned child.
    pub popup: Option<SubmenuCell<'a>>,
}

/// R985 — the submenu adornment of a [`MenuItemCell`] parent: its open
/// state (`aria-expanded`) and, when open, the nested [`AriaRole::Menu`] it
/// owns. Folded into one [`MenuItemCell::popup`] `Option` so the cell does
/// not carry a loose `has_popup` / `expanded` bool pair (the
/// `Some` ⟺ "is a submenu parent" invariant is type-level).
#[derive(Clone, Copy, Debug, Default)]
pub struct SubmenuCell<'a> {
    /// Whether the nested menu is currently open (`aria-expanded`).
    pub expanded: bool,
    /// For an *open* submenu parent, the container tag of the nested
    /// [`AriaRole::Menu`] this item owns; referenced as a child so the AT
    /// tree links the parent menuitem to its submenu. `None` when collapsed.
    pub owns: Option<&'a str>,
}

/// Build the `menu` container + one `menuitem` / `menuitemcheckbox` per
/// cell.
///
/// The container carries an explicit `name` and references every item tag
/// as a child; each item carries its focus / disabled state, the WAI-ARIA
/// `aria-posinset` / `aria-setsize` derived from its index in `items`,
/// `aria-checked` when [`MenuItemCell::checked`] is `Some`, and — when
/// [`MenuItemCell::label`] is `Some` — an explicit accessible name (else
/// left to [`enrich_names_from_scene`](crate::enrich_names_from_scene)).
/// The returned vector is `[menu, item_0, item_1, ...]` — the container
/// first, mirroring the flat-list convention `lower_access_node` resolves
/// into an AT subtree.
#[must_use]
pub fn menu_item_nodes(
    menu_tag: &str,
    menu_name: &str,
    items: &[MenuItemCell<'_>],
) -> Vec<AccessNode> {
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(items.len() + 1);
    let mut menu = AccessNode::new(menu_tag, AriaRole::Menu).with_name(menu_name);
    for item in items {
        menu = menu.with_child(item.tag);
    }
    nodes.push(menu);
    for (i, item) in items.iter().enumerate() {
        let role = if item.checked.is_some() {
            AriaRole::MenuItemCheckbox
        } else {
            AriaRole::MenuItem
        };
        let mut node = AccessNode::new(item.tag, role)
            .with_state(AccessState {
                focused: item.focused,
                disabled: item.disabled,
                checked: item.checked,
                ..AccessState::default()
            })
            .with_set_position(i, items.len());
        if let Some(label) = item.label {
            node = node.with_name(label);
        }
        // R985 — a submenu parent advertises its popup and open state, and
        // (when open) references the nested menu it owns as a child so the AT
        // tree links parent menuitem -> submenu.
        if let Some(sub) = item.popup {
            node = node
                .with_has_popup(HasPopup::Menu)
                .with_expanded(sub.expanded);
            if let Some(child) = sub.owns {
                node = node.with_child(child);
            }
        }
        nodes.push(node);
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> [MenuItemCell<'static>; 3] {
        [
            MenuItemCell {
                tag: "m#0",
                label: Some("New"),
                checked: None,
                disabled: false,
                focused: true,
                ..MenuItemCell::default()
            },
            MenuItemCell {
                tag: "m#1",
                label: None,
                checked: Some(true),
                disabled: false,
                focused: false,
                ..MenuItemCell::default()
            },
            MenuItemCell {
                tag: "m#2",
                label: Some("Print"),
                checked: None,
                disabled: true,
                focused: false,
                ..MenuItemCell::default()
            },
        ]
    }

    fn by_tag<'a>(nodes: &'a [AccessNode], tag: &str) -> &'a AccessNode {
        nodes.iter().find(|n| n.tag == tag).expect("node present")
    }

    #[test]
    fn emits_menu_plus_one_item_per_cell() {
        let it = items();
        let nodes = menu_item_nodes("menu", "File", &it);
        assert_eq!(nodes.len(), it.len() + 1, "one menu + N items");
        assert_eq!(nodes[0].role, AriaRole::Menu);
        assert_eq!(nodes[0].name.as_deref(), Some("File"));
        assert_eq!(
            nodes[0].children.len(),
            it.len(),
            "menu references every item"
        );
    }

    #[test]
    fn checked_some_is_menuitemcheckbox_none_is_menuitem() {
        let nodes = menu_item_nodes("menu", "File", &items());
        assert_eq!(
            by_tag(&nodes, "m#0").role,
            AriaRole::MenuItem,
            "checked None → menuitem"
        );
        assert_eq!(
            by_tag(&nodes, "m#1").role,
            AriaRole::MenuItemCheckbox,
            "checked Some → menuitemcheckbox",
        );
        assert_eq!(
            by_tag(&nodes, "m#1").state.checked,
            Some(true),
            "aria-checked lowered"
        );
        assert_eq!(
            by_tag(&nodes, "m#0").state.checked,
            None,
            "plain item omits aria-checked"
        );
    }

    #[test]
    fn disabled_and_focused_state_lower() {
        let nodes = menu_item_nodes("menu", "File", &items());
        assert!(
            by_tag(&nodes, "m#0").state.focused,
            "the active item is focused"
        );
        assert!(
            by_tag(&nodes, "m#2").state.disabled,
            "the disabled command lowers aria-disabled"
        );
        assert!(!by_tag(&nodes, "m#0").state.disabled);
    }

    #[test]
    fn items_carry_posinset_and_setsize_over_the_slice() {
        // The caller passes only real items (separators dropped), so the
        // numbering counts menu items only.
        let nodes = menu_item_nodes("menu", "File", &items());
        assert_eq!(by_tag(&nodes, "m#0").position_in_set, Some(1));
        assert_eq!(by_tag(&nodes, "m#0").size_of_set, Some(3));
        assert_eq!(by_tag(&nodes, "m#2").position_in_set, Some(3));
    }

    #[test]
    fn explicit_label_set_and_none_left_for_enrichment() {
        let nodes = menu_item_nodes("menu", "File", &items());
        assert_eq!(
            by_tag(&nodes, "m#0").name.as_deref(),
            Some("New"),
            "explicit label applied"
        );
        assert_eq!(
            by_tag(&nodes, "m#1").name,
            None,
            "label None left to scene enrichment"
        );
    }

    #[test]
    fn empty_items_emit_menu_only() {
        let nodes = menu_item_nodes("menu", "File", &[]);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Menu);
        assert!(nodes[0].children.is_empty());
    }

    // ----- R985: submenu parent (aria-haspopup / aria-expanded / owns) -----

    #[test]
    fn r985_submenu_parent_collapsed_haspopup_no_owns() {
        let cells = [MenuItemCell {
            tag: "m#1",
            label: Some("Open Recent"),
            popup: Some(SubmenuCell {
                expanded: false,
                owns: None,
            }),
            ..MenuItemCell::default()
        }];
        let nodes = menu_item_nodes("menu", "File", &cells);
        let parent = by_tag(&nodes, "m#1");
        assert_eq!(
            parent.role,
            AriaRole::MenuItem,
            "a submenu parent is a plain menuitem"
        );
        assert_eq!(
            parent.has_popup,
            Some(HasPopup::Menu),
            "aria-haspopup=menu lowered"
        );
        assert_eq!(
            parent.expanded,
            Some(false),
            "aria-expanded=false while collapsed"
        );
        assert!(
            parent.children.is_empty(),
            "no owned submenu node while collapsed"
        );
    }

    #[test]
    fn r985_submenu_parent_open_owns_child_menu() {
        let cells = [MenuItemCell {
            tag: "m#1",
            label: Some("Open Recent"),
            popup: Some(SubmenuCell {
                expanded: true,
                owns: Some("submenu"),
            }),
            ..MenuItemCell::default()
        }];
        let nodes = menu_item_nodes("menu", "File", &cells);
        let parent = by_tag(&nodes, "m#1");
        assert_eq!(parent.expanded, Some(true), "aria-expanded=true while open");
        assert_eq!(
            parent.children,
            vec!["submenu".to_string()],
            "owns the nested menu node"
        );
    }

    #[test]
    fn r985_plain_items_carry_no_haspopup() {
        let nodes = menu_item_nodes("menu", "File", &items());
        for n in &nodes {
            if n.role != AriaRole::Menu {
                assert_eq!(n.has_popup, None, "a non-submenu item omits aria-haspopup");
            }
        }
    }
}
