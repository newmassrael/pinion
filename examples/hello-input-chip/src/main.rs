//! `hello-input-chip` — R756 §5.38 §5.40 Material 3 **input chips**: a row of
//! removable preset tokens (recipients), each a content-hugging M3 chip with a
//! trailing `×` delete affordance that carries real hover / pressed
//! state-layer feedback.
//!
//! ## 2nd consumer of the chip paint lift
//!
//! R753 `hello-filter-chip` built the M3 chip body inline (the 1st chip-paint
//! consumer). An input chip is the same M3 *container* skin — an 8 px-radius,
//! 36 px-tall rounded rectangle with a `6` px inner gap, optionally outlined,
//! tinted by the shared [`pinion_widget_paint::state_layer`] overlay —
//! differing only in width strategy (content-hug vs the filter chip's fixed
//! width), children (trailing `×` vs leading check), and fill choice. Two
//! consumers of the same container skin is the
//! [[abstraction-needs-second-consumer]] gate firing, so R756 lifted the
//! shared core into [`pinion_widget_paint::chip`]
//! ([`chip::chip_style`] /
//! [`chip::chip_layout`] + the
//! [`CHIP_HEIGHT`] /
//! [`CHIP_RADIUS`](pinion_widget_paint::chip::CHIP_RADIUS) /
//! [`OUTLINE_W`] tokens); both this
//! binding and the retrofitted filter chip now consume it.
//!
//! ## Delete machinery (mirrors `todomvc`)
//!
//! The chip collection is a `Signal<Vec<InputChip>>` ([`use_chips`], seeded
//! with five stable-id tokens). A single [`ChipDeleteExternal`] (registered
//! under the primary [`DELETE_TAG`]) owns both the delete and the per-chip `×`
//! button posture: the framework's R51.42 composite-tag wire forwards each
//! `chip_delete#<id>` pointer event as a `"<id>:<EventName>"` send payload, the
//! external drives that chip's [`Button`] SCXML (so the `×` shows the shared M3
//! hover / pressed state layer through `chip_style`), and the canonical
//! button-like `Pressed → Hover` click edge commits the delete. A removed chip
//! simply drops out of the `Vec` and is no longer painted — the row shrinks.
//!
//! ## No add-via-textfield flow
//!
//! This binding ships the removable preset tokens only; the add-via-textfield
//! composition (a `TextField` whose Enter pushes a new chip) is a separate
//! composed app, deferred (carry).
//!
//! Keyboard (WAI-ARIA): each chip's `×` is its own Tab stop, so `Tab` /
//! `Shift+Tab` move between the delete affordances; `Enter` / `Space` /
//! `Delete` / `Backspace` on a focused `×` removes that chip.
//!
//! `cargo run --release -p hello-input-chip`. Hover a `×` to see the state
//! layer; click it (or focus + Enter) to remove the chip.

use std::cell::RefCell;
use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::composite_tag::parse_send_payload;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg, SchemaField,
    ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue,
    TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::button::{Button, ButtonEvent, ButtonState};
use pinion_core::{Color, Frame, Scene, WidgetCore, WidgetEventName, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::chip::{self, CHIP_HEIGHT, OUTLINE_W};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloInputChipRenderer, HelloInputChipRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 120;
/// [`use_theme`] cache key — the `"app"` convention shared across the example
/// gallery.
const THEME_TAG: &str = "app";

/// Number of seeded chips. Stable ids are `1..=N`, allocated once; deletes
/// only remove (no add flow), so the id `i` always maps to button-state slot
/// `i - 1`.
const N: usize = 5;

/// Seed tokens — recipient names. Single source of truth for the painted
/// label and the per-chip `×` AccessKit `Remove {label}` name, so the
/// screen-reader text and painted text never diverge.
const LABELS: [&str; N] = ["Alice", "Bob", "Carol", "Dave", "Erin"];

/// Primary composite tag for [`ChipDeleteExternal`] — and the WAI-ARIA
/// `group` container tag. Carried by both the (invisible) flex row holding the
/// chips (so the group's AT bounds + the R55.G.17 composite paint-root anchor
/// attach to the chip strip) and the shell-wrapped primary `External`. The
/// per-chip `×` paints with tag `chip_delete#<id>`; the R51.42 §5.35 wire
/// splits at the `#` so the router routes every pointer event into the single
/// external under this primary tag — the canonical composite-widget shape
/// (primary tag on the group container, `#id` on the children).
const DELETE_TAG: &str = "chip_delete";

/// `Owner::cache` key for the shared chip collection.
const CHIPS_KEY: &str = "input_chips";

/// Font size for the chip label.
const LABEL_FONT_PX: u32 = 15;
/// Font size for the trailing `×` glyph.
const CLOSE_FONT_PX: u32 = 14;
/// Horizontal padding inside a content-hugging input chip (left + right
/// insets); the chip width then tracks `label + gap + ×`.
const CHIP_PAD_X: u32 = 12;
/// `×` hit-target square (centred inside the chip's trailing slot).
const CLOSE_HIT: u32 = 24;

/// U+2715 MULTIPLICATION X — the M3 input-chip trailing "remove" affordance.
/// Named const + escape per the non-ASCII-source rule (raw glyph only in doc
/// strings).
const CLOSE_GLYPH: &str = "\u{2715}";

/// One removable input chip: a stable id + a preset label.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InputChip {
    /// Stable id `1..=N`, allocated once; never reused. Survives sibling
    /// deletes (the surviving chips keep their tags, e.g. deleting `#2` from
    /// `[#1, #2, #3]` leaves `chip_delete#1` + `chip_delete#3`).
    pub id: u64,
    /// The recipient token text. Owned `String` (not `&'static str`) because
    /// the [`Signal`] reactive substrate requires the element to be
    /// `DeserializeOwned` — a borrowed lifetime is not general enough. Seeded
    /// from the static [`LABELS`] presets.
    pub label: String,
}

/// `Owner::cache`-keyed hook returning the shared `Rc<Signal<Vec<InputChip>>>`
/// of present chips, seeded with the [`N`] preset tokens. Mirrors `todomvc`'s
/// `use_todos`: the [`Owner::cache`] dedup guarantees one `Rc` across the view
/// fn (which subscribes via `.get()` to re-paint on delete) and
/// [`WidgetCore::create_extra_externals`] (which hands a clone to
/// [`ChipDeleteExternal`]).
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope; every runtime call
/// site is wrapped by the substrate's `root_owner.run`.
#[must_use]
pub fn use_chips() -> Rc<Signal<Vec<InputChip>>> {
    Owner::current()
        .expect("use_chips requires an active Owner scope")
        .cache(CHIPS_KEY, || {
            Signal::new(
                LABELS
                    .iter()
                    .enumerate()
                    .map(|(i, label)| InputChip {
                        // ids are 1..=N; `i` is a 0..N array index so the
                        // `+ 1` is in-range for `u64` without a fallible cast.
                        id: u64::try_from(i).expect("chip index fits in u64") + 1,
                        label: (*label).to_string(),
                    })
                    .collect::<Vec<_>>(),
            )
        })
}

/// Per-chip `×` interaction posture, indexed by `id - 1`. `Copy` so it
/// satisfies the [`WidgetCore::State`] bound (the chip *membership* lives in
/// the [`use_chips`] Signal, read in the view fn — only the postures need to
/// flow through the Copy state snapshot).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct ChipRowState {
    /// `close_states[id - 1]` is chip `id`'s `×` button posture.
    close_states: [ButtonState; N],
    /// Which chip's `×` currently owns shell focus, if any (drives the AT
    /// `focused` flag — each `×` is its own Tab stop).
    focused: Option<u64>,
}

impl ChipRowState {
    fn idle() -> Self {
        Self {
            close_states: [ButtonState::Idle; N],
            focused: None,
        }
    }

    /// Posture for chip `id` (`Idle` when out of range — defensive).
    fn close_state(&self, id: u64) -> ButtonState {
        usize::try_from(id)
            .ok()
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| self.close_states.get(i).copied())
            .unwrap_or(ButtonState::Idle)
    }
}

/// Per-chip `×` paint tag, e.g. `chip_delete#3`.
fn close_tag(id: u64) -> String {
    format!("{DELETE_TAG}#{id}")
}

/// view-fn (§6.3): pure sync `ChipRowState -> Scene`. Paints one chip per
/// *present* member of the [`use_chips`] Signal; removed chips are simply not
/// painted (the collection shrank).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ChipRowState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let chips_snapshot = use_chips().get();
    let chips: Vec<Scene> = chips_snapshot
        .iter()
        .map(|c| chip(c, state.close_state(c.id), &theme))
        .collect();
    // The row carries `DELETE_TAG` (primary composite anchor + WAI-ARIA
    // group) and no fill — a passive container holding the free-standing
    // chips; per-chip clicks route to it via the `chip_delete#<id>` children.
    let row = Scene::Container(
        ContainerNode::new(chips).with_tag(DELETE_TAG).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(chip::INNER_GAP),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![row])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// One input chip — a content-hugging outlined container holding the leading
/// label and a trailing `×` whose own sub-container carries the
/// `chip_delete#<id>` composite tag (so a click routes into
/// [`ChipDeleteExternal`]). The chip container itself uses the shared
/// [`chip::chip_style`] / [`chip::chip_layout`] substrate.
fn chip(item: &InputChip, close_state: ButtonState, theme: &Theme) -> Scene {
    let label = Scene::Text(TextNode::styled(
        item.label.clone(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(LABEL_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    // Trailing `×` — its own hit-target sub-container carries the
    // composite tag and the per-chip M3 state-layer overlay (hover / pressed
    // through `chip_style`, applied to a transparent base so only the overlay
    // shows). The `×` is the deletable affordance; the chip body is not.
    let close_glyph = Scene::Text(TextNode::styled(
        CLOSE_GLYPH,
        Rect::default(),
        TextStyle::new()
            .with_size_px(CLOSE_FONT_PX)
            .with_fg(close_ink(theme, close_state)),
    ));
    let close = Scene::Container(
        ContainerNode::new(vec![close_glyph])
            .with_tag(close_tag(item.id))
            .with_style(chip::chip_style(
                Color::rgba(0, 0, 0, 0),
                None,
                close_state,
                theme,
            ))
            // R1020 §5.39 — each present chip's `×` close affordance is a Tab
            // stop; opt its tag-carrying Container into the scene-derived
            // enumeration. Painted in chip-vector (id) order, so the derived
            // order matches the seeded CLOSE_TAGS; a deleted chip simply drops
            // out of both the paint walk and the live enumeration.
            .with_layout(
                chip::chip_layout(Size::px(CLOSE_HIT, CLOSE_HIT), None).with_focusable(true),
            ),
    );

    // Outlined chip container — `OnSurface`-tinted `Outline` border, no fill
    // (window surface shows through), content-hugging width (Auto) with
    // horizontal padding so the chip tracks `label + gap + ×`.
    let border = Border::new(theme.resolve(ColorRole::Outline), OUTLINE_W);
    Scene::Container(
        ContainerNode::new(vec![label, close])
            .with_style(chip::chip_style(
                Color::rgba(0, 0, 0, 0),
                Some(border),
                ButtonState::Idle,
                theme,
            ))
            .with_layout(chip::chip_layout(
                Size::auto().with_height(SizeValue::Px(CHIP_HEIGHT)),
                Some(Rect::new(0, CHIP_PAD_X, 0, CHIP_PAD_X)),
            )),
    )
}

/// `×` glyph ink — muted on-surface at rest, full on-surface on hover/press
/// (disabled never occurs for these affordances).
fn close_ink(theme: &Theme, state: ButtonState) -> Color {
    if matches!(state, ButtonState::Hover | ButtonState::Pressed) {
        theme.resolve(ColorRole::OnSurface)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    }
}

/// Per-chip `×` delete handler + button-posture owner. Single external
/// registered under [`DELETE_TAG`]; the R51.42 §5.35 composite-tag wire
/// forwards every `chip_delete#<id>` pointer event as `"<id>:<EventName>"`.
///
/// Owns one [`Button`] SCXML per chip id (in a [`RefCell`], indexed by
/// `id - 1`) so the `×` shows the shared M3 hover / pressed state layer; the
/// canonical button-like `Pressed → Hover` click edge (`PointerUp` on target
/// after a press) commits the delete. `delete_by_id` retains the
/// `Signal<Vec<InputChip>>` minus the matched id; the framework's reactive
/// equality-skip suppresses the no-op case.
///
/// Synchronous-pure delete — bypasses the §5.23 R27 `update` reducer so the
/// next paint observes the mutation directly (standard pinion convention for
/// paint-side hit events, mirroring `todomvc`'s `TodoDeleteExternal`).
///
/// ## SSOT note — 2nd composite-delete-over-collection consumer (declared defer, R756.1)
///
/// This external structurally mirrors `todomvc`'s `TodoDeleteExternal`: the
/// `Signal<Vec<T>>` + `delete_by_id` (`retain`), the `count` / `ids` query,
/// the typed `delete` invoke, and the `parse_send_payload` send-id extraction
/// are the same shape (~80 % of the body). That shared core is a lift
/// candidate — a `CompositeCollection<T: HasId>` substrate composed by both
/// bindings, with the **commit policy** as the consumer-supplied seam (the one
/// genuine divergence: todo commits on `PointerDown`; this chip commits on the
/// `Button` `Pressed → Hover` edge so the `×` carries hover/press feedback).
///
/// Deliberately **deferred to the 3rd consumer** ([[abstraction-needs-second-consumer]]):
/// parameterising over two impls whose commit semantics diverge is premature —
/// the divergence is precisely the part an abstraction would have to
/// special-case, so lifting now would trade one duplication for a leaky seam.
/// When a 3rd id-keyed removable collection lands (a tag-input field, editable
/// table rows), lift the shared core then and retrofit todo + chip + the new
/// consumer together.
pub struct ChipDeleteExternal {
    chips: Rc<Signal<Vec<InputChip>>>,
    /// One `Button` SCXML per seeded chip id, indexed by `id - 1`.
    buttons: RefCell<[Button; N]>,
}

impl core::fmt::Debug for ChipDeleteExternal {
    // `Button` (the SCXML widget) does not implement `Debug`; surface the
    // observable per-chip posture instead so logs stay useful.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let postures: Vec<&'static str> = self
            .buttons
            .borrow()
            .iter()
            .map(|b| b.state().as_name())
            .collect();
        f.debug_struct("ChipDeleteExternal")
            .field("count", &self.chips.get().len())
            .field("close_postures", &postures)
            .finish()
    }
}

impl ChipDeleteExternal {
    #[must_use]
    pub fn new(chips: Rc<Signal<Vec<InputChip>>>) -> Self {
        Self {
            chips,
            buttons: RefCell::new(core::array::from_fn(|_| Button::new())),
        }
    }

    fn delete_by_id(&self, target_id: u64) {
        self.chips.set_with(|prev| {
            let mut next = prev.clone();
            next.retain(|item| item.id != target_id);
            next
        });
    }

    fn is_present(&self, id: u64) -> bool {
        self.chips.get().iter().any(|item| item.id == id)
    }

    /// Drive chip `id`'s `×` button SCXML by event name; returns the
    /// click-committed flag (`true` when this event was the `Pressed → Hover`
    /// activate edge). Out-of-range ids are a silent no-op.
    fn drive(&self, id: u64, event_name: &str) -> bool {
        let Some(ev) = ButtonEvent::from_name(event_name) else {
            return false;
        };
        let Some(slot) = usize::try_from(id).ok().and_then(|i| i.checked_sub(1)) else {
            return false;
        };
        let mut buttons = self.buttons.borrow_mut();
        let Some(button) = buttons.get_mut(slot) else {
            return false;
        };
        let before = button.state();
        button.send(ev);
        let after = button.state();
        matches!(before, ButtonState::Pressed) && matches!(after, ButtonState::Hover)
    }

    /// Current `×` posture for chip `id` (`Idle` for out-of-range).
    fn state_of(&self, id: u64) -> ButtonState {
        usize::try_from(id)
            .ok()
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| self.buttons.borrow().get(i).map(Button::state))
            .unwrap_or(ButtonState::Idle)
    }
}

impl External for ChipDeleteExternal {
    /// R741 §5.35 — capture the pointer on press so a real-mouse click on the
    /// small `×` target is robust to sub-pixel jitter between press and
    /// release.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R741 §5.35 — a deliberate release off the `×` cancels the press.
    fn cancel_on_release_off_target(&self) -> bool {
        true
    }

    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for ChipDeleteExternal {
    /// `count` / `ids` = read-only derived state. `state:<id>` = the per-chip
    /// `×` button posture (read by [`WidgetCore::read_state`] for the paint).
    /// `send` = R51.42 composite-tag wire (`<id>:<EventName>` drives the `×`
    /// button; the click edge deletes). `delete` = direct typed-id shortcut
    /// (RPC AI-driving path), returns `Bool(was_present)`.
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("count", "int"),
                    SchemaField::new("ids", "json"),
                    // Parameterized read key — queried as `state:<id>` (e.g.
                    // `state:2`) for chip `<id>`'s `×` button-posture variant name;
                    // the literal key advertises the `:<id>` suffix so the introspect
                    // contract (§5.21) matches what `query` actually accepts.
                    SchemaField::parametric(
                        "state:<id>",
                        "string",
                        const { &[SchemaArg::open("id", "int")] },
                    ),
                    SchemaField::new("send", "string"),
                    SchemaField::new("delete", "int"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "count" => {
                let n = self.chips.get().len();
                Some(IntrospectValue::Int(
                    i64::try_from(n).expect("chip count must fit in i64"),
                ))
            }
            "ids" => {
                let arr: Vec<serde_json::Value> = self
                    .chips
                    .get()
                    .iter()
                    .map(|item| serde_json::Value::from(item.id))
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            // `state:<id>` — the per-chip `×` button posture variant name.
            _ => path.strip_prefix("state:").and_then(|id_str| {
                let id: u64 = id_str.parse().ok()?;
                Some(IntrospectValue::Text(
                    self.state_of(id).as_name().to_string(),
                ))
            }),
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "count" | "ids" => Err(InterveneError::ReadOnly),
            p if p.starts_with("state:") => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // R51.42 wire: drive the per-chip `×` button SCXML; the
            // `Pressed → Hover` click edge commits the delete. Returns
            // `Bool(deleted)` so the dispatch loop / AI client sees whether
            // this event removed a chip.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    let (id, event_name, _): (u64, &str, _) =
                        parse_send_payload(payload).ok_or(InvokeError::Rejected)?;
                    let committed = self.drive(id, event_name);
                    if committed && self.is_present(id) {
                        self.delete_by_id(id);
                        Ok(IntrospectValue::Bool(true))
                    } else {
                        Ok(IntrospectValue::Bool(false))
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Direct typed-id delete (RPC AI-driving path). Returns
            // `Bool(was_present)` so callers distinguish "deleted existing"
            // from "no-op stale id".
            "delete" => match args {
                IntrospectValue::Int(i) => {
                    let id = u64::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    let was_present = self.is_present(id);
                    self.delete_by_id(id);
                    Ok(IntrospectValue::Bool(was_present))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

struct InputChipView;

impl WidgetCore for InputChipView {
    type State = ChipRowState;
    // Every state change flows through `apply_key` (keyboard) or the
    // InputRouter's per-chip pointer dispatch (pointer), never the shell's
    // enum-typed `keybinding` channel — so `()` satisfies the `Copy` bound.
    type Event = ();

    /// The single [`ChipDeleteExternal`] is the primary; it owns the delete +
    /// every per-chip `×` button posture, so there are no sibling externals.
    fn create_external() -> Box<dyn External> {
        Box::new(ChipDeleteExternal::new(use_chips()))
    }

    fn tag() -> &'static str {
        DELETE_TAG
    }

    fn read_state(scene: &Scene) -> ChipRowState {
        let mut out = ChipRowState::idle();
        if let Some(node) = scene.find_external_with_tag(DELETE_TAG) {
            if let Some(intro) = node.handle.introspect() {
                for (slot, posture) in out.close_states.iter_mut().enumerate() {
                    let id = u64::try_from(slot).expect("slot fits in u64") + 1;
                    if let Some(IntrospectValue::Text(name)) = intro.query(&format!("state:{id}")) {
                        *posture = ButtonState::from_name_or_default(&name);
                    }
                }
            }
        }
        out
    }

    fn view(state: ChipRowState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        // Keys flow through `apply_key` directly; the enum-typed channel
        // stays unused (mirrors hello-filter-chip).
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-input-chip (R756 §5.38 Material 3 input chips)"
    }

    /// WAI-ARIA: `Enter` / `Space` / `Delete` / `Backspace` on a focused `×`
    /// removes that chip. The focused tag is `chip_delete#<id>`; parse the id
    /// and route through the same delete path the pointer click uses.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let Some(tag) = focused else {
            return false;
        };
        if !matches!(key, "Enter" | "Space" | " " | "Delete" | "Backspace") {
            return false;
        }
        let (_, sub) = pinion_core::composite_tag::split_subindex(tag);
        let Some(id) = sub.and_then(|s| s.parse::<u64>().ok()) else {
            return false;
        };
        let Ok(id_i64) = i64::try_from(id) else {
            return false;
        };
        if let Some(node) = scene.find_external_with_tag_mut(DELETE_TAG) {
            if let Some(intro) = node.handle.introspect_mut() {
                let r = intro.invoke("delete", IntrospectValue::Int(id_i64));
                return matches!(r, Ok(IntrospectValue::Bool(true)));
            }
        }
        false
    }

    fn fmt_state_log(state: &ChipRowState) -> String {
        let postures = state
            .close_states
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{}={}", i + 1, s.as_name()))
            .collect::<Vec<_>>()
            .join(" ");
        match state.focused {
            Some(id) => format!("{postures} focused={id}"),
            None => postures,
        }
    }
}

impl WidgetA11y for InputChipView {
    /// One [`AriaRole::Group`] parent (`"Recipients"`) + one node per *present*
    /// chip, each with a nested [`AriaRole::Button`] `×` named
    /// `"Remove {label}"`. The `×` button carries the R755-SSOT
    /// [`AccessState::from_interaction`] posture (hover / pressed) with
    /// `focused` spread in.
    fn access_node(state: &ChipRowState, focused: Option<&str>) -> Vec<AccessNode> {
        let chips = Owner::current().map_or_else(Vec::new, |o| o.run(|| use_chips().get()));
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(chips.len() * 2 + 1);
        let mut group = AccessNode::new(DELETE_TAG, AriaRole::Group).with_name("Recipients");
        for item in &chips {
            group = group.with_child(close_tag(item.id));
        }
        nodes.push(group);
        for item in &chips {
            let tag = close_tag(item.id);
            let close_state = state.close_state(item.id);
            let access = AccessState {
                focused: focused == Some(tag.as_str()),
                ..AccessState::from_interaction(close_state, None)
            };
            nodes.push(
                AccessNode::new(tag, AriaRole::Button)
                    .with_name(format!("Remove {}", item.label))
                    .with_state(access),
            );
        }
        nodes
    }
}

impl WidgetView for InputChipView {
    type Renderer = HelloInputChipRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<InputChipView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-chip `×` Tab-stop tags for the seeded ids `1..=N`; the paint
    /// scene's focus stops should collect these in tree order (deleted
    /// chips simply do not resolve in the live scene).
    const CLOSE_TAGS: [&str; N] = [
        "chip_delete#1",
        "chip_delete#2",
        "chip_delete#3",
        "chip_delete#4",
        "chip_delete#5",
    ];

    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    // ── view / composition (binding-specific chip paint) ──────────────────

    #[test]
    fn view_carries_primary_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<InputChipView>(
            ChipRowState::idle(),
            &Frame::new(),
        );
    }

    /// Recursively find the container tagged `tag`.
    fn find_tagged<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(c);
                }
                c.children.iter().find_map(|ch| find_tagged(ch, tag))
            }
            _ => None,
        }
    }

    fn collect_text(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => {
                for ch in &c.children {
                    collect_text(ch, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn present_chip_carries_its_close_tag_and_label() {
        let scene = with_owner(|| view(ChipRowState::idle(), &Frame::new()));
        // Every seeded chip's `×` sub-container is present.
        let n = u64::try_from(N).unwrap();
        for i in 1..=n {
            assert!(
                find_tagged(&scene, &close_tag(i)).is_some(),
                "chip {i} close tag present",
            );
        }
        // Labels and the `×` glyph are painted.
        let mut texts = Vec::new();
        collect_text(&scene, &mut texts);
        for label in LABELS {
            assert!(
                texts.iter().any(|t| t.as_str() == label),
                "label {label} painted"
            );
        }
        assert_eq!(
            texts.iter().filter(|t| t.as_str() == CLOSE_GLYPH).count(),
            N,
            "one × per chip",
        );
    }

    #[test]
    fn deleted_chip_is_not_painted() {
        let scene = with_owner(|| {
            // Remove Bob (id 2) before painting.
            let chips = use_chips();
            chips.set_with(|prev| prev.iter().filter(|c| c.id != 2).cloned().collect());
            view(ChipRowState::idle(), &Frame::new())
        });
        assert!(
            find_tagged(&scene, &close_tag(2)).is_none(),
            "deleted chip dropped"
        );
        assert!(
            find_tagged(&scene, &close_tag(1)).is_some(),
            "survivor #1 kept"
        );
        assert!(
            find_tagged(&scene, &close_tag(3)).is_some(),
            "survivor #3 kept"
        );
    }

    // ── ChipDeleteExternal (delete machinery + button posture) ────────────

    #[test]
    fn delete_by_id_shrinks_the_collection() {
        with_owner(|| {
            let chips = use_chips();
            let ext = ChipDeleteExternal::new(chips.clone());
            assert_eq!(chips.get().len(), N);
            ext.delete_by_id(3);
            assert_eq!(chips.get().len(), N - 1);
            assert!(!chips.get().iter().any(|c| c.id == 3), "Carol (#3) gone");
            // Re-deleting a stale id is a no-op (reactive equality-skip).
            ext.delete_by_id(3);
            assert_eq!(chips.get().len(), N - 1);
        });
    }

    #[test]
    fn close_button_full_click_cycle_drives_posture_and_deletes() {
        with_owner(|| {
            let chips = use_chips();
            let mut ext = ChipDeleteExternal::new(chips.clone());
            // Enter + Down → Pressed posture, no delete yet.
            let _ = ext.invoke("send", IntrospectValue::Text("2:PointerEnter".into()));
            assert_eq!(ext.state_of(2), ButtonState::Hover);
            let _ = ext.invoke("send", IntrospectValue::Text("2:PointerDown".into()));
            assert_eq!(ext.state_of(2), ButtonState::Pressed);
            assert!(
                chips.get().iter().any(|c| c.id == 2),
                "still present mid-press"
            );
            // PointerUp on target = click edge → delete.
            let r = ext.invoke("send", IntrospectValue::Text("2:PointerUp".into()));
            assert_eq!(r, Ok(IntrospectValue::Bool(true)), "click committed delete");
            assert!(
                !chips.get().iter().any(|c| c.id == 2),
                "Bob (#2) removed on click"
            );
        });
    }

    #[test]
    fn press_then_leave_cancels_without_delete() {
        with_owner(|| {
            let chips = use_chips();
            let mut ext = ChipDeleteExternal::new(chips.clone());
            let _ = ext.invoke("send", IntrospectValue::Text("1:PointerEnter".into()));
            let _ = ext.invoke("send", IntrospectValue::Text("1:PointerDown".into()));
            let r = ext.invoke("send", IntrospectValue::Text("1:PointerLeave".into()));
            assert_eq!(
                r,
                Ok(IntrospectValue::Bool(false)),
                "leave is a cancel, not a click"
            );
            assert!(
                chips.get().iter().any(|c| c.id == 1),
                "Alice (#1) survives the cancel"
            );
        });
    }

    #[test]
    fn direct_delete_returns_was_present() {
        with_owner(|| {
            let chips = use_chips();
            let mut ext = ChipDeleteExternal::new(chips);
            assert_eq!(
                ext.invoke("delete", IntrospectValue::Int(4)),
                Ok(IntrospectValue::Bool(true)),
                "deleting an existing chip reports was_present",
            );
            assert_eq!(
                ext.invoke("delete", IntrospectValue::Int(4)),
                Ok(IntrospectValue::Bool(false)),
                "re-deleting a stale id reports absent",
            );
        });
    }

    #[test]
    fn query_count_and_ids_track_deletes() {
        with_owner(|| {
            let chips = use_chips();
            let mut ext = ChipDeleteExternal::new(chips);
            let n = i64::try_from(N).unwrap();
            assert_eq!(ext.query("count"), Some(IntrospectValue::Int(n)));
            let _ = ext.invoke("delete", IntrospectValue::Int(5));
            assert_eq!(ext.query("count"), Some(IntrospectValue::Int(n - 1)));
            // `ids` no longer lists the removed id.
            let ids = ext.query("ids").expect("ids slot");
            if let IntrospectValue::Json(serde_json::Value::Array(arr)) = ids {
                assert!(
                    !arr.iter().any(|v| v.as_u64() == Some(5)),
                    "id 5 dropped from ids"
                );
            } else {
                panic!("ids must be a JSON array");
            }
        });
    }

    // ── WidgetCore wiring ─────────────────────────────────────────────────

    #[test]
    fn every_close_affordance_is_a_tab_stop() {
        // §5.39: tree order IS tab order — collected from the paint scene.
        let scene = with_owner(|| view(ChipRowState::idle(), &Frame::new()));
        assert_eq!(
            scene.collect_focusable_tags(),
            CLOSE_TAGS
                .iter()
                .map(|t| (*t).to_owned())
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn read_state_reflects_close_button_posture() {
        with_owner(|| {
            let scene = scene_fixture();
            // Drive chip 2's × into Hover through the live external, then read
            // it back via read_state (the paint-feeding path).
            let mut scene = scene;
            if let Some(node) = scene.find_external_with_tag_mut(DELETE_TAG) {
                let intro = node.handle.introspect_mut().unwrap();
                let _ = intro.invoke("send", IntrospectValue::Text("2:PointerEnter".into()));
            }
            let st = InputChipView::read_state(&scene);
            assert_eq!(st.close_state(2), ButtonState::Hover, "× posture surfaced");
            assert_eq!(st.close_state(1), ButtonState::Idle, "sibling untouched");
        });
    }

    #[test]
    fn apply_key_enter_on_focused_close_deletes() {
        with_owner(|| {
            let chips = use_chips();
            let mut scene = scene_fixture();
            assert!(InputChipView::apply_key(
                &mut scene,
                Some("chip_delete#3"),
                "Enter",
                pinion_core::Modifiers::empty(),
            ));
            assert!(
                !chips.get().iter().any(|c| c.id == 3),
                "Carol removed via keyboard"
            );
        });
    }

    // ── a11y ──────────────────────────────────────────────────────────────

    #[test]
    fn emits_group_parent_plus_remove_buttons() {
        with_owner(|| {
            let nodes = InputChipView::access_node(&ChipRowState::idle(), None);
            assert_eq!(nodes.len(), N + 1, "one group + N chips");
            assert_eq!(nodes[0].role, AriaRole::Group);
            assert_eq!(nodes[0].name.as_deref(), Some("Recipients"));
            assert_eq!(
                nodes[0].children.len(),
                N,
                "group references every present chip"
            );
            for (i, node) in nodes[1..].iter().enumerate() {
                assert_eq!(node.role, AriaRole::Button, "× is a button");
                assert_eq!(
                    node.name.as_deref(),
                    Some(format!("Remove {}", LABELS[i]).as_str()),
                );
            }
        });
    }

    #[test]
    fn close_access_node_reflects_focus_and_posture() {
        with_owner(|| {
            let mut state = ChipRowState::idle();
            state.close_states[1] = ButtonState::Hover; // chip #2
            let nodes = InputChipView::access_node(&state, Some("chip_delete#2"));
            // node[2] is chip #2's × button.
            let bob = &nodes[2];
            assert_eq!(bob.name.as_deref(), Some("Remove Bob"));
            assert!(bob.state.focused, "focused × carries aria focus");
            assert!(
                bob.state.hovered,
                "hover posture surfaced from from_interaction"
            );
        });
    }

    /// Build a fresh scene mirroring the shell's boot composition so
    /// `read_state` / `apply_key` walk the live topology.
    fn scene_fixture() -> Scene {
        use pinion_core::scene::ExternalNode;
        let primary = Scene::External(
            ExternalNode::new(InputChipView::create_external()).with_tag(DELETE_TAG),
        );
        Scene::Container(ContainerNode::new(vec![primary]))
    }
}
