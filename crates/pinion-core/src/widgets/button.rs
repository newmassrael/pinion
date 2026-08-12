// `unsafe_code` intentionally absent — the workspace `forbid` policy
// (Cargo.toml) rejects per-site overrides, and the sce-build codegen
// output does not use `unsafe`. The remaining allows silence the
// stylistic / dead-code lints that generated code routinely trips.
#[allow(
    non_snake_case,
    unused_imports,
    dead_code,
    unused_variables,
    unused_mut,
    unused_labels,
    unreachable_patterns,
    unreachable_code,
    unused_assignments,
    clippy::style,
    clippy::complexity,
    clippy::pedantic,
    clippy::all
)]
mod sm {
    include!(concat!(env!("OUT_DIR"), "/button_sm.rs"));
}

use sm::ButtonPolicy;
pub use sm::{ButtonEvent, ButtonState};

// SCE-002 §5.16 — the `WidgetStateName` / `WidgetEventName` impls for the
// sce-generated `ButtonState` / `ButtonEvent` enums are injected as
// `#[derive]`s by `build.rs` (`compile_scxml_with_derives`), reconstructed
// from the codegen's `#[default]` state + `EXTERNALLY_DRIVABLE_EVENTS`
// const (see `pinion-derive`); the per-widget `widget_{state,event}_name!`
// macros are retired. Bindings still opt into the derived
// `WidgetCore::read_state` + `WidgetCore::event_name` via the
// `state_name_derive` flag on `#[pinion_derive::widget]`.

use crate::WidgetStateName;
use crate::external::{
    ArgForm, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner,
    SchemaArg, SchemaField, ThreadOwnership,
};
use crate::input::AutoRepeat;
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};

/// R12 Button widget. R51.4 §5.38 refactor: a type alias over the
/// shared [`Widget<P>`] facade — Button has no value sidecar, so the
/// alias gives `Button::new` / `Button::send` / `Button::state` /
/// `Button::default` for free via the generic impls.
pub type Button = Widget<ButtonPolicy>;

/// R51.12 §5.38 — Button transition contract. Snapshot is just the
/// interaction state (no value sidecar); detect emits a `"click"`
/// intent on the `Pressed → Hover` activate path with a [`Null`]
/// payload — Button has no semantic value to carry, the kind alone
/// is the signal.
///
/// [`Null`]: IntrospectValue::Null
impl WidgetTransition for Button {
    type Event = ButtonEvent;
    type Snapshot = ButtonState;

    fn snapshot(&self) -> Self::Snapshot {
        self.state()
    }

    fn drive(&mut self, event: Self::Event) {
        self.send(event);
    }

    /// Two emitting paths land the same `click` intent — the order
    /// matters only for readability, both Null-payload click events
    /// are semantically identical:
    ///
    /// 1. R51.12 (pointer) — `Pressed → Hover` transition fires on
    ///    `PointerUp` when the cursor stayed on the widget through
    ///    the press / release cycle.
    /// 2. R51.54 §5.39 (keyboard) — `KeyboardActivate` event from
    ///    `Idle` or `Hover` while focused; the SCXML internal
    ///    transition leaves state unchanged, so without the `event`
    ///    argument the detection would be ambiguous.
    ///
    /// The ARIA Button keyboard pattern says Space / Enter on a
    /// focused button has the same effect as a click — the two paths
    /// converge on the same intent kind precisely because the spec
    /// equates them.
    fn detect(before: Self::Snapshot, event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
        let pointer_click =
            matches!(before, ButtonState::Pressed) && matches!(after, ButtonState::Hover);
        let keyboard_click = matches!(event, ButtonEvent::KeyboardActivate)
            && !matches!(before, ButtonState::Disabled);
        if pointer_click || keyboard_click {
            vec![Intent::new_static("click", IntrospectValue::Null)]
        } else {
            Vec::new()
        }
    }
}

/// `External` adapter wrapping a [`Button`] SCXML widget. Surfaces
/// the button's [`ButtonState`] to the §5.12 `scene/query` RPC
/// method via the §5.15 item 8 introspect path `state` (read-only,
/// returns [`IntrospectValue::Text`] carrying the variant name).
///
/// First concrete §5.15 reference impl bridging an R12 widget into
/// the RPC plane — `CountedExternal` covers trait-surface mechanics,
/// but `ButtonExternal` is the first time a real widget's state
/// machine round-trips through `dispatch`.
pub struct ButtonExternal {
    /// R51.5 §5.38 refactor: §5.20 intent buffer + wrapped widget
    /// share the [`IntentEmitter`] helper; the adapter only owns the
    /// transition-detection logic in `send`.
    em: IntentEmitter<Button>,
    /// R694 §5.39 — keyboard-focus posture, mirrored from the shell
    /// [`FocusManager`](crate) through
    /// [`External::on_focus_change`].
    /// Orthogonal to [`ButtonState`] (a focused button can be Idle /
    /// Hover / Pressed); surfaced via the `focused` introspect slot so
    /// the binding's `read_state` threads it into
    /// `view_button`'s focus-ring argument — the same External → query →
    /// view channel the hover posture already uses.
    focused: bool,
    /// R1549 §5.35 — declared press-and-hold repeat cadence: the toolkit
    /// `setAutoRepeat` + `setAutoRepeatDelay` +
    /// `setAutoRepeatInterval` collapsed into one value. `None` — the
    /// default, matching push button — means one activation per press,
    /// however long it is held.
    repeat: Option<AutoRepeat>,
}

impl ButtonExternal {
    #[must_use]
    pub fn new() -> Self {
        Self {
            em: IntentEmitter::default(),
            focused: false,
            repeat: None,
        }
    }

    /// R1549 §5.35 — opt this button into press-and-hold auto-repeat at
    /// `repeat`'s cadence. The toolkit `setAutoRepeat(true)` +
    /// `setAutoRepeatDelay` + `setAutoRepeatInterval` triple as one call:
    /// there is no separate enable flag to disagree with the timings,
    /// because a declared cadence *is* the enable.
    ///
    /// A repeating button re-runs its own `Pressed → Hover → Pressed`
    /// activation arc, so it emits the same `"click"` intent a real click
    /// does — a consumer needs no new handler to become repeatable.
    #[must_use]
    pub fn with_auto_repeat(mut self, repeat: AutoRepeat) -> Self {
        self.repeat = Some(repeat);
        self
    }

    /// The declared press-and-hold cadence, or `None` for a
    /// fires-once-per-press button (the toolkit `autoRepeat()`).
    #[must_use]
    pub const fn auto_repeat_policy(&self) -> Option<AutoRepeat> {
        self.repeat
    }

    /// Drive a [`ButtonEvent`] through the wrapped SCXML and enqueue
    /// any §5.20 intent the transition produces.
    ///
    /// R51.12 §5.38 refactor: the snapshot → drive → detect → push
    /// pipeline lives on [`IntentEmitter::dispatch`]; the detection
    /// rule (Pressed → Hover ⇒ `"click"`/Null) lives on the
    /// [`WidgetTransition`] impl for [`Button`]. The widget emits
    /// only the kind — the §5.20 R22 runtime walk prefixes
    /// `ExternalNode.tag` (e.g. `"save_btn"` → `"save_btn.click"`),
    /// so widget-internal identity stays decoupled from the
    /// user-chosen scene-side identifier.
    pub fn send(&mut self, event: ButtonEvent) {
        self.em.dispatch(event);
    }

    #[must_use]
    pub fn state(&self) -> ButtonState {
        self.em.inner.state()
    }

    /// R694 §5.39 — current keyboard-focus posture (set by the shell via
    /// [`External::on_focus_change`]).
    #[must_use]
    pub fn focused(&self) -> bool {
        self.focused
    }
}

impl Default for ButtonExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for ButtonExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ButtonExternal")
            .field("state", &self.state())
            .finish()
    }
}

impl External for ButtonExternal {
    /// R741 §5.35 — capture the pointer on press so a real-mouse click is
    /// robust to sub-pixel jitter between press and release.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R741 §5.35 — a deliberate release off the widget cancels the press.
    fn cancel_on_release_off_target(&self) -> bool {
        true
    }

    /// R1549 §5.35 — the declared cadence, gated on the button actually being
    /// held. The toolkit keeps `autoRepeat` and `isDown` as two facts and a basic timer
    /// bridging them; here the second fact is the statechart's own [`ButtonState::Pressed`], so a
    /// repeat that outlives its press has nowhere to live. A press that slid
    /// off the widget is already `Hover` / `Idle` (R741 capture defers the leave to
    /// the release), and a `Disabled` button answers `None` without a disable hook.
    fn auto_repeat(&self) -> Option<AutoRepeat> {
        matches!(self.state(), ButtonState::Pressed)
            .then_some(self.repeat)
            .flatten()
    }

    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
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

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        self.em.drain(sink);
    }

    fn is_dirty(&self) -> bool {
        self.em.is_dirty()
    }

    /// R694 §5.39 — mirror the shell focus posture so the focus ring
    /// paints. The shell fires this on the gaining tag (`true`) and the
    /// losing tag (`false`) whenever
    /// [`FocusManager`](crate) focus moves; the repaint that focus change
    /// already drives (the a11y tree rebuild) re-reads the posture.
    fn on_focus_change(&mut self, focused: bool) {
        self.focused = focused;
    }
}

impl ExternalIntrospect for ButtonExternal {
    fn schema(&self) -> IntrospectSchema {
        // `state` — read-only slot (query). `send` — action channel
        // accepting a `ButtonEvent` variant name (invoke); §5.15
        // schema does not yet distinguish state slots from actions
        // syntactically, that classification lands with §5.3 DSL.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("state", "string"),
                    SchemaField::new("focused", "bool"),
                    SchemaField::action_with(
                        "send",
                        "string",
                        ArgForm::Scalar,
                        const { &[SchemaArg::event(&ButtonEvent::DRIVABLE_NAMES)] },
                    ),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "state" => Ok(IntrospectValue::Text(self.state().as_name().to_string())),
            // R694 §5.39 — keyboard-focus posture for the focus-ring read.
            "focused" => Ok(IntrospectValue::Bool(self.focused)),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        // State is observed-only via intervene; mutation flows through
        // the `send` action on the invoke channel (R17 bidirectional
        // RPC spec round) — see `invoke` below.
        match path {
            "state" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Action: drive a `ButtonEvent` symbolically by name.
            // Returns the resulting `ButtonState` as `Text`, so the
            // caller (winit handler or RPC client) sees the transition
            // outcome in a single round-trip.
            "send" => match args {
                IntrospectValue::Text(ref name) => {
                    let ev = crate::widget_core::require_event::<ButtonEvent>("button", name)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(self.state().as_name().to_string()))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

impl ButtonExternal {
    /// Capture the current state as an owned, `Send`-friendly,
    /// read-only RPC view (see [`ButtonStateSnapshot`]). Lets a live
    /// app feed its current `ButtonState` to `dispatch` without
    /// surrendering ownership of the wrapped SCXML engine.
    #[must_use]
    pub fn snapshot(&self) -> ButtonStateSnapshot {
        ButtonStateSnapshot::new(self.state())
    }
}

/// Read-only RPC view of a single `Button`'s state at a point in
/// time. Implements [`External`] + [`ExternalIntrospect`] so it can
/// be embedded in `Scene::External` and queried via the §5.12
/// `scene/query` method, while remaining cheap (single enum field)
/// and `Send` — the live `Button` itself stays on the UI thread.
///
/// `intervene` always errors with [`InterveneError::ReadOnly`]: this
/// type is a *snapshot*, not a control surface. Live-mutating RPC
/// (e.g. RPC-driven `ButtonEvent::PointerDown`) requires a `Box<dyn
/// External>` downcast story that is carry-forward to a later spec
/// round.
#[derive(Debug, Clone, Copy)]
pub struct ButtonStateSnapshot {
    state: ButtonState,
}

impl ButtonStateSnapshot {
    #[must_use]
    pub const fn new(state: ButtonState) -> Self {
        Self { state }
    }

    #[must_use]
    pub const fn state(&self) -> ButtonState {
        self.state
    }
}

impl External for ButtonStateSnapshot {
    fn backends(&self) -> BackendSupport {
        // RPC-only: snapshot does not paint or take input, it only
        // surfaces state to the §5.12 query path.
        BackendSupport::new(&[Backend::Rpc], BackendFallback::Skip)
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

impl ExternalIntrospect for ButtonStateSnapshot {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(const { &[SchemaField::new("state", "string")] })
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "state" => Ok(IntrospectValue::Text(self.state.as_name().to_string())),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        // Snapshot is observation-only by design — see type doc.
        Err(InterveneError::ReadOnly)
    }

    fn invoke(
        &mut self,
        _path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        // Snapshot has no action channel — the live `ButtonExternal`
        // is where transitions land.
        Err(InvokeError::rejected(
            "button snapshot: this surface is a read-only copy; \
             drive the live button external instead",
        ))
    }
}

#[cfg(test)]
mod tests {
    /// R1639 — the vocabulary a client discovers is exactly the one
    /// [`WidgetEventName::from_name`] admits, in BOTH directions.
    ///
    /// `DRIVABLE_NAMES` is a `const` projected from `EXTERNALLY_DRIVABLE_EVENTS`
    /// and `drivable_names()` is the runtime `Vec` a refusal is built from; they
    /// are two renderings of one list and this holds them to it. A list that is
    /// too SHORT leaves a real event undiscoverable while every published name
    /// still parses, and one that is too LONG promises an event the parser
    /// refuses — only the pair pins the set.
    #[test]
    fn r1639_the_published_event_vocabulary_is_what_from_name_admits() {
        use crate::WidgetEventName;
        for name in ButtonEvent::DRIVABLE_NAMES {
            assert!(
                ButtonEvent::from_name(name).is_some(),
                "{name:?} is published, so it must be accepted",
            );
        }
        for name in ButtonEvent::drivable_names() {
            assert!(
                ButtonEvent::DRIVABLE_NAMES.contains(&name),
                "{name:?} is accepted, so it must be published",
            );
        }
        // An INTERNAL event is neither. `ButtonActivate` is raised by the chart
        // and must not be forgeable over RPC, which is the whole reason the
        // vocabulary is the drivable const rather than the variant list.
        assert!(!ButtonEvent::DRIVABLE_NAMES.contains(&"ButtonActivate"));
        assert!(ButtonEvent::from_name("ButtonActivate").is_none());
    }

    /// R1639 — and the widget's `send` DECLARES that vocabulary, so an agent
    /// reads it instead of provoking a refusal to learn it.
    #[test]
    fn r1639_send_declares_the_events_it_accepts() {
        use crate::external::{ArgDomain, ArgForm, ExternalIntrospect};
        let ext = ButtonExternal::new();
        let field = ext
            .schema()
            .fields
            .iter()
            .find(|f| f.path == "send")
            .copied()
            .expect("the widget declares its composite channel");
        assert_eq!(field.form, ArgForm::Scalar, "one argument, sent bare");
        assert_eq!(field.args.len(), 1);
        let ArgDomain::OneOf(values) = field.args[0].domain else {
            panic!(
                "the event argument names its vocabulary: {:?}",
                field.args[0]
            );
        };
        assert_eq!(values, ButtonEvent::DRIVABLE_NAMES);
        assert!(!values.is_empty(), "and it is not the empty promise");
    }

    use super::*;
    use crate::WidgetEventName;
    use crate::test_fixtures::assert_refused_saying;

    /// Hold a `ButtonExternal` down over its own send surface.
    fn hold(b: &mut ButtonExternal) {
        b.send(ButtonEvent::PointerEnter);
        b.send(ButtonEvent::PointerDown);
    }

    // ─────────────────────────────────────────────────────────────
    // R1549 §5.35 §5.38 — `setAutoRepeat` peer.
    // ─────────────────────────────────────────────────────────────

    /// push button's default: one activation per press, however long it
    /// is held. A button that never declared a cadence answers `None`
    /// whether or not it is down.
    #[test]
    fn plain_button_never_repeats_even_while_held() {
        let mut b = ButtonExternal::new();
        assert_eq!(b.auto_repeat_policy(), None);
        hold(&mut b);
        assert_eq!(b.state(), ButtonState::Pressed);
        assert_eq!(b.auto_repeat(), None);
    }

    /// The declared cadence IS the enable — there is no second `autoRepeat` bool that
    /// could disagree with the timings, which is the pair the toolkit keeps
    /// separate.
    #[test]
    fn declared_cadence_is_the_enable() {
        let policy = AutoRepeat::new(0.25, 0.05);
        let mut b = ButtonExternal::new().with_auto_repeat(policy);
        assert_eq!(b.auto_repeat_policy(), Some(policy));
        assert_eq!(b.auto_repeat(), None, "declared, but not held");
        hold(&mut b);
        assert_eq!(b.auto_repeat(), Some(policy), "held and declared");
    }

    /// Release, stray and disable all stop the repeat through the
    /// statechart, with no un-arming call on any of the three paths — the
    /// runaway-timer bug class has nowhere to live.
    #[test]
    fn every_way_out_of_pressed_stops_the_declaration() {
        for exit in [
            ButtonEvent::PointerUp,
            ButtonEvent::PointerLeave,
            ButtonEvent::Disable,
        ] {
            let mut b = ButtonExternal::new().with_auto_repeat(AutoRepeat::desktop());
            hold(&mut b);
            assert!(b.auto_repeat().is_some(), "held");
            b.send(exit);
            assert_eq!(
                b.auto_repeat(),
                None,
                "{exit:?} left Pressed, so the repeat is over",
            );
        }
    }

    #[test]
    fn initial_state_is_idle() {
        let button = Button::new();
        assert_eq!(button.state(), ButtonState::Idle);
    }

    #[test]
    fn pointer_enter_transitions_to_hover() {
        let mut button = Button::new();
        button.send(ButtonEvent::PointerEnter);
        assert_eq!(button.state(), ButtonState::Hover);
    }

    #[test]
    fn full_click_cycle_idle_hover_pressed_hover() {
        let mut button = Button::new();
        button.send(ButtonEvent::PointerEnter);
        assert_eq!(button.state(), ButtonState::Hover);
        button.send(ButtonEvent::PointerDown);
        assert_eq!(button.state(), ButtonState::Pressed);
        button.send(ButtonEvent::PointerUp);
        assert_eq!(button.state(), ButtonState::Hover);
    }

    #[test]
    fn pointer_leave_during_press_cancels_to_idle() {
        let mut button = Button::new();
        button.send(ButtonEvent::PointerEnter);
        button.send(ButtonEvent::PointerDown);
        button.send(ButtonEvent::PointerLeave);
        assert_eq!(button.state(), ButtonState::Idle);
    }

    // R51.93 §5.35 — PointerCancel regression tests.

    #[test]
    fn r51_93_pointer_cancel_during_press_returns_to_idle_without_click() {
        // OS-revoked touch (TouchPhase::Cancelled) — the gesture
        // reached `Pressed` but must NOT commit a click.
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::PointerEnter);
        bx.send(ButtonEvent::PointerDown);
        assert!(matches!(bx.state(), ButtonState::Pressed));
        bx.send(ButtonEvent::PointerCancel);
        assert!(matches!(bx.state(), ButtonState::Idle));
        // The activate-edge intent must not have been emitted.
        assert!(
            !bx.is_dirty(),
            "PointerCancel from Pressed must not fire `click` intent"
        );
    }

    #[test]
    fn r51_93_pointer_cancel_during_hover_drops_to_idle() {
        // Mid-hover cancellation (defensive — the OS may revoke
        // before pointer_down lands).
        let mut button = Button::new();
        button.send(ButtonEvent::PointerEnter);
        assert_eq!(button.state(), ButtonState::Hover);
        button.send(ButtonEvent::PointerCancel);
        assert_eq!(button.state(), ButtonState::Idle);
    }

    #[test]
    fn r51_93_pointer_cancel_from_idle_is_silent_no_op() {
        let mut button = Button::new();
        button.send(ButtonEvent::PointerCancel);
        assert_eq!(button.state(), ButtonState::Idle);
    }

    #[test]
    fn r51_93_pointer_cancel_when_disabled_is_silent_no_op() {
        let mut button = Button::new();
        button.send(ButtonEvent::Disable);
        button.send(ButtonEvent::PointerCancel);
        assert_eq!(button.state(), ButtonState::Disabled);
    }

    #[test]
    fn r51_93_parse_pointer_cancel_event_name() {
        assert_eq!(
            ButtonEvent::from_name("PointerCancel"),
            Some(ButtonEvent::PointerCancel)
        );
        // R699 §5.16 — internal raise + Null + unknown all reject.
        assert_eq!(ButtonEvent::from_name("ButtonActivate"), None);
        assert_eq!(ButtonEvent::from_name("Null"), None);
        assert_eq!(ButtonEvent::from_name("Bogus"), None);
        // R699 — `as_name` is total over internal variants too.
        assert_eq!(ButtonEvent::ButtonActivate.as_name(), "ButtonActivate");
    }

    #[test]
    fn r773_pointer_wire_vocab_pins_to_scxml_canonical_names() {
        // The hand-maintained `PointerWireEvent` vocabulary (the router
        // emit / command-widget decode SSOT) and the SCE-emitted
        // `ButtonEvent` vocabulary (the variant ident string via the
        // SCE-002 `WidgetEventName` derive) carry the same five pointer names but
        // own them in different layers. This pins the two so a rename on
        // either side fails at test time instead of silently desyncing
        // the router from the statechart (`as_name` is the SCXML canon).
        use crate::input::PointerWireEvent;
        assert_eq!(
            PointerWireEvent::Enter.as_wire_name(),
            ButtonEvent::PointerEnter.as_name()
        );
        assert_eq!(
            PointerWireEvent::Down.as_wire_name(),
            ButtonEvent::PointerDown.as_name()
        );
        assert_eq!(
            PointerWireEvent::Up.as_wire_name(),
            ButtonEvent::PointerUp.as_name()
        );
        assert_eq!(
            PointerWireEvent::Leave.as_wire_name(),
            ButtonEvent::PointerLeave.as_name()
        );
        assert_eq!(
            PointerWireEvent::Cancel.as_wire_name(),
            ButtonEvent::PointerCancel.as_name()
        );
    }

    #[test]
    fn disable_absorbs_pointer_events() {
        let mut button = Button::new();
        button.send(ButtonEvent::Disable);
        assert_eq!(button.state(), ButtonState::Disabled);
        button.send(ButtonEvent::PointerEnter);
        assert_eq!(button.state(), ButtonState::Disabled);
        button.send(ButtonEvent::PointerDown);
        assert_eq!(button.state(), ButtonState::Disabled);
    }

    #[test]
    fn enable_returns_to_idle() {
        let mut button = Button::new();
        button.send(ButtonEvent::Disable);
        button.send(ButtonEvent::Enable);
        assert_eq!(button.state(), ButtonState::Idle);
    }

    #[test]
    fn disable_from_hover_to_disabled() {
        let mut button = Button::new();
        button.send(ButtonEvent::PointerEnter);
        button.send(ButtonEvent::Disable);
        assert_eq!(button.state(), ButtonState::Disabled);
    }

    #[test]
    fn button_external_initial_query_state_is_idle() {
        let bx = ButtonExternal::new();
        let v = bx.query("state").expect("schema declares `state`");
        assert_eq!(v, IntrospectValue::Text("Idle".to_string()));
    }

    #[test]
    fn button_external_query_tracks_send_transitions() {
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::PointerEnter);
        let v = bx.query("state").unwrap();
        assert_eq!(v, IntrospectValue::Text("Hover".to_string()));
        bx.send(ButtonEvent::PointerDown);
        let v = bx.query("state").unwrap();
        assert_eq!(v, IntrospectValue::Text("Pressed".to_string()));
    }

    #[test]
    fn button_external_unknown_query_path_returns_none() {
        let bx = ButtonExternal::new();
        assert!(bx.query("nope").is_err());
    }

    #[test]
    fn r694_button_external_focus_posture_mirrors_on_focus_change() {
        use crate::external::External;
        let mut bx = ButtonExternal::new();
        assert!(!bx.focused(), "buttons boot unfocused");
        assert_eq!(bx.query("focused"), Ok(IntrospectValue::Bool(false)));
        bx.on_focus_change(true);
        assert!(bx.focused(), "focus posture follows the shell");
        assert_eq!(
            bx.query("focused"),
            Ok(IntrospectValue::Bool(true)),
            "focus posture surfaces on the introspect slot for the ring read",
        );
        bx.on_focus_change(false);
        assert_eq!(bx.query("focused"), Ok(IntrospectValue::Bool(false)));
    }

    #[test]
    fn r694_button_external_focus_orthogonal_to_state() {
        use crate::external::External;
        // Focus does not perturb the SCXML interaction state, and a
        // state transition does not clear focus.
        let mut bx = ButtonExternal::new();
        bx.on_focus_change(true);
        bx.send(ButtonEvent::PointerEnter);
        assert_eq!(bx.state(), ButtonState::Hover);
        assert!(bx.focused(), "send did not clear the focus posture");
    }

    #[test]
    fn button_external_intervene_state_is_read_only() {
        let mut bx = ButtonExternal::new();
        let r = bx.intervene("state", IntrospectValue::Text("Pressed".to_string()));
        assert_eq!(r, Err(InterveneError::ReadOnly));
        let r = bx.intervene("nope", IntrospectValue::Null);
        assert_eq!(r, Err(InterveneError::UnknownPath));
    }

    #[test]
    fn button_external_schema_declares_state_slot() {
        let bx = ButtonExternal::new();
        let schema = bx.schema();
        assert!(
            schema
                .fields
                .iter()
                .any(|f| f.path == "state" && f.ty == "string")
        );
    }

    #[test]
    fn button_external_snapshot_captures_current_state() {
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::PointerEnter);
        let snap = bx.snapshot();
        assert_eq!(snap.state(), ButtonState::Hover);
        let v = snap.query("state").unwrap();
        assert_eq!(v, IntrospectValue::Text("Hover".to_string()));
    }

    #[test]
    fn button_state_snapshot_intervene_is_always_read_only() {
        let mut snap = ButtonStateSnapshot::new(ButtonState::Idle);
        let r = snap.intervene("state", IntrospectValue::Text("Pressed".to_string()));
        assert_eq!(r, Err(InterveneError::ReadOnly));
        let r = snap.intervene("nope", IntrospectValue::Null);
        assert_eq!(r, Err(InterveneError::ReadOnly));
    }

    #[test]
    fn button_state_snapshot_clone_is_independent() {
        let snap = ButtonStateSnapshot::new(ButtonState::Pressed);
        let copy = snap;
        assert_eq!(snap.state(), copy.state());
    }

    #[test]
    fn button_external_invoke_send_drives_transition_and_returns_new_state() {
        let mut bx = ButtonExternal::new();
        let out = bx
            .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
            .expect("PointerEnter is a known ButtonEvent variant");
        assert_eq!(out, IntrospectValue::Text("Hover".to_string()));
        assert_eq!(bx.state(), ButtonState::Hover);
    }

    #[test]
    fn button_external_invoke_unknown_event_name_is_rejected() {
        let mut bx = ButtonExternal::new();
        let r = bx.invoke("send", IntrospectValue::Text("Teleport".to_string()));
        assert_refused_saying(&r, "\"Teleport\" is not an event this widget accepts");
        // R1564 — and it names the vocabulary that WOULD have been accepted,
        // which is the difference between a refusal read and one acted on.
        assert_refused_saying(&r, "PointerDown");
        // State unchanged because the action did not fire.
        assert_eq!(bx.state(), ButtonState::Idle);
    }

    #[test]
    fn button_external_invoke_wrong_arg_type_is_type_mismatch() {
        let mut bx = ButtonExternal::new();
        let r = bx.invoke("send", IntrospectValue::Int(42));
        assert_eq!(r, Err(InvokeError::TypeMismatch));
    }

    #[test]
    fn button_external_invoke_unknown_path_is_unknown_path() {
        let mut bx = ButtonExternal::new();
        let r = bx.invoke("nope", IntrospectValue::Null);
        assert_eq!(r, Err(InvokeError::UnknownPath));
    }

    #[test]
    fn button_state_snapshot_invoke_always_rejects() {
        let mut snap = ButtonStateSnapshot::new(ButtonState::Idle);
        let r = snap.invoke("send", IntrospectValue::Text("PointerEnter".to_string()));
        assert_refused_saying(&r, "read-only copy");
    }

    #[test]
    fn button_external_schema_includes_send_action() {
        let bx = ButtonExternal::new();
        let schema = bx.schema();
        assert_eq!(
            schema.fields,
            // R694 §5.39 — `focused` joins the read-only state slot.
            &[
                SchemaField::new("state", "string"),
                SchemaField::new("focused", "bool"),
                SchemaField::action_with(
                    "send",
                    "string",
                    ArgForm::Scalar,
                    const { &[SchemaArg::event(&ButtonEvent::DRIVABLE_NAMES)] },
                )
            ]
        );
    }

    #[test]
    fn button_external_emits_click_intent_on_pressed_to_hover() {
        // §5.20 R22: widget emits only the kind ("click"); the
        // scene-side ExternalNode.tag supplies the widget prefix at
        // walk time. This isolates widget identity from UI naming.
        let mut bx = ButtonExternal::new();
        assert!(!bx.is_dirty());
        bx.send(ButtonEvent::PointerEnter);
        bx.send(ButtonEvent::PointerDown);
        assert!(!bx.is_dirty(), "PointerDown alone is not a click");
        bx.send(ButtonEvent::PointerUp);
        assert!(bx.is_dirty(), "PointerUp from Pressed should arm click");
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "click");
        assert_eq!(harvested[0].payload, IntrospectValue::Null);
        assert!(!bx.is_dirty(), "drain should leave the buffer empty");
    }

    #[test]
    fn button_external_pointer_down_alone_emits_no_intent() {
        // Mid-press without release should not signal a click —
        // guards against premature emission.
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::PointerEnter);
        bx.send(ButtonEvent::PointerDown);
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }

    #[test]
    fn button_external_press_then_leave_cancels_click() {
        // Pressed → Idle (via PointerLeave) is a *cancel*, not a
        // click; nothing should land on the intent channel.
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::PointerEnter);
        bx.send(ButtonEvent::PointerDown);
        bx.send(ButtonEvent::PointerLeave);
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }

    #[test]
    fn button_external_invoke_send_pointer_up_emits_click_intent() {
        // §5.20 cross-channel: a click driven through the §5.15
        // invoke action also queues the intent — same emission path
        // whether the transition comes from winit or RPC.
        let mut bx = ButtonExternal::new();
        let _ = bx
            .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
            .unwrap();
        let _ = bx
            .invoke("send", IntrospectValue::Text("PointerDown".to_string()))
            .unwrap();
        let _ = bx
            .invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .unwrap();
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "click");
    }

    #[test]
    fn button_state_snapshot_emits_no_intents() {
        // Snapshots are observation-only; the §5.20 channel must stay
        // silent on them.
        let mut snap = ButtonStateSnapshot::new(ButtonState::Hover);
        assert!(!snap.is_dirty());
        let mut harvested: Vec<Intent> = Vec::new();
        snap.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }

    // ----- R51.54 §5.39 keyboard activation -----

    #[test]
    fn keyboard_activate_from_idle_emits_click_intent_state_unchanged() {
        // ARIA: Space/Enter on a focused button = click. The SCXML
        // internal transition raises `button.activate` without
        // changing visible state.
        let mut bx = ButtonExternal::new();
        assert_eq!(bx.state(), ButtonState::Idle);
        bx.send(ButtonEvent::KeyboardActivate);
        assert_eq!(
            bx.state(),
            ButtonState::Idle,
            "keyboard activation must be a state-stable internal transition",
        );
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "click");
        assert_eq!(harvested[0].payload, IntrospectValue::Null);
    }

    #[test]
    fn keyboard_activate_from_hover_emits_click_intent_state_unchanged() {
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::PointerEnter);
        assert_eq!(bx.state(), ButtonState::Hover);
        bx.send(ButtonEvent::KeyboardActivate);
        assert_eq!(bx.state(), ButtonState::Hover);
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "click");
    }

    #[test]
    fn keyboard_activate_from_disabled_emits_no_intent() {
        // ARIA: disabled controls do not respond to keyboard
        // activation. The SCXML template has no
        // `keyboard_activate` transition from `disabled`.
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::Disable);
        assert_eq!(bx.state(), ButtonState::Disabled);
        bx.send(ButtonEvent::KeyboardActivate);
        assert_eq!(bx.state(), ButtonState::Disabled);
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }

    #[test]
    fn keyboard_activate_via_invoke_send_emits_click() {
        // Same path the shell's `WidgetView::apply_key` uses: the
        // event-name string round-trips through `ButtonEvent::from_name`.
        let mut bx = ButtonExternal::new();
        let _ = bx
            .invoke(
                "send",
                IntrospectValue::Text("KeyboardActivate".to_string()),
            )
            .unwrap();
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "click");
    }
}
