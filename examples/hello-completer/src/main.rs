// R1449 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the architectural narrative carries many proper-noun
// identifiers (QCompleter, CompletionState, TextFieldExternal, WAI-ARIA, …).
#![allow(clippy::doc_markdown)]

//! `hello-completer` — R1449 §5.27 §5.38 §5.40 **a completer attached to a
//! plain text input**: Qt's `QCompleter`, which is not a widget but a model you
//! hang off any input.
//!
//! ## Why this binding exists (and is not the editable combobox again)
//!
//! R717 `hello-combobox-editable` filters a *fixed option set* a value must
//! come from, with one hard-coded rule: case-insensitive substring. A completer
//! is the other thing — the input accepts free text and the candidates are only
//! *suggestions* — and its rule is configurable. This binding is the first
//! pinion consumer of [`pinion_core::widgets::completion`], and drives all
//! three Qt knobs live:
//!
//! * **filter mode** — `starts_with` / `contains` / `ends_with`
//!   (`QCompleter::setFilterMode`). The candidate list is code identifiers, so
//!   each mode gives a visibly different answer: `render` starts three of them,
//!   `Scene` is contained in three others, `Buffer` ends two.
//! * **case sensitivity** — `QCompleter::setCaseSensitivity`. `render` matches
//!   `RenderPass` insensitively and not sensitively, over the same list.
//! * **completion mode** — `popup` / `unfiltered_popup` / `inline`
//!   (`QCompleter::setCompletionMode`). The unfiltered popup lists **every**
//!   candidate with the match marked; inline mode has no popup at all and
//!   instead completes the field in place.
//!
//! ## The cursor cannot live in the listbox (why this differs from R717)
//!
//! The combobox keeps its active descendant in the popup `ListBoxExternal`'s
//! `focused_index`. A completer cannot: in **inline** mode there *is* no popup,
//! and `current_completion` still has to answer. So the
//! [`CompletionState`] cursor is the single
//! active-descendant authority here, and the listbox contributes only what it
//! is uniquely good at — per-row hit-testing, hover / pressed states, and the
//! `"selected"` commit intent. Its own `focused_index` is deliberately unused;
//! two cursors would be a bug waiting for a mode switch.
//!
//! ## Inline completion is a real text mutation (Qt's, not a ghost label)
//!
//! In inline mode a keystroke sets the field text to `prefix + suffix` and
//! **selects the appended part** ([`apply_inline`]), so the next keystroke types
//! over it — the type-to-replace path the text field already has. Typing `r`
//! yields `renderScene` with `enderScene` selected; typing `e` next replaces the
//! selection and re-completes from `re`. A knob change never rewrites the field:
//! the *readout* updates immediately, the text changes only when the user types.
//!
//! ## AI clients (§2 #7 + §2 #2 — the part Qt cannot do)
//!
//! `QCompleter` answers `currentCompletion()` to C++ and nothing to anyone else.
//! Here `comp_model` is a [`CompleterExternal`]: `query("prefix" | "filter" |
//! "case" | "mode" | "completion_count" | "current" | "current_completion" |
//! "inline" | "completion.<i>")` reads the whole completion, and
//! `intervene` on the four knobs drives it — so an agent can ask "what would you
//! complete `ren` to, matching case-sensitively, as an inline completion?"
//! without touching the keyboard. The status rows paint the same values, so
//! `scene/snapshot` and the pixels cannot disagree.

use pinion_a11y::{
    AccessFocus, AccessNode, AccessState, AccessValue, AriaRole, AutoComplete, ListOption,
    ToolbarControl, WidgetA11y, listbox_option_nodes, toolbar_button_nodes,
};
use pinion_core::command::Command;
use pinion_core::external::{External, IntrospectValue};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::ButtonExternal;
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::completion::{
    CompleterExternal, CompletionCase, CompletionFilter, CompletionMode, CompletionState,
    use_completion,
};
use pinion_core::widgets::listbox::ListBoxExternal;
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::widgets::toolbar::{ToolItem, ToolbarExternal};
use pinion_core::{Frame, Intent, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::listbox::{OptionRow, view_option};
use pinion_widget_paint::popup::popup_surface;
use pinion_widget_paint::text_field as tf_paint;
use pinion_widget_paint::toolbar::{ToolbarStyle, composite_item_tag, view_toolbar};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloCompleterRenderer, HelloCompleterRendererError);

const WIN_W: u32 = 640;
const WIN_H: u32 = 520;
const THEME_TAG: &str = "app";

/// The text input the completer attaches to (primary, focusable external).
const INPUT_TAG: &str = "comp_input";
/// The completion model + its wire surface (extra external).
const MODEL_TAG: &str = "comp_model";
/// The popup listbox (extra external). Rows paint as `comp_options#<source>`.
const OPTIONS_TAG: &str = "comp_options";
/// The transparent full-window dismiss barrier (extra external).
const BARRIER_TAG: &str = "comp_barrier";
/// The three knob controls (extra external): filter / case / mode.
const KNOBS_TAG: &str = "comp_knobs";
/// The dropdown panel container tag (queryable via `scene/snapshot`).
const PANEL_TAG: &str = "comp_panel";
/// Status line: prefix + the three knobs + the completion count.
const STATUS_TAG: &str = "comp_status";
/// Status line: the current completion + what inline mode would append.
const CURRENT_TAG: &str = "comp_current";

/// The knob command intent, dotted with its scope tag (R51.173).
const KNOB_INTENT: &str = pinion_core::intent_tag!("comp_knobs", "command");
/// The commit intent the popup listbox emits.
const OPTION_INTENT: &str = pinion_core::intent_tag!("comp_options", "selected");
/// The barrier's click intent.
const BARRIER_INTENT: &str = pinion_core::intent_tag!("comp_barrier", "click");

/// Candidate identifiers, chosen so **each knob changes the answer** over one
/// list: three begin with `render` (one of them only case-insensitively), three
/// contain `Scene`, two end with `Buffer`. A completer's natural domain, and the
/// one a self-hosted editor needs first.
const CANDIDATES: [&str; 8] = [
    "renderScene",
    "renderTarget",
    "RenderPass",
    "sceneGraph",
    "SceneNode",
    "targetBuffer",
    "depthBuffer",
    "presentSurface",
];
/// Candidate count.
const N: usize = CANDIDATES.len();

/// Placeholder shown in the empty field.
const PLACEHOLDER: &str = "Type a symbol";

/// `Owner::cache` key for the shared [`CompletionState`].
const MODEL_KEY: &str = "hello_completer.model";
/// `Owner::cache` key for the `Signal<bool>` "is the popup open".
const OPEN_KEY: &str = "hello_completer.open";

// Absolute geometry so the popup anchors deterministically below the input
// (and a boot-frame pixel guard can sample fixed points).
const KNOBS_X: u32 = 40;
const KNOBS_Y: u32 = 40;
const INPUT_X: u32 = 40;
const INPUT_Y: u32 = 150;
const INPUT_W: u32 = 320;
const INPUT_H: u32 = 40;
const OPT_H: u32 = 34;
const PANEL_X: u32 = INPUT_X;
const PANEL_Y: u32 = INPUT_Y + INPUT_H + 4;
const PANEL_PAD: u32 = 6;

/// The shared completion model — the `External`, the view, the a11y tree, and
/// the reducer all reach the same `Rc` through this hook.
fn use_model() -> Rc<CompletionState> {
    use_completion(MODEL_KEY, || {
        CANDIDATES.iter().map(|s| (*s).to_string()).collect()
    })
}

/// The shared "is the popup open" flag. Orthogonal to the completion model: the
/// model always knows its current completion, the flag only says whether a
/// surface is showing it.
fn use_open() -> Rc<Signal<bool>> {
    Owner::current()
        .expect("use_open requires an active Owner scope")
        .cache(OPEN_KEY, || Signal::new(false))
}

/// Whether a popup is actually on screen: the mode presents one, the user has
/// asked for suggestions, and there is something to list. The one predicate the
/// view, the keyboard model, and the a11y tree share, so they cannot disagree
/// about whether the popup exists.
///
/// R1449 — "a mode that presents no popup shows no popup" is encoded **here and
/// only here**. An earlier draft also *wrote* `open = false` when a knob click
/// selected inline mode; that second copy of the rule was reachable from the
/// toolbar but not from `scene/intervene`, so the human path and the RPC path
/// disagreed about a mode round-trip — the §2 #2 convergence this whole
/// framework rests on. Deriving instead of writing makes the divergence
/// unrepresentable. The visible consequence is honest: returning to a popup mode
/// shows the popup again, because switching *presentation* was never a dismissal
/// (Escape and the barrier are).
fn popup_showing(model: &CompletionState) -> bool {
    model.mode().is_popup() && use_open().get() && model.completion_count() > 0
}

/// Set the completion prefix from what is actually in the field — the one
/// direction of truth (the field owns the text, the model reads it).
fn sync_prefix() {
    let text = use_text_edit_state(INPUT_TAG).text();
    use_model().set_prefix(&text);
}

/// Qt's `InlineCompletion`, as a real text mutation: replace the field text with
/// `prefix + suffix` and **select the appended part**, so the next keystroke
/// types over it (the field's type-to-replace path). A no-op when the mode does
/// not complete inline, or when there is nothing to append.
fn apply_inline() {
    let model = use_model();
    let Some(suffix) = model.inline_completion() else {
        return;
    };
    if suffix.is_empty() {
        return;
    }
    let prefix = model.prefix();
    let full = format!("{prefix}{suffix}");
    let ts = use_text_edit_state(INPUT_TAG);
    ts.set_text(full.clone());
    ts.set_selection(prefix.len(), full.len());
}

/// Commit candidate `source`: its text becomes the field value (caret at the
/// end), the prefix follows it, and the popup closes. The completer's
/// `activated()` — what a click on a row or Enter on the current one does.
fn commit(source: usize) {
    let model = use_model();
    let label = model.candidate(source).to_string();
    use_text_edit_state(INPUT_TAG).seed(label.clone());
    model.set_prefix(&label);
    use_open().set(false);
}

/// Whether `key` is a text-content key (a single printable char, or a delete) —
/// typing one of these re-syncs the prefix and re-completes.
fn is_text_input_key(key: &str) -> bool {
    key.chars().count() == 1 || matches!(key, "Backspace" | "Delete")
}

/// Cycle knob `idx` (0 = filter, 1 = case, 2 = mode) to its next value. A knob
/// change never rewrites the field — only the readout moves — so a click can
/// never type text the user did not ask for. Whether a popup is on screen after
/// the change is [`popup_showing`]'s business, not this function's: a knob click
/// and a `scene/intervene` write reach the same state through the same rule.
fn cycle_knob(idx: usize) {
    let model = use_model();
    match idx {
        0 => {
            model.set_filter(match model.filter() {
                CompletionFilter::StartsWith => CompletionFilter::Contains,
                CompletionFilter::Contains => CompletionFilter::EndsWith,
                CompletionFilter::EndsWith => CompletionFilter::StartsWith,
            });
        }
        1 => {
            model.set_case(match model.case() {
                CompletionCase::Insensitive => CompletionCase::Sensitive,
                CompletionCase::Sensitive => CompletionCase::Insensitive,
            });
        }
        2 => {
            model.set_mode(match model.mode() {
                CompletionMode::Popup => CompletionMode::UnfilteredPopup,
                CompletionMode::UnfilteredPopup => CompletionMode::Inline,
                CompletionMode::Inline => CompletionMode::Popup,
            });
        }
        _ => {}
    }
}

/// Cached posture for the paint fn. The completion model is reactive (read via
/// [`use_model`]); this Copy snapshot carries only the interaction state per
/// `[[update-by-value-snapshot]]`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct CompleterViewState {
    /// Text-field interaction posture (SCXML state) for the field paint.
    input: TextFieldState,
    /// Caret byte offset for the field paint.
    caret: u32,
    /// Per-row interaction state (hover / pressed) from the popup listbox.
    options: [ListboxItemState; N],
}

impl CompleterViewState {
    const fn idle() -> Self {
        Self {
            input: TextFieldState::Idle,
            caret: 0,
            options: [ListboxItemState::Idle; N],
        }
    }
}

/// Read the completer posture from the state scene: the field's interaction
/// state + caret, then the popup rows' hover / pressed states. The *cursor* is
/// not read here — it lives in the shared model, not in an external.
fn read_completer_state(scene: &Scene) -> CompleterViewState {
    let mut out = CompleterViewState::idle();
    let (input, caret) = tf_paint::read_text_field_state(scene, INPUT_TAG);
    out.input = input;
    out.caret = caret;
    let Some(intro) = scene
        .find_external_with_tag(OPTIONS_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return out;
    };
    for (i, slot) in out.options.iter_mut().enumerate() {
        *slot = match intro.query(&format!("state.{i}")) {
            Some(IntrospectValue::Text(name)) => ListboxItemState::from_name_or_default(&name),
            _ => ListboxItemState::Idle,
        };
    }
    out
}

/// The knob labels, each naming its live value — the toolbar *is* the readout,
/// so a human sees the same three tokens an agent queries.
fn knob_labels(model: &CompletionState) -> [String; 3] {
    [
        format!("Filter: {}", model.filter().to_wire()),
        format!("Case: {}", model.case().to_wire()),
        format!("Mode: {}", model.mode().to_wire()),
    ]
}

/// Paint one popup row, tagged `comp_options#<source>` so the InputRouter
/// `'#'`-split reaches the composite [`ListBoxExternal`] with the **source**
/// index — stable across filter changes, exactly as R717 keeps absolute indices.
fn option_scene(
    source: usize,
    state: &CompleterViewState,
    current: Option<usize>,
    theme: &Theme,
) -> Scene {
    view_option(
        &OptionRow {
            tag: format!("{OPTIONS_TAG}#{source}"),
            label: CANDIDATES[source],
            state: state.options[source],
            active: current == Some(source),
            selected: false,
        },
        INPUT_W - 2 * PANEL_PAD,
        OPT_H,
        theme,
    )
}

/// The prefix + the three knobs + the completion count, as scene-as-data. Every
/// token here is the one `scene/query` returns, so the pixels and the wire
/// cannot disagree (§2 #7).
fn status_row(model: &CompletionState, count: usize, theme: &Theme) -> Scene {
    Scene::Text(
        TextNode::styled(
            format!(
                "prefix=\"{}\" | {} | {} | {} | {count} completion(s)",
                model.prefix(),
                model.filter().to_wire(),
                model.case().to_wire(),
                model.mode().to_wire(),
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(STATUS_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(INPUT_X, WIN_H - 84)),
    )
}

/// The current completion, what inline mode would append, and the field text.
/// `inline` is deliberately three-valued: an absent completion and a mode that
/// appends nothing are not the same as an empty suffix.
fn current_row(model: &CompletionState, text: &str, theme: &Theme) -> Scene {
    Scene::Text(
        TextNode::styled(
            format!(
                "current=\"{}\" | inline={} | text=\"{text}\"",
                model.current_completion().unwrap_or_default(),
                model
                    .inline_completion()
                    .map_or_else(|| "(none)".to_string(), |s| format!("\"{s}\"")),
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(CURRENT_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(INPUT_X, WIN_H - 58)),
    )
}

/// The popup surface over the **displayed** completions, anchored under the
/// field. One row per entry the model lists, in the model's order.
fn popup_panel(
    completions: &[usize],
    state: &CompleterViewState,
    current_source: Option<usize>,
    theme: &Theme,
) -> Scene {
    let rows: Vec<Scene> = completions
        .iter()
        .map(|&src| option_scene(src, state, current_source, theme))
        .collect();
    let panel_h =
        u32::try_from(rows.len()).expect("completion count fits in u32") * OPT_H + 2 * PANEL_PAD;
    Scene::Container(
        ContainerNode::new(rows)
            .with_tag(PANEL_TAG)
            .with_style(popup_surface(theme))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(PANEL_X, PANEL_Y)
                    .with_size(Size::px(INPUT_W, panel_h))
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_gap(2)
                    .with_padding(Rect::new(PANEL_PAD, PANEL_PAD, PANEL_PAD, PANEL_PAD)),
            ),
    )
}

/// view-fn (§6.3): pure sync `(posture) -> Scene`. Every completion value comes
/// from the shared model, so the pixels and `scene/query` answer from one place.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &CompleterViewState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let model = use_model();
    let text = use_text_edit_state(INPUT_TAG).text();
    let completions = model.completions();
    let current_source = model.current_source();

    let labels = knob_labels(&model);
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let knobs_inner = view_toolbar(
        KNOBS_TAG,
        &label_refs,
        &[false; 3],
        &[false; 3],
        0,
        false,
        &theme,
        &ToolbarStyle::m3_default(),
    );
    let knobs = Scene::Container(
        ContainerNode::new(vec![knobs_inner])
            .with_layout(LayoutStyle::new().with_absolute_position(KNOBS_X, KNOBS_Y)),
    );

    let caption = Scene::Text(
        TextNode::styled(
            "Symbol",
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(INPUT_X, INPUT_Y - 26)),
    );

    let field_style = tf_paint::TextFieldStyle {
        field_w: INPUT_W,
        field_h: INPUT_H,
        ..tf_paint::TextFieldStyle::m3_filled()
    };
    let field = Scene::Container(
        ContainerNode::new(vec![tf_paint::view_field(
            INPUT_TAG,
            state.input,
            state.caret,
            &theme,
            &field_style,
            PLACEHOLDER,
        )])
        .with_layout(LayoutStyle::new().with_absolute_position(INPUT_X, INPUT_Y)),
    );

    let mut children = vec![
        knobs,
        caption,
        field,
        status_row(&model, completions.len(), &theme),
        current_row(&model, &text, &theme),
    ];

    if popup_showing(&model) {
        // R715 §5.16 — the transparent click-outside barrier goes *under* the
        // absolutely-positioned panel, which is pushed last so it hit-tests above.
        children.push(dismiss_barrier(BARRIER_TAG, (0, 0), (WIN_W, WIN_H)));
        children.push(popup_panel(&completions, state, current_source, &theme));
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

/// Delegate `key` to the text field's edit dispatch. Returns whether the field
/// recognized it (W3C `defaultPrevented` semantic).
fn delegate_to_field(scene: &mut Scene, key: &str) -> bool {
    pinion_core::forward_key_to_field(scene, INPUT_TAG, key, pinion_core::Modifiers::empty())
}

struct CompleterView;

impl WidgetCore for CompleterView {
    type State = CompleterViewState;
    type Event = ();

    /// Primary = the text input the completer attaches to.
    fn create_external() -> Box<dyn External> {
        let text_state = use_text_edit_state(INPUT_TAG);
        let blink = use_caret_blink(INPUT_TAG);
        Box::new(
            TextFieldExternal::new()
                .attach_state(text_state)
                .attach_blink(blink),
        )
    }

    /// The completion model's wire surface, the popup listbox, the dismiss
    /// barrier, and the knob strip. The model external carries no paint — it is
    /// the §2 #2 path to the same `Rc` the view reads.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(MODEL_TAG, Box::new(CompleterExternal::new(use_model()))),
            ExtraExternal::new(OPTIONS_TAG, Box::new(ListBoxExternal::new(N))),
            ExtraExternal::new(BARRIER_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(
                KNOBS_TAG,
                Box::new(ToolbarExternal::new(vec![ToolItem::Command; 3])),
            ),
        ]
    }

    fn tag() -> &'static str {
        INPUT_TAG
    }

    fn read_state(scene: &Scene) -> CompleterViewState {
        read_completer_state(scene)
    }

    fn view(state: CompleterViewState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-completer (R1449 §5.38 QCompleter parity)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// The completer keyboard model. A shown popup reserves ArrowDown / ArrowUp
    /// / Enter / Escape; inline mode reserves the arrows (they walk the
    /// completions with no popup to walk, exactly as `QCompleter` does) and
    /// Enter (accept). Everything else reaches the text field.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(INPUT_TAG) {
            return false;
        }
        let model = use_model();
        let popup = popup_showing(&model);
        let inline = model.mode() == CompletionMode::Inline;
        match key {
            "Escape" if popup => {
                use_open().set(false);
                true
            }
            "ArrowDown" if popup => model.next().is_some(),
            "ArrowUp" if popup => model.prev().is_some(),
            "ArrowDown" if inline => {
                let moved = model.next().is_some();
                apply_inline();
                moved
            }
            "ArrowUp" if inline => {
                let moved = model.prev().is_some();
                apply_inline();
                moved
            }
            // A popup mode with the popup closed: ArrowDown opens it (the
            // combobox affordance).
            "ArrowDown" => {
                use_open().set(true);
                true
            }
            "Enter" if popup => match model.current_source() {
                Some(src) => {
                    commit(src);
                    true
                }
                None => false,
            },
            // Inline mode has nothing to open: Enter *accepts* the completion by
            // collapsing the selection to the end and adopting it as the prefix.
            "Enter" if inline => {
                let ts = use_text_edit_state(INPUT_TAG);
                ts.set_caret(ts.text().len());
                sync_prefix();
                true
            }
            _ => {
                let handled = delegate_to_field(scene, key);
                if handled && is_text_input_key(key) {
                    sync_prefix();
                    if inline {
                        apply_inline();
                    } else {
                        use_open().set(true);
                    }
                }
                handled
            }
        }
    }

    /// Bridge the row / barrier / knob intents. A committed row copies its text
    /// into the field; an outside click closes; a knob cycles its value.
    fn update(_state: CompleterViewState, intent: &Intent) -> Vec<Command> {
        match intent.tag_str() {
            OPTION_INTENT => {
                if let IntrospectValue::Int(i) = &intent.payload {
                    if let Ok(src) = usize::try_from(*i) {
                        commit(src);
                    }
                } else {
                    use_open().set(false);
                }
            }
            BARRIER_INTENT => use_open().set(false),
            KNOB_INTENT => {
                if let IntrospectValue::Text(idx) = &intent.payload {
                    if let Ok(i) = idx.parse::<usize>() {
                        cycle_knob(i);
                    }
                }
            }
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &CompleterViewState) -> String {
        format!("input={:?} caret={}", state.input, state.caret)
    }
}

impl WidgetA11y for CompleterView {
    /// The WAI-ARIA tree: the input is a `combobox` whose `aria-autocomplete`
    /// **follows the completion mode** — `inline` when the field completes in
    /// place, `list` when a popup presents the candidates. R717 could only ever
    /// say `list`; this is the first binding where the value is a live fact
    /// about the model rather than a constant. The knob strip is a second root.
    fn access_node(state: &CompleterViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let model = use_model();
        let input_focused = focused == Some(INPUT_TAG);
        let popup = popup_showing(&model);
        let auto = if model.mode() == CompletionMode::Inline {
            AutoComplete::Inline
        } else {
            AutoComplete::List
        };
        let combo = AccessNode::new(INPUT_TAG, AriaRole::EditableComboBox)
            .with_name("Symbol")
            .with_value(AccessValue::Text(use_text_edit_state(INPUT_TAG).text()))
            .with_expanded(popup)
            .with_controls(OPTIONS_TAG)
            .with_auto_complete(auto)
            .with_state(AccessState {
                focused: input_focused,
                ..AccessState::default()
            });
        let mut nodes = vec![combo];

        if popup {
            let completions = model.completions();
            let current_source = model.current_source();
            let tags: Vec<String> = completions
                .iter()
                .map(|&src| format!("{OPTIONS_TAG}#{src}"))
                .collect();
            let options: Vec<ListOption<'_>> = completions
                .iter()
                .enumerate()
                .map(|(pos, &src)| ListOption {
                    tag: &tags[pos],
                    label: Some(CANDIDATES[src]),
                    state: state.options[src],
                    selected: false,
                    focused: input_focused && current_source == Some(src),
                })
                .collect();
            nodes.extend(listbox_option_nodes(
                OPTIONS_TAG,
                "Symbol completions",
                false,
                &options,
            ));
        }

        let labels = knob_labels(&model);
        let ctl_tags: Vec<String> = (0..3).map(|i| composite_item_tag(KNOBS_TAG, i)).collect();
        let controls: Vec<ToolbarControl<'_>> = (0..3)
            .map(|i| ToolbarControl {
                tag: &ctl_tags[i],
                name: Some(&labels[i]),
                checked: None,
                disabled: false,
            })
            .collect();
        nodes.extend(toolbar_button_nodes(
            KNOBS_TAG,
            "Completion settings",
            &controls,
            None,
        ));
        nodes
    }

    /// Composite focus: while the input owns shell focus and a popup is showing,
    /// focus stays on the input and the model's cursor is the
    /// `aria-activedescendant`.
    fn access_focus_target(
        _state: &CompleterViewState,
        focused: Option<&str>,
    ) -> Option<AccessFocus> {
        let model = use_model();
        if focused == Some(INPUT_TAG) && popup_showing(&model) {
            if let Some(src) = model.current_source() {
                return Some(AccessFocus::composite(
                    INPUT_TAG,
                    format!("{OPTIONS_TAG}#{src}"),
                ));
            }
        }
        focused.map(AccessFocus::atomic)
    }
}

impl WidgetView for CompleterView {
    type Renderer = HelloCompleterRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<CompleterView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;
    use pinion_core::scene::ExternalNode;

    /// Run `f` inside an Owner scope, the way the shell wraps `view` /
    /// `create_external`, so the `Owner::cache` hooks resolve.
    fn with_owner<T>(f: impl FnOnce() -> T) -> T {
        let owner = Owner::new();
        owner.run(f)
    }

    /// Build the multi-external state scene the way the shell does, so
    /// `apply_key` has real input / listbox / barrier / knob externals.
    fn boot_scene() -> Scene {
        let mut children = vec![Scene::External(
            ExternalNode::new(CompleterView::create_external()).with_tag(INPUT_TAG),
        )];
        for extra in CompleterView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }

    fn type_text(scene: &mut Scene, body: &str) {
        for ch in body.chars() {
            assert!(
                CompleterView::apply_key(
                    scene,
                    Some(INPUT_TAG),
                    &ch.to_string(),
                    pinion_core::Modifiers::empty(),
                ),
                "the field should have taken '{ch}'"
            );
        }
    }

    /// Press a named key, asserting the binding claimed it.
    fn press(scene: &mut Scene, key: &str) {
        assert!(
            CompleterView::apply_key(scene, Some(INPUT_TAG), key, pinion_core::Modifiers::empty()),
            "the binding should have taken {key}"
        );
    }

    #[test]
    fn r1449_typing_opens_the_popup_and_lands_on_the_first_completion() {
        with_owner(|| {
            let mut scene = boot_scene();
            type_text(&mut scene, "render");
            let model = use_model();
            assert_eq!(model.prefix(), "render");
            assert_eq!(
                model.completion_count(),
                3,
                "renderScene + renderTarget + RenderPass (case-insensitive)"
            );
            assert!(use_open().get(), "typing opens the popup");
            assert_eq!(model.current_completion().as_deref(), Some("renderScene"));
        });
    }

    #[test]
    fn r1449_the_case_knob_changes_the_answer_over_the_same_list() {
        with_owner(|| {
            let mut scene = boot_scene();
            type_text(&mut scene, "render");
            assert_eq!(use_model().completion_count(), 3);
            cycle_knob(1);
            assert_eq!(
                use_model().completion_count(),
                2,
                "case-sensitive drops RenderPass"
            );
            assert_eq!(use_model().case(), CompletionCase::Sensitive);
        });
    }

    #[test]
    fn r1449_inline_mode_completes_the_field_and_selects_the_appended_part() {
        with_owner(|| {
            let mut scene = boot_scene();
            // popup -> unfiltered_popup -> inline
            cycle_knob(2);
            cycle_knob(2);
            assert_eq!(use_model().mode(), CompletionMode::Inline);
            type_text(&mut scene, "r");
            let ts = use_text_edit_state(INPUT_TAG);
            assert_eq!(ts.text(), "renderScene", "the field carries the completion");
            assert_eq!(
                ts.selection_range(),
                Some((1, "renderScene".len())),
                "and the appended part is selected"
            );
            assert_eq!(use_model().prefix(), "r", "the prefix stays what was typed");
            // The next keystroke types over the selection, exactly as Qt's does.
            type_text(&mut scene, "e");
            assert_eq!(use_model().prefix(), "re");
            assert_eq!(use_text_edit_state(INPUT_TAG).text(), "renderScene");
        });
    }

    #[test]
    fn r1449_a_knob_change_never_rewrites_the_field() {
        with_owner(|| {
            let mut scene = boot_scene();
            type_text(&mut scene, "ren");
            let before = use_text_edit_state(INPUT_TAG).text();
            cycle_knob(2);
            cycle_knob(2);
            assert_eq!(use_model().mode(), CompletionMode::Inline);
            assert_eq!(
                use_text_edit_state(INPUT_TAG).text(),
                before,
                "switching to inline mode does not type for the user"
            );
            assert!(
                use_model().inline_completion().is_some(),
                "but the readout answers immediately"
            );
        });
    }

    /// Set the mode the way `scene/intervene` does — through the model
    /// External living in the state scene, not by calling the state directly.
    fn intervene_mode(scene: &mut Scene, wire: &str) {
        let intro = scene
            .find_external_with_tag_mut(MODEL_TAG)
            .and_then(|n| n.handle.introspect_mut())
            .expect("the model external is in the state scene");
        intro
            .intervene("mode", IntrospectValue::Text(wire.to_string()))
            .expect("mode is writable");
    }

    /// The §2 #2 convergence: a knob click and a `scene/intervene` write are the
    /// same state change, so the popup must be showing-or-not identically after
    /// each. An earlier draft closed the popup only on the knob path, and the
    /// two paths disagreed after a mode round-trip.
    #[test]
    fn r1449_the_human_path_and_the_rpc_path_agree_about_the_popup() {
        let knob_trace = with_owner(|| {
            let mut scene = boot_scene();
            type_text(&mut scene, "render");
            let mut trace = vec![popup_showing(&use_model())];
            for _ in 0..3 {
                cycle_knob(2);
                trace.push(popup_showing(&use_model()));
            }
            trace
        });
        let rpc_trace = with_owner(|| {
            let mut scene = boot_scene();
            type_text(&mut scene, "render");
            let mut trace = vec![popup_showing(&use_model())];
            for wire in ["unfiltered_popup", "inline", "popup"] {
                intervene_mode(&mut scene, wire);
                trace.push(popup_showing(&use_model()));
            }
            trace
        });
        assert_eq!(
            knob_trace, rpc_trace,
            "the two paths must reach the same popup state at every step"
        );
        assert_eq!(
            knob_trace,
            vec![true, true, false, true],
            "popup -> unfiltered (still a popup) -> inline (none) -> popup again"
        );
    }

    #[test]
    fn r1449_enter_commits_the_current_completion_into_the_field() {
        with_owner(|| {
            let mut scene = boot_scene();
            type_text(&mut scene, "render");
            press(&mut scene, "ArrowDown");
            assert_eq!(
                use_model().current_completion().as_deref(),
                Some("renderTarget")
            );
            press(&mut scene, "Enter");
            assert_eq!(use_text_edit_state(INPUT_TAG).text(), "renderTarget");
            assert!(!use_open().get(), "committing closes the popup");
        });
    }

    #[test]
    fn r1449_the_unfiltered_popup_lists_everything_and_marks_the_match() {
        with_owner(|| {
            let mut scene = boot_scene();
            cycle_knob(2);
            type_text(&mut scene, "scene");
            let model = use_model();
            assert_eq!(model.mode(), CompletionMode::UnfilteredPopup);
            assert_eq!(model.completion_count(), N, "every candidate is listed");
            assert_eq!(
                model.current_completion().as_deref(),
                Some("sceneGraph"),
                "and the best match is marked"
            );
        });
    }

    #[test]
    fn r1449_the_a11y_autocomplete_follows_the_mode() {
        with_owner(|| {
            let mut scene = boot_scene();
            type_text(&mut scene, "render");
            let nodes = CompleterView::access_node(&read_completer_state(&scene), Some(INPUT_TAG));
            assert_eq!(nodes[0].auto_complete, Some(AutoComplete::List));
            assert!(nodes[0].expanded == Some(true), "the popup is expanded");
            cycle_knob(2);
            cycle_knob(2);
            let nodes = CompleterView::access_node(&read_completer_state(&scene), Some(INPUT_TAG));
            assert_eq!(
                nodes[0].auto_complete,
                Some(AutoComplete::Inline),
                "inline mode reports aria-autocomplete=inline"
            );
            assert_eq!(nodes[0].expanded, Some(false));
        });
    }
}
