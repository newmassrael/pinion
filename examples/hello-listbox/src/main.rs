//! `hello-listbox` — R51.97 §5.38 `ListBox` composite first-client.
//!
//! N=4 [`ListBox`](pinion_core::widgets::listbox::ListBox) rendered as
//! a vertical column of selectable rows, each row tagged with the
//! paint convention `"main_list#<index>"` from the R51.41 RFC. The
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

use pinion_a11y::{AccessAction, AccessFocus, AccessNode, AccessState, AriaRole};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::listbox::ListBoxExternal;
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::{Color, Frame, Scene};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloListboxRenderer, HelloListboxRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 320;
const BG_FILL: Color = Color::rgb(0x18, 0x24, 0x30);
const N: usize = 4;
const PRIMARY_TAG: &str = "main_list";

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
/// vertical column of N rows, each row tagged `"main_list#<i>"` so
/// the `InputRouter`'s R51.42 sub-index split routes cursor hits on
/// row `i` to `invoke("send", Text("<i>:<EventName>"))` against the
/// single composite `ListBoxExternal`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ListState, _frame: &Frame) -> Scene {
    let active = active_option_index(state);
    let rows: Vec<Scene> = (0..N)
        .map(|i| listbox_row(i, state.rows[i].0, state.rows[i].1, Some(i) == Some(active)))
        .collect();
    let column = Scene::Container(
        ContainerNode::new(rows).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_gap(ROW_GAP),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![column])
            .with_style(BoxStyle::filled(BG_FILL))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
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
fn listbox_row(index: usize, state: ListboxItemState, selected: bool, focused: bool) -> Scene {
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
    let label = Scene::Text(TextNode::styled(
        option_label(index),
        Rect::default(),
        TextStyle::new().with_size_px(15).with_fg(label_color),
    ));
    let row_tag = format!("{PRIMARY_TAG}#{index}");
    let mut row_style = BoxStyle::filled(fill).with_corner_radius(4);
    if let Some(b) = border {
        row_style = row_style.with_border(b);
    }
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(row_tag)
            .with_style(row_style)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(ROW_WIDTH, ROW_HEIGHT))
                    // R51.97 §5.38 — Rect insets via `Rect::new` (the
                    // public constructor; `#[non_exhaustive]` blocks
                    // the literal struct syntax). x=left, y=top,
                    // w=right, h=bottom.
                    .with_padding(Rect::new(12, 4, 12, 4)),
            ),
    )
}

fn option_label(index: usize) -> &'static str {
    match index {
        0 => "Apple",
        1 => "Banana",
        2 => "Cherry",
        3 => "Date",
        _ => "?",
    }
}

struct ListBoxView;

impl WidgetView for ListBoxView {
    type State = ListState;
    // The composite drives every state change through `apply_key`
    // (typed wire format `"<i>:<EventName>"` for commits, direct
    // `intervene "focused_index"` for navigation), so the keybinding-
    // channel typed event slot stays unused. `()` satisfies the
    // trait's `Copy` bound without an unused variant.
    type Event = ();
    type Renderer = HelloListboxRenderer;

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

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
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
        let list_focused = focused == Some(Self::tag());
        let active_idx = active_option_index(*state);
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(N + 1);
        let mut list = AccessNode::new(Self::tag(), AriaRole::Listbox)
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
            // CheckBox / RadioButton). The R51.97 hello-listbox
            // emitted `state.checked + AccessValue::Bool` for option
            // selection; that conflated the two ARIA axes and
            // misreported the option to AT (NVDA / VoiceOver would
            // announce "checked" instead of "selected"). The option's
            // accessible name comes from its TextNode label via
            // `enrich_names_from_scene` — no `AccessValue` needed.
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
    /// sub-tag as the `aria-activedescendant`. Pinion-shell focus
    /// ring and key dispatch address `"main_list"` itself; only
    /// AccessKit consumers see the descendant hint.
    fn access_focus_target(
        state: &ListState,
        focused: Option<&str>,
    ) -> Option<AccessFocus> {
        if focused == Some(Self::tag()) {
            let idx = active_option_index(*state);
            Some(AccessFocus::composite(
                Self::tag(),
                format!("{PRIMARY_TAG}#{idx}"),
            ))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// R51.70 §5.40 — composite child action dispatch. Mirrors the
    /// `commit_focused` wire-format path
    /// (`PointerEnter` / `Down` / `Up` / `Leave` against
    /// `format!("{idx}:{ev}")`) when an AT invokes
    /// `AccessKit::Action::Click` / `Default` on a specific option's
    /// `NodeId` (`"main_list#<i>"`).
    ///
    /// `Click` / `Default` activate the addressed option (mutual
    /// exclusion + `"selected"` intent fires through the standard
    /// composite path). `Focus` mutates the parent's `focused_index`
    /// to mark the addressed row as the active descendant without
    /// committing — the WAI-ARIA Listbox roving-tabindex textbook
    /// model. Other actions decline so the shell stays in charge of
    /// the fallback chain.
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

/// R51.99 §5.38 — WAI-ARIA Listbox optional type-ahead jump (W3C
/// ARIA Authoring Practices). Accepts a single printable
/// alphanumeric character; finds the next option whose label starts
/// with that character (case-insensitive, ASCII fold), wrapping
/// cyclically from `focused_index + 1` so successive presses of the
/// same letter cycle through every match. The jump only mutates
/// `focused_index` (no commit) — `Space` / `Enter` commits after.
///
/// Returns `true` when the key is a single alphanumeric character
/// that triggered a focus move; `false` for multi-character key
/// strings (`Tab`, `Escape`, named navigation keys handled by the
/// caller before reaching this fallback) or when no option label
/// begins with the typed character.
///
/// Multi-character prefix buffering with timeout (e.g. typing
/// `"Ba"` to disambiguate `Banana` from `Berry`) is a future axis
/// — the four fruit labels here have unique first letters and the
/// single-character form is the WAI-ARIA APG baseline. Non-ASCII
/// label first-character match (Unicode case folding) is also a
/// carry; the current fold uses `eq_ignore_ascii_case` which is
/// correct for the ASCII fruit names.
fn type_ahead_jump(node: &mut pinion_core::scene::ExternalNode, key: &str) -> bool {
    let Some(first) = single_printable_char(key) else {
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
    let Some(target) = find_next_match(current, first) else {
        return false;
    };
    set_focus(node, target)
}

/// R51.99 §5.38 — extract the single-printable-character predicate so
/// the type-ahead unit tests can exercise the gate independent of a
/// live `ExternalNode`. A printable type-ahead key is exactly one
/// ASCII alphanumeric (`A-Za-z0-9`); named keys (`ArrowDown`, `F1`,
/// `Tab`) and multi-character / control keys return `None`.
fn single_printable_char(key: &str) -> Option<char> {
    let mut iter = key.chars();
    let first = iter.next()?;
    if iter.next().is_some() {
        return None;
    }
    first.is_ascii_alphanumeric().then_some(first)
}

/// R51.99 §5.38 — wrap-around search for the next option whose label
/// starts with `key` (case-insensitive, ASCII fold). Starts from
/// `current + 1` (so successive presses of the same letter cycle
/// through every match); falls back to `0` when `current` is `None`
/// or out of range. Returns the matched index or `None` when no
/// label begins with `key`.
fn find_next_match(current: Option<usize>, key: char) -> Option<usize> {
    let start = match current {
        Some(c) if c < N => (c + 1) % N,
        _ => 0,
    };
    for offset in 0..N {
        let i = (start + offset) % N;
        let label = option_label(i);
        let Some(label_first) = label.chars().next() else {
            continue;
        };
        if label_first.eq_ignore_ascii_case(&key) {
            return Some(i);
        }
    }
    None
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

    // R51.99 §5.38 — type-ahead navigation (WAI-ARIA APG optional).

    #[test]
    fn r51_99_single_printable_char_accepts_letter() {
        assert_eq!(single_printable_char("A"), Some('A'));
        assert_eq!(single_printable_char("z"), Some('z'));
    }

    #[test]
    fn r51_99_single_printable_char_accepts_digit() {
        assert_eq!(single_printable_char("0"), Some('0'));
        assert_eq!(single_printable_char("7"), Some('7'));
    }

    #[test]
    fn r51_99_single_printable_char_rejects_named_keys() {
        assert_eq!(single_printable_char("ArrowDown"), None);
        assert_eq!(single_printable_char("Tab"), None);
        assert_eq!(single_printable_char("Escape"), None);
        assert_eq!(single_printable_char("F1"), None);
        assert_eq!(single_printable_char(""), None);
    }

    #[test]
    fn r51_99_single_printable_char_rejects_punctuation() {
        // Space, dash, etc. are not type-ahead targets (Space is a
        // commit key in the keyboard model).
        assert_eq!(single_printable_char(" "), None);
        assert_eq!(single_printable_char("-"), None);
        assert_eq!(single_printable_char("!"), None);
    }

    #[test]
    fn r51_99_find_next_match_from_unfocused_finds_first() {
        // Labels = [Apple, Banana, Cherry, Date]. From no focus, 'A'
        // should land on index 0 (Apple).
        assert_eq!(find_next_match(None, 'A'), Some(0));
        assert_eq!(find_next_match(None, 'a'), Some(0));
        assert_eq!(find_next_match(None, 'B'), Some(1));
        assert_eq!(find_next_match(None, 'D'), Some(3));
    }

    #[test]
    fn r51_99_find_next_match_starts_after_current() {
        // From Apple (0), 'B' jumps to Banana (1).
        assert_eq!(find_next_match(Some(0), 'B'), Some(1));
        // From Cherry (2), 'A' wraps to Apple (0).
        assert_eq!(find_next_match(Some(2), 'A'), Some(0));
    }

    #[test]
    fn r51_99_find_next_match_no_match_returns_none() {
        // No fruit starts with 'Z'.
        assert_eq!(find_next_match(None, 'Z'), None);
        assert_eq!(find_next_match(Some(1), 'X'), None);
    }

    #[test]
    fn r51_99_find_next_match_same_letter_from_match_finds_self_after_wrap() {
        // From Apple (0), 'A' searches 1..=3, none match, wraps to 0;
        // returns Some(0) (the only Apple, cyclic).
        assert_eq!(find_next_match(Some(0), 'A'), Some(0));
    }

    #[test]
    fn r51_99_find_next_match_out_of_range_current_treated_as_unfocused() {
        // Defensive: focused_index larger than N is treated as None
        // (start from 0).
        assert_eq!(find_next_match(Some(99), 'A'), Some(0));
    }
}
