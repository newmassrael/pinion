// R717 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (WAI-ARIA, TextFieldExternal, ListBoxExternal, dismiss_barrier, …).
#![allow(clippy::doc_markdown)]

//! `hello-combobox-editable` — R717 §5.38 §5.40 §5.50 **editable
//! combobox** (the WAI-ARIA 1.2 §4.5 "Editable Combobox With List
//! Autocomplete" pattern): a single-line text input that owns a popup
//! list which *filters* to the options matching what the user types.
//! The Qt / Flutter / web form factor (`QComboBox{editable:true}`,
//! `Autocomplete`, `<input role=combobox aria-autocomplete=list>`).
//!
//! ## 2nd combobox-class consumer of R714 — still pure composition
//!
//! R714 `hello-combobox` (select-only) decomposed a combobox into proven
//! substrate with **no** `ComboBoxExternal` coordinator. The editable
//! variant is the 2nd consumer of that decomposition; per
//! `[[abstraction-needs-second-consumer]]` it still mints no coordinator
//! — it swaps the trigger and adds binding-side typeahead:
//!
//! * **input** — a [`TextFieldExternal`] (`combo_input`, the primary,
//!   focusable external) instead of R714's read-only button trigger. It
//!   owns the editable text + caret (`use_text_edit_state` /
//!   `use_caret_blink`, the same hooks `hello-textfield` uses). Typing
//!   filters the popup; the field's text *is* the combobox value.
//! * **popup** — the same single-select [`ListBoxExternal`]
//!   (`combo_options`) of N options, reused verbatim. The binding paints
//!   only the *filtered* rows and drives `focused_index` over filtered
//!   absolute indices, so the listbox's roving active descendant always
//!   lands on a visible match.
//! * **dismiss barrier** — the shared [`dismiss_barrier`]
//!   (`combo_barrier`), R715's transparent non-modal click-catcher
//!   (click-outside dismiss). Identical to R714.
//!
//! ## Typeahead lives in the binding, not a new primitive
//!
//! [`filter_indices`] is a pure `&str -> Vec<usize>` (case-insensitive
//! substring) over the option labels. The view renders only matching
//! rows; `apply_key` keeps the listbox `focused_index` on a filtered
//! index. The full option set stays in `ListBoxExternal::new(N)` (boot-
//! time fixed); filtering is a render + focus-mapping concern, not a
//! resize — so no dynamic-count substrate is needed.
//!
//! ## The new a11y axis (R717 substrate)
//!
//! [`AriaRole::EditableComboBox`] (AccessKit's typed-value combobox
//! variant; WAI-ARIA literal stays `combobox`) + [`AccessNode`]'s new
//! `aria-autocomplete` ([`AutoComplete::List`]). The input lowers to
//! `Role::EditableComboBox` carrying `aria-expanded`, `aria-controls` →
//! the listbox, `aria-autocomplete=list`, and an
//! `aria-activedescendant` pointing at the highlighted *filtered* option
//! while focus stays in the input (the single-Tab-stop + roving model).
//!
//! ## Keyboard model (WAI-ARIA §4.5 editable + list autocomplete)
//!
//! Only **ArrowDown / ArrowUp / Enter / Escape** are combobox-reserved.
//! Everything else (printable chars, Backspace/Delete, ArrowLeft/Right,
//! Home/End) flows to the text field — Home/End move the *caret*, not
//! the list (the defining difference from R714's select-only model,
//! where they jumped the list). Typing or ArrowDown opens the popup;
//! Enter commits the active option's label into the field; Escape
//! closes the popup (R695 widget-first Escape); a click outside (the
//! barrier) or selecting an option also closes. All commits funnel
//! through `combo_options.selected` → [`ComboView::update`].
//!
//! ## Known gaps (honest carry)
//!
//! - No dropdown-arrow affordance (the field is the trigger; focus +
//!   typing + ArrowDown open it). A decorative arrow would create a
//!   dead click-zone over the opaque `view_field`; deferred until a
//!   `view_field` trailing-icon slot exists.
//! - IME composition (`apply_composition`) is not wired — ASCII typing
//!   flows through `apply_key`; an IME-composed editable combobox is an
//!   additive carry (the `hello-textfield` `apply_composition` shape).
//! - The input paints no keyboard-focus ring (the shared shell-focus-
//!   paint axis R690 deferred; focus is real + RPC-observable).

use pinion_a11y::{
    listbox_option_nodes, AccessFocus, AccessNode, AccessState, AccessValue, AriaRole, AutoComplete,
    ListOption, WidgetA11y,
};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::ButtonExternal;
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::listbox::ListBoxExternal;
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::listbox::{view_option, OptionRow};
use pinion_widget_paint::popup::popup_surface;
use pinion_widget_paint::text_field as tf_paint;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloComboBoxEditableRenderer, HelloComboBoxEditableRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 460;
const THEME_TAG: &str = "app";

/// Number of options in the (unfiltered) popup.
const N: usize = 6;

/// The editable text input (primary external) — the focusable combobox.
const INPUT_TAG: &str = "combo_input";
/// The popup listbox (extra external). Options paint as `combo_options#<i>`.
const OPTIONS_TAG: &str = "combo_options";
/// The transparent full-window dismiss barrier (extra external).
const BARRIER_TAG: &str = "combo_barrier";
/// The dropdown panel container tag (queryable via `scene/snapshot`).
const PANEL_TAG: &str = "combo_panel";
/// The status line tag echoing the current value + match count.
const STATUS_TAG: &str = "combo_status";

/// Option labels — fruit, chosen so prefixes overlap ("A…", "B…", "Cr…")
/// to exercise substring filtering. Each label doubles as the option's
/// AT accessible name and the committed field text. The single SoT.
const LABELS: [&str; N] = [
    "Apple", "Apricot", "Banana", "Blueberry", "Cherry", "Cranberry",
];

/// Placeholder shown in the empty field before any text is typed.
const PLACEHOLDER: &str = "Type to filter fruit";

/// `Owner::cache` key for the `Signal<bool>` "is the popup open".
const OPEN_KEY: &str = "hello_combobox_editable.open";

// Absolute geometry so the dropdown anchors deterministically below the
// input (and the boot-frame pixel guard can sample fixed points).
const INPUT_X: u32 = 170;
const INPUT_Y: u32 = 120;
const INPUT_W: u32 = 220;
const INPUT_H: u32 = 40;
const OPT_H: u32 = 36;
const PANEL_X: u32 = INPUT_X;
const PANEL_Y: u32 = INPUT_Y + INPUT_H + 4;
const PANEL_PAD: u32 = 6;

/// `Owner::cache`-keyed hook: the shared `Rc<Signal<bool>>` open flag.
fn use_combo_open() -> Rc<Signal<bool>> {
    Owner::current()
        .expect("use_combo_open requires an active Owner scope")
        .cache(OPEN_KEY, || Signal::new(false))
}

/// Open the popup (no focus trap — non-modal).
fn open_combo() {
    use_combo_open().set(true);
}

/// Close the popup.
fn close_combo() {
    use_combo_open().set(false);
}

/// The editable combobox's filter: the absolute indices of the options
/// whose label contains the typed text (case-insensitive). Empty text
/// matches everything (the full list). This is the entire "typeahead"
/// — a pure function, no new substrate.
fn filter_indices(text: &str) -> Vec<usize> {
    let q = text.trim().to_lowercase();
    if q.is_empty() {
        return (0..N).collect();
    }
    (0..N)
        .filter(|&i| LABELS[i].to_lowercase().contains(&q))
        .collect()
}

/// Commit option `idx`: replace the field text with its label, park the
/// caret at the end, and close the popup. The field's text is the
/// combobox value, so committing *is* a `set_text`. Requires an Owner
/// scope (the reducer / `apply_key` paths are owner-wrapped).
fn commit_selection(idx: usize) {
    let label = LABELS[idx.min(N - 1)];
    let ts = use_text_edit_state(INPUT_TAG);
    ts.seed(label.to_string());
    close_combo();
}

/// Whether `key` is a text-content key (a single printable char, or a
/// delete) — typing one of these (re)opens + refilters the popup.
/// Caret-movement / control keys are excluded.
fn is_text_input_key(key: &str) -> bool {
    key.chars().count() == 1 || matches!(key, "Backspace" | "Delete")
}

/// Cached posture for the paint fn. The text + open flag are reactive
/// (read via hooks); this Copy snapshot carries only the listbox/field
/// interaction state per `[[update-by-value-snapshot]]`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct ComboViewState {
    /// Text-field interaction posture (SCXML state) for the field paint.
    input: TextFieldState,
    /// Caret byte offset for the field paint.
    caret: u32,
    /// The committed selection (absolute index), or `None`.
    selected: Option<usize>,
    /// The roving active descendant (`focused_index`, absolute), or `None`.
    focused: Option<usize>,
    /// Per-option interaction state (hover / pressed) from the listbox.
    options: [ListboxItemState; N],
}

impl ComboViewState {
    fn idle() -> Self {
        Self {
            input: TextFieldState::Idle,
            caret: 0,
            selected: None,
            focused: None,
            options: [ListboxItemState::Idle; N],
        }
    }
}

/// Resolve the active descendant *within the filtered set*: the roving
/// `focused` when it is itself a current match, else the first filtered
/// option. `None` when nothing matches (empty popup).
fn resolve_active(state: &ComboViewState, filtered: &[usize]) -> Option<usize> {
    if filtered.is_empty() {
        return None;
    }
    if let Some(f) = state.focused {
        if filtered.contains(&f) {
            return Some(f);
        }
    }
    filtered.first().copied()
}

/// Read the combobox posture from the state scene: the text field's
/// interaction state + caret, then the popup listbox's selection / focus
/// / per-option state (mirror of R714's `read_combo_state`).
fn read_combo_state(scene: &Scene) -> ComboViewState {
    let mut out = ComboViewState::idle();
    let (input, caret) = tf_paint::read_text_field_state(scene, INPUT_TAG);
    out.input = input;
    out.caret = caret;
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

/// Paint one popup option cell, tagged `combo_options#<abs_index>` so the
/// InputRouter `'#'`-split reaches the composite [`ListBoxExternal`]. Only
/// filtered options are passed here, but the tag keeps the *absolute*
/// index so selection / roving stay consistent with the full listbox.
fn option_scene(abs_index: usize, state: &ComboViewState, active: Option<usize>, theme: &Theme) -> Scene {
    // The option-cell skin is the lifted [`view_option`] SSOT (R867, 2nd of
    // three consumers — combobox / editable-combobox / property-grid choice).
    view_option(
        &OptionRow {
            tag: format!("{OPTIONS_TAG}#{abs_index}"),
            label: LABELS[abs_index],
            state: state.options[abs_index],
            active: active == Some(abs_index),
            selected: state.selected == Some(abs_index),
        },
        INPUT_W - 2 * PANEL_PAD,
        OPT_H,
        theme,
    )
}

/// view-fn (§6.3): pure sync `(postures) -> Scene`. Reads the reactive
/// text + open flag via hooks. When open the transparent barrier +
/// filtered dropdown are pushed **last** so they hit-test above the
/// input.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &ComboViewState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let open = use_combo_open().get();
    let text = use_text_edit_state(INPUT_TAG).text();
    let filtered = filter_indices(&text);

    let label = Scene::Text(
        TextNode::styled(
            "Fruit",
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(INPUT_X, INPUT_Y - 26)),
    );

    // The lifted TextField paint, wrapped in an absolutely-positioned
    // container so the popup below anchors deterministically. The inner
    // field keeps the `combo_input` hit-test tag.
    let field_style = tf_paint::TextFieldStyle {
        field_w: INPUT_W,
        field_h: INPUT_H,
        ..tf_paint::TextFieldStyle::m3_filled()
    };
    let field_inner = tf_paint::view_field(
        INPUT_TAG,
        state.input,
        state.caret,
        &theme,
        &field_style,
        PLACEHOLDER,
    );
    let field = Scene::Container(
        ContainerNode::new(vec![field_inner])
            .with_layout(LayoutStyle::new().with_absolute_position(INPUT_X, INPUT_Y)),
    );

    let status = Scene::Text(
        TextNode::styled(
            format!(
                "Value: \"{}\" | {} match{}",
                text,
                filtered.len(),
                if filtered.len() == 1 { "" } else { "es" }
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(15)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(STATUS_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(INPUT_X, WIN_H - 60)),
    );

    let mut children = vec![label, field, status];

    if open && !filtered.is_empty() {
        let active = resolve_active(state, &filtered);
        let options: Vec<Scene> = filtered
            .iter()
            .map(|&abs| option_scene(abs, state, active, &theme))
            .collect();
        let panel_h = u32::try_from(filtered.len()).expect("filtered len fits in u32") * OPT_H
            + 2 * PANEL_PAD;
        let panel = Scene::Container(
            ContainerNode::new(options)
                .with_tag(PANEL_TAG)
                .with_style(popup_surface(&theme))
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(PANEL_X, PANEL_Y)
                        .with_size(Size::px(INPUT_W, panel_h))
                        .flex(FlexDirection::Column)
                        .with_align_items(AlignItems::Stretch)
                        .with_gap(2)
                        .with_padding(Rect::new(PANEL_PAD, PANEL_PAD, PANEL_PAD, PANEL_PAD)),
                ),
        );
        // R715 §5.16 — shared transparent click-outside barrier (the
        // input sits *under* it; the absolutely-positioned panel is
        // pushed after so it hit-tests above the barrier).
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

/// Set the popup's `focused_index` active descendant to absolute `idx`.
fn set_focused(scene: &mut Scene, idx: usize) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(OPTIONS_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    intro
        .intervene(
            "focused_index",
            IntrospectValue::Int(i64::try_from(idx).expect("option index fits in i64")),
        )
        .is_ok()
}

/// Move the active descendant by `delta` *within the filtered set*. A
/// fresh open (no roving `focused`) lands on the resolved active without
/// stepping; subsequent moves step through the filtered positions
/// (clamped).
fn move_active(scene: &mut Scene, state: &ComboViewState, filtered: &[usize], delta: i32) -> bool {
    let Some(active) = resolve_active(state, filtered) else {
        return false;
    };
    let pos = i32::try_from(filtered.iter().position(|&x| x == active).unwrap_or(0)).unwrap_or(0);
    let next_pos = if state.focused.is_none() {
        pos
    } else {
        let last = i32::try_from(filtered.len() - 1).unwrap_or(0);
        (pos + delta).clamp(0, last)
    };
    let next = filtered[usize::try_from(next_pos).unwrap_or(0)];
    set_focused(scene, next)
}

/// Activate the active-descendant option through the listbox wire format
/// (full pointer cycle) so single-select exclusion + the `"selected"`
/// intent fire exactly as a click would. The intent drives `update` →
/// `commit_selection` (text + close).
fn activate_active(scene: &mut Scene, state: &ComboViewState, filtered: &[usize]) -> bool {
    let Some(idx) = resolve_active(state, filtered) else {
        return false;
    };
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

/// Delegate `key` to the text field's edit dispatch. Returns whether the
/// field recognized it (W3C `defaultPrevented` semantic).
fn delegate_to_field(scene: &mut Scene, key: &str) -> bool {
    // R764.1 §5.38 — forward through the lifted SSOT (empty modifiers =
    // bare Text, behaviour-identical to the pre-lift call).
    pinion_core::forward_key_to_field(scene, INPUT_TAG, key, pinion_core::Modifiers::empty())
}

struct ComboView;

impl WidgetCore for ComboView {
    type State = ComboViewState;
    // Text editing + listbox roving + ARIA Enter/Space flow through the
    // pointer router + apply_key; no keybinding-channel events.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        // Same hooks hello-textfield uses; `create_external` runs inside
        // `root_owner.run(...)` so these resolve to the instances the
        // view fn reads later (Owner::cache dedup).
        let text_state = use_text_edit_state(INPUT_TAG);
        let blink = use_caret_blink(INPUT_TAG);
        Box::new(
            TextFieldExternal::new()
                .attach_state(text_state)
                .attach_blink(blink),
        )
    }

    /// The popup listbox + the click-only transparent barrier, as extra
    /// externals (both persist in the state scene; the view only paints
    /// them while open).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(OPTIONS_TAG, Box::new(ListBoxExternal::new(N))),
            ExtraExternal::new(BARRIER_TAG, Box::new(ButtonExternal::new())),
        ]
    }

    fn tag() -> &'static str {
        INPUT_TAG
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
        "pinion hello-combobox-editable (R717 §5.38 editable combobox)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// The editable combobox is a single Tab stop — only the input is
    /// focusable. The filtered options are the input's roving active
    /// descendants (never their own Tab stops); the barrier is a click
    /// target only.
    fn focusable_tags() -> Vec<&'static str> {
        vec![INPUT_TAG]
    }

    /// WAI-ARIA §4.5 editable-combobox keyboard model. Only ArrowDown /
    /// ArrowUp / Enter / Escape are combobox-reserved; every other key
    /// flows to the text field (Home/End/Left/Right move the caret).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(INPUT_TAG) {
            return false;
        }
        let state = read_combo_state(scene);
        let filtered = filter_indices(&use_text_edit_state(INPUT_TAG).text());
        let open = use_combo_open().get();
        if open {
            match key {
                "Escape" => {
                    close_combo();
                    true
                }
                "ArrowDown" => move_active(scene, &state, &filtered, 1),
                "ArrowUp" => move_active(scene, &state, &filtered, -1),
                "Enter" => activate_active(scene, &state, &filtered),
                _ => {
                    let handled = delegate_to_field(scene, key);
                    if handled && is_text_input_key(key) {
                        // Text changed: refilter + park the active
                        // descendant on the first new match, stay open.
                        let nf = filter_indices(&use_text_edit_state(INPUT_TAG).text());
                        if let Some(&first) = nf.first() {
                            set_focused(scene, first);
                        }
                    }
                    handled
                }
            }
        } else if key == "ArrowDown" {
            open_combo();
            resolve_active(&state, &filtered).is_some_and(|a| set_focused(scene, a))
        } else {
            let handled = delegate_to_field(scene, key);
            if handled && is_text_input_key(key) {
                open_combo();
                let nf = filter_indices(&use_text_edit_state(INPUT_TAG).text());
                if let Some(&first) = nf.first() {
                    set_focused(scene, first);
                }
            }
            handled
        }
    }

    /// R717 §5.40 — bridge the option / barrier intents into the combobox
    /// state. A committed option copies its label into the field (the
    /// combobox value) and closes; an outside click just closes.
    fn update(
        _state: ComboViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "combo_options.selected" => match &intent.payload {
                // Post-flip authority (`[[intent-payload-post-flip-authority]]`):
                // the listbox carries the committed absolute index.
                IntrospectValue::Int(i) => {
                    if let Ok(idx) = usize::try_from(*i) {
                        commit_selection(idx);
                    } else {
                        close_combo();
                    }
                }
                _ => close_combo(),
            },
            "combo_barrier.click" => close_combo(),
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &ComboViewState) -> String {
        format!(
            "input={:?} caret={} selected={:?} focused={:?}",
            state.input, state.caret, state.selected, state.focused
        )
    }
}

impl WidgetA11y for ComboView {
    /// R717 §5.40 — WAI-ARIA editable-combobox tree. The input is always
    /// an `EditableComboBox` carrying `aria-expanded` + `aria-controls` →
    /// the listbox + `aria-autocomplete=list` + its current text value.
    /// When open, the `listbox` popup follows with one `option` per
    /// *filtered* match (the active one marked focused = the
    /// `aria-activedescendant`).
    fn access_node(state: &ComboViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let input_focused = focused == Some(INPUT_TAG);
        let open = use_combo_open().get();
        let text = use_text_edit_state(INPUT_TAG).text();
        let filtered = filter_indices(&text);
        let combo = AccessNode::new(INPUT_TAG, AriaRole::EditableComboBox)
            .with_name("Fruit")
            .with_value(AccessValue::Text(text))
            .with_expanded(open && !filtered.is_empty())
            .with_controls(OPTIONS_TAG)
            .with_auto_complete(AutoComplete::List)
            .with_state(AccessState {
                focused: input_focused,
                ..AccessState::default()
            });
        let mut nodes = vec![combo];
        if open && !filtered.is_empty() {
            // R814 §5.40 — popup via the lifted `listbox_option_nodes`
            // builder over the *filtered* matches; the builder numbers
            // `aria-posinset` / `aria-setsize` within the filtered slice
            // (the active match is the `aria-activedescendant`).
            let active = resolve_active(state, &filtered);
            let tags: Vec<String> =
                filtered.iter().map(|&abs| format!("{OPTIONS_TAG}#{abs}")).collect();
            let options: Vec<ListOption<'_>> = filtered
                .iter()
                .enumerate()
                .map(|(pos, &abs)| ListOption {
                    tag: &tags[pos],
                    label: Some(LABELS[abs]),
                    state: state.options[abs],
                    selected: state.selected == Some(abs),
                    focused: input_focused && active == Some(abs),
                })
                .collect();
            nodes.extend(listbox_option_nodes(OPTIONS_TAG, "Fruit options", false, &options));
        }
        nodes
    }

    /// R717 §5.40 — composite focus: while the input owns shell focus and
    /// the popup is open, focus stays on the input (the combobox) and the
    /// active filtered option is the `aria-activedescendant` child.
    fn access_focus_target(state: &ComboViewState, focused: Option<&str>) -> Option<AccessFocus> {
        let filtered = filter_indices(&use_text_edit_state(INPUT_TAG).text());
        if focused == Some(INPUT_TAG) && use_combo_open().get() {
            if let Some(active) = resolve_active(state, &filtered) {
                return Some(AccessFocus::composite(
                    INPUT_TAG,
                    format!("{OPTIONS_TAG}#{active}"),
                ));
            }
        }
        focused.map(AccessFocus::atomic)
    }
}

impl WidgetView for ComboView {
    type Renderer = HelloComboBoxEditableRenderer;

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

    fn intent_with(tag: &str, payload: IntrospectValue) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(tag.to_string(), payload)
    }

    /// Build the multi-external state scene the way the shell does so
    /// `apply_key` has real input / listbox / barrier externals.
    fn boot_scene() -> Scene {
        let mut children = vec![Scene::External(
            ExternalNode::new(ComboView::create_external()).with_tag(INPUT_TAG),
        )];
        for extra in ComboView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }

    // ----- filter -----

    #[test]
    fn r717_filter_empty_matches_all() {
        assert_eq!(filter_indices(""), (0..N).collect::<Vec<_>>());
        assert_eq!(filter_indices("   "), (0..N).collect::<Vec<_>>());
    }

    #[test]
    fn r717_filter_substring_case_insensitive() {
        // "ap" matches Apple (0) + Apricot (1).
        assert_eq!(filter_indices("ap"), vec![0, 1]);
        assert_eq!(filter_indices("AP"), vec![0, 1]);
        // "cr" matches Cranberry (5) only (substring, not just prefix).
        assert_eq!(filter_indices("cr"), vec![5]);
        // "berry" matches Blueberry (3) + Cranberry (5).
        assert_eq!(filter_indices("berry"), vec![3, 5]);
        // no match.
        assert!(filter_indices("zzz").is_empty());
    }

    #[test]
    fn r717_resolve_active_prefers_focused_in_filter_else_first() {
        let mut s = idle();
        let filtered = vec![0, 1];
        assert_eq!(resolve_active(&s, &filtered), Some(0), "no focus → first match");
        s.focused = Some(1);
        assert_eq!(resolve_active(&s, &filtered), Some(1), "focus in filter kept");
        s.focused = Some(4); // out of filter
        assert_eq!(resolve_active(&s, &filtered), Some(0), "stale focus → first match");
        assert_eq!(resolve_active(&s, &[]), None, "empty filter → none");
    }

    // ----- reducer -----

    #[test]
    fn r717_option_commit_sets_text_and_closes() {
        Owner::new().run(|| {
            open_combo();
            // Commit absolute index 2 = "Banana".
            let _ = ComboView::update(idle(), &intent_with("combo_options.selected", IntrospectValue::Int(2)));
            assert!(!use_combo_open().get(), "committing closes the popup");
            assert_eq!(use_text_edit_state(INPUT_TAG).text(), "Banana", "field value = committed label");
        });
    }

    #[test]
    fn r717_barrier_click_closes_without_commit() {
        Owner::new().run(|| {
            use_text_edit_state(INPUT_TAG).set_text("App".to_string());
            open_combo();
            let _ = ComboView::update(idle(), &intent_with("combo_barrier.click", IntrospectValue::Null));
            assert!(!use_combo_open().get(), "outside click dismisses");
            assert_eq!(use_text_edit_state(INPUT_TAG).text(), "App", "barrier dismiss keeps typed text");
        });
    }

    // ----- apply_key -----

    #[test]
    fn r717_typing_opens_and_filters_focus() {
        Owner::new().run(|| {
            close_combo();
            use_text_edit_state(INPUT_TAG).set_text(String::new());
            let mut scene = boot_scene();
            let handled = ComboView::apply_key(
                &mut scene,
                Some(INPUT_TAG),
                "a",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled, "the field consumes the printable key");
            assert!(use_combo_open().get(), "typing opens the popup");
            // "a" matched → focus parked on first match (Apple = 0).
            assert_eq!(read_combo_state(&scene).focused, Some(0));
        });
    }

    #[test]
    fn r717_escape_closes_when_open() {
        Owner::new().run(|| {
            open_combo();
            let mut scene = boot_scene();
            let handled = ComboView::apply_key(
                &mut scene,
                Some(INPUT_TAG),
                "Escape",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled);
            assert!(!use_combo_open().get(), "Escape closes the open popup");
        });
    }

    #[test]
    fn r717_arrowdown_opens_when_closed() {
        Owner::new().run(|| {
            close_combo();
            use_text_edit_state(INPUT_TAG).set_text(String::new());
            let mut scene = boot_scene();
            let handled = ComboView::apply_key(
                &mut scene,
                Some(INPUT_TAG),
                "ArrowDown",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled);
            assert!(use_combo_open().get(), "ArrowDown opens the closed combobox");
        });
    }

    #[test]
    fn r717_arrow_navigation_moves_active_within_filter() {
        Owner::new().run(|| {
            use_text_edit_state(INPUT_TAG).set_text("ap".to_string()); // filter = [0,1]
            open_combo();
            let mut scene = boot_scene();
            let m = pinion_core::Modifiers::empty();
            // Fresh open: first ArrowDown lands on the active (first match, 0).
            assert!(ComboView::apply_key(&mut scene, Some(INPUT_TAG), "ArrowDown", m));
            assert_eq!(read_combo_state(&scene).focused, Some(0));
            // Next ArrowDown steps to the 2nd filtered match (1 = Apricot).
            assert!(ComboView::apply_key(&mut scene, Some(INPUT_TAG), "ArrowDown", m));
            assert_eq!(read_combo_state(&scene).focused, Some(1));
            // ArrowDown again clamps at the last filtered match (still 1).
            assert!(ComboView::apply_key(&mut scene, Some(INPUT_TAG), "ArrowDown", m));
            assert_eq!(read_combo_state(&scene).focused, Some(1));
        });
    }

    #[test]
    fn r717_keys_ignored_when_input_not_focused() {
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
    fn r717_focusable_tags_lists_only_input() {
        assert_eq!(ComboView::focusable_tags(), vec![INPUT_TAG]);
    }

    // ----- a11y -----

    #[test]
    fn r717_closed_emits_only_editable_combobox_node() {
        Owner::new().run(|| {
            close_combo();
            use_text_edit_state(INPUT_TAG).set_text(String::new());
            let nodes = ComboView::access_node(&idle(), None);
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].role, AriaRole::EditableComboBox);
            assert_eq!(nodes[0].tag, INPUT_TAG);
            assert_eq!(nodes[0].expanded, Some(false), "closed carries aria-expanded=false");
            assert_eq!(nodes[0].controls.as_deref(), Some(OPTIONS_TAG), "aria-controls → listbox");
            assert_eq!(nodes[0].auto_complete, Some(AutoComplete::List), "aria-autocomplete=list");
        });
    }

    #[test]
    fn r717_open_emits_listbox_with_filtered_options() {
        Owner::new().run(|| {
            use_text_edit_state(INPUT_TAG).set_text("ap".to_string()); // [0,1]
            open_combo();
            let nodes = ComboView::access_node(&idle(), Some(INPUT_TAG));
            // combobox + listbox + 2 filtered options.
            assert_eq!(nodes.len(), 4);
            assert_eq!(nodes[0].role, AriaRole::EditableComboBox);
            assert_eq!(nodes[0].expanded, Some(true));
            assert_eq!(nodes[1].role, AriaRole::Listbox);
            assert_eq!(nodes[1].children.len(), 2, "only filtered options are children");
            for (i, node) in nodes.iter().skip(2).enumerate() {
                assert_eq!(node.role, AriaRole::ListBoxOption);
                assert_eq!(node.size_of_set, Some(2), "setsize = filtered count");
                assert_eq!(node.position_in_set, Some(u32::try_from(i + 1).unwrap()));
            }
        });
    }

    #[test]
    fn r717_value_reflects_typed_text() {
        Owner::new().run(|| {
            close_combo();
            use_text_edit_state(INPUT_TAG).set_text("Cher".to_string());
            let nodes = ComboView::access_node(&idle(), None);
            assert_eq!(
                nodes[0].value,
                Some(AccessValue::Text("Cher".to_string())),
                "editable combobox value = the field text"
            );
        });
    }

    #[test]
    fn r717_active_descendant_composite_when_open() {
        Owner::new().run(|| {
            use_text_edit_state(INPUT_TAG).set_text("a".to_string());
            open_combo();
            let mut s = idle();
            s.focused = Some(1);
            let focus = ComboView::access_focus_target(&s, Some(INPUT_TAG));
            assert!(focus.is_some(), "composite active descendant when open");
        });
    }

    // ----- view / paint -----

    #[test]
    fn r717_closed_view_omits_barrier_and_panel() {
        Owner::new().run(|| {
            close_combo();
            let scene = view(&idle(), &Frame::new());
            assert!(!scene.contains_tag(BARRIER_TAG), "closed: no barrier");
            assert!(!scene.contains_tag(PANEL_TAG), "closed: no panel");
            assert!(scene.contains_tag(INPUT_TAG), "input always painted");
        });
    }

    #[test]
    fn r717_open_view_paints_only_filtered_rows() {
        Owner::new().run(|| {
            use_text_edit_state(INPUT_TAG).set_text("ap".to_string()); // [0,1]
            open_combo();
            let scene = view(&idle(), &Frame::new());
            assert!(scene.contains_tag(BARRIER_TAG), "open: barrier painted");
            assert!(scene.contains_tag(PANEL_TAG), "open: panel painted");
            assert!(scene.contains_tag(&format!("{OPTIONS_TAG}#0")), "Apple row painted");
            assert!(scene.contains_tag(&format!("{OPTIONS_TAG}#1")), "Apricot row painted");
            assert!(!scene.contains_tag(&format!("{OPTIONS_TAG}#2")), "Banana filtered out");
        });
    }

    #[test]
    fn r717_open_with_no_match_paints_no_panel() {
        Owner::new().run(|| {
            use_text_edit_state(INPUT_TAG).set_text("zzz".to_string());
            open_combo();
            let scene = view(&idle(), &Frame::new());
            assert!(!scene.contains_tag(PANEL_TAG), "no match → no popup even when open");
        });
    }

    #[test]
    fn r717_view_contains_input_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ComboView>(
            idle(),
            &Frame::default(),
        );
    }
}
