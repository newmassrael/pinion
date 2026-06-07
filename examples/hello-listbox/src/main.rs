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

use pinion_a11y::{
    listbox_option_nodes, AccessAction, AccessFocus, AccessNode, ListOption, WidgetA11y,
};
// R814 §5.40 — `AriaRole` is now only referenced by the test asserts (the
// lifted `listbox_option_nodes` builder owns the role tagging in prod).
#[cfg(test)]
use pinion_a11y::AriaRole;
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, ScrollNode, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::listbox::ListBoxExternal;
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::WidgetStateName;
// R659 §5.45 — `build_scrollbar_visual` lifted to
// `pinion_widget_paint::scrollbar` after the 2nd-consumer signal
// fired with the R659 todomvc scrollbar peer wire. The local helper
// + (`SCROLLBAR_W`, `MIN_THUMB`) constants are retired —
// `VerticalScrollbarStyle::material` carries the M3-canonical
// defaults the R55.D.4 first-client established.
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
#[cfg(test)]
use pinion_core::theme::ThemeMode;
use pinion_core::{Frame, Owner, Scene, WidgetCore};
use pinion_shell::typeahead::{is_typeahead_char, TypeaheadCursor};
use pinion_shell::{vello_renderer_impl, WidgetView};

use pinion_widget_paint::state_layer::{HOVER, PRESSED};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloListboxRenderer, HelloListboxRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 320;
/// (R57.X.listbox §5.50) [`ThemeProvider`] cache key. Matches the
/// `"app"` convention shared with `hello-toggle` + `hello-theme` so
/// future cross-widget retrofits resolve the same provider; widgets
/// embedded inside a host application would receive the host's tag.
const THEME_TAG: &str = "app";
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
/// (R55.D.5 §5.45) Paint-side tag for the visible scrollbar peer +
/// state-scene tag for the matching `ScrollBarExternal`. The
/// substrate registers the external under this tag through
/// [`WidgetCore::create_extra_externals`]; the view fn attaches the
/// same tag to the scrollbar visual `Container` so the input router's
/// hit-test routes pointer events to the matching external. Sibling
/// to [`PRIMARY_TAG`] inside the wrapper composite — the
/// `ListBoxExternal` and the `ScrollBarExternal` are independent
/// state slots on a shared `Rc<ScrollState>`.
const SCROLLBAR_TAG: &str = "main_list_scrollbar";
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

// R659 §5.45 — `SCROLLBAR_W` / `MIN_THUMB` retired from the binding;
// the M3-canonical defaults live on
// [`VerticalScrollbarStyle::material`] in pinion-widget-paint. Both
// constants migrated verbatim so the visible scrollbar shape stays
// bit-identical (8-px gutter + 24-px thumb floor).

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
/// R55.G.2 §5.45 — content uses flex Column with `gap = ROW_GAP`,
/// so the inner [`compute_layout`](pinion-runtime) pass writes each
/// row's content-local y. The pre-R55.G.2 manual `i * (h + gap)`
/// math + `content_container.rect = ...` workaround is retired.
/// `content_h` survives only to feed `ScrollState::set_max` —
/// `set_max` runs before the layout pass so the bound cannot read
/// from the post-layout `rect`. A future round can route the bound
/// through layout output instead.
///
/// R55.G.3 §5.45 — the outer-window flex pass centres the Scroll
/// inside the window via `JustifyContent::Center` +
/// `AlignItems::Center` on the outer Container. The pre-R55.G.3
/// `outer.rect = Rect::new(0, 0, WIN_W, WIN_H)` + manual
/// `vp_x = (WIN_W - VIEWPORT_W) / 2` math are retired; the runtime
/// layout pass writes the Scroll's `viewport.{x, y}` from the
/// computed flex position (`viewport.{w, h}` stays as the
/// app-supplied clip-window dimensions).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ListState, _frame: &Frame) -> Scene {
    let active = active_option_index(state);
    // (R51.190) `from_state` derives both offset and tag from the
    // cached `ScrollState`. The §5.45 R55.G.5 runtime layout pass
    // now writes the state's max bounds from the laid-out content
    // dimensions, so the view fn no longer has to duplicate the
    // row-count × row-height arithmetic to seed the bound manually.
    let scroll_state = use_scroll_state(SCROLL_KEY);

    // (R57.X.listbox §5.50) Resolve the active palette via
    // [`use_theme`] so the view auto-subscribes to palette swaps —
    // a [`ThemeProvider::set_theme`] anywhere in the application
    // re-runs `view` and the rows / scrollbar repaint with the new
    // tones. The same provider is shared with `hello-toggle` /
    // `hello-theme` (matching `THEME_TAG`) so a future cross-widget
    // toggle binary swaps every retrofitted listbox in lock-step.
    let theme = use_theme(THEME_TAG).theme_animated();

    let rows: Vec<Scene> = (0..N)
        .map(|i| listbox_row(i, state.rows[i].0, state.rows[i].1, Some(i) == Some(active), &theme))
        .collect();
    let content = Scene::Container(
        ContainerNode::new(rows).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_gap(ROW_GAP),
        ),
    );

    // R659 §5.45 — paint helper lifted to
    // `pinion_widget_paint::scrollbar::view_vertical_scrollbar`
    // (2nd-consumer signal fired with the R659 todomvc scrollbar
    // peer wire). Same M3 role mapping
    // ([`SurfaceContainerHighest`] track + [`Outline`] thumb), same
    // R55.D.1 closed-form geometry, same R55.D.6 absolute-position
    // composition — paint scene bit-identical vs the pre-R659
    // helper. Built before the `ScrollNode` so the helper snapshots
    // `(offset, max)` through the same `Rc<ScrollState>` clone
    // `ScrollNode::from_state` consumes below.
    let scrollbar_style = VerticalScrollbarStyle::material(VIEWPORT_H, SCROLLBAR_TAG);
    // R660 §5.45 — same handle the matching `ScrollBarExternal`
    // writes to in `create_extra_externals`. `.get()` auto-subscribes
    // the rendering Effect so a Hover / Dragging transition repaints
    // the thumb with the M3 state-layer overlay.
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll_state,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    let scroll = ScrollNode::from_state(
        scroll_state,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        content,
    );

    // R55.G.17 §5.49 — wrap the `Scroll` (now paired with the
    // R55.D.4 §5.45 visible scrollbar peer) in a transparent
    // `Container` tagged `PRIMARY_TAG` so the composite root is
    // paint-addressable via `{path: "main_list"}` for AI-side
    // `scene/click` / `scene/key` / `scene/wheel` routing, and the
    // AT bounds attached by `enrich_names_from_scene` + `rect_for_tag`
    // land on the listbox's visible area (the scroll viewport plus
    // the scrollbar gutter the user actually sees). The wrapper
    // declares a flex Row layout so the row column and the
    // scrollbar peer sit side-by-side; flexbox auto-sizes the
    // wrapper to the union of the two children and the outer
    // centering then positions it correctly inside the window.
    let listbox_root = Scene::Container(
        ContainerNode::new(vec![Scene::Scroll(scroll), scrollbar_visual])
            .with_tag(PRIMARY_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    Scene::Container(
        ContainerNode::new(vec![listbox_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

// R659 §5.45 — `build_scrollbar_visual` lifted to
// [`pinion_widget_paint::scrollbar::view_vertical_scrollbar`]. The
// helper now lives one tier above the per-binding seed so
// `examples/todomvc` (R659 2nd consumer) reaches the same M3 role
// mapping + R55.D.1 closed-form geometry + R55.D.6 absolute-position
// composition without duplicating the ~65-LOC seed. See module docs
// at `pinion_widget_paint::scrollbar` for the substrate contract.

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
/// (R55.G.2 §5.45) Build one listbox row as a flex Row container
/// sized `ROW_WIDTH × ROW_HEIGHT`. The content-flex Column wrapper
/// in [`view`] supplies the vertical stacking; this helper only
/// declares intrinsic size + visual style + the inline label.
/// `align_items: Center` centres the label vertically, padding
/// supplies the 12-px horizontal inset that the pre-R51.191 helper
/// hand-rolled with `label_baseline_y`.
fn listbox_row(
    index: usize,
    state: ListboxItemState,
    selected: bool,
    focused: bool,
    theme: &Theme,
) -> Scene {
    // (R57.X.listbox §5.50) Material 3 ListItem role mapping. Each
    // visible state resolves to either a palette role directly (e.g.
    // `Accent` for the committed selection) or a state-layer
    // composition (`Color::lerp(base, OnSurface, alpha)`) per the M3
    // state-layer system — `hover = 0.08`, `pressed = 0.12`,
    // `disabled = 0.38` opacity overlays computed against the row's
    // resting `SurfaceContainerLow` base. Mirrors the R57.X.toggle
    // (Round 573) Switch role mapping cascade.
    //
    // Priority: selected > pressed > hover > disabled > focused-idle
    // > idle. The selected state always wins (committed truth);
    // focused is the secondary cursor hint only when not selected.
    let surface = theme.resolve(ColorRole::Surface);
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let row_base = theme.resolve(ColorRole::SurfaceContainerLow);
    let (fill, label_color, border) = if selected {
        (
            theme.resolve(ColorRole::Accent),
            theme.resolve(ColorRole::OnAccent),
            None,
        )
    } else {
        let fill = match state {
            ListboxItemState::Pressed => row_base.lerp(on_surface, PRESSED),
            ListboxItemState::Hover => row_base.lerp(on_surface, HOVER),
            ListboxItemState::Disabled => row_base.lerp(surface, 0.38),
            ListboxItemState::Idle => {
                if focused {
                    // Focused (non-selected) idle row sits one M3
                    // surface tier above the unfocused row — the
                    // textbook tonal-elevation hint for the WAI-ARIA
                    // active descendant.
                    theme.resolve(ColorRole::SurfaceContainer)
                } else {
                    row_base
                }
            }
        };
        let label_color = match state {
            ListboxItemState::Disabled => theme.resolve(ColorRole::OnSurfaceMuted),
            _ => on_surface,
        };
        let border = if focused {
            // R51.87 §5.40 — focused-row visual cue. The 2-px left
            // accent doubles as the WAI-ARIA "active descendant"
            // hint without consuming the row's selection slot.
            Some(Border::new(theme.resolve(ColorRole::Accent), 2))
        } else {
            None
        };
        (fill, label_color, border)
    };
    // Inline label — `rect` left default; the row's flex Row +
    // `AlignItems::Center` + padding lays out width/height and the
    // 12-px horizontal inset that the pre-R55.G.2 helper hand-rolled
    // through `label_baseline_y` + explicit `rect.x = 12`.
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
    let row_layout = LayoutStyle::new()
        .flex(FlexDirection::Row)
        .with_align_items(AlignItems::Center)
        .with_size(Size::px(ROW_WIDTH, ROW_HEIGHT))
        .with_padding(Rect::new(12, 0, 12, 0));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(row_tag)
            .with_style(row_style)
            .with_layout(row_layout),
    )
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

    /// (R55.D.5 §5.45) Sibling `ScrollBarExternal` for the
    /// `main_list` scroll container — registered alongside the primary
    /// `ListBoxExternal` so the substrate composes the state scene as
    /// `Container([primary, scrollbar])`. The scrollbar shares the
    /// same `Rc<ScrollState>` the view fn resolves via
    /// [`use_scroll_state`] (Owner-cache key parity — both factories
    /// run inside `root_owner.run`, so the cache returns the same
    /// instance).
    ///
    /// First multi-External binding in the example catalogue. Drag
    /// the visible R55.D.4 scrollbar peer on the right of the listbox
    /// → the cursor stays captured under the framework's
    /// [`InputRouter`] R51.34 capture lock → `pointer_move` flows
    /// through the framework into `ScrollBarExternal::pointer_move`
    /// → `ScrollState::scroll_to` clamps and writes; the next paint
    /// re-runs the view fn against the new offset.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        // R746 §5.45 — the scrollbar peer wiring (state + interaction-state
        // mirror, registered under SCROLLBAR_TAG) is the shared
        // `scrollbar_extra_external` substrate; the interaction Signal it
        // attaches is the same one the view fn reads every paint for the
        // live Hover / Dragging state-layer.
        vec![scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG)]
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> ListState {
        let mut out = ListState::idle();
        // (R55.D.5 §5.45) The substrate now wraps the state scene in
        // a `Container([listbox, scrollbar])`. Walk by tag to recover
        // the primary `ListBoxExternal`; the previous
        // `Scene::External(node)` match arm would silently fall
        // through and return the empty idle state on every paint.
        let Some(node) = scene.find_external_with_tag(PRIMARY_TAG) else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        for (i, slot) in out.rows.iter_mut().enumerate() {
            let state = match intro.query(&format!("state.{i}")) {
                Some(IntrospectValue::Text(name)) => ListboxItemState::from_name_or_default(&name),
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
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: pinion_core::Modifiers) -> bool {
        // ARIA Listbox roving tabindex: the composite is a single
        // tab stop. Keys only route when the listbox itself is
        // focused (no sibling-widget aliasing).
        if focused != Some(Self::tag()) {
            return false;
        }
        // (R55.D.5 §5.45) State scene is wrapped in
        // `Container([listbox, scrollbar])` once the binding overrides
        // `create_extra_externals`. Walk to the primary `ListBox` by
        // tag instead of pattern-matching `Scene::External` directly;
        // the previous match arm fell through silently and `apply_key`
        // returned `false` for every key after R55.D.5 substrate
        // landed.
        let Some(node) = scene.find_external_with_tag_mut(Self::tag()) else {
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
        // R814 §5.40 — lifted `listbox_option_nodes` builder (one of four
        // consumers). `ListBoxOption` uses WAI-ARIA `aria-selected` (the
        // container-membership axis), not `aria-checked`. `label: None` —
        // option names come from `enrich_names_from_scene` (the painted
        // row text).
        let list_focused = focused == Some(<Self as WidgetCore>::tag());
        let active_idx = active_option_index(*state);
        let tags: Vec<String> = (0..N).map(|i| format!("{PRIMARY_TAG}#{i}")).collect();
        let options: Vec<ListOption<'_>> = state
            .rows
            .iter()
            .copied()
            .enumerate()
            .map(|(i, (item_state, selected))| ListOption {
                tag: &tags[i],
                label: None,
                state: item_state,
                selected,
                focused: list_focused && i == active_idx,
            })
            .collect();
        listbox_option_nodes(<Self as WidgetCore>::tag(), "Fruit picker", false, &options)
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
    fn access_child_invoke(scene: &mut Scene, _parent_tag: &str, sub_tag: &str, action: AccessAction) -> bool {
        let Ok(idx) = sub_tag.parse::<usize>() else {
            return false;
        };
        if idx >= N {
            return false;
        }
        // (R55.D.5 §5.45) Multi-External state-scene shape — walk to
        // the primary `ListBox` external rather than pattern-matching
        // `Scene::External` directly.
        let Some(node) = scene.find_external_with_tag_mut(<Self as WidgetCore>::tag()) else {
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

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
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
    use pinion_core::Color;

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

    // R55.G.5 §5.45 — the pre-R51.191 view-fn `set_max` call is
    // retired: the runtime layout pass now writes the scroll max
    // bounds from the laid-out content rect. End-to-end coverage
    // lives in `pinion-runtime`'s `r55_g5_layout_writes_scroll_max_from_content_height`
    // test, which constructs an equivalent scroll-content shape and
    // asserts the layout-derived max_y value.

    #[test]
    fn r55_g2_view_rows_carry_flex_layout_sidecar_not_manual_rects() {
        // R55.G.2 §5.45 — the view fn no longer hand-rolls each row's
        // `rect.y`. Instead it declares a flex Column on the content
        // container + intrinsic size + padding on each row; the
        // runtime layout pass derives the rects later. This test
        // pins the *intent* the view fn carries (the source of truth
        // for what the layout pass will compute), so a regression
        // back to manual positioning surfaces as a clear failure.
        let scene = run_view(unselected_state());
        let scroll = find_scroll(&scene).expect("scroll exists");
        let Scene::Container(content) = scroll.content.as_ref() else {
            panic!("scroll content must be a Container of rows");
        };
        assert_eq!(content.children.len(), N);
        // Content's flex Column + gap sidecar — what compute_layout
        // consumes to stack rows.
        assert_eq!(
            content.layout.flex_direction,
            FlexDirection::Column,
            "content must declare flex Column"
        );
        assert_eq!(content.layout.gap, ROW_GAP, "content gap");
        // Pre-layout rects are default zeros — proves view fn no
        // longer sets `rect.y` by hand.
        assert_eq!(
            content.rect, Rect::default(),
            "content rect must be layout-derived (was hand-set pre-R55.G.2)"
        );
        for (i, child) in content.children.iter().enumerate() {
            let Scene::Container(row) = child else {
                panic!("row {i} must be a Container");
            };
            assert_eq!(
                row.rect, Rect::default(),
                "row {i} rect must be layout-derived"
            );
            // Each row declares its intrinsic size + flex Row + 12-px
            // horizontal padding — the layout pass converts this
            // into `rect`.
            assert_eq!(
                row.layout.flex_direction, FlexDirection::Row,
                "row {i} flex Row"
            );
            assert_eq!(
                row.layout.size,
                Size::px(ROW_WIDTH, ROW_HEIGHT),
                "row {i} intrinsic size"
            );
        }
    }

    /// (R55.D.4 §5.45) Walk `scene` recursively until a `Container`
    /// carrying `tag` shows up. Returns a reference to the matching
    /// node so callers can inspect its layout / children. Used by
    /// the R55.D.4 paint-shape assertions below.
    fn find_container_with_tag<'a>(
        scene: &'a Scene,
        tag: &str,
    ) -> Option<&'a pinion_core::scene::ContainerNode> {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
            Scene::Container(c) => c.children.iter().find_map(|s| find_container_with_tag(s, tag)),
            Scene::Scroll(s) => find_container_with_tag(s.content.as_ref(), tag),
            _ => None,
        }
    }

    #[test]
    fn r55_d4_view_adds_scrollbar_visual_as_listbox_root_sibling() {
        // R55.D.4 §5.45 — the `PRIMARY_TAG` paint root now holds
        // two children: the `Scroll` (rows) + the scrollbar visual
        // (track + thumb). Tests the listbox_root composite
        // structure stayed flex Row so the two siblings land
        // side-by-side rather than stacked.
        let scene = run_view(unselected_state());
        let listbox_root =
            find_container_with_tag(&scene, PRIMARY_TAG).expect("listbox_root container");
        assert_eq!(
            listbox_root.children.len(),
            2,
            "listbox_root must hold Scroll + scrollbar visual",
        );
        assert!(
            matches!(&listbox_root.children[0], Scene::Scroll(_)),
            "first child must be the Scroll (rows)",
        );
        let Scene::Container(scrollbar) = &listbox_root.children[1] else {
            panic!("second child must be the scrollbar visual Container");
        };
        // R659 §5.45 — gutter width now sourced from the lifted
        // `VerticalScrollbarStyle::material(...)` default (8 px,
        // M3-canonical). The binding still composes the bar through
        // `view_vertical_scrollbar(viewport_h=VIEWPORT_H)`, so the
        // expected size stays `(material.gutter_w × VIEWPORT_H)`.
        let expected_gutter = VerticalScrollbarStyle::material(VIEWPORT_H, SCROLLBAR_TAG).gutter_w;
        assert_eq!(
            scrollbar.layout.size,
            Size::px(expected_gutter, VIEWPORT_H),
            "scrollbar track size = gutter × viewport height",
        );
        // Listbox root must declare flex Row so the row column and
        // the scrollbar peer sit side-by-side.
        assert_eq!(
            listbox_root.layout.flex_direction,
            FlexDirection::Row,
            "listbox_root must declare flex Row to position scrollbar at right",
        );
    }

    #[test]
    fn r55_d6_scrollbar_visual_uses_absolute_positioned_thumb() {
        // R55.D.6 §5.45 §5.21 — scrollbar's internal composition
        // post-absolute-position substrate: outer track Container
        // holds exactly ONE child — the thumb Container — positioned
        // via `LayoutStyle::with_absolute_position(0, thumb_y)`.
        // The pre-R55.D.6 spacer + flex Column workaround is retired
        // now that the substrate exposes CSS-mirror absolute
        // positioning directly. A regression that re-introduces the
        // spacer (or drops the `absolute_position` on the thumb)
        // would warp the thumb back to the top of the track at every
        // scroll offset.
        let scene = run_view(unselected_state());
        let listbox_root =
            find_container_with_tag(&scene, PRIMARY_TAG).expect("listbox_root container");
        let Scene::Container(scrollbar) = &listbox_root.children[1] else {
            panic!("scrollbar Container missing");
        };
        assert_eq!(scrollbar.children.len(), 1, "thumb only, no spacer");
        let Scene::Container(thumb) = &scrollbar.children[0] else {
            panic!("thumb must be a Container");
        };
        // R659 §5.45 — gutter width sourced from the lifted style
        // default (M3 canonical 8 px). Thumb width matches gutter.
        let expected_gutter = VerticalScrollbarStyle::material(VIEWPORT_H, SCROLLBAR_TAG).gutter_w;
        assert_eq!(
            thumb.layout.size.width,
            Size::px(expected_gutter, 0).width,
            "thumb width matches lifted material gutter (8 px M3 default)",
        );
        assert!(
            thumb.layout.absolute_position.is_some(),
            "thumb must declare absolute_position (R55.D.6 substrate)",
        );
        // First-frame scroll state has `max_y == 0`, so the helper
        // produces a thumb covering the full track at the track
        // origin — `absolute_position` is `(0, 0)`. Later frames
        // shift the y as the user scrolls.
        let Some((left, top)) = thumb.layout.absolute_position else {
            panic!("absolute_position just asserted as Some");
        };
        assert_eq!(left, 0, "thumb hugs left edge of track");
        assert_eq!(top, 0, "first-frame thumb sits at track origin");
    }

    #[test]
    fn r55_g17_view_contains_composite_paint_root_tag() {
        // R55.G.17 §5.49 — paint scene must carry the composite
        // `PRIMARY_TAG` somewhere so `scene/click` / `scene/key` /
        // `scene/wheel` `{path: "main_list"}` resolves and AT bounds
        // attach to the listbox's visible area. The wrapper
        // Container introduced in `view` carries the tag; this test
        // pins the convention so a regression that drops the wrapper
        // surfaces as a typed failure rather than a silent path
        // lookup error.
        //
        // R55.G.22 §5.49 — pinned via the framework helper
        // `pinion_core::test_fixtures::assert_widget_view_carries_tag`
        // which wraps `V::view` in an `Owner::new()` scope (matching
        // the local `run_view` precedent) and calls
        // `Scene::contains_tag(V::tag())`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ListBoxView>(
            unselected_state(),
            &Frame::default(),
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R57.X.listbox §5.50 — theme retrofit regression. Pins three
    // key invariants of the role mapping: (1) outer panel uses
    // `ColorRole::Surface`; (2) palette swap propagates through the
    // view-fn auto-subscribe; (3) scrollbar thumb uses
    // `ColorRole::Outline`. Asserting role-driven (not literal-RGB)
    // colors keeps the M3 mapping the source of truth — a future
    // palette tweak that adjusts the `Surface` tone will not require
    // updating this test.
    // ─────────────────────────────────────────────────────────────

    fn run_view_with_palette(state: ListState, theme: Theme) -> Scene {
        // R57.1 substrate: install `theme` as the light palette and
        // force Light mode so the provider resolves to exactly `theme`
        // regardless of the global `SystemColorScheme` value an
        // earlier test on the same thread might have written.
        let owner = Owner::new();
        owner.run(|| {
            let provider = use_theme(THEME_TAG);
            provider.set_light_palette(theme);
            provider.set_mode(ThemeMode::Light);
            view(state, &Frame::default())
        })
    }

    fn outer_panel_fill(scene: &Scene) -> Color {
        // The outermost Container is the panel; its BoxStyle fill is
        // `theme.resolve(Surface)`.
        if let Scene::Container(c) = scene {
            c.style.fill
        } else {
            panic!("view's root must be a Container");
        }
    }

    fn scrollbar_thumb_fill(scene: &Scene) -> Color {
        // Walk: root container → listbox_root container (carries
        // PRIMARY_TAG) → second child = scrollbar visual container
        // (carries SCROLLBAR_TAG) → first (and only) child = thumb.
        let bar = find_container_with_tag(scene, SCROLLBAR_TAG)
            .expect("scene must carry the SCROLLBAR_TAG container");
        let Scene::Container(thumb) = bar
            .children
            .first()
            .expect("scrollbar must have a thumb child")
        else {
            panic!("thumb must be a Container");
        };
        thumb.style.fill
    }

    #[test]
    fn r57_x_listbox_outer_panel_uses_surface_role() {
        // Panel fill must equal the active theme's Surface role —
        // not a hard-coded RGB literal. Pin both palettes to confirm
        // the swap actually propagates.
        let light_scene = run_view_with_palette(unselected_state(), Theme::light());
        assert_eq!(outer_panel_fill(&light_scene), Theme::light().surface);
        let dark_scene = run_view_with_palette(unselected_state(), Theme::dark());
        assert_eq!(outer_panel_fill(&dark_scene), Theme::dark().surface);
    }

    #[test]
    fn r57_x_listbox_scrollbar_thumb_uses_outline_role() {
        // Thumb fill = `ColorRole::Outline` so the visible weight
        // tracks the active theme's hairline / divider color.
        let light_scene = run_view_with_palette(unselected_state(), Theme::light());
        assert_eq!(scrollbar_thumb_fill(&light_scene), Theme::light().outline);
        let dark_scene = run_view_with_palette(unselected_state(), Theme::dark());
        assert_eq!(scrollbar_thumb_fill(&dark_scene), Theme::dark().outline);
    }
}
