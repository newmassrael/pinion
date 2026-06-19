// R1007 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates (proper-noun identifiers in the narrative).
#![allow(clippy::doc_markdown)]

//! `hello-row-dissect` — R1007 §5.27 §5.40 **master-detail row dissection**.
//!
//! The Model/View-at-scale campaign's earlier slices act on the row *set* — the
//! R997 filter removes rows, the R998 coloring tints them, the R1004 search
//! cursor walks them. **Master-detail** turns the other way: pick the *one*
//! selected row up top and read its structured payload exploded into a
//! recursive field tree below. This is Wireshark's packet-detail pane under the
//! packet list, the dlt-viewer message-detail tree, the inspector that follows
//! a selection.
//!
//! ## What is new vs. reused
//!
//! The genuinely new substrate is
//! [`dissect_row`](pinion_core::widgets::row_dissect::dissect_row): it
//! transduces a row's [`serde_json::Value`] payload into a
//! [`DissectNode`](pinion_core::widgets::row_dissect::DissectNode) tree — one
//! branch per object field, one per array element, a leaf per scalar — the peer
//! of `scene_to_tree_item` for the Model/View data domain. Everything
//! downstream is mature substrate reused verbatim: the
//! [`TreeNode`](pinion_core::widgets::tree_nav::TreeNode) collapse / flat-walk,
//! the [`view_tree_focused`] paint, and **two**
//! [`TreeRowClickExternal`] routers — one over the master list (a click
//! `select`s a row), one over the detail tree (a click `toggle`s a branch).
//! The master list itself is a modest fixed list; windowing it to millions of
//! rows is the already-proven virtualization axis (R923/R934), orthogonal to
//! the dissection this slice adds.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! The [`RowDissectionExternal`] surfaces the whole master-detail state to
//! `scene/query` / `scene/invoke`: `query("selected")` is the master row,
//! `query("node_count")` the visible detail-row count, `query("name.<i>")` /
//! `query("value.<i>")` / `query("kind.<i>")` / `query("depth.<i>")` the i-th
//! visible field; `invoke "select" <row>` re-dissects and `invoke "toggle" /
//! "expand" / "collapse" <path>` drive a branch — the dissection is observable
//! and driveable without a pixel (see `tools/demos/r1007_row_dissect.py`).

use pinion_a11y::{
    listbox_option_nodes, tree_access_nodes, AccessAction, AccessNode, AriaRole, ListOption,
    WidgetA11y,
};
use pinion_core::command::Command;
use pinion_core::external::{External, IntrospectValue};
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::row_dissect::{
    use_row_dissection, DissectNode, RowDissectionExternal, RowDissectionState,
};
use pinion_core::widgets::tree_nav::flat_visible;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, SizeStrategy, WidgetView};
use pinion_widget_paint::tree_view::{
    composite_row_tag, view_tree_focused, TreeItem, TreeRowClickExternal, TreeViewFocus,
    TreeViewStyle,
};
use serde_json::{json, Value};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloRowDissectRenderer, HelloRowDissectRendererError);

const WIN_W: u32 = 660;
const WIN_H: u32 = 460;
const THEME_TAG: &str = "app";

/// The invisible focusable root + a11y group anchor.
const ROOT_TAG: &str = "dissect_root";
/// The master-list click router + a11y listbox tag. A row is tagged
/// `{MASTER_TAG}#{i}` so a click `select`s row `i`.
const MASTER_TAG: &str = "master";
/// The detail-tree click router + paint + a11y tag. A row is tagged
/// `{TREE_TAG}#{path}` so a click `toggle`s that branch.
const TREE_TAG: &str = "detail_tree";
/// The [`RowDissectionExternal`] coordinator (RPC surface + `use_row_dissection`
/// cache key).
const DISSECT_TAG: &str = "dissect";

/// Dotted wire-form intent tags the [`WidgetCore::update`] reducer matches —
/// the master-row click (`select`) and the detail-branch click (`toggle`). The
/// [`intent_tag!`] macro is literal-only, so the tag literals must match
/// [`MASTER_TAG`] / [`TREE_TAG`].
const MASTER_CLICK_INTENT_TAG: &str = intent_tag!("master", "click");
const TREE_CLICK_INTENT_TAG: &str = intent_tag!("detail_tree", "click");

const MASTER_W: u32 = 240;
const HEADER_FONT_PX: u32 = 13;
const ROW_FONT_PX: u32 = 13;

/// Sample data: five HTTP-request-log records, each a structured payload with
/// nested objects (`headers` / `body` / `error`), arrays (`tags` / `attempts`)
/// and scalars of every JSON kind — so the dissection exercises each
/// branch / leaf path.
fn rows() -> Vec<Value> {
    vec![
        json!({
            "method": "GET", "url": "/api/users", "status": 200, "secure": true,
            "duration_ms": 12.4,
            "headers": { "accept": "application/json", "host": "example.com" },
            "tags": ["api", "list"]
        }),
        json!({
            "method": "POST", "url": "/api/login", "status": 401, "secure": true,
            "duration_ms": 88.1,
            "headers": { "content-type": "application/json" },
            "body": { "user": "alice", "remember": false }
        }),
        json!({
            "method": "GET", "url": "/static/app.js", "status": 304, "secure": false,
            "duration_ms": 3.0,
            "headers": { "if-none-match": "abc123" },
            "tags": []
        }),
        json!({
            "method": "DELETE", "url": "/api/users/42", "status": 204, "secure": true,
            "duration_ms": 21.7,
            "headers": { "authorization": "Bearer xyz" }
        }),
        json!({
            "method": "GET", "url": "/api/metrics", "status": 500, "secure": true,
            "duration_ms": 140.0,
            "error": { "code": "DB_TIMEOUT", "retryable": true, "attempts": [1, 2, 3] }
        }),
    ]
}

/// The shared dissection proxy (rows + selection + detail tree). The view + the
/// [`RowDissectionExternal`] + the reducer all reach the same `Rc`.
fn use_dissection() -> Rc<RowDissectionState> {
    use_row_dissection(DISSECT_TAG, rows)
}

/// One-line master-row summary `"<method> <url>  → <status>"`.
fn row_summary(v: &Value) -> String {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("");
    let status = v.get("status").and_then(Value::as_i64).unwrap_or(0);
    format!("{} {}  \u{2192} {status}", s("method"), s("url"))
}

/// Project the retained [`DissectNode`] tree onto the paint
/// [`TreeItem`] — `label` is the combined `"name: value"` / `"name  (n)"`
/// display, so the painter shows the field and its value on one row.
fn to_tree_items(nodes: &[DissectNode]) -> Vec<TreeItem> {
    nodes
        .iter()
        .map(|n| {
            if n.children.is_empty() {
                TreeItem::leaf(n.path.clone(), n.display())
            } else {
                TreeItem::branch(n.path.clone(), n.display(), n.expanded, to_tree_items(&n.children))
            }
        })
        .collect()
}

/// The master list: one clickable row per record (`{MASTER_TAG}#{i}`), the
/// selected row filled with the primary-container tint.
fn master_list(state: &RowDissectionState, theme: &Theme) -> Scene {
    let selected = state.selected();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let sel_fill = theme.resolve(ColorRole::Accent);
    let sel_text = theme.resolve(ColorRole::OnAccent);

    let header = Scene::Text(TextNode::styled(
        "Records",
        Rect::default(),
        TextStyle::new().with_size_px(HEADER_FONT_PX).with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    let mut children = vec![header];
    for (i, v) in rows().iter().enumerate() {
        let is_sel = selected == Some(i);
        let fg = if is_sel { sel_text } else { on_surface };
        let text = Scene::Text(TextNode::styled(
            row_summary(v),
            Rect::default(),
            TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(fg),
        ));
        let mut row = ContainerNode::new(vec![text])
            .with_tag(composite_row_tag(MASTER_TAG, &i.to_string()))
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::auto().with_height(SizeValue::Px(32)))
                    .with_align_items(AlignItems::Center)
                    .with_padding(Rect::new(8, 0, 8, 0)),
            );
        if is_sel {
            row = row.with_style(BoxStyle::filled(sel_fill));
        }
        children.push(Scene::Container(row));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(MASTER_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainer)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_size(Size::auto().with_width(SizeValue::Px(MASTER_W)))
                    .with_flex_grow(0.0)
                    .with_gap(2)
                    .with_padding(Rect::new(8, 8, 8, 8)),
            ),
    )
}

/// The detail panel: the dissection tree of the selected row, or a hint when
/// nothing is selected.
fn detail_panel(state: &RowDissectionState, theme: &Theme) -> Scene {
    let on_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let header = Scene::Text(TextNode::styled(
        match state.selected() {
            Some(i) => format!("Detail \u{00B7} row {i} \u{00B7} {} nodes", state.node_count()),
            None => "Detail \u{00B7} (no selection)".to_owned(),
        },
        Rect::default(),
        TextStyle::new().with_size_px(HEADER_FONT_PX).with_fg(on_muted),
    ));

    let body = if state.selected().is_some() {
        let items = to_tree_items(&state.tree());
        view_tree_focused(TREE_TAG, &items, theme, &TreeViewStyle::m3_default(), &TreeViewFocus { focused_id: None })
    } else {
        Scene::Text(TextNode::styled(
            "Click a record to dissect its payload",
            Rect::default(),
            TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(on_muted),
        ))
    };

    Scene::Container(
        ContainerNode::new(vec![header, body])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_flex_grow(1.0)
                    .with_gap(6)
                    .with_padding(Rect::new(10, 8, 10, 8)),
            ),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: ButtonState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_dissection();

    // The invisible root keeps the framework's SCXML state surface alive for
    // read_state / RPC introspect (no visible button paints).
    let invisible_root = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(ROOT_TAG)
            .with_layout(LayoutStyle::new().with_size(Size::px(0, 0))),
    );

    let split = Scene::Container(
        ContainerNode::new(vec![master_list(&state, &theme), detail_panel(&state, &theme)])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Stretch)
                    .with_flex_grow(1.0),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![split, invisible_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_align_items(AlignItems::Stretch)),
    )
}

struct RowDissectView;

impl WidgetCore for RowDissectView {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn tag() -> &'static str {
        ROOT_TAG
    }

    /// The invisible root button keeps the SCXML state surface alive.
    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    /// Register the two composite-row click routers (master `select` + detail
    /// `toggle`) and the [`RowDissectionExternal`] RPC coordinator, seeding the
    /// selection on the first record so the detail tree is non-empty on boot.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let state = use_dissection();
        if state.selected().is_none() {
            state.select(Some(0));
        }
        vec![
            ExtraExternal::new(MASTER_TAG, Box::new(TreeRowClickExternal::new())),
            ExtraExternal::new(TREE_TAG, Box::new(TreeRowClickExternal::new())),
            ExtraExternal::new(DISSECT_TAG, Box::new(RowDissectionExternal::new(state))),
        ]
    }

    fn read_state(scene: &Scene) -> Self::State {
        if let Some(node) = scene.find_external_with_tag(ROOT_TAG)
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return <Self::State as pinion_core::WidgetStateName>::from_name_or_default(&name);
        }
        ButtonState::Idle
    }

    fn event_name(event: Self::Event) -> &'static str {
        <Self::Event as pinion_core::WidgetEventName>::as_name(&event)
    }

    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, frame)
    }

    /// Bridge the two click routers into the shared [`RowDissectionState`]: a
    /// master-row click `select`s that record, a detail-branch click `toggle`s
    /// that branch. Side-effect-only ([[scxml-as-model-update-transient]]) — the
    /// `Signal` writes inside `select` / `toggle_path` are the mutation.
    fn update(_state: Self::State, intent: &Intent) -> Vec<Command> {
        let state = use_dissection();
        if intent.tag_str() == MASTER_CLICK_INTENT_TAG {
            if let IntrospectValue::Text(id) = &intent.payload
                && let Ok(i) = id.parse::<usize>()
            {
                state.select(Some(i));
            }
        } else if intent.tag_str() == TREE_CLICK_INTENT_TAG
            && let IntrospectValue::Text(id) = &intent.payload
        {
            state.toggle_path(id);
        }
        Vec::new()
    }

    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-row-dissect (R1007 §5.27 master-detail row dissection)"
    }

    fn fmt_state_log(_state: &Self::State) -> String {
        "display-only (selection + detail tree in the dissection proxy)".to_string()
    }
}

impl WidgetA11y for RowDissectView {
    /// A `group` root referencing the master `listbox` (one `option` per record,
    /// the selected one `aria-selected`) and the detail `tree` (one `treeitem`
    /// per visible field, carrying the WAI-ARIA hierarchical axes derived from
    /// the same [`flat_visible`] SSOT the painter walks). The list / tree row
    /// tags equal the painted composite tags, so an AT `Click` resolves to the
    /// painted row's bounds and routes through [`access_child_invoke`](RowDissectView::access_child_invoke).
    fn access_node(_state: &<Self as WidgetCore>::State, _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_dissection();
        let selected = state.selected();
        let data = rows();
        let tags: Vec<String> =
            (0..data.len()).map(|i| composite_row_tag(MASTER_TAG, &i.to_string())).collect();
        let labels: Vec<String> = data.iter().map(row_summary).collect();
        let options: Vec<ListOption<'_>> = (0..data.len())
            .map(|i| ListOption {
                tag: &tags[i],
                label: Some(&labels[i]),
                state: ListboxItemState::Idle,
                selected: selected == Some(i),
                focused: false,
            })
            .collect();

        let mut nodes = vec![AccessNode::new(ROOT_TAG, AriaRole::Group)
            .with_name("Request dissection")
            .with_child(MASTER_TAG)
            .with_child(TREE_TAG)];
        nodes.extend(listbox_option_nodes(MASTER_TAG, "Records", false, &options));
        // Flatten the SAME TreeItem list the painter walks (R809.1 single
        // flattening), so each treeitem's AT name is the painted "name: value"
        // row text, not the bare field name DissectNode::label() carries.
        let items = to_tree_items(&state.tree());
        nodes.extend(tree_access_nodes(
            TREE_TAG,
            TREE_TAG,
            Some("Field dissection"),
            &flat_visible(&items),
            None,
            None,
        ));
        nodes
    }

    /// AT-activation: a `Click` / `Default` on a master row `select`s it; on a
    /// detail-tree branch `toggle`s it — the same `select` / `toggle` wire the
    /// pointer routers and the RPC client drive, reached through the shared
    /// [`RowDissectionExternal`].
    fn access_child_invoke(
        scene: &mut Scene,
        parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        if !matches!(action, AccessAction::Click | AccessAction::Default) {
            return matches!(action, AccessAction::Focus);
        }
        let Some(node) = scene.find_external_with_tag_mut(DISSECT_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match parent_tag {
            MASTER_TAG => match sub_tag.parse::<i64>() {
                Ok(i) => {
                    let _ = intro.invoke("select", IntrospectValue::Int(i));
                    true
                }
                Err(_) => false,
            },
            TREE_TAG => {
                let _ = intro.invoke("toggle", IntrospectValue::Text(sub_tag.to_owned()));
                true
            }
            _ => false,
        }
    }
}

impl WidgetView for RowDissectView {
    type Renderer = HelloRowDissectRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<RowDissectView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    #[test]
    fn boot_selects_first_row_and_dissects_it() {
        Owner::new().run(|| {
            let state = use_dissection();
            assert_eq!(state.row_count(), 5);
            // create_extra_externals seeds the first record.
            if state.selected().is_none() {
                state.select(Some(0));
            }
            assert_eq!(state.selected(), Some(0));
            // Row 0 alphabetical: duration_ms, headers{accept,host}, method,
            // secure, status, tags[api,list], url = 7 top-level + 2 + 2 = 11.
            assert_eq!(state.node_count(), 11);
        });
    }

    #[test]
    fn master_click_intent_selects_row() {
        Owner::new().run(|| {
            let state = use_dissection();
            state.select(Some(0));
            // Row 3 is { method, status, url, secure, duration_ms, headers } —
            // headers has one child (authorization), no tags/array.
            let intent = Intent::new_owned(
                MASTER_CLICK_INTENT_TAG.to_owned(),
                IntrospectValue::Text("3".to_owned()),
            );
            let _ = RowDissectView::update(ButtonState::Idle, &intent);
            assert_eq!(state.selected(), Some(3));
        });
    }

    #[test]
    fn tree_click_intent_toggles_branch() {
        Owner::new().run(|| {
            let state = use_dissection();
            state.select(Some(0));
            let full = state.node_count();
            let intent = Intent::new_owned(
                TREE_CLICK_INTENT_TAG.to_owned(),
                IntrospectValue::Text("headers".to_owned()),
            );
            let _ = RowDissectView::update(ButtonState::Idle, &intent);
            assert_eq!(state.node_count(), full - 2, "collapsing headers hides its 2 children");
        });
    }

    #[test]
    fn row_summary_reads_method_url_status() {
        assert_eq!(row_summary(&rows()[1]), "POST /api/login  \u{2192} 401");
    }
}
