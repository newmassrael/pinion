//! pinion-a11y — AccessKit-backed accessibility substrate per §5.40.
//!
//! Wraps the `accesskit` crate behind a pinion-native surface so the
//! framework's `WidgetView::access_node` trait method (R51.63 wiring)
//! and the platform Adapter (R51.62 wiring, lives in `pinion-shell`
//! because the Adapter type is winit-coupled) share a single upgrade
//! boundary. A future `accesskit` minor bump rewrites only this crate
//! — every widget impl keeps its declarative shape.
//!
//! ## Why a wrapper, not direct accesskit use
//!
//! Four reasons make a thin wrapper textbook here:
//!
//! 1. **Stable surface across accesskit bumps.** `accesskit::Role` has
//!    180+ variants; pinion needs the seven that match its standard
//!    widget catalogue (`Button` / `Switch` / `CheckBox` / `RadioButton` /
//!    `Slider` / `RadioGroup` / `Generic`). [`role::AriaRole`] is a stable
//!    subset; future accesskit role additions cannot leak into
//!    `WidgetView` impls.
//! 2. **Tag-as-identity.** `accesskit::NodeId` is an opaque `u64`;
//!    pinion uses widget tags (`"main_btn"`, `"main_group"`) as the
//!    cross-system identity (input router, focus manager, introspect
//!    schema). [`tree::tag_to_node_id`] resolves the two
//!    deterministically.
//! 3. **Introspect lockstep.** The RPC introspect schema (§5.21)
//!    reports the same role / value / state identifiers AT clients
//!    see. The wrapper centralises the canonical form so both
//!    audiences stay in sync.
//! 4. **Action narrowing.** AccessKit ships 22 actions; pinion maps
//!    five (Click / Focus / Increment / Decrement / Default) and
//!    drops the rest silently via [`action::AccessAction::Other`].
//!    Dispatch code stays exhaustive over the pinion enum.
//!
//! ## Public surface
//!
//! - [`AccessNode`] — per-widget descriptor (tag, role, name, value,
//!   state, bounds, composite children).
//! - [`AccessState`] / [`AccessValue`] — interaction state + value
//!   shape (boolean / float-with-range / text).
//! - [`AriaRole`] — pinion's role enum, with [`AriaRole::to_accesskit`]
//!   lowering and [`AriaRole::aria_name`] for introspect output.
//! - [`AccessTreeBuilder`] — assembles `accesskit::TreeUpdate` from a
//!   flat list of `AccessNode`s; resolves composite topology.
//! - [`AccessAction`] / [`PinionAccessAction`] / [`translate_action`]
//!   — pinion-native action lift + AccessKit `ActionRequest` router.

#![forbid(unsafe_code)]

pub mod action;
pub mod assemble;
pub mod described;
pub mod focus;
pub mod grid;
pub mod grouped;
pub mod heading;
pub mod listbox;
pub mod menu;
pub mod navigation;
pub mod node;
pub mod radiogroup;
pub mod role;
pub mod scene_label;
pub mod tabs;
pub mod toggle_group;
pub mod toolbar;
pub mod tree;
pub mod tree_view;
pub mod virtual_grid;
pub mod virtual_list;
pub mod widget_a11y;

// R51.129 §5.40 — `WidgetA11y` impl for `pinion_core::test_fixtures::
// ButtonFixture`. Gated on the `test-fixtures` feature; never reaches
// production binaries.
#[cfg(any(test, feature = "test-fixtures"))]
mod test_fixtures;

pub use action::{AccessAction, PinionAccessAction, translate_action};
pub use assemble::{build_access_tree, resolve_access_bounds};
pub use described::describedby_region;
pub use focus::AccessFocus;
pub use grid::{GridCell, GridColumn, GridRow, grid_table_nodes};
pub use grouped::{
    GroupedGridSelection, GroupedGridSpec, GroupedTreeSpec, grouped_focus_target,
    grouped_grid_access_nodes, grouped_tree_access_nodes,
};
pub use heading::attach_block_headings;
pub use listbox::{ListOption, listbox_option_nodes};
pub use menu::{MenuItemCell, SubmenuCell, menu_item_nodes};
pub use navigation::{NavLink, navigation_link_nodes};
pub use node::{AccessNode, AccessState, AccessValue, attach_child_button};
pub use radiogroup::{RadioCell, radiogroup_radio_nodes};
pub use role::{AriaCurrent, AriaRole, AutoComplete, HasPopup, SortDirection};
pub use scene_label::{enrich_access_keys_from_scene, enrich_names_from_scene};
pub use tabs::{TabCell, tablist_tab_nodes};
pub use toggle_group::{ToggleSegment, toggle_button_group_nodes};
pub use toolbar::{ToolbarControl, toolbar_button_nodes};
pub use tree::{AccessTreeBuilder, ROOT_NODE_ID, tag_to_node_id};
pub use tree_view::{
    TreeGridSelection, tree_access_nodes, tree_row_tag, treegrid_nodes, windowed_tree_access_nodes,
};
pub use virtual_grid::{
    attach_row_headers, mark_grid_editability, windowed_grid_nodes, windowed_grid_nodes_frozen,
    windowed_grid_nodes_multiselected, windowed_grid_nodes_selected, windowed_grid_nodes_sorted,
    windowed_grid_nodes_wide,
};
pub use virtual_list::{
    windowed_list_nodes, windowed_list_nodes_multiselected, windowed_list_nodes_selected,
};
pub use widget_a11y::WidgetA11y;
