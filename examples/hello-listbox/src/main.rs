//! `hello-listbox` — R51.97 §5.38 `ListBox` composite first-client,
//! R51.191 §5.45 R55.G first `ScrollNode` consumer.
//!
//! N=12 [`ListBox`](pinion_core::widgets::listbox::ListBox) rendered
//! inside a 5-row [`ScrollNode`](pinion_core::scene::ScrollNode)
//! viewport, each row tagged with the paint convention
//! `"main_list#<index>"` from the R51.41 RFC. The
//! state scene carries one composite
//! [`ListBoxExternal`](pinion_core::widgets::listbox::ListBoxExternal)
//! tagged `"main_list"`; the
//! [`InputRouter`](pinion_runtime::InputRouter) splits the `'#'`
//! suffix (R51.42) so a cursor on row `i` drives
//! `invoke("send", Text("<i>:<EventName>"))` against the single
//! composite handle. Framework-owned mutual exclusion (R51.96) snaps
//! `selected_index` to the activated row and emits the `"selected"`
//! intent.
//!
//! **WAI-ARIA Listbox keyboard model — distinct from
//! [`hello-radio-group`]** (the R51.44 first-client for the
//! `RadioGroup` composite):
//!
//! * **`Arrow` keys move focus only.** The W3C ARIA Listbox single-
//!   select pattern says Arrow keys move the AT-side active
//!   descendant without committing the selection. `Home` / `End`
//!   jump to the first / last option, also focus-only. Pinion's
//!   [`ListBox::set_focused_index`] is the framework primitive for
//!   this navigation cursor.
//! * **`Space` / `Enter` commits the focused row.** The activation
//!   runs the full `PointerEnter → Down → Up → Leave` cycle on the
//!   focused index, firing the `"selected"` intent through the
//!   composite. R51.90 §5.40 syncs `focused_index` back to the new
//!   `selected_index` on the activation edge, but the AT-only
//!   divergence (focused 2, selected 0) is the textbook intermediate
//!   state any `apply_key` test can observe.
//!
//! This is the dual of the [`RadioGroup`] keyboard model, where
//! Arrow keys *activate* immediately (W3C ARIA Radio Group
//! convention). The composite primitive API is identical (both expose
//! `send` for activation + `set_focused_index` for focus-only
//! navigation); only the application's `apply_key` mapping diverges
//! per the active ARIA pattern.
//!
//! AccessKit semantics (R51.96.1 §5.40): the parent reports
//! `AriaRole::Listbox` and each child reports `AriaRole::ListBoxOption`
//! — distinct from `RadioGroup` / `RadioButton` even though the
//! underlying widget primitive ([`ListBoxItem`]) shares the button-
//! like statechart with `Radio` / `Toggle` / `Checkbox`. AT clients
//! (`Narrator` / `VoiceOver` / Orca / `TalkBack`) see a proper Listbox
//! widget and route keyboard / pointer / programmatic Focus through
//! the same composite hooks the WAI-ARIA spec defines.
//!
//! R51.93 §5.35 — touch cancellation propagation: an `iOS` /
//! `Android` system-side gesture (4-finger, phone call, notification
//! pull-down) that revokes an in-flight touch on a row mid-press
//! routes `Pressed → Idle` via `PointerCancel` without firing
//! `"selected"`. Inherited from the [`ListBoxItem`] template via
//! R51.93; verified in
//! [`pinion_core::widgets::listbox::tests::r51_93_pointer_cancel_does_not_select`].
//!
//! [`hello-radio-group`]: ../../hello-radio-group/index.html
//! [`RadioGroup`]: pinion_core::widgets::radio_group::RadioGroup
//! [`ListBoxItem`]: pinion_core::widgets::listbox_item::ListBoxItem

use std::cell::RefCell;
use std::time::Instant;

use pinion_a11y::{AccessAction, AccessFocus, AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, ScrollNode, TextNode};
use pinion_core::style::{Border, BoxStyle, TextStyle};
use pinion_core::widgets::listbox::ListBoxExternal;
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::{Color, Frame, Owner, Scene, WidgetCore};
use pinion_shell::typeahead::{is_typeahead_char, TypeaheadCursor};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloListboxRenderer, HelloListboxRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 320;
const BG_FILL: Color = Color::rgb(0x18, 0x24, 0x30);
/// (R51.191 §5.45 R55.G) Bumped from 4 → 12 so the content overflows
/// the scroll viewport and the wheel / Arrow keys actually drive a
/// visible offset change. 12 items also exhausts the type-ahead
/// initial-letter alphabet richer than the four-fruit set.
const N: usize = 12;
const PRIMARY_TAG: &str = "main_list";
/// (R51.191 §5.45 R55.G) Cache key for the scroll container's
/// reactive [`ScrollState`]. Resolves via
/// [`use_scroll_state`] in `view` so the offset survives view
/// re-runs. Distinct from [`PRIMARY_TAG`] — the listbox composite
/// state machine and the scroll input-router live on independent
/// tag namespaces (the input router walks `scroll_state_at` via the
/// attached `Rc<ScrollState>`, not the tag string).
const SCROLL_KEY: &str = "main_list_scroll";
/// (R51.191 §5.45 R55.G) Scroll viewport width, sized to match the
/// row width so the rows fit horizontally and the user sees no
/// horizontal scrollbar (the substrate supports both axes; this
/// demo only exercises vertical).
const VIEWPORT_W: u32 = ROW_WIDTH;
/// (R51.191 §5.45 R55.G) Scroll viewport height — exactly 5 rows
/// plus the 4 inter-row gaps. Picked so the user always sees a
/// half-window of content scrolling past the visible window edge,
/// making the wheel / Arrow input visibly drive the offset.
const VIEWPORT_H: u32 = 5 * ROW_HEIGHT + 4 * ROW_GAP;

// Row visual constants — a horizontal Box (filled background tint
// when focused/selected) holding a label. The visual emphasises the
// Listbox semantic of "the focused row is the AT-side cursor; the
// selected row is the user's commit"; both states have distinct fill
// colours so the divergence is visible without an AT client.
const ROW_HEIGHT: u32 = 28;
const ROW_WIDTH: u32 = 220;
const ROW_GAP: u32 = 6;

/// Cached projection of the list. One `(ListboxItemState, selected)`
/// pair per option plus the R51.87 §5.40 AT-side active-descendant
/// index. `Copy` because both inner types are `Copy`.
///
/// R51.87 §5.40 — `focused` is the WAI-ARIA Listbox active descendant.
/// In the Listbox model `focused` is the **primary** navigation
/// cursor (Arrow keys move it without selecting), so the fallback
/// resolution in [`active_option_index`] prefers `focused` over
/// `selected` and falls through to `0` only when neither is set.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct ListState {
    rows: [(ListboxItemState, bool); N],
    focused: Option<usize>,
}

impl ListState {
    fn idle() -> Self {
        Self {
            rows: [(ListboxItemState::Idle, false); N],
            focused: None,
        }
    }
}

/// view-fn (§6.3): pure sync mapping `ListState -> Scene`. Builds a
/// column of N rows wrapped in a [`ScrollNode`] (R51.191 §5.45
/// R55.G) so content larger than the viewport scrolls under
/// wheel / Arrow input. Each row is tagged `"main_list#<i>"` so the
/// `InputRouter`'s R51.42 sub-index split routes cursor hits on row
/// `i` to `invoke("send", Text("<i>:<EventName>"))` against the
/// single composite `ListBoxExternal`.
///
/// R51.191 R55.G — substrate-incompleteness-signal closure: the
/// view fn no longer uses taffy flex to position the rows because
/// [`crate::layout::compute_layout`] (pinion-runtime) does not yet
/// recurse into [`Scene::Scroll`] children. Manual positioning
/// inside the scroll content keeps the demo runtime-stable. The
/// taffy-into-Scroll integration is the R55.G.2 carry — when it
/// lands, this view fn can revert to a flex layout inside the
/// scroll content without changing the substrate API.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ListState, _frame: &Frame) -> Scene {
    let active = active_option_index(state);
    // (R51.190) `from_state` derives both offset and tag from the
    // cached `ScrollState`. `set_max` updates the upper bound from
    // the current content size — when N is constant the bound never
    // changes, but the call is idempotent and survives future
    // dynamic-N demos.
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let content_h = u32::try_from(N).unwrap_or(0) * ROW_HEIGHT
        + u32::try_from(N.saturating_sub(1)).unwrap_or(0) * ROW_GAP;
    scroll_state.set_max(
        0,
        i32::try_from(content_h.saturating_sub(VIEWPORT_H)).unwrap_or(0),
    );

    // Manual per-row positioning (content-intrinsic y starts at 0).
    let rows: Vec<Scene> = (0..N)
        .map(|i| {
            let y = u32::try_from(i).unwrap_or(0) * (ROW_HEIGHT + ROW_GAP);
            listbox_row_at_y(i, y, state.rows[i].0, state.rows[i].1, Some(i) == Some(active))
        })
        .collect();
    let mut content_container = ContainerNode::new(rows);
    content_container.rect = Rect::new(0, 0, ROW_WIDTH, content_h);
    let content = Scene::Container(content_container);

    // Center the scroll viewport inside the window.
    let vp_x = WIN_W.saturating_sub(VIEWPORT_W) / 2;
    let vp_y = WIN_H.saturating_sub(VIEWPORT_H) / 2;
    let scroll = ScrollNode::from_state(
        scroll_state,
        Rect::new(vp_x, vp_y, VIEWPORT_W, VIEWPORT_H),
        content,
    );

    let mut outer =
        ContainerNode::new(vec![Scene::Scroll(scroll)]).with_style(BoxStyle::filled(BG_FILL));
    outer.rect = Rect::new(0, 0, WIN_W, WIN_H);
    Scene::Container(outer)
}

/// One row of the composite — filled box (focused / selected tint) +
/// label. Tagged `"main_list#<i>"` so the `InputRouter` R51.42 sub-
/// index split reaches the composite `ListBoxExternal` with the right
/// item index in the payload.
///
/// Visual axis (R51.97 §5.38 — Listbox-distinctive):
///
/// * Idle row: dark fill, light grey label.
/// * Hover row: lighter fill (cursor over the row).
/// * Pressed row: darker fill (active button-down).
/// * Focused row (AT-side active descendant): subtle blue tint +
///   left border accent — the WAI-ARIA Listbox model's visual hint
///   that this row is the *navigation cursor* (Arrow keys move
///   here) even when it is not the committed selection.
/// * Selected row: solid blue fill + white label — the committed
///   selection (changed only by `Space` / `Enter` activate).
///
/// `focused` and `selected` can diverge in this composite (unlike
/// `RadioGroup` where Arrow activates immediately); the two
/// distinct visual states make the divergence observable.
/// (R51.191 §5.45 R55.G) Build one row at content-intrinsic
/// position `(0, y)`. Replaces the pre-R51.191 `listbox_row` flex
/// helper — scroll content is not currently taffy-laid-out (see
/// [`view`]'s carry note), so the row and its label rect are set
/// manually. The visual rules (fill / label / border priority) are
/// identical to the pre-R51.191 helper.
fn listbox_row_at_y(
    index: usize,
    y: u32,
    state: ListboxItemState,
    selected: bool,
    focused: bool,
) -> Scene {
    // Background fill priority: selected > pressed > hover > focused
    // > idle. The selected state always wins (committed truth);
    // focused is the secondary cursor hint that only shows when the
    // row is not yet selected.
    let (fill, label_color, border) = if selected {
        (
            Color::rgb(0x30, 0x70, 0xd0),
            Color::rgb(0xff, 0xff, 0xff),
            None,
        )
    } else {
        let fill = match state {
            ListboxItemState::Pressed => Color::rgb(0x2a, 0x38, 0x48),
            ListboxItemState::Hover => Color::rgb(0x2c, 0x3e, 0x52),
            ListboxItemState::Disabled => Color::rgb(0x20, 0x28, 0x30),
            ListboxItemState::Idle => {
                if focused {
                    Color::rgb(0x24, 0x36, 0x4a)
                } else {
                    Color::rgb(0x1f, 0x2c, 0x3a)
                }
            }
        };
        let label_color = match state {
            ListboxItemState::Disabled => Color::rgb(0x70, 0x76, 0x80),
            _ => Color::rgb(0xe0, 0xe6, 0xee),
        };
        let border = if focused {
            // R51.87 §5.40 — focused-row visual cue. The 2-px left
            // accent doubles as the WAI-ARIA "active descendant"
            // hint without consuming the row's selection slot.
            Some(Border::new(Color::rgb(0x40, 0x80, 0xe0), 2))
        } else {
            None
        };
        (fill, label_color, border)
    };
    // Label rect inside the row — 12 px left/right padding (mirrors
    // the pre-R51.191 `with_padding(Rect::new(12, 4, 12, 4))`),
    // vertically centred for the 15-px font in the 28-px row.
    // `w = 0` keeps parley single-line (no wrap).
    let label_baseline_y = y + (ROW_HEIGHT - 15) / 2;
    let label = Scene::Text(TextNode::styled(
        option_label(index),
        Rect::new(12, label_baseline_y, 0, ROW_HEIGHT),
        TextStyle::new().with_size_px(15).with_fg(label_color),
    ));
    let row_tag = format!("{PRIMARY_TAG}#{index}");
    let mut row_style = BoxStyle::filled(fill).with_corner_radius(4);
    if let Some(b) = border {
        row_style = row_style.with_border(b);
    }
    let mut row_container = ContainerNode::new(vec![label])
        .with_tag(row_tag)
        .with_style(row_style);
    row_container.rect = Rect::new(0, y, ROW_WIDTH, ROW_HEIGHT);
    Scene::Container(row_container)
}

/// (R51.191 §5.45 R55.G) Twelve fruit labels — alphabetised so
/// the WAI-ARIA type-ahead jump cycles through distinct initial
/// letters. Out-of-range indices return `"?"` as a safe fallback
/// (only reachable if `N` exceeds 12; today they agree).
fn option_label(index: usize) -> &'static str {
    match index {
        0 => "Apple",
        1 => "Banana",
        2 => "Cherry",
        3 => "Date",
        4 => "Elderberry",
        5 => "Fig",
        6 => "Grape",
        7 => "Honeydew",
        8 => "Kiwi",
        9 => "Lemon",
        10 => "Mango",
        11 => "Nectarine",
        _ => "?",
    }
}

struct ListBoxView;

impl WidgetCore for ListBoxView {
    type State = ListState;
    // The composite drives every state change through `apply_key`
    // (typed wire format `"<i>:<EventName>"` for commits, direct
    // `intervene "focused_index"` for navigation), so the keybinding-
    // channel typed event slot stays unused. `()` satisfies the
    // trait's `Copy` bound without an unused variant.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ListBoxExternal::new(N))
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> ListState {
        let mut out = ListState::idle();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        for (i, slot) in out.rows.iter_mut().enumerate() {
            let state = match intro.query(&format!("state.{i}")) {
                Some(IntrospectValue::Text(name)) => parse_listbox_item_state(&name),
                _ => ListboxItemState::Idle,
            };
            let selected = matches!(
                intro.query(&format!("selected.{i}")),
                Some(IntrospectValue::Bool(true)),
            );
            *slot = (state, selected);
        }
        // R51.87 §5.40 — AT-side active descendant. `Null` until
        // Arrow navigation or AT `Focus` lands. First-class for
        // Listbox (the navigation cursor); falls back to `selected`-
        // or-0 in `active_option_index` when `None`.
        out.focused = match intro.query("focused_index") {
            Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
            _ => None,
        };
        out
    }

    fn view(state: ListState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-listbox (R51.97 §5.38 Listbox composite)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        // Every key the composite cares about flows through
        // `apply_key` so the enum-typed channel stays unused.
        None
    }

    /// WAI-ARIA Listbox keyboard model (R51.97 §5.38 —
    /// distinguishing axis vs `hello-radio-group`):
    ///
    /// * `ArrowDown` / `ArrowRight` — move `focused_index` one step
    ///   forward, wrapping at the end (the W3C ARIA Listbox cyclic
    ///   navigation convention).
    /// * `ArrowUp` / `ArrowLeft` — symmetric reverse step.
    /// * `Home` — move `focused_index` to the first option.
    /// * `End` — move `focused_index` to the last option.
    /// * `Space` / `Enter` — commit: activate the currently
    ///   `focused` option (or `0` if no focus yet). This runs the
    ///   full `PointerEnter / Down / Up / Leave` cycle through the
    ///   composite wire format so the `"selected"` intent fires the
    ///   same way a mouse click on the row would.
    /// * R51.99 §5.38 — printable letter / digit — type-ahead jump
    ///   to the next option whose label starts with that character
    ///   (case-insensitive, ASCII fold), wrapping cyclically from
    ///   `focused_index + 1`. Successive presses of the same letter
    ///   cycle through every matching option. WAI-ARIA Authoring
    ///   Practices Listbox optional convention; bundled because the
    ///   four fruit labels have unique first letters and the model
    ///   completes the textbook keyboard surface for the demo. Does
    ///   not commit — pairs with `Space` / `Enter` after the jump.
    ///
    /// Unrecognised keys return `false` so the shell's swallow path
    /// matches the unrecognised-keybinding contract.
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
        // ARIA Listbox roving tabindex: the composite is a single
        // tab stop. Keys only route when the listbox itself is
        // focused (no sibling-widget aliasing).
        if focused != Some(Self::tag()) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        match key {
            "ArrowDown" | "ArrowRight" => move_focus(node, 1),
            "ArrowUp" | "ArrowLeft" => move_focus(node, -1),
            "Home" => set_focus(node, 0),
            "End" => set_focus(node, N - 1),
            "Space" | "Enter" => commit_focused(node),
            // R51.99 §5.38 — single-character keys fall through to
            // the WAI-ARIA optional type-ahead jump. The match arm
            // sits last so named keys (`ArrowDown`, `Home`, etc.)
            // win on key-string length disambiguation.
            other => type_ahead_jump(node, other),
        }
    }

    fn fmt_state_log(state: &ListState) -> String {
        let rows = state
            .rows
            .iter()
            .enumerate()
            .map(|(i, (s, sel))| {
                format!(
                    "{i}={}{}",
                    listbox_state_short(*s),
                    if *sel { "+" } else { "-" },
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        match state.focused {
            Some(idx) => format!("{rows} focused={idx}"),
            None => rows,
        }
    }
}

impl WidgetA11y for ListBoxView {
    /// R51.96.1 §5.40 — composite AccessKit semantic tree
    /// contribution. Emits N+1 nodes: one `AriaRole::Listbox` parent
    /// holding every option's sub-tag as a child, plus one
    /// `AriaRole::ListBoxOption` per index. The parent claims its
    /// children via [`AccessNode::with_child`] so the tree builder
    /// routes them under the listbox instead of the synthetic root.
    ///
    /// R51.69 §5.40 — per-option accessible names are derived by
    /// `enrich_names_from_scene` from each row's `option_label(i)`
    /// `TextNode`. The parent listbox's `"Fruit picker"` name has no
    /// paint-scene equivalent (the listbox is a logical container)
    /// so it stays as an explicit override on the parent
    /// `AccessNode`.
    fn access_node(state: &ListState, focused: Option<&str>) -> Vec<AccessNode> {
        let list_focused = focused == Some(<Self as WidgetCore>::tag());
        let active_idx = active_option_index(*state);
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(N + 1);
        let mut list = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Listbox)
            .with_name("Fruit picker");
        for i in 0..N {
            list = list.with_child(format!("{PRIMARY_TAG}#{i}"));
        }
        nodes.push(list);
        for (i, (item_state, selected)) in state.rows.iter().copied().enumerate() {
            let option_tag = format!("{PRIMARY_TAG}#{i}");
            // R51.98 §5.40 — ListBoxOption uses WAI-ARIA
            // `aria-selected` (container-membership axis), not
            // `aria-checked` (two-state truthy axis used by Switch /
            // CheckBox / RadioButton).
            let access_state = AccessState {
                focused: list_focused && i == active_idx,
                disabled: matches!(item_state, ListboxItemState::Disabled),
                hovered: matches!(item_state, ListboxItemState::Hover),
                pressed: matches!(item_state, ListboxItemState::Pressed),
                checked: None,
            };
            nodes.push(
                AccessNode::new(&option_tag, AriaRole::ListBoxOption)
                    .with_selected(selected)
                    .with_state(access_state),
            );
        }
        nodes
    }

    /// R51.71 §5.40 — composite focus model. When the listbox itself
    /// is focused, return [`AccessFocus::composite`] with the parent
    /// tag as the `TreeUpdate::focus` target and the active option's
    /// sub-tag as the `aria-activedescendant`.
    fn access_focus_target(
        state: &ListState,
        focused: Option<&str>,
    ) -> Option<AccessFocus> {
        if focused == Some(<Self as WidgetCore>::tag()) {
            let idx = active_option_index(*state);
            Some(AccessFocus::composite(
                <Self as WidgetCore>::tag(),
                format!("{PRIMARY_TAG}#{idx}"),
            ))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// R51.70 §5.40 — composite child action dispatch.
    fn access_child_invoke(scene: &mut Scene, sub_tag: &str, action: AccessAction) -> bool {
        let Ok(idx) = sub_tag.parse::<usize>() else {
            return false;
        };
        if idx >= N {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match action {
            AccessAction::Click | AccessAction::Default => {
                for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                    let _ = intro.invoke(
                        "send",
                        IntrospectValue::Text(format!("{idx}:{ev}")),
                    );
                }
                true
            }
            AccessAction::Focus => {
                if let Ok(i) = i64::try_from(idx) {
                    let _ = intro.intervene(
                        "focused_index",
                        IntrospectValue::Int(i),
                    );
                }
                true
            }
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => {
                false
            }
        }
    }
}

impl WidgetView for ListBoxView {
    type Renderer = HelloListboxRenderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }
}

/// Move the listbox's `focused_index` by `direction` (`+1` for Down /
/// Right, `-1` for Up / Left). Wraps cyclically. When no focus is
/// set yet, lands on `0` for forward direction and `N - 1` for
/// reverse direction (W3C ARIA Listbox first-Arrow boundary).
fn move_focus(
    node: &mut pinion_core::scene::ExternalNode,
    direction: i32,
) -> bool {
    let current: Option<usize> = node
        .handle
        .introspect()
        .and_then(|i| i.query("focused_index"))
        .and_then(|v| match v {
            IntrospectValue::Int(i) => usize::try_from(i).ok(),
            _ => None,
        });
    let next = match (current, direction) {
        (Some(c), 1) => (c + 1) % N,
        (Some(c), -1) => (c + N - 1) % N,
        (None, 1) => 0,
        (None, -1) => N - 1,
        _ => 0,
    };
    set_focus(node, next)
}

/// Set the listbox's `focused_index` to `idx` via the composite
/// intervene path. Returns `true` (key was handled) regardless of
/// whether the index actually changed — the focus mutation is the
/// commit-class signal for Arrow keys.
fn set_focus(node: &mut pinion_core::scene::ExternalNode, idx: usize) -> bool {
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let Ok(i) = i64::try_from(idx) else {
        return false;
    };
    let _ = intro.intervene("focused_index", IntrospectValue::Int(i));
    true
}

// R51.106 §5.38 — type-ahead state lifted to
// `pinion_shell::typeahead` substrate after the second consumer
// (hello-listbox-multi) landed; this binary now holds only the
// per-binding typeahead cursor and delegates the algorithm + the
// reset window constant to the substrate. R51.99/R51.103 historical
// behavior (single-char cyclic + multi-char prefix + 500 ms
// timeout + Unicode case fold) is preserved verbatim through the
// `TypeaheadCursor::step` contract.
//
// R51.152 §5.22 — owner-cache key for the typeahead cursor. Replaces
// the pre-R51.152 `thread_local! TYPEAHEAD` workaround per the
// R51.150 `[[textbook-long-term-correct]]` recovery: the cursor now
// lives on the binding's root [`Owner`] and drops with the shell
// (no stale state across multiple shells in the same thread).
const TYPEAHEAD_KEY: &str = "hello_listbox::typeahead";

fn type_ahead_jump(node: &mut pinion_core::scene::ExternalNode, key: &str) -> bool {
    let Some(first) = is_typeahead_char(key) else {
        return false;
    };
    let current = node
        .handle
        .introspect()
        .and_then(|i| i.query("focused_index"))
        .and_then(|v| match v {
            IntrospectValue::Int(i) => usize::try_from(i).ok(),
            _ => None,
        });
    let labels: [&str; N] = std::array::from_fn(option_label);
    // R51.152 — `Owner::current()` resolves to the shell's root scope
    // because `CoreShell::apply_key` wraps the dispatch in
    // `root_owner().run(...)`. Panicking here means the apply_key
    // path is bypassing the framework wrap (a broken integration).
    let owner = Owner::current()
        .expect("hello-listbox apply_key must run inside CoreShell::apply_key wrap (R51.152)");
    let cursor_cell: std::rc::Rc<RefCell<TypeaheadCursor>> =
        owner.cache(TYPEAHEAD_KEY, || RefCell::new(TypeaheadCursor::new()));
    let target = cursor_cell
        .borrow_mut()
        .step(first, current, Instant::now(), &labels);
    let Some(idx) = target else {
        return false;
    };
    set_focus(node, idx)
}

/// Commit the currently focused option (Space / Enter on a focused
/// listbox). Runs the full activation cycle on the focused index
/// through the composite wire format so the `"selected"` intent fires
/// the same way a mouse click on the row would. R51.90 syncs the
/// `focused_index` back to the activated index automatically.
///
/// When no focus is set yet, commits index `0` (matches the W3C
/// convention: first Space on an unfocused listbox lands on the
/// first option).
fn commit_focused(node: &mut pinion_core::scene::ExternalNode) -> bool {
    let current: Option<usize> = node
        .handle
        .introspect()
        .and_then(|i| i.query("focused_index"))
        .and_then(|v| match v {
            IntrospectValue::Int(i) => usize::try_from(i).ok(),
            _ => None,
        });
    let idx = current.unwrap_or(0);
    if idx >= N {
        return false;
    }
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
        let _ = intro.invoke(
            "send",
            IntrospectValue::Text(format!("{idx}:{ev}")),
        );
    }
    true
}

/// R51.96 §5.40 — active option index (the row reported as the
/// AT-side "active descendant" of the focused listbox). Resolution
/// order — distinct from `hello-radio-group`'s
/// `active_radio_index` in that **focused wins over selected**:
///
/// 1. `state.focused` if `Some(_)` (Arrow-key navigation or AT
///    `Focus` action pinned a row). This is the WAI-ARIA Listbox
///    primary cursor.
/// 2. The currently selected row, if any (post-commit display).
/// 3. `0` (start of the cyclic ring — same boundary `move_focus`
///    uses when no focus is set and `ArrowDown` lands first).
///
/// The keyboard navigation, the visual focused-row tint, and the
/// AccessKit `access_focus_target` redirect all agree on this
/// resolution so the three views never diverge.
fn active_option_index(state: ListState) -> usize {
    if let Some(idx) = state.focused {
        return idx;
    }
    state.rows.iter().position(|(_, sel)| *sel).unwrap_or(0)
}

fn parse_listbox_item_state(name: &str) -> ListboxItemState {
    match name {
        "Hover" => ListboxItemState::Hover,
        "Pressed" => ListboxItemState::Pressed,
        "Disabled" => ListboxItemState::Disabled,
        _ => ListboxItemState::Idle,
    }
}

fn listbox_state_short(state: ListboxItemState) -> &'static str {
    match state {
        ListboxItemState::Idle => "I",
        ListboxItemState::Hover => "H",
        ListboxItemState::Pressed => "P",
        ListboxItemState::Disabled => "D",
    }
}

fn main() {
    pinion_shell::run::<ListBoxView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn unselected_state() -> ListState {
        ListState::idle()
    }

    fn selected_state(idx: usize) -> ListState {
        let mut s = unselected_state();
        s.rows[idx].1 = true;
        s
    }

    fn focused_state(focused_idx: usize) -> ListState {
        let mut s = unselected_state();
        s.focused = Some(focused_idx);
        s
    }

    #[test]
    fn emits_one_listbox_plus_n_option_nodes() {
        let nodes = ListBoxView::access_node(&unselected_state(), None);
        assert_eq!(nodes.len(), N + 1, "1 listbox parent + N option children");
        assert_eq!(nodes[0].role, AriaRole::Listbox);
        for node in &nodes[1..=N] {
            assert_eq!(node.role, AriaRole::ListBoxOption);
        }
    }

    #[test]
    fn listbox_parent_claims_all_option_tags_as_children() {
        let nodes = ListBoxView::access_node(&unselected_state(), None);
        let parent = &nodes[0];
        assert_eq!(parent.children.len(), N);
        for i in 0..N {
            assert!(
                parent.children.iter().any(|c| c == &format!("{PRIMARY_TAG}#{i}")),
                "listbox parent must claim {PRIMARY_TAG}#{i}",
            );
        }
    }

    #[test]
    fn parent_carries_explicit_name_override() {
        let nodes = ListBoxView::access_node(&unselected_state(), None);
        assert_eq!(nodes[0].name.as_deref(), Some("Fruit picker"));
    }

    #[test]
    fn r51_98_options_report_selected_via_aria_selected() {
        let nodes = ListBoxView::access_node(&selected_state(2), None);
        // nodes[0] is the listbox parent; option N starts at nodes[1].
        for i in 0..N {
            let opt = &nodes[i + 1];
            let expect_selected = i == 2;
            assert_eq!(
                opt.selected,
                Some(expect_selected),
                "option {i} aria-selected must match selection",
            );
            // R51.98 §5.40 — ListBoxOption no longer carries the
            // `aria-checked` axis or a `Bool` `AccessValue`. WAI-ARIA
            // explicitly distinguishes the two; previously the test
            // pinned the wrong axis.
            assert_eq!(
                opt.state.checked, None,
                "option {i} must not carry aria-checked (wrong axis)",
            );
            assert_eq!(
                opt.value, None,
                "option {i} value field unused (name comes from label)",
            );
        }
    }

    #[test]
    fn focused_listbox_returns_composite_focus_with_active_descendant() {
        let target = ListBoxView::access_focus_target(
            &focused_state(2),
            Some(ListBoxView::tag()),
        )
        .expect("listbox focused → composite focus");
        assert_eq!(target.focus_tag, ListBoxView::tag());
        assert_eq!(
            target.active_descendant.as_deref(),
            Some(format!("{PRIMARY_TAG}#2").as_str())
        );
    }

    #[test]
    fn unfocused_listbox_returns_atomic_for_sibling_tag() {
        let target = ListBoxView::access_focus_target(
            &unselected_state(),
            Some("other_widget"),
        )
        .expect("sibling-focused → atomic focus on the sibling");
        assert_eq!(target.focus_tag, "other_widget");
        assert!(target.active_descendant.is_none());
    }

    #[test]
    fn active_option_resolution_prefers_focused_over_selected() {
        // Selected = 0, focused = 2 → active = focused (2).
        let mut s = selected_state(0);
        s.focused = Some(2);
        assert_eq!(active_option_index(s), 2);
    }

    #[test]
    fn active_option_fallback_to_selected_when_no_focus() {
        let s = selected_state(1);
        assert_eq!(active_option_index(s), 1);
    }

    #[test]
    fn active_option_fallback_to_zero_when_nothing_set() {
        assert_eq!(active_option_index(unselected_state()), 0);
    }

    // R51.106 §5.38 — type-ahead algorithm tests moved to
    // [`pinion_shell::typeahead::tests`]; this binary now imports the
    // substrate and only carries the thread-local cursor + the
    // `type_ahead_jump` wiring (covered indirectly by the integration
    // path — no application-side algorithm duplication).

    // ─────────────────────────────────────────────────────────────
    // R51.191 §5.45 R55.G — view-fn smoke tests confirming the
    // ScrollNode + ScrollState wiring lands the expected scene
    // shape. The view fn runs inside a fresh `Owner` because
    // `use_scroll_state` requires an active scope (R51.146
    // callback-root-owner-wrap discipline).
    // ─────────────────────────────────────────────────────────────

    fn run_view(state: ListState) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(state, &Frame::default()))
    }

    fn find_scroll(scene: &Scene) -> Option<&pinion_core::scene::ScrollNode> {
        match scene {
            Scene::Scroll(s) => Some(s),
            Scene::Container(c) => c.children.iter().find_map(find_scroll),
            _ => None,
        }
    }

    #[test]
    fn r51_191_view_wraps_rows_in_scroll_node() {
        // The scene must contain a `Scene::Scroll` so wheel + key
        // events reach the scroll input-router rather than dropping.
        let scene = run_view(unselected_state());
        let scroll = find_scroll(&scene).expect("view must contain Scene::Scroll");
        // Viewport sized to 5 rows × (height + gap) - gap.
        assert_eq!(scroll.viewport.w, VIEWPORT_W);
        assert_eq!(scroll.viewport.h, VIEWPORT_H);
        // State attached so `scroll_state_at` can resolve a target
        // when the input router walks the scene.
        assert!(scroll.state.is_some(), "ScrollNode must carry a state Rc");
        // Tag derived from the cache key — closes the R51.190
        // boilerplate gap (no string repeat at the view fn).
        assert_eq!(scroll.tag.as_deref(), Some(SCROLL_KEY));
    }

    #[test]
    fn r51_191_view_sets_scroll_max_from_content_overflow() {
        // Content height = N rows × (height + gap) - gap; the
        // matching ScrollState bound = content - viewport. The view
        // fn updates this each call so the bound tracks N.
        let _scene = run_view(unselected_state());
        // The state lives on the current Owner — re-resolve via the
        // same key to inspect.
        let owner = Owner::new();
        owner.run(|| {
            // First view call inside this owner populates the cache.
            let _ = view(unselected_state(), &Frame::default());
            let state = use_scroll_state(SCROLL_KEY);
            let (_max_x, max_y) = state.max();
            let content_h = u32::try_from(N).unwrap_or(0) * ROW_HEIGHT
                + u32::try_from(N.saturating_sub(1)).unwrap_or(0) * ROW_GAP;
            let expected = i32::try_from(content_h.saturating_sub(VIEWPORT_H)).unwrap_or(0);
            assert_eq!(max_y, expected);
        });
    }

    #[test]
    fn r51_191_view_rows_positioned_at_intrinsic_y() {
        // Inside the scroll content, the N row containers stack at
        // intrinsic y = i × (ROW_HEIGHT + ROW_GAP). The hit test +
        // input router rely on this layout to map clicks to rows.
        let scene = run_view(unselected_state());
        let scroll = find_scroll(&scene).expect("scroll exists");
        let Scene::Container(content) = scroll.content.as_ref() else {
            panic!("scroll content must be a Container of rows");
        };
        assert_eq!(content.children.len(), N);
        for (i, child) in content.children.iter().enumerate() {
            let Scene::Container(row) = child else {
                panic!("row {i} must be a Container");
            };
            let expected_y = u32::try_from(i).unwrap_or(0) * (ROW_HEIGHT + ROW_GAP);
            assert_eq!(row.rect.y, expected_y, "row {i} y position");
            assert_eq!(row.rect.h, ROW_HEIGHT, "row {i} height");
        }
    }
}
