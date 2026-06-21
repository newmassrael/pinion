// R714 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (WAI-ARIA, ListBoxExternal, ButtonExternal, dismiss_barrier, …).
#![allow(clippy::doc_markdown)]

//! `hello-combobox` — R714 §5.38 §5.40 §5.50 select-only **combobox**
//! (dropdown): the first Phase-B catalogue widget for choosing one value
//! from a popup list. The defining Qt / Flutter / Compose form factor
//! (`QComboBox`, `DropdownButton`, `ExposedDropdownMenu`).
//!
//! ## Why this binding is pure composition (no new coordinator)
//!
//! A select-only combobox decomposes cleanly into substrate that already
//! exists, so per `[[abstraction-needs-second-consumer]]` R714 mints **no**
//! `ComboBoxExternal` coordinator — it composes three proven externals
//! and one app signal, exactly the way R697 `hello-accordion` composed
//! N R696 disclosures:
//!
//! * **trigger** — a [`ButtonExternal`] (`combo_trigger`, the primary
//!   external) painted as the field showing the current value + a
//!   chevron. A click (or `ArrowDown` / `Enter` / `Space` while focused)
//!   opens the popup.
//! * **popup** — a single-select [`ListBoxExternal`] (`combo_options`,
//!   an extra external) of N options. Reused verbatim — it already owns
//!   mutual exclusion, the `focused_index` active descendant (R51.87),
//!   and the full `scene/query` / `intervene` / `invoke` RPC surface.
//! * **dismiss barrier** — a click-only [`ButtonExternal`]
//!   (`combo_barrier`, an extra external) bound to the shared
//!   [`dismiss_barrier`]: a **transparent**, non-modal click-catcher.
//!   This is the click-outside dismiss — a click anywhere outside the
//!   popup emits `combo_barrier.click` and the reducer closes the
//!   combobox. R714 introduced the transparent barrier inline (as a
//!   modal scrim at zero opacity); R715 lifted it to its own module with
//!   `hello-menu` as the 2nd consumer. (Qt's
//!   `Qt::Popup` grab, Flutter's transparent route barrier, and Compose's
//!   focusable `Popup` are all this same full-window catch-the-outside
//!   layer.)
//!
//! ## Non-modal (vs the R702 drawer)
//!
//! The drawer is modal — it installs a `FocusManager` trap and dims the
//! background. A combobox is **non-modal**: focus stays on the combobox
//! element and the active option surfaces as `aria-activedescendant`
//! (the single-Tab-stop + roving model `hello-datepicker` / RadioGroup
//! use). So there is no `modal_scope_request`; the barrier is invisible
//! and the only "trap" is that an outside click is consumed to dismiss.
//!
//! ## The new a11y axis (R714 substrate)
//!
//! [`AriaRole::ComboBox`] + [`AccessNode::with_controls`]
//! (`aria-controls`). The trigger lowers to `Role::ComboBox` carrying
//! `aria-expanded` (open/closed) + `aria-controls` → the listbox, and
//! its `aria-activedescendant` points at the active option. The popup is
//! the existing `Role::ListBox` + `Role::ListBoxOption` family.
//!
//! ## Dismissal paths (all converge on `close_combo`)
//!
//! - **Barrier tap** — `combo_barrier.click` (click-outside);
//! - **Escape** — routed to [`apply_key`](ComboView) (R695 widget-first
//!   Escape) while the trigger is focused;
//! - **Selecting an option** — `combo_options.selected` records the value
//!   *and* closes (the select-only combobox convention).
//!
//! ## Known gap (honest carry)
//!
//! The trigger paints no keyboard-focus ring (the shared shell-focus-
//! paint axis R690 deferred — same carry hello-dialog / hello-drawer
//! note); focus is real + RPC-observable (`focus/get`, the a11y
//! `aria-activedescendant`). A 2nd combobox-class consumer (an editable
//! combobox, a menu-button) would trigger lifting a `ComboBoxExternal`
//! coordinator + a `pinion_widget_paint::combobox` chrome helper.

use pinion_a11y::{
    AccessFocus, AccessNode, AccessState, AriaRole, ListOption, WidgetA11y, listbox_option_nodes,
};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::listbox::ListBoxExternal;
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::button::{read_button_focused, read_button_state};
use pinion_widget_paint::listbox::{OptionRow, view_option};
use pinion_widget_paint::popup::popup_surface;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloComboBoxRenderer, HelloComboBoxRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 420;
const THEME_TAG: &str = "app";

/// Number of options in the popup.
const N: usize = 5;

/// The trigger (primary external) — the focusable combobox element.
const TRIGGER_TAG: &str = "combo_trigger";
/// The popup listbox (extra external). Options paint as `combo_options#<i>`.
const OPTIONS_TAG: &str = "combo_options";
/// The transparent full-window dismiss barrier (extra external).
const BARRIER_TAG: &str = "combo_barrier";
/// The dropdown panel container tag (queryable via `scene/snapshot`).
const PANEL_TAG: &str = "combo_panel";
/// The status line tag echoing the committed value (read by the demo).
const STATUS_TAG: &str = "combo_status";

/// Option labels — T-shirt sizes. Double as each option's AT accessible
/// name and the trigger / status value copy. The single source of truth.
const LABELS: [&str; N] = ["Extra small", "Small", "Medium", "Large", "Extra large"];

/// Placeholder shown in the trigger before any option is committed.
const PLACEHOLDER: &str = "Select a size";

/// U+25BE BLACK DOWN-POINTING SMALL TRIANGLE — collapsed-state chevron
/// ([[non-ascii-literal-named-const-escape]]: named const + escape).
const CHEVRON_DOWN: &str = "\u{25BE}";
/// U+25B4 BLACK UP-POINTING SMALL TRIANGLE — expanded-state chevron.
const CHEVRON_UP: &str = "\u{25B4}";

/// `Owner::cache` key for the `Signal<bool>` "is the popup open".
const OPEN_KEY: &str = "hello_combobox.open";

// Absolute geometry so the dropdown anchors deterministically under the
// trigger (and the boot-frame pixel guard can sample fixed points).
const TRIGGER_X: u32 = 180;
const TRIGGER_Y: u32 = 110;
const TRIGGER_W: u32 = 200;
const TRIGGER_H: u32 = 48;
const OPT_H: u32 = 40;
/// Panel sits 4 px below the trigger, same width.
const PANEL_X: u32 = TRIGGER_X;
const PANEL_Y: u32 = TRIGGER_Y + TRIGGER_H + 4;
const PANEL_PAD: u32 = 6;

/// `Owner::cache`-keyed hook: the shared `Rc<Signal<bool>>` open flag.
fn use_combo_open() -> Rc<Signal<bool>> {
    Owner::current()
        .expect("use_combo_open requires an active Owner scope")
        .cache(OPEN_KEY, || Signal::new(false))
}

/// Open the popup (no focus trap — non-modal). The active descendant
/// seed (keyboard open) is handled by `apply_key`, which has the state
/// scene; pointer open falls back to the `selected`-or-0 resolution.
fn open_combo() {
    use_combo_open().set(true);
}

/// Close the popup.
fn close_combo() {
    use_combo_open().set(false);
}

/// Cached posture for the paint fn. `open` lives in a signal the
/// owner-scoped view + access_node read directly.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct ComboViewState {
    /// Trigger button posture (hover / pressed / focused for paint).
    trigger: ButtonState,
    trigger_focused: bool,
    /// Per-option interaction state (hover / pressed) from the listbox.
    options: [ListboxItemState; N],
    /// The committed selection, or `None` (placeholder).
    selected: Option<usize>,
    /// The roving active descendant (`focused_index`), or `None`.
    focused: Option<usize>,
}

impl ComboViewState {
    fn idle() -> Self {
        Self {
            trigger: ButtonState::Idle,
            trigger_focused: false,
            options: [ListboxItemState::Idle; N],
            selected: None,
            focused: None,
        }
    }
}

/// The active option in the popup: the roving `focused` if set, else the
/// committed `selected`, else option 0 — the WAI-ARIA combobox active-
/// descendant resolution (mirror of `hello-datepicker`'s `active_day`).
fn active_option(state: &ComboViewState) -> usize {
    state.focused.or(state.selected).unwrap_or(0)
}

/// Read the combobox posture from the state scene: the trigger button
/// state + the popup listbox's selection / focus / per-option state.
fn read_combo_state(scene: &Scene) -> ComboViewState {
    let mut out = ComboViewState::idle();
    out.trigger = read_button_state(scene, TRIGGER_TAG);
    out.trigger_focused = read_button_focused(scene, TRIGGER_TAG);
    let Some(node) = scene.find_external_with_tag(OPTIONS_TAG) else {
        return out;
    };
    let Some(intro) = node.handle.introspect() else {
        return out;
    };
    out.selected = match intro.query("selected_index") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    };
    out.focused = match intro.query("focused_index") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    };
    for (i, slot) in out.options.iter_mut().enumerate() {
        *slot = match intro.query(&format!("state.{i}")) {
            Some(IntrospectValue::Text(name)) => ListboxItemState::from_name_or_default(&name),
            _ => ListboxItemState::Idle,
        };
    }
    out
}

/// Paint the trigger: a tonal field showing the current value on the
/// left and a chevron (down when closed, up when open) on the right.
fn trigger_scene(state: &ComboViewState, open: bool, theme: &Theme) -> Scene {
    let bg = match state.trigger {
        ButtonState::Hover | ButtonState::Pressed => theme.resolve(ColorRole::SurfaceContainerHigh),
        _ => theme.resolve(ColorRole::SurfaceContainer),
    };
    let border_color = if state.trigger_focused {
        theme.resolve(ColorRole::Accent)
    } else {
        theme.resolve(ColorRole::Outline)
    };
    let value_text = state.selected.map_or(PLACEHOLDER, |i| LABELS[i.min(N - 1)]);
    let value_color = if state.selected.is_some() {
        theme.resolve(ColorRole::OnSurface)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    // Untagged (like a button label) so a click anywhere on the field —
    // including over this text — falls through to the `combo_trigger`
    // container's `ButtonExternal`. A standalone tag here would shadow
    // the container and swallow the open click. The value is the
    // trigger's first Text child, read back by walking the subtree.
    let value = Scene::Text(TextNode::styled(
        value_text,
        Rect::default(),
        TextStyle::new().with_size_px(16).with_fg(value_color),
    ));
    let chevron = Scene::Text(TextNode::styled(
        if open { CHEVRON_UP } else { CHEVRON_DOWN },
        Rect::default(),
        TextStyle::new()
            .with_size_px(16)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![value, chevron])
            .with_tag(TRIGGER_TAG)
            .with_style(
                BoxStyle::filled(bg)
                    .with_corner_radius(6)
                    .with_border(Border::new(
                        border_color,
                        if state.trigger_focused { 2 } else { 1 },
                    )),
            )
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(TRIGGER_X, TRIGGER_Y)
                    .with_size(Size::px(TRIGGER_W, TRIGGER_H))
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::SpaceBetween)
                    .with_padding(Rect::new(14, 0, 12, 0))
                    .with_focusable(true),
            ),
    )
}

/// Paint one popup option cell, tagged `combo_options#<i>` so the
/// InputRouter `'#'`-split reaches the composite [`ListBoxExternal`]. The
/// option-cell skin is the lifted [`view_option`] SSOT (R867, 1st of three
/// consumers — combobox / editable-combobox / property-grid choice cell).
fn option_scene(index: usize, state: &ComboViewState, active: usize, theme: &Theme) -> Scene {
    view_option(
        &OptionRow {
            tag: format!("{OPTIONS_TAG}#{index}"),
            label: LABELS[index],
            state: state.options[index],
            active: index == active,
            selected: state.selected == Some(index),
        },
        TRIGGER_W - 2 * PANEL_PAD,
        OPT_H,
        theme,
    )
}

/// view-fn (§6.3): pure sync `(postures, open signal) -> Scene`. When
/// open the transparent barrier + dropdown panel are pushed **last** so
/// they hit-test above the trigger (a click on the trigger while open
/// lands on the barrier → close, the toggle-close convention).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &ComboViewState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let open = use_combo_open().get();

    let label = Scene::Text(
        TextNode::styled(
            "T-shirt size",
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(TRIGGER_X, TRIGGER_Y - 26)),
    );

    let status = Scene::Text(
        TextNode::styled(
            format!(
                "Committed: {}",
                state.selected.map_or("(none)", |i| LABELS[i.min(N - 1)])
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(15)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(STATUS_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(TRIGGER_X, WIN_H - 60)),
    );

    let mut children = vec![label, trigger_scene(state, open, &theme), status];

    if open {
        let active = active_option(state);
        let options: Vec<Scene> = (0..N)
            .map(|i| option_scene(i, state, active, &theme))
            .collect();
        let panel = Scene::Container(
            ContainerNode::new(options)
                .with_tag(PANEL_TAG)
                .with_style(popup_surface(&theme))
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(PANEL_X, PANEL_Y)
                        .with_size(Size::px(
                            TRIGGER_W,
                            u32::try_from(N).expect("N fits in u32") * OPT_H + 2 * PANEL_PAD,
                        ))
                        .flex(FlexDirection::Column)
                        .with_align_items(AlignItems::Stretch)
                        .with_gap(2)
                        .with_padding(Rect::new(PANEL_PAD, PANEL_PAD, PANEL_PAD, PANEL_PAD)),
                ),
        );
        // R715 §5.16 — transparent full-window dismiss barrier (the
        // shared light-dismiss layer, `[[abstraction-needs-second-consumer]]`
        // lift of R714's inline transparent scrim; hello-menu is the
        // other consumer). The trigger sits *under* the barrier, so
        // re-clicking it while open also dismisses (toggle-close).
        // Pushed before the panel so the absolutely-positioned dropdown
        // hit-tests above the barrier.
        children.push(dismiss_barrier(BARRIER_TAG, (0, 0), (WIN_W, WIN_H)));
        children.push(panel);
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start),
            ),
    )
}

/// Move the popup's `focused_index` (active descendant) by `delta`,
/// clamped to `0..N`. From no active descendant the first step lands on
/// `selected`-or-0 then applies `delta`. Returns whether a write landed.
fn move_focused(scene: &mut Scene, state: &ComboViewState, delta: i32) -> bool {
    let from = i32::try_from(active_option(state)).unwrap_or(0);
    // First Arrow from a fresh open: land on the active option itself
    // (selected-or-0) without moving past it; subsequent Arrows step.
    let next = if state.focused.is_none() {
        from
    } else {
        (from + delta).clamp(0, i32::try_from(N - 1).unwrap_or(0))
    };
    set_focused(scene, next)
}

/// Set the popup's `focused_index` active descendant to `idx`.
fn set_focused(scene: &mut Scene, idx: i32) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(OPTIONS_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    intro
        .intervene("focused_index", IntrospectValue::Int(i64::from(idx)))
        .is_ok()
}

/// Activate the active-descendant option through the listbox's wire
/// format (full pointer cycle) so single-select exclusion + the
/// `"selected"` intent fire exactly as a click would. The intent drives
/// `update` → `close_combo`.
fn activate_focused(scene: &mut Scene, state: &ComboViewState) -> bool {
    let idx = active_option(state);
    let Some(node) = scene.find_external_with_tag_mut(OPTIONS_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let mut handled = false;
    for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
        handled |= intro
            .invoke("send", IntrospectValue::Text(format!("{idx}:{ev}")))
            .is_ok();
    }
    handled
}

struct ComboView;

impl WidgetCore for ComboView {
    type State = ComboViewState;
    // Trigger + options are driven by the pointer router + ARIA
    // Enter/Space + apply_key; no keybinding-channel events flow.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    /// The popup listbox + the click-only transparent barrier, as extra
    /// externals. The barrier is a real [`ButtonExternal`] so an
    /// outside click light-dismisses (the click-outside substrate this
    /// round exercises). Both persist in the state scene; the view only
    /// *paints* them while open.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(OPTIONS_TAG, Box::new(ListBoxExternal::new(N))),
            ExtraExternal::new(BARRIER_TAG, Box::new(ButtonExternal::new())),
        ]
    }

    fn tag() -> &'static str {
        TRIGGER_TAG
    }

    fn read_state(scene: &Scene) -> ComboViewState {
        read_combo_state(scene)
    }

    fn view(state: ComboViewState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-combobox (R714 §5.38 select-only combobox)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// WAI-ARIA §4.5 select-only combobox keyboard model. All keys route
    /// only while the trigger owns shell focus (single Tab stop):
    /// - **closed** — `ArrowDown` / `ArrowUp` / `Enter` / `Space` open
    ///   the popup; `ArrowUp` lands the active descendant on the last
    ///   option, the others on `selected`-or-0.
    /// - **open** — `ArrowDown` / `ArrowUp` move the active descendant
    ///   (clamped); `Home` / `End` jump to first / last; `Enter` /
    ///   `Space` commit the active option (which closes via the reducer);
    ///   `Escape` closes without committing (R695 widget-first Escape).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(TRIGGER_TAG) {
            return false;
        }
        let state = read_combo_state(scene);
        if use_combo_open().get() {
            match key {
                "Escape" => {
                    close_combo();
                    true
                }
                "ArrowDown" => move_focused(scene, &state, 1),
                "ArrowUp" => move_focused(scene, &state, -1),
                "Home" => set_focused(scene, 0),
                "End" => set_focused(scene, i32::try_from(N - 1).unwrap_or(0)),
                "Enter" | "Space" => activate_focused(scene, &state),
                _ => false,
            }
        } else {
            match key {
                "ArrowDown" | "Enter" | "Space" => {
                    open_combo();
                    set_focused(scene, i32::try_from(active_option(&state)).unwrap_or(0))
                }
                "ArrowUp" => {
                    open_combo();
                    set_focused(scene, i32::try_from(N - 1).unwrap_or(0))
                }
                _ => false,
            }
        }
    }

    /// R714 §5.40 — bridge the trigger / barrier / option intents into
    /// the `combo_open` signal. The listbox owns the selection itself, so
    /// a committed option only needs to *close* here. Side-effect-only.
    fn update(
        _state: ComboViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "combo_trigger.click" => open_combo(),
            // Outside (barrier) click light-dismisses; a committed option
            // closes too (select-only convention). Both just close.
            "combo_barrier.click" | "combo_options.selected" => close_combo(),
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &ComboViewState) -> String {
        format!(
            "trigger={:?} selected={:?} focused={:?}",
            state.trigger, state.selected, state.focused
        )
    }
}

impl WidgetA11y for ComboView {
    /// R714 §5.40 — WAI-ARIA combobox tree. The trigger is always a
    /// `combobox` carrying `aria-expanded` + `aria-controls` → the
    /// listbox + its committed `value`. When open, the `listbox` popup
    /// and its `option` children follow (the active option marked focused
    /// = the `aria-activedescendant` target resolved by
    /// `access_focus_target`).
    fn access_node(state: &ComboViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let trigger_focused = focused == Some(TRIGGER_TAG);
        let open = use_combo_open().get();
        let combo = AccessNode::new(TRIGGER_TAG, AriaRole::ComboBox)
            .with_name("T-shirt size")
            .with_value(pinion_a11y::AccessValue::Text(state.selected.map_or_else(
                || PLACEHOLDER.to_string(),
                |i| LABELS[i.min(N - 1)].to_string(),
            )))
            .with_expanded(open)
            .with_controls(OPTIONS_TAG)
            .with_state(AccessState {
                focused: trigger_focused,
                ..AccessState::default()
            });
        let mut nodes = vec![combo];
        if open {
            // R814 §5.40 — the popup is the lifted `listbox_option_nodes`
            // topology (the combobox trigger above stays bespoke). The
            // builder derives `aria-posinset` / `aria-setsize` from the
            // slice; the active option is reported focused while the trigger
            // owns shell focus (the `aria-activedescendant`).
            let active = active_option(state);
            let tags: Vec<String> = (0..N).map(|i| format!("{OPTIONS_TAG}#{i}")).collect();
            let options: Vec<ListOption<'_>> = LABELS
                .iter()
                .zip(state.options.iter())
                .enumerate()
                .map(|(i, (label, &item_state))| ListOption {
                    tag: &tags[i],
                    label: Some(*label),
                    state: item_state,
                    selected: state.selected == Some(i),
                    focused: trigger_focused && i == active,
                })
                .collect();
            nodes.extend(listbox_option_nodes(
                OPTIONS_TAG,
                "T-shirt size options",
                false,
                &options,
            ));
        }
        nodes
    }

    /// R714 §5.40 — composite focus. While the combobox owns shell focus
    /// and the popup is open, focus stays on the trigger
    /// (`AccessFocus::composite` parent) and the active option is the
    /// `aria-activedescendant` child. Closed (or other focus) is atomic.
    fn access_focus_target(state: &ComboViewState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(TRIGGER_TAG) && use_combo_open().get() {
            Some(AccessFocus::composite(
                TRIGGER_TAG,
                format!("{OPTIONS_TAG}#{}", active_option(state)),
            ))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }
}

impl WidgetView for ComboView {
    type Renderer = HelloComboBoxRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ComboView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn idle() -> ComboViewState {
        ComboViewState::idle()
    }

    fn intent(tag: &str) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(tag.to_string(), IntrospectValue::Null)
    }

    /// Build the multi-external state scene the way the shell does so
    /// `apply_key` has real trigger / listbox / barrier externals.
    fn boot_scene() -> Scene {
        let mut children = vec![Scene::External(
            ExternalNode::new(ComboView::create_external()).with_tag(TRIGGER_TAG),
        )];
        for extra in ComboView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }

    // ----- reducer + signal -----

    #[test]
    fn r714_trigger_click_opens() {
        Owner::new().run(|| {
            close_combo();
            let _ = ComboView::update(idle(), &intent("combo_trigger.click"));
            assert!(use_combo_open().get(), "trigger click opens the popup");
        });
    }

    #[test]
    fn r714_barrier_click_closes() {
        Owner::new().run(|| {
            open_combo();
            let _ = ComboView::update(idle(), &intent("combo_barrier.click"));
            assert!(!use_combo_open().get(), "outside (barrier) click dismisses");
        });
    }

    #[test]
    fn r714_option_commit_closes() {
        Owner::new().run(|| {
            open_combo();
            let _ = ComboView::update(idle(), &intent("combo_options.selected"));
            assert!(
                !use_combo_open().get(),
                "committing an option closes the popup"
            );
        });
    }

    // ----- apply_key -----

    #[test]
    fn r714_arrowdown_opens_when_closed() {
        Owner::new().run(|| {
            close_combo();
            let mut scene = boot_scene();
            let handled = ComboView::apply_key(
                &mut scene,
                Some(TRIGGER_TAG),
                "ArrowDown",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled);
            assert!(
                use_combo_open().get(),
                "ArrowDown opens the closed combobox"
            );
        });
    }

    #[test]
    fn r714_escape_closes_when_open() {
        Owner::new().run(|| {
            open_combo();
            let mut scene = boot_scene();
            let handled = ComboView::apply_key(
                &mut scene,
                Some(TRIGGER_TAG),
                "Escape",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled);
            assert!(!use_combo_open().get(), "Escape closes the open combobox");
        });
    }

    #[test]
    fn r714_escape_ignored_when_closed() {
        Owner::new().run(|| {
            close_combo();
            let mut scene = boot_scene();
            assert!(
                !ComboView::apply_key(
                    &mut scene,
                    Some(TRIGGER_TAG),
                    "Escape",
                    pinion_core::Modifiers::empty(),
                ),
                "Escape on a closed combobox falls through to the shell quit path"
            );
        });
    }

    #[test]
    fn r714_keys_ignored_when_trigger_not_focused() {
        Owner::new().run(|| {
            close_combo();
            let mut scene = boot_scene();
            assert!(!ComboView::apply_key(
                &mut scene,
                None,
                "ArrowDown",
                pinion_core::Modifiers::empty(),
            ));
            assert!(!use_combo_open().get());
        });
    }

    #[test]
    fn r714_arrow_navigation_moves_active_descendant() {
        Owner::new().run(|| {
            open_combo();
            let mut scene = boot_scene();
            let m = pinion_core::Modifiers::empty();
            // Fresh open: first ArrowDown lands on the active option (0).
            assert!(ComboView::apply_key(
                &mut scene,
                Some(TRIGGER_TAG),
                "ArrowDown",
                m
            ));
            assert_eq!(read_combo_state(&scene).focused, Some(0));
            // Next ArrowDown steps to 1, then End jumps to the last.
            assert!(ComboView::apply_key(
                &mut scene,
                Some(TRIGGER_TAG),
                "ArrowDown",
                m
            ));
            assert_eq!(read_combo_state(&scene).focused, Some(1));
            assert!(ComboView::apply_key(
                &mut scene,
                Some(TRIGGER_TAG),
                "End",
                m
            ));
            assert_eq!(read_combo_state(&scene).focused, Some(N - 1));
        });
    }

    #[test]
    fn r714_focusable_tags_lists_only_trigger() {
        // §5.39: collected from the paint scene.
        let scene = Owner::new().run(|| view(&idle(), &Frame::new()));
        assert_eq!(scene.collect_focusable_tags(), vec![TRIGGER_TAG.to_owned()]);
    }

    // ----- a11y -----

    #[test]
    fn r714_closed_emits_only_combobox_node() {
        Owner::new().run(|| {
            close_combo();
            let nodes = ComboView::access_node(&idle(), None);
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].role, AriaRole::ComboBox);
            assert_eq!(nodes[0].tag, TRIGGER_TAG);
            assert_eq!(
                nodes[0].expanded,
                Some(false),
                "closed combobox carries aria-expanded=false"
            );
            assert_eq!(
                nodes[0].controls.as_deref(),
                Some(OPTIONS_TAG),
                "aria-controls → listbox"
            );
        });
    }

    #[test]
    fn r714_open_emits_listbox_with_options() {
        Owner::new().run(|| {
            open_combo();
            let mut s = idle();
            s.selected = Some(2);
            let nodes = ComboView::access_node(&s, Some(TRIGGER_TAG));
            assert_eq!(nodes.len(), N + 2, "combobox + listbox + N options");
            assert_eq!(nodes[0].role, AriaRole::ComboBox);
            assert_eq!(nodes[0].expanded, Some(true));
            assert_eq!(nodes[1].role, AriaRole::Listbox);
            assert_eq!(nodes[1].children.len(), N);
            for (i, node) in nodes.iter().skip(2).enumerate() {
                assert_eq!(node.role, AriaRole::ListBoxOption);
                assert_eq!(
                    node.selected,
                    Some(i == 2),
                    "only the committed option is selected"
                );
            }
        });
    }

    #[test]
    fn r714_combobox_value_reflects_selection() {
        Owner::new().run(|| {
            close_combo();
            let mut s = idle();
            s.selected = Some(3);
            let nodes = ComboView::access_node(&s, None);
            assert_eq!(
                nodes[0].value,
                Some(pinion_a11y::AccessValue::Text("Large".to_string())),
                "combobox value = committed label"
            );
        });
    }

    #[test]
    fn r714_active_descendant_composite_when_open() {
        Owner::new().run(|| {
            open_combo();
            let mut s = idle();
            s.focused = Some(2);
            let focus = ComboView::access_focus_target(&s, Some(TRIGGER_TAG));
            // Composite: parent = trigger, active descendant = option 2.
            assert!(focus.is_some());
        });
    }

    // ----- view / paint -----

    #[test]
    fn r714_closed_view_omits_barrier_and_panel() {
        Owner::new().run(|| {
            close_combo();
            let scene = view(&idle(), &Frame::new());
            assert!(
                !scene.contains_tag(BARRIER_TAG),
                "closed: no barrier painted"
            );
            assert!(!scene.contains_tag(PANEL_TAG), "closed: no panel painted");
            assert!(scene.contains_tag(TRIGGER_TAG), "trigger always painted");
        });
    }

    #[test]
    fn r714_open_view_paints_barrier_panel_and_options() {
        Owner::new().run(|| {
            open_combo();
            let scene = view(&idle(), &Frame::new());
            assert!(
                scene.contains_tag(BARRIER_TAG),
                "open: transparent barrier painted"
            );
            assert!(
                scene.contains_tag(PANEL_TAG),
                "open: dropdown panel painted"
            );
            for i in 0..N {
                assert!(
                    scene.contains_tag(&format!("{OPTIONS_TAG}#{i}")),
                    "option {i} painted"
                );
            }
        });
    }

    /// The trigger's displayed value = the first Text node under the
    /// `combo_trigger` subtree (the value precedes the chevron).
    fn value_text(scene: &Scene) -> Option<String> {
        fn first_text(scene: &Scene) -> Option<String> {
            match scene {
                Scene::Text(t) => Some(t.content.clone()),
                Scene::Container(c) => c.children.iter().find_map(first_text),
                _ => None,
            }
        }
        fn find_trigger(scene: &Scene) -> Option<&Scene> {
            match scene {
                Scene::Container(c) if c.tag.as_deref() == Some(TRIGGER_TAG) => Some(scene),
                Scene::Container(c) => c.children.iter().find_map(find_trigger),
                _ => None,
            }
        }
        find_trigger(scene).and_then(first_text)
    }

    #[test]
    fn r714_trigger_value_shows_placeholder_then_selection() {
        Owner::new().run(|| {
            close_combo();
            let scene = view(&idle(), &Frame::new());
            assert_eq!(value_text(&scene).as_deref(), Some(PLACEHOLDER));
            let mut s = idle();
            s.selected = Some(1);
            let scene = view(&s, &Frame::new());
            assert_eq!(value_text(&scene).as_deref(), Some("Small"));
        });
    }

    #[test]
    fn r714_view_contains_trigger_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ComboView>(
            idle(),
            &Frame::default(),
        );
    }
}
