//! R51.121 §5.41 — `WidgetCore` backend-free widget binding trait.
//!
//! Carries the application-side widget binding contract that does NOT
//! depend on the renderer choice or window-size unit. Subtraits in
//! downstream crates add the backend-specific surface:
//!
//! | Trait | Crate | Adds |
//! |---|---|---|
//! | [`WidgetCore`] | `pinion-core` | state / event / view-fn / input / title / keyboard / focusable tags / log format |
//! | `WidgetA11y` | `pinion-a11y` | `access_node` / `access_focus_target` / `access_child_invoke` (a11y semantic tree) |
//! | `WidgetView` | `pinion-shell` | `type Renderer: VelloRenderer` + `initial_size() -> (u32, u32)` (logical pixels) |
//! | `WidgetViewTui` | `pinion-tui` | `type Renderer: WidgetRenderer<Frame=Buffer, Context=TuiContext>` + `initial_size() -> (u16, u16)` (cells) |
//!
//! ## Why a supertrait split, not a single backend-generic trait
//!
//! The [[substrate-incompleteness-signal]] surfaced by R51.113
//! (`hello-toggle-tui` as the 2nd TUI binding) made the duplication
//! between `pinion_shell::WidgetView` and `pinion_tui::WidgetViewTui`
//! impossible to ignore — every binding declared the same 9 methods
//! (state / event / `create_external` / tag / `read_state` / view /
//! `event_name` / title / keyboard) twice across the two backends, and
//! the a11y trio (`access_node` / `access_focus_target` /
//! `access_child_invoke`) was already pinion-a11y-typed on both sides.
//!
//! The textbook ISP (Interface Segregation Principle) end-state is a
//! supertrait chain where each trait carries exactly the surface its
//! concrete clients need:
//!
//! - `WidgetCore` lives at the framework root (pinion-core) so any
//!   future backend can `impl WidgetCore for X` first, then layer the
//!   backend-specific renderer trait on top.
//! - `WidgetA11y` lives in pinion-a11y because its return types
//!   (`AccessNode` / `AccessFocus` / `AccessAction`) depend on
//!   pinion-a11y's stable wrapper around `accesskit`.
//! - The two backend traits (`WidgetView` / `WidgetViewTui`) reduce to
//!   "renderer + initial size unit" because every other binding
//!   method already lives upstream.
//!
//! The alternative — a single `WidgetView<R: WidgetRenderer>` generic
//! trait — would require either (a) folding both initial-size units
//! into one method (loses the textbook "cells vs pixels" semantic
//! split) or (b) parameterising on a unit type as well (over-fitted
//! generics for one method). The supertrait split keeps each backend's
//! window-sizing primitive in the language its consumers actually use.
//!
//! ## §6.3 view-fn purity preserved
//!
//! [`WidgetCore::view`] is sync and pure (same `(state, frame)` always
//! yields the same `Scene`), preserving the §6.3 R51.27 `dry_run`
//! invariant across both backends — the supertrait split moves where
//! the trait surface lives, never what it guarantees.

use crate::external::External;
use crate::{Frame, Scene};

/// R51.121 §5.41 — backend-free widget binding contract.
///
/// Every application-side widget binding (`HelloButton`,
/// `HelloToggleTui`, etc.) carries an `impl WidgetCore for X` block
/// supplying the backend-agnostic surface; the backend trait
/// (`pinion_shell::WidgetView` for Vello GUI, `pinion_tui::WidgetViewTui`
/// for ratatui TUI) supplies the renderer + initial-size pair on top.
///
/// All methods are *associated functions* (no `&self`) because each
/// `impl WidgetCore for X` lives on a unit type and the trait is used
/// purely for namespacing: `<HelloButton as WidgetCore>::view(state,
/// &frame)`. Default impls make the trait shape conservative — bindings
/// without keyboard affordances or composite focus enumerate exactly
/// the required methods.
pub trait WidgetCore: 'static {
    /// Cached projection of the live state scene. `Copy` so the shell
    /// can clone it into the paint closure without lifetime
    /// gymnastics; `Debug` + `PartialEq` for the transition log +
    /// change-detection redraw request.
    type State: Copy + core::fmt::Debug + PartialEq;

    /// Typed widget event enum — usually the SCXML-emitted
    /// `<Widget>Event` (e.g. `ButtonEvent`, `ToggleEvent`). Threaded
    /// through [`Self::event_name`] before reaching the §5.15
    /// `invoke("send", Text(<name>))` channel so the application keeps
    /// typed event payloads without giving up the symbolic RPC
    /// contract.
    type Event: Copy;

    /// Build a fresh state scene root. Called once at shell boot —
    /// should return `Scene::External(ExternalNode::new(<my widget>)
    /// .with_tag(Self::tag()))` so the input router's hit-test on the
    /// paint-side tag routes to this node.
    fn create_external() -> Box<dyn External>;

    /// Stable identifier matching the paint-side `Container::tag` the
    /// view fn attaches to the interactive surface. The input router
    /// forwards pointer / key events to any `Scene::External` in the
    /// state scene whose tag equals this hit-test target.
    fn tag() -> &'static str;

    /// Extract the cached projection from the live state scene via
    /// the §5.15 introspect channel — same path an RPC
    /// `scene/query /external/<slot>` request uses, so the cached
    /// state and the AI client always see the same value.
    fn read_state(scene: &Scene) -> Self::State;

    /// Build the paint scene for the current cached state. Pure sync
    /// per §6.3 R51.27 `dry_run` invariant: same `(state, frame)`
    /// always yields the same `Scene`. The shell calls the layout
    /// pass on the result before handing it to the backend paint
    /// adapter, so the view fn need not (and should not) resolve
    /// pixel rects.
    fn view(state: Self::State, frame: &Frame) -> Scene;

    /// Convert a typed widget event into the symbolic name the §5.15
    /// `invoke("send", IntrospectValue::Text(<name>))` channel
    /// expects. SCXML-internal variants that never come from input
    /// should route through a wildcard with a sentinel name the
    /// parser rejects (mirrors the `ButtonEvent::__internal__`
    /// precedent).
    fn event_name(event: Self::Event) -> &'static str;

    /// Window / terminal title displayed by the OS. Static because
    /// neither winit nor crossterm takes ownership of a `String` at
    /// the title-set call.
    fn title() -> &'static str;

    /// Optional keyboard event mapping. The shell consults this on
    /// every key press whose W3C `KeyboardEvent.key` string the input
    /// bridge can produce; `None` means "no keybinding for this key"
    /// and the shell falls through to [`Self::apply_key`]. `Esc` /
    /// `Tab` / `Shift+Tab` are shell-reserved and never reach this
    /// hook.
    ///
    /// Default returns `None` for every key — widgets without
    /// keyboard affordances need no override.
    #[must_use]
    fn keybinding(_key: &str) -> Option<Self::Event> {
        None
    }

    /// Escape hatch for keyboard affordances that the enum-typed
    /// [`Self::keybinding`] channel cannot express. The shell
    /// consults this AFTER `keybinding` returns `None` for character
    /// keys, and as the *only* hook for non-character named keys
    /// (`ArrowLeft`, `ArrowRight`, `ArrowUp`, `ArrowDown`, `Home`,
    /// `End`, `PageUp`, `PageDown`, `Enter`, `Space`). `Escape` and
    /// `Tab` / `Shift+Tab` are shell-reserved — `Escape` quits the
    /// window, `Tab` advances the focus manager, neither reaches
    /// this hook.
    ///
    /// `focused` carries the focus manager's currently-focused tag
    /// at dispatch time. Widgets that match against `focused` route
    /// keys only when their own tag is focused; the broadcast model
    /// (every keypress fires every widget's `apply_key`) caused
    /// aliasing with multiple focusable widgets on screen.
    ///
    /// Implementations receive the authoritative state scene `&mut`
    /// and may walk it to the matching `Scene::External` to call
    /// [`ExternalIntrospect::intervene`](crate::external::ExternalIntrospect::intervene)
    /// — the same side door the RPC `scene/intervene` route uses.
    ///
    /// Returns `true` if the key was handled (the shell bumps the
    /// §5.34 revision, re-reads state, drains intents, and repaints
    /// on visible change). Returns `false` to defer to whatever
    /// fallback the shell adds next.
    ///
    /// Default returns `false` for every key — widgets without
    /// keyboard affordances beyond `keybinding` need no override.
    #[must_use]
    fn apply_key(_scene: &mut Scene, _focused: Option<&str>, _key: &str) -> bool {
        false
    }

    /// Focusable tag enumeration in Tab order. Returned tags must
    /// match either [`Self::tag`] (the top-level widget) or a sub-tag
    /// the view fn paints inside the widget (composite widgets like
    /// `RadioGroup` register the group's `tag()` as a single tab
    /// stop and roving-tabindex among its children internally).
    ///
    /// Default returns a single-entry list containing `Self::tag()`,
    /// which is the right shape for every single-widget example.
    /// Composite widget bindings or multi-widget views override to
    /// enumerate all focusable children.
    #[must_use]
    fn focusable_tags() -> Vec<&'static str> {
        vec![Self::tag()]
    }

    /// Format the cached state for stderr logging on the transition
    /// path (`from -> to`) and the final-state line. Default falls
    /// back to `Debug`; widgets with composite state can format a
    /// human-readable view (e.g. `Toggle::fmt_state_log` may render
    /// `"Idle / Off"`).
    fn fmt_state_log(state: &Self::State) -> String {
        format!("{state:?}")
    }
}
