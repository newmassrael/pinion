//! R692 §5.38 §5.40 — `Toolbar` widget: roving-tabindex strip of
//! command + toggle controls.
//!
//! Phase B widget-catalog entry — the editor format / command strip
//! every pro DCC / IDE / CAD tool ships (a text editor's
//! Bold / Italic / Underline toggles next to Undo / Redo commands; a
//! browser's Back / Forward / Reload). A `Toolbar` owns N controls and
//! a single roving keyboard cursor; the whole strip is one Tab stop.
//!
//! ## Two control classes, one container
//!
//! A toolbar groups two kinds of control (WAI-ARIA 1.2 §3.4):
//!
//! - [`ToolItem::Command`] — a one-shot [`button`](crate::widgets::button)
//!   (Undo / Redo). Activating it fires a command and changes no
//!   persistent state, exactly like a [`menuitem`](crate::widgets::menu).
//! - [`ToolItem::Toggle`] — a button carrying `aria-pressed` (Bold /
//!   Italic). Activating it flips a persistent `pressed` bit. The
//!   WAI-ARIA `aria-pressed` axis is the `button` role + a toggled
//!   state — *not* the `aria-checked` axis a `checkbox` carries — so a
//!   toolbar toggle stays distinct from a [`CheckBox`](crate::widgets::checkbox)
//!   at the AT surface even though both lower to the same AccessKit
//!   `Toggled` attribute.
//!
//! Unlike a [`MenuBar`](crate::widgets::menu::MenuBar) (whose titles
//! open dropdown menus), a toolbar's controls are directly actionable;
//! there is no open/closed structural state, only the roving cursor +
//! the per-toggle pressed bits.
//!
//! ## State model
//!
//! - `kinds: Vec<ToolItem>` — one entry per control; `kinds.len()` is
//!   the control count.
//! - `pressed: Vec<bool>` — parallel pressed bits. A
//!   [`ToolItem::Command`] entry's bit never flips (stays `false`);
//!   only [`ToolItem::Toggle`] entries toggle.
//! - `focus: usize` — the roving keyboard cursor (WAI-ARIA roving
//!   tabindex). Arrow Left/Right move it (wrap), Home/End jump.
//!
//! ## Wire surface
//!
//! The §5.12 introspect surface drives the toolbar identically from the
//! human cursor, the keyboard, and an RPC headless client (§2
//! invariant #2):
//!
//! * `invoke("send", "<i>:<PointerWireEvent>")` — a control pointer event
//!   (composite tag `<bar>#<i>`). `PointerUp` focuses control `i` and
//!   activates it (a command fires `"command"`; a toggle flips its bit
//!   and fires `"toggle"`).
//! * `invoke("key", "<W3CKeyName>")` — the WAI-ARIA §3.4 toolbar
//!   keyboard model: Arrow Left/Right move the roving cursor (wrap),
//!   Home/End jump to first/last, Enter/Space activate the focused
//!   control. Returns [`IntrospectValue::Bool`] = "did this key do
//!   anything" so the binding's `apply_key` reports the right swallow
//!   verdict (`Tab` is unhandled → the shell advances focus past the
//!   toolbar).
//!
//! On activation the §5.20 channel emits one of two intents whose tag
//! the runtime walk prefixes with the scene `ExternalNode` tag (e.g.
//! `"toolbar"` → `"toolbar.command"`):
//!
//! * `"command"`, payload [`IntrospectValue::Text`] `"<i>"` — a command
//!   control fired.
//! * `"toggle"`, payload [`IntrospectValue::Text`] `"<i>.<pressed>"`
//!   (`<pressed>` is `true` / `false`) — a toggle flipped to that
//!   state.
//!
//! ## Disabled controls (R989 — reflective, focusable-but-not-operable)
//!
//! A control can be **disabled** (greyed, `aria-disabled`) — the canonical
//! contextual-toolbar gate, e.g. a selection action bar that disables
//! "Delete" while nothing is selected. Disabled is **reflective**, mirroring
//! the `aria-pressed` story above: the toolbar does *not* own a disabled bit.
//! The binding recomputes the mask each frame from whatever state drives it
//! (the selection, the document, …) and passes it to the paint
//! (`pinion_widget_paint::toolbar::view_toolbar`'s `disabled` slice) and the
//! a11y tree (`pinion_a11y::ToolbarControl::disabled`). Because the bit is not
//! owned, the toolbar itself stays state-light and the mask can never drift
//! out of sync with its source.
//!
//! A disabled control follows the W3C **`aria-disabled`** discipline (as
//! distinct from the HTML `disabled` *attribute*): it stays focusable and in
//! the tree — the roving cursor may rest on it and AT announces it as
//! unavailable (the WAI-ARIA APG *toolbar* allowance for keeping disabled
//! items discoverable) — but it is **not operable**, and per the
//! `aria-disabled` contract *the author* prevents the action. Concretely the
//! toolbar still emits its `"command"` / `"toggle"` intent (it cannot see the
//! reflective mask), and the binding's reducer drops it. This is a deliberate,
//! principled divergence from the *owned*-disabled widgets in this catalogue —
//! [`button`](crate::widgets::button) (the statechart has no activate
//! transition from `Disabled`, so it absorbs the event and emits nothing) and
//! [`menu`](crate::widgets::menu) (a disabled item is skipped by the roving
//! cursor and `activate_item` is inert): those own their disabled bit, so the
//! HTML-`disabled` *suppress-at-the-widget* model fits. A *contextual* toolbar's
//! disabled state is **derived** from external state (the selection, the
//! document), not owned; threading that derived mask into the External would
//! force a per-frame cross-external sync, whereas `aria-disabled` keeps the
//! operable barrier in the one place that already owns the action's effect (the
//! reducer). The cost is a no-op intent on the wire for a disabled control —
//! benign because the disabled state is itself introspectable (`aria-disabled`
//! via `scene/access`), so an AI client checks before invoking.
//!
//! ## Future axes (per [[abstraction-needs-second-consumer]])
//!
//! *Separators / section groups*, *overflow menu* (controls that
//! collapse into a `…` dropdown when the strip is narrow), and *vertical
//! orientation* (`aria-orientation=vertical` swaps Arrow Left/Right for
//! Up/Down) are additive axes once a consumer needs them.

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg, SchemaField,
    ThreadOwnership,
};
use crate::input::PointerWireEvent;
use crate::intent::Intent;
use crate::widgets::IntentEmitter;
use crate::widgets::wire::resolve_index;

/// R692 §5.38 — the two control classes a [`Toolbar`] groups. A
/// `Command` is a one-shot button; a `Toggle` carries a persistent
/// pressed bit (`aria-pressed`). See the module docs for the
/// command-vs-toggle rationale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolItem {
    /// One-shot command button (Undo / Redo). Activation fires a
    /// `"command"` intent; no persistent state.
    Command,
    /// Toggle button (Bold / Italic). Activation flips a persistent
    /// pressed bit and fires a `"toggle"` intent.
    Toggle,
}

/// R692 §5.38 — the outcome of activating a control, returned by
/// `Toolbar::activate` so the [`ToolbarExternal`] adapter can emit
/// the matching §5.20 intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolActivation {
    /// Command control `index` fired.
    Command(usize),
    /// Toggle control `index` flipped to `pressed`.
    Toggle(usize, bool),
}

/// R692 §5.38 — logical toolbar with N controls + one roving cursor.
/// See module docs for the command-vs-toggle rationale and the state
/// model.
pub struct Toolbar {
    /// One entry per control; `kinds.len()` is the control count.
    kinds: Vec<ToolItem>,
    /// Parallel pressed bits (same length as `kinds`). A `Command`
    /// entry's bit never flips.
    pressed: Vec<bool>,
    /// Roving keyboard cursor (WAI-ARIA roving tabindex).
    focus: usize,
}

impl Toolbar {
    /// Construct a toolbar with the given control `kinds`. All toggle
    /// bits start `false` and the roving cursor starts on control 0.
    #[must_use]
    pub fn new(kinds: Vec<ToolItem>) -> Self {
        let pressed = vec![false; kinds.len()];
        Self {
            kinds,
            pressed,
            focus: 0,
        }
    }

    /// Number of controls.
    #[must_use]
    pub fn count(&self) -> usize {
        self.kinds.len()
    }

    /// Control class of control `i`, or `None` when `i` is out of
    /// range.
    #[must_use]
    pub fn kind(&self, i: usize) -> Option<ToolItem> {
        self.kinds.get(i).copied()
    }

    /// Pressed bit of control `i`, or `None` when `i` is out of range.
    /// A command control always reads `false`.
    #[must_use]
    pub fn is_pressed(&self, i: usize) -> Option<bool> {
        self.pressed.get(i).copied()
    }

    /// The roving keyboard cursor.
    #[must_use]
    pub fn focus(&self) -> usize {
        self.focus
    }

    /// Move the roving cursor to control `i` (hover / AT focus). No-op
    /// when `i` is out of range.
    fn set_focus(&mut self, i: usize) {
        if i < self.count() {
            self.focus = i;
        }
    }

    /// Keyboard: move the roving cursor by one with wrap-around. No-op
    /// on an empty toolbar.
    fn nav(&mut self, forward: bool) {
        let n = self.count();
        if n == 0 {
            return;
        }
        self.focus = step(self.focus, forward, n);
    }

    /// Keyboard: jump the roving cursor to the first (`!last`) or last
    /// (`last`) control. No-op on an empty toolbar.
    fn edge(&mut self, last: bool) {
        let n = self.count();
        if n == 0 {
            return;
        }
        self.focus = if last { n - 1 } else { 0 };
    }

    /// Activate control `i`: focus it and return the activation
    /// outcome (a [`ToolActivation::Command`] for a command control, a
    /// [`ToolActivation::Toggle`] with the flipped bit for a toggle).
    /// Returns `None` when `i` is out of range. The caller (the
    /// [`ToolbarExternal`] adapter) emits the matching intent.
    fn activate(&mut self, i: usize) -> Option<ToolActivation> {
        let kind = self.kind(i)?;
        self.focus = i;
        match kind {
            ToolItem::Command => Some(ToolActivation::Command(i)),
            ToolItem::Toggle => {
                let next = !self.pressed[i];
                self.pressed[i] = next;
                Some(ToolActivation::Toggle(i, next))
            }
        }
    }
}

/// One-step wrap-around over `0..n` (`n > 0`). `forward` advances,
/// `!forward` retreats; both wrap at the ends.
fn step(current: usize, forward: bool, n: usize) -> usize {
    if forward {
        (current + 1) % n
    } else {
        (current + n - 1) % n
    }
}

impl Default for Toolbar {
    /// Default constructs an empty toolbar (no controls). Applications
    /// pass concrete `kinds` to [`Toolbar::new`]; `Default` exists for
    /// the `IntentEmitter<W: Default>` bound.
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

/// R692 §5.38 — `External` adapter wrapping a [`Toolbar`]. Surfaces the
/// toolbar to the §5.12 RPC plane and emits a `"command"` /  `"toggle"`
/// intent on control activation (see module docs for the payload
/// shapes).
pub struct ToolbarExternal {
    em: IntentEmitter<Toolbar>,
    /// R694 §5.39 — whether the toolbar group owns the shell
    /// [`FocusManager`](crate) focus, mirrored via
    /// [`External::on_focus_change`].
    /// The roving cursor ([`Self::focus`]) is always defined, but its
    /// focus ring should paint only while the strip is the focused
    /// widget — `view_toolbar`'s `group_focused` argument. Surfaced via
    /// the `focused` introspect slot so the binding's `read_state`
    /// sources it (the same channel hover / pressed already use), instead
    /// of the pre-R694 hard-coded `true`.
    group_focused: bool,
}

impl ToolbarExternal {
    /// Construct with the given control `kinds`.
    #[must_use]
    pub fn new(kinds: Vec<ToolItem>) -> Self {
        Self {
            em: IntentEmitter::new(Toolbar::new(kinds)),
            group_focused: false,
        }
    }

    /// Number of controls.
    #[must_use]
    pub fn count(&self) -> usize {
        self.em.inner.count()
    }

    /// Control class of control `i`, or `None` when out of range.
    #[must_use]
    pub fn kind(&self, i: usize) -> Option<ToolItem> {
        self.em.inner.kind(i)
    }

    /// Pressed bit of control `i`, or `None` when out of range.
    #[must_use]
    pub fn is_pressed(&self, i: usize) -> Option<bool> {
        self.em.inner.is_pressed(i)
    }

    /// The roving keyboard cursor.
    #[must_use]
    pub fn focus(&self) -> usize {
        self.em.inner.focus()
    }

    /// R694 §5.39 — whether the toolbar group currently owns the shell
    /// focus (set by the shell via
    /// [`External::on_focus_change`]).
    #[must_use]
    pub fn group_focused(&self) -> bool {
        self.group_focused
    }

    /// Drive a control pointer event (`i`, `event`). `PointerUp`
    /// focuses + activates control `i`; the other phases are no-ops
    /// (hover / pressed visuals are router-tracked, not stored here).
    fn send_item(&mut self, i: usize, event: PointerWireEvent) {
        match event {
            PointerWireEvent::Up => {
                if let Some(act) = self.em.inner.activate(i) {
                    self.emit_activation(act);
                }
            }
            PointerWireEvent::Enter
            | PointerWireEvent::Down
            | PointerWireEvent::Leave
            | PointerWireEvent::Cancel => {}
        }
    }

    /// Apply a W3C key name through the WAI-ARIA §3.4 toolbar keyboard
    /// model. Returns `true` when the key was handled, `false` for keys
    /// the toolbar ignores (e.g. `Tab`) so the shell swallow contract
    /// falls through to focus traversal.
    fn apply_key(&mut self, key: &str) -> bool {
        match key {
            "ArrowRight" => {
                self.em.inner.nav(true);
                true
            }
            "ArrowLeft" => {
                self.em.inner.nav(false);
                true
            }
            "Home" => {
                self.em.inner.edge(false);
                true
            }
            "End" => {
                self.em.inner.edge(true);
                true
            }
            "Enter" | "Space" => {
                let focus = self.em.inner.focus();
                if let Some(act) = self.em.inner.activate(focus) {
                    self.emit_activation(act);
                }
                true
            }
            _ => false,
        }
    }

    /// Push the §5.20 intent for an activation outcome.
    fn emit_activation(&mut self, act: ToolActivation) {
        let intent = match act {
            ToolActivation::Command(i) => {
                Intent::new_static("command", IntrospectValue::Text(format!("{i}")))
            }
            ToolActivation::Toggle(i, pressed) => {
                Intent::new_static("toggle", IntrospectValue::Text(format!("{i}.{pressed}")))
            }
        };
        self.em.push(intent);
    }

    /// Shared parse for the `send` wire payload `"<i>:<EventName>[:<mods>]"`
    /// via the [`parse_send_payload`](crate::composite_tag::parse_send_payload)
    /// `:` grammar SSOT (R880.1 — the hand-rolled `split_once` read a
    /// modifier-held release as the event name `"PointerUp:c"` and silently
    /// rejected the activation). Returns the roving cursor as the
    /// round-trip outcome.
    fn dispatch_send(&mut self, payload: &str) -> Result<IntrospectValue, InvokeError> {
        let (
            idx,
            crate::composite_tag::SendPayload {
                event: event_name, ..
            },
        ): (usize, _) = crate::composite_tag::require_parsed_send_payload("toolbar.send", payload)?;
        let event = PointerWireEvent::from_wire_name(event_name).ok_or_else(|| {
            InvokeError::rejected(format!(
                "toolbar.send: {event_name:?} is not a pointer event name"
            ))
        })?;
        self.send_item(idx, event);
        Ok(focus_value(self.focus()))
    }
}

/// Lower a roving-cursor index to `Int`.
fn focus_value(idx: usize) -> IntrospectValue {
    IntrospectValue::Int(i64::try_from(idx).expect("toolbar focus fits in i64"))
}

/// Decode a roving widget's `(focus, focused)` introspect posture — the roving
/// cursor index (`query("focus")`, an `Int`, default `0`) and whether the
/// widget owns the shell focus (`query("focused")`, a `Bool`). This is the
/// **toolbar peer** of [`read_selected`](crate::widgets::virtual_select::read_selected)
/// and [`read_open_state`](crate::widgets::context_menu::read_open_state): a
/// `WidgetCore::read_state` body projecting a `ToolbarExternal` (or any external
/// mirroring the same `focus` / `focused` slots — e.g. a binding-local
/// multi-select list) calls this instead of hand-decoding the two slots, so a
/// slot rename cannot silently break the (R990.1 — then three) binding
/// hand-decoders the way a hand-rolled `match query(...)` would.
#[must_use]
pub fn read_roving_focus(intro: &dyn ExternalIntrospect) -> (usize, bool) {
    let focus = match intro.query("focus") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).unwrap_or(0),
        _ => 0,
    };
    let focused = matches!(intro.query("focused"), Some(IntrospectValue::Bool(true)));
    (focus, focused)
}

impl Default for ToolbarExternal {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl core::fmt::Debug for ToolbarExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ToolbarExternal")
            .field("count", &self.count())
            .field("focus", &self.focus())
            .finish()
    }
}

impl External for ToolbarExternal {
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

    /// R694 §5.39 — mirror the shell focus posture so the roving cursor's
    /// focus ring paints only while the strip owns shell focus.
    fn on_focus_change(&mut self, focused: bool) {
        self.group_focused = focused;
    }
}

impl ExternalIntrospect for ToolbarExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("count", "int"),
                    SchemaField::new("focus", "int"),
                    SchemaField::new("focused", "bool"),
                    SchemaField::parametric(
                        "kind.<i>",
                        "string",
                        const { &[SchemaArg::index("i", "count")] },
                    ),
                    SchemaField::parametric(
                        "pressed.<i>",
                        "bool",
                        const { &[SchemaArg::index("i", "count")] },
                    ),
                    SchemaField::send("string"),
                    SchemaField::action("key", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.count()).expect("count fits in i64"),
            )),
            "focus" => Some(focus_value(self.focus())),
            // R694 §5.39 — group shell-focus posture for the ring gate.
            "focused" => Some(IntrospectValue::Bool(self.group_focused)),
            _ => {
                if let Some(suffix) = path.strip_prefix("kind.") {
                    let i: usize = suffix.parse().ok()?;
                    return Some(IntrospectValue::Text(kind_name(self.kind(i)?).to_string()));
                }
                let suffix = path.strip_prefix("pressed.")?;
                let i: usize = suffix.parse().ok()?;
                Some(IntrospectValue::Bool(self.is_pressed(i)?))
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "count" => Err(InterveneError::ReadOnly),
            "focus" => match value {
                IntrospectValue::Int(i) => {
                    let idx = resolve_index("item", i, self.count())?;
                    self.em.inner.set_focus(idx);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => {
                let suffix = path
                    .strip_prefix("pressed.")
                    .ok_or(InterveneError::UnknownPath)?;
                let i: usize = suffix.parse().map_err(|_| InterveneError::UnknownPath)?;
                // Programmatic pressed set (RPC restore / form default) —
                // no `"toggle"` intent fires. Only toggle controls carry
                // a pressed slot.
                match value {
                    IntrospectValue::Bool(b) => {
                        if self.kind(i) != Some(ToolItem::Toggle) {
                            return Err(InterveneError::out_of_range(format!(
                                "item {i} is a {:?}, and only a Toggle carries a \
                                 pressed slot",
                                self.kind(i)
                            )));
                        }
                        self.em.inner.pressed[i] = b;
                        Ok(())
                    }
                    _ => Err(InterveneError::TypeMismatch),
                }
            }
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Mouse: "<i>:<Event>". Returns the roving cursor.
            "send" => match args {
                IntrospectValue::Text(ref s) => self.dispatch_send(s),
                _ => Err(InvokeError::TypeMismatch),
            },
            // Keyboard: a W3C key name through the WAI-ARIA toolbar
            // model. Returns Bool(handled) for the swallow verdict.
            "key" => match args {
                IntrospectValue::Text(ref name) => Ok(IntrospectValue::Bool(self.apply_key(name))),
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// WAI-ARIA-flavoured name for a control class (the `query("kind.<i>")`
/// lowering).
fn kind_name(kind: ToolItem) -> &'static str {
    match kind {
        ToolItem::Command => "command",
        ToolItem::Toggle => "toggle",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::assert_out_of_range_saying;
    use crate::test_fixtures::assert_refused_saying;

    /// Bold(toggle) / Italic(toggle) / Underline(toggle) / Undo(cmd) /
    /// Redo(cmd) — the formatting-toolbar shape the `hello-toolbar`
    /// consumer renders.
    fn kinds() -> Vec<ToolItem> {
        vec![
            ToolItem::Toggle,
            ToolItem::Toggle,
            ToolItem::Toggle,
            ToolItem::Command,
            ToolItem::Command,
        ]
    }

    fn bar() -> Toolbar {
        Toolbar::new(kinds())
    }

    fn ext() -> ToolbarExternal {
        ToolbarExternal::new(kinds())
    }

    // ----- model: initial state -----

    #[test]
    fn new_toolbar_is_unpressed_focus_zero() {
        let t = bar();
        assert_eq!(t.count(), 5);
        assert_eq!(t.focus(), 0);
        assert_eq!(t.kind(0), Some(ToolItem::Toggle));
        assert_eq!(t.kind(3), Some(ToolItem::Command));
        assert_eq!(t.kind(5), None);
        for i in 0..5 {
            assert_eq!(t.is_pressed(i), Some(false));
        }
        assert_eq!(t.is_pressed(9), None);
    }

    // ----- model: roving cursor -----

    #[test]
    fn nav_wraps_both_directions() {
        let mut t = bar();
        t.nav(true);
        assert_eq!(t.focus(), 1);
        for _ in 0..4 {
            t.nav(true);
        }
        assert_eq!(t.focus(), 0, "ArrowRight wraps 4 -> 0");
        t.nav(false);
        assert_eq!(t.focus(), 4, "ArrowLeft wraps 0 -> 4");
    }

    #[test]
    fn edge_jumps_first_and_last() {
        let mut t = bar();
        t.edge(true);
        assert_eq!(t.focus(), 4);
        t.edge(false);
        assert_eq!(t.focus(), 0);
    }

    #[test]
    fn nav_on_empty_toolbar_is_noop() {
        let mut t = Toolbar::default();
        t.nav(true);
        t.edge(true);
        assert_eq!(t.focus(), 0);
        assert_eq!(t.count(), 0);
    }

    #[test]
    fn set_focus_out_of_range_is_noop() {
        let mut t = bar();
        t.set_focus(9);
        assert_eq!(t.focus(), 0);
        t.set_focus(2);
        assert_eq!(t.focus(), 2);
    }

    // ----- model: activate -----

    #[test]
    fn activate_command_returns_command_focuses() {
        let mut t = bar();
        assert_eq!(t.activate(3), Some(ToolActivation::Command(3)));
        assert_eq!(t.focus(), 3, "activation focuses the control");
        assert_eq!(t.is_pressed(3), Some(false), "command has no pressed bit");
    }

    #[test]
    fn activate_toggle_flips_and_returns_state() {
        let mut t = bar();
        assert_eq!(t.activate(0), Some(ToolActivation::Toggle(0, true)));
        assert_eq!(t.is_pressed(0), Some(true));
        assert_eq!(t.activate(0), Some(ToolActivation::Toggle(0, false)));
        assert_eq!(t.is_pressed(0), Some(false), "re-activate flips back");
    }

    #[test]
    fn activate_out_of_range_returns_none() {
        let mut t = bar();
        assert_eq!(t.activate(9), None);
    }

    // ----- External: query / intervene -----

    #[test]
    fn external_query_initial() {
        let e = ext();
        assert_eq!(e.query("count"), Some(IntrospectValue::Int(5)));
        assert_eq!(e.query("focus"), Some(IntrospectValue::Int(0)));
        assert_eq!(
            e.query("kind.0"),
            Some(IntrospectValue::Text("toggle".to_string()))
        );
        assert_eq!(
            e.query("kind.3"),
            Some(IntrospectValue::Text("command".to_string()))
        );
        assert_eq!(e.query("pressed.0"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("kind.9"), None);
        assert_eq!(e.query("pressed.9"), None);
        assert_eq!(e.query("nope"), None);
    }

    #[test]
    fn external_intervene_focus() {
        let mut e = ext();
        e.intervene("focus", IntrospectValue::Int(2)).unwrap();
        assert_eq!(e.focus(), 2);
        assert!(!e.is_dirty(), "intervene fires no intent");
        assert_out_of_range_saying(
            &e.intervene("focus", IntrospectValue::Int(9)),
            "no item 9 here",
        );
        assert_out_of_range_saying(
            &e.intervene("focus", IntrospectValue::Int(-1)),
            "-1 is not a item index",
        );
    }

    #[test]
    fn external_intervene_pressed_toggle_only() {
        let mut e = ext();
        e.intervene("pressed.1", IntrospectValue::Bool(true))
            .unwrap();
        assert_eq!(e.is_pressed(1), Some(true));
        assert!(!e.is_dirty(), "programmatic pressed fires no toggle intent");
        // A command control has no pressed slot.
        // R1565 — "index out of range" and "that control has no pressed slot"
        // were one value; the comment above was the only thing distinguishing.
        assert_out_of_range_saying(
            &e.intervene("pressed.3", IntrospectValue::Bool(true)),
            "only a Toggle carries a pressed slot",
        );
    }

    #[test]
    fn external_intervene_count_read_only() {
        let mut e = ext();
        assert_eq!(
            e.intervene("count", IntrospectValue::Int(2)),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn external_intervene_unknown_path() {
        let mut e = ext();
        assert_eq!(
            e.intervene("nope", IntrospectValue::Int(0)),
            Err(InterveneError::UnknownPath)
        );
    }

    #[test]
    fn external_intervene_wrong_variant_is_type_mismatch() {
        let mut e = ext();
        assert_eq!(
            e.intervene("focus", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch)
        );
    }

    // ----- External: send (mouse) -----

    fn click(e: &mut ToolbarExternal, i: usize) {
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            e.invoke("send", IntrospectValue::Text(format!("{i}:{ev}")))
                .unwrap();
        }
    }

    #[test]
    fn external_send_command_click_emits_command() {
        let mut e = ext();
        click(&mut e, 4);
        assert_eq!(e.focus(), 4, "click focuses the control");
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tag_str(), "command");
        assert_eq!(got[0].payload, IntrospectValue::Text("4".to_string()));
    }

    #[test]
    fn external_send_toggle_click_flips_and_emits_toggle() {
        let mut e = ext();
        click(&mut e, 0);
        assert_eq!(e.is_pressed(0), Some(true));
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tag_str(), "toggle");
        assert_eq!(got[0].payload, IntrospectValue::Text("0.true".to_string()));
        // Re-click flips back to false.
        click(&mut e, 0);
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got[0].payload, IntrospectValue::Text("0.false".to_string()));
    }

    #[test]
    fn external_send_enter_leave_no_activation() {
        let mut e = ext();
        e.invoke("send", IntrospectValue::Text("0:PointerEnter".to_string()))
            .unwrap();
        e.invoke("send", IntrospectValue::Text("0:PointerLeave".to_string()))
            .unwrap();
        assert!(!e.is_dirty(), "hover does not activate");
        assert_eq!(e.is_pressed(0), Some(false));
    }

    #[test]
    fn external_send_returns_focus_index() {
        let mut e = ext();
        let out = e
            .invoke("send", IntrospectValue::Text("2:PointerUp".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Int(2));
    }

    #[test]
    fn r880_1_send_with_modifier_segment_still_activates() {
        // The router emits "<i>:PointerUp:<token>" when modifiers are held
        // (R781); the pre-R880.1 hand-rolled split read "PointerUp:c" as
        // the event name and silently rejected a Ctrl+click.
        let mut e = ext();
        let out = e
            .invoke("send", IntrospectValue::Text("2:PointerUp:c".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Int(2));
    }

    #[test]
    fn external_send_malformed_rejected() {
        let mut e = ext();
        assert_refused_saying(
            &e.invoke("send", IntrospectValue::Text("no_colon".to_string())),
            "malformed send payload \"no_colon\"",
        );
        assert_refused_saying(
            &e.invoke("send", IntrospectValue::Text("0:Teleport".to_string())),
            "\"Teleport\" is not a pointer event name",
        );
        // R1564 — the three refusals this test drives were indistinguishable
        // before the reason existed; a malformed payload and an unaddressable
        // target now say different things.
        assert_refused_saying(
            &e.invoke("send", IntrospectValue::Text("A:PointerUp".to_string())),
            "send key \"A\" is not a valid target",
        );
    }

    // ----- External: key (keyboard) -----

    fn key(e: &mut ToolbarExternal, name: &str) -> bool {
        match e
            .invoke("key", IntrospectValue::Text(name.to_string()))
            .unwrap()
        {
            IntrospectValue::Bool(b) => b,
            other => panic!("key invoke must return Bool, got {other:?}"),
        }
    }

    #[test]
    fn external_key_arrows_move_roving_cursor() {
        let mut e = ext();
        assert!(key(&mut e, "ArrowRight"));
        assert_eq!(e.focus(), 1);
        assert!(key(&mut e, "ArrowLeft"));
        assert_eq!(e.focus(), 0);
        assert!(key(&mut e, "ArrowLeft"));
        assert_eq!(e.focus(), 4, "ArrowLeft wraps 0 -> 4");
    }

    #[test]
    fn external_key_home_end() {
        let mut e = ext();
        assert!(key(&mut e, "End"));
        assert_eq!(e.focus(), 4);
        assert!(key(&mut e, "Home"));
        assert_eq!(e.focus(), 0);
    }

    #[test]
    fn external_key_enter_activates_focused_toggle() {
        let mut e = ext();
        key(&mut e, "ArrowRight"); // focus 1 (toggle)
        assert!(key(&mut e, "Enter"));
        assert_eq!(e.is_pressed(1), Some(true));
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tag_str(), "toggle");
        assert_eq!(got[0].payload, IntrospectValue::Text("1.true".to_string()));
    }

    #[test]
    fn external_key_space_activates_focused_command() {
        let mut e = ext();
        key(&mut e, "End"); // focus 4 (command)
        assert!(key(&mut e, "Space"));
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got[0].tag_str(), "command");
        assert_eq!(got[0].payload, IntrospectValue::Text("4".to_string()));
    }

    #[test]
    fn external_key_tab_unhandled() {
        let mut e = ext();
        assert!(!key(&mut e, "Tab"), "Tab falls through to focus traversal");
    }

    #[test]
    fn external_invoke_unknown_path() {
        let mut e = ext();
        assert_eq!(
            e.invoke("nope", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
    }

    #[test]
    fn external_schema_declares_slots() {
        let e = ext();
        assert_eq!(
            e.schema().fields,
            &[
                SchemaField::new("count", "int"),
                SchemaField::new("focus", "int"),
                // R694 §5.39 — group shell-focus posture for the ring gate.
                SchemaField::new("focused", "bool"),
                SchemaField::parametric(
                    "kind.<i>",
                    "string",
                    const { &[SchemaArg::index("i", "count")] }
                ),
                SchemaField::parametric(
                    "pressed.<i>",
                    "bool",
                    const { &[SchemaArg::index("i", "count")] }
                ),
                SchemaField::send("string"),
                SchemaField::action("key", "string"),
            ]
        );
    }

    #[test]
    fn r694_group_focus_posture_mirrors_on_focus_change() {
        use crate::external::External;
        let mut e = ext();
        assert!(!e.group_focused(), "toolbar boots without group focus");
        assert_eq!(e.query("focused"), Some(IntrospectValue::Bool(false)));
        e.on_focus_change(true);
        assert!(e.group_focused());
        assert_eq!(
            e.query("focused"),
            Some(IntrospectValue::Bool(true)),
            "group focus surfaces on the introspect slot for the ring gate",
        );
        e.on_focus_change(false);
        assert_eq!(e.query("focused"), Some(IntrospectValue::Bool(false)));
    }

    #[test]
    fn r990_1_read_roving_focus_decodes_both_slots() {
        use crate::external::External;
        let mut e = ext();
        // Boot: cursor at 0, not group-focused.
        assert_eq!(read_roving_focus(&e), (0, false));
        e.em.inner.set_focus(2);
        e.on_focus_change(true);
        assert_eq!(
            read_roving_focus(&e),
            (2, true),
            "decodes the moved cursor + group focus"
        );
        e.on_focus_change(false);
        assert_eq!(read_roving_focus(&e), (2, false));
    }
}
