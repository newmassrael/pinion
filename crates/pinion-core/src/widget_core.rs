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

use crate::command::Command;
use crate::external::External;
use crate::intent::Intent;
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

    /// R51.166 §5.23 R27 — Update reducer mapping a wire-form
    /// [`Intent`] (§5.20) to a `Vec<Command>` (§5.23) for async
    /// handler dispatch.
    ///
    /// The framework's §5.23 R27 contract is
    /// `Update(&mut Model, Intent) -> Vec<Command<Intent>>`: a pure
    /// reducer that mutates the application-side `Model`/state and
    /// returns a declarative list of IO/async work for the framework
    /// (or registered `Handler`) to execute. Commands are *described*
    /// here and *executed* outside reducer purity — preserving the
    /// §6.3 `dry_run` invariant (the reducer is replayable, only the
    /// `Command` dispatch is the side-effecting boundary).
    ///
    /// ## Wiring path (R51.166-170)
    ///
    /// - **R51.166** (this round) — substrate only: trait method
    ///   added with a default `Vec::new()` no-op so every existing
    ///   `impl WidgetCore for X` keeps compiling unchanged; the
    ///   `[[substrate-incompleteness-signal]]` is avoided by
    ///   defaulting (no caller forced into boilerplate).
    /// - **R51.167** (carry) — `CoreShell::dispatch_intent` routes
    ///   every incoming [`Intent`] through `<V as WidgetCore>::update`
    ///   before forwarding the original intent to the SCXML
    ///   `invoke("send", …)` channel; produced `Vec<Command>` is
    ///   queued onto the active [`Owner`](crate::reactive::Owner) via
    ///   `dispatch_command`.
    /// - **R51.168** (carry) — `Intent.payload` typed routing through
    ///   the SCXML invoke send (currently the tag-only path drops the
    ///   payload — see direct-session carry "Intent.payload SCXML
    ///   send 누락").
    /// - **R51.169** (carry) — `hello-commands(-tui)` migrate from
    ///   the R51.163 `Owner::cache` one-shot hack to the real
    ///   reducer-driven Command flow.
    /// - **R51.170** (carry) — Forge codegen emits the `update` body
    ///   from SCE schema `effect` + `command` tables.
    ///
    /// ## Why `&mut Self::State` here, not `&mut Model`?
    ///
    /// `WidgetCore::State` IS the widget's slice of the application
    /// model: it's the cached projection [`Self::read_state`] extracts
    /// from the live `Scene`, and `Copy + Debug + PartialEq` already
    /// constrain it to a value-typed snapshot. Reducer mutation
    /// happens *here* on the cached state, and the framework persists
    /// it back to the `Scene::External` via the existing intervene
    /// channel on the next paint cycle (R51.167 carry wires this).
    ///
    /// ## Why borrow [`Intent`], not consume?
    ///
    /// `Intent` is `Clone` and reducers commonly want to log /
    /// inspect the intent without consuming it; the framework owns
    /// the wire-form value (it just popped it off the §5.20 drain).
    /// A `&Intent` parameter keeps the framework's copy authoritative
    /// while letting the reducer match on `intent.tag_str()` and
    /// `&intent.payload` freely.
    ///
    /// ## Default impl
    ///
    /// Returns an empty `Vec<Command>` — widgets without async/IO
    /// side-effects (the entire current example catalogue except
    /// `hello-commands(-tui)`) need no override. The reducer-driven
    /// Command flow opts in per widget binding.
    #[must_use]
    fn update(_state: &mut Self::State, _intent: &Intent) -> Vec<Command> {
        Vec::new()
    }
}

#[cfg(test)]
mod r51_166_tests {
    //! R51.166 §5.23 R27 — `WidgetCore::update` reducer substrate
    //! contract tests. Verifies the default no-op shape on the
    //! existing [`crate::test_fixtures::ButtonFixture`] (which carries
    //! no override) and exercises a custom reducer fixture that
    //! mutates state + emits `Vec<Command>` on intent receipt.
    //!
    //! `CoreShell` integration is the R51.167 carry — these tests
    //! pin the trait-side contract only.
    use super::WidgetCore;
    use crate::command::Command;
    use crate::external::{External, IntrospectValue};
    use crate::intent::Intent;
    use crate::scene::Scene;
    use crate::test_fixtures::ButtonFixture;
    use crate::widgets::button::ButtonState;
    use crate::Frame;

    #[test]
    fn default_update_returns_empty_vec() {
        let mut state = ButtonState::Idle;
        let intent = Intent::new_static("test_btn.click", IntrospectValue::Null);
        let commands = <ButtonFixture as WidgetCore>::update(&mut state, &intent);
        assert!(commands.is_empty());
        // Default impl must NOT mutate caller-provided state — the
        // §6.3 `dry_run` invariant relies on the reducer being a
        // pure identity when overridden.
        assert_eq!(state, ButtonState::Idle);
    }

    struct EchoReducerFixture;

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct CounterState(u32);

    #[derive(Debug, Clone, Copy)]
    struct CounterEvent;

    impl WidgetCore for EchoReducerFixture {
        type State = CounterState;
        type Event = CounterEvent;

        fn create_external() -> Box<dyn External> {
            unreachable!("R51.166 reducer test fixture does not exercise paint")
        }

        fn tag() -> &'static str {
            "echo_reducer"
        }

        fn read_state(_: &Scene) -> Self::State {
            CounterState(0)
        }

        fn view(_: Self::State, _: &Frame) -> Scene {
            unreachable!("R51.166 reducer test fixture does not exercise paint")
        }

        fn event_name(_: Self::Event) -> &'static str {
            "__internal__"
        }

        fn title() -> &'static str {
            "EchoReducer"
        }

        fn update(state: &mut Self::State, intent: &Intent) -> Vec<Command> {
            state.0 = state.0.saturating_add(1);
            vec![Command::new_static(
                "echo.reply",
                IntrospectValue::Text(intent.tag_str().to_string()),
                u64::from(state.0),
            )]
        }
    }

    #[test]
    fn custom_update_mutates_state_and_emits_command() {
        let mut state = CounterState(0);
        let intent = Intent::new_static("echo_reducer.tick", IntrospectValue::Null);
        let commands = <EchoReducerFixture as WidgetCore>::update(&mut state, &intent);
        assert_eq!(state, CounterState(1));
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind_str(), "echo.reply");
        assert_eq!(
            commands[0].payload,
            IntrospectValue::Text("echo_reducer.tick".to_string())
        );
        assert_eq!(commands[0].scope_id, 1);
    }

    #[test]
    fn update_borrows_intent_without_consuming() {
        // Reducers commonly want to inspect / log the intent without
        // consuming it; the framework owns the wire-form value. The
        // borrow signature lets the same intent feed multiple
        // observers on the dispatch path (R51.167 routes it to the
        // SCXML send channel AFTER the reducer runs).
        let mut state_a = CounterState(0);
        let mut state_b = CounterState(10);
        let intent = Intent::new_static("echo_reducer.shared", IntrospectValue::Null);
        let cmds_a = <EchoReducerFixture as WidgetCore>::update(&mut state_a, &intent);
        let cmds_b = <EchoReducerFixture as WidgetCore>::update(&mut state_b, &intent);
        assert_eq!(state_a, CounterState(1));
        assert_eq!(state_b, CounterState(11));
        assert_eq!(cmds_a[0].scope_id, 1);
        assert_eq!(cmds_b[0].scope_id, 11);
        // Intent is still usable after both calls — confirms borrow,
        // not move.
        assert_eq!(intent.tag_str(), "echo_reducer.shared");
    }
}
