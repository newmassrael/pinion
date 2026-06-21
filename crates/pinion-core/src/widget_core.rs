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

use std::borrow::Cow;

use crate::command::Command;
use crate::external::{External, IntrospectValue};
use crate::intent::Intent;
use crate::{Frame, Scene};

/// (R55.D.5 §5.45) Sibling [`External`] slot registered alongside the
/// primary widget by [`WidgetCore::create_extra_externals`].
///
/// The substrate composes the state scene as
/// `Scene::Container([primary, ...extras])` when the extras list is
/// non-empty (`Scene::External(primary)` stays the shape when empty,
/// preserving every existing single-widget binding bit-for-bit). The
/// input router's existing depth-first walk over the state scene
/// already handles a `Container` of `External` children — so the
/// substrate change is one composition step in
/// [`CoreShell::new`](../../pinion_runtime/struct.CoreShell.html#method.new),
/// not a router rewrite.
///
/// First consumer: `examples/hello-listbox` registers a
/// [`ScrollBarExternal`](crate::widgets::scrollbar::ScrollBarExternal)
/// tagged `"main_list_scrollbar"` with the same `Rc<ScrollState>` the
/// main scroll node uses, so a drag on the visible thumb (R55.D.4
/// paint peer) routes through the existing pointer-capture + capture-
/// lock `External::pointer_move` path without per-widget substrate
/// gymnastics.
pub struct ExtraExternal {
    /// Symbolic identifier — must match the `Container::tag` the view
    /// fn attaches to this widget's paint surface so the input
    /// router's hit-test routes to the same node.
    ///
    /// (R688 §5.16) `Cow<'static, str>` so a binding can register an
    /// external under a **runtime-generated** tag (e.g. a dock
    /// reorganize mints `reorg-split-{n}`) without leaking a
    /// `&'static str`. Static literals stay `Cow::Borrowed` (no
    /// allocation); runtime ids are `Cow::Owned`. The reactive
    /// reconcile path
    /// ([`CoreShell::reconcile_externals`](../../pinion_runtime/struct.CoreShell.html#method.reconcile_externals))
    /// keys the external set by this tag.
    pub tag: Cow<'static, str>,
    /// The widget handle. Boxed for the closed-form `External` trait
    /// object slot the substrate stores on `ExternalNode`.
    pub handle: Box<dyn External>,
}

impl ExtraExternal {
    /// Constructor that takes ownership of `handle` and pairs it with
    /// `tag`. Equivalent to the struct-literal form; matches the
    /// `Intent::new_static` / `Command::new_static` convention so the
    /// per-widget binding site reads idiomatically.
    ///
    /// (R688 §5.16) `tag` accepts `impl Into<Cow<'static, str>>` —
    /// a `&'static str` literal or an owned `String` runtime id.
    #[must_use]
    pub fn new(tag: impl Into<Cow<'static, str>>, handle: Box<dyn External>) -> Self {
        Self {
            tag: tag.into(),
            handle,
        }
    }
}

impl core::fmt::Debug for ExtraExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExtraExternal")
            .field("tag", &self.tag)
            .finish_non_exhaustive()
    }
}

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

    /// (R55.D.5 §5.45) Sibling [`External`]s registered alongside the
    /// widget produced by [`Self::create_external`].
    ///
    /// Default empty — every single-External binding (the entire
    /// example catalogue except hello-listbox, which lands the R55.D.5
    /// first-consumer slot) needs no override. When non-empty, the
    /// substrate wraps the state scene root in a [`Scene::Container`]
    /// holding `[primary, ...extras]` in declaration order; the
    /// existing input-router depth-first walk over the state scene
    /// dispatches by tag without further changes.
    ///
    /// Implementations that wire shared reactive state (e.g. attach
    /// the same `Rc<ScrollState>` to both the main scroll node and a
    /// sibling [`ScrollBarExternal`](crate::widgets::scrollbar::ScrollBarExternal))
    /// rely on the substrate calling this inside
    /// [`Owner::run`](crate::reactive::Owner::run) for the root owner
    /// — so [`use_scroll_state`](crate::widgets::scroll::use_scroll_state)
    /// and other [`Owner::cache`](crate::reactive::Owner::cache)
    /// helpers resolve to the same instance the view fn will later
    /// resolve from the same cache key.
    ///
    /// Applications that override this method MUST update
    /// [`Self::read_state`] to call
    /// [`Scene::find_external_with_tag`] instead of pattern-matching
    /// `Scene::External` directly, since the state scene shape is now
    /// `Scene::Container` rather than `Scene::External`.
    #[must_use]
    fn create_extra_externals() -> Vec<ExtraExternal> {
        Vec::new()
    }

    /// (R689 §5.16 §5.35) Whether [`Self::create_extra_externals`] can
    /// return a **different tag set** over the binding's lifetime.
    ///
    /// Default `false` — the external set is frozen at boot. The
    /// overwhelming majority of bindings declare their extras once (a
    /// fixed list of scrollbars / toggles / composite-tag routers) and
    /// never add or remove a routable surface afterwards. For those the
    /// substrate skips
    /// [`CoreShell::reconcile_externals`](../../pinion_runtime/struct.CoreShell.html#method.reconcile_externals)
    /// entirely: no per-frame factory re-run, no throwaway
    /// [`External`] allocation, and — importantly — no re-execution of
    /// the factory's boot-time seeding side effects (a factory that
    /// calls `intervene("value", …)` or `use_theme(...).set_mode(...)`
    /// to seed first-paint state must run exactly once, at
    /// [`CoreShell::new`](../../pinion_runtime/struct.CoreShell.html#method.new)).
    ///
    /// Override to `true` **only** when the tag set is a projection of
    /// runtime-mutable reactive state — e.g. a dock editor whose
    /// `create_extra_externals` walks a `Signal<DockTopology>` and mints
    /// a `SplitterExternal` per split, so a reorganize gesture that
    /// spawns `reorg-split-{n}` must register a routable
    /// [`External`] for the new surface mid-session. Such a binding
    /// pays the reactive reconcile cost (re-run + tag diff each frame)
    /// because that cost is intrinsic to having a dynamic surface set;
    /// a static binding must not subsidise it.
    ///
    /// Contract: forgetting to return `true` from a genuinely dynamic
    /// binding leaves new surfaces painted-but-inert (no router target)
    /// — the exact R688 motivating symptom, caught immediately in
    /// interaction testing.
    #[must_use]
    fn external_set_is_dynamic() -> bool {
        false
    }

    /// Stable identifier matching the paint-side `Container::tag` the
    /// view fn attaches to the interactive surface. The input router
    /// forwards pointer / key events to any `Scene::External` in the
    /// state scene whose tag equals this hit-test target.
    ///
    /// R55.G.17 §5.49 — composite paint-root tag convention. For AI-
    /// side `scene/click` / `scene/key` / `scene/wheel`
    /// `{path: V::tag()}` routing and `rect_for_tag` AT bounds attach
    /// to resolve, the paint scene returned by [`Self::view`] must
    /// contain at least one node tagged `V::tag()` somewhere. Pin
    /// the convention per widget with
    /// `assert!(V::view(state, &frame).contains_tag(V::tag()))` —
    /// see [`Scene::contains_tag`] for the depth-first walker
    /// primitive and `examples/hello-listbox/src/main.rs`
    /// `r55_g17_view_contains_composite_paint_root_tag` for the
    /// reference regression test.
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
    ///
    /// R55.G.17 §5.49 — composite paint-root tag convention. The
    /// returned scene must contain a node tagged [`Self::tag`]
    /// somewhere (typically the outermost interactive container or a
    /// transparent wrapper around it). Without this, AI-side
    /// `scene/click` / `scene/key` / `scene/wheel`
    /// `{path: V::tag()}` routing and `rect_for_tag` AT bounds
    /// attach both fail silently. See [`Scene::contains_tag`] for the
    /// regression-test primitive and the `r55_g20_*` test family
    /// across `examples/hello-*/src/main.rs` for the per-widget
    /// pinning pattern.
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
    /// `modifiers` carries the W3C `KeyboardEvent` four-bit modifier
    /// surface (`shiftKey` / `ctrlKey` / `altKey` / `metaKey`) at
    /// dispatch time (R56.1.f.0 §5.13). Widgets with modifier-aware
    /// keyboard affordances — `TextField` Shift+Arrow selection
    /// extension, future Ctrl+A select-all, Ctrl+C / Ctrl+V clipboard
    /// — branch on the bits; widgets without modifier semantics
    /// ignore the parameter (`_modifiers` is the canonical no-op).
    /// Shell-reserved modifier shortcuts (`Shift+Tab` reverse focus,
    /// `Ctrl+Q` / platform-quit) are consumed upstream and do not
    /// reach this hook.
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
    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: crate::input::Modifiers,
    ) -> bool {
        false
    }

    /// R772.1 §5.38 — the canonical [`Self::apply_key`] body for a binding
    /// whose model is a single root [`Scene::External`] carrying its whole
    /// keyboard model on the `"key"` invoke wire — the command-menu family
    /// ([`MenuBar`](crate::widgets::menu) / [`Toolbar`](crate::widgets::toolbar)
    /// / [`ContextMenu`](crate::widgets::context_menu), R691 / R692 / R772).
    ///
    /// Routes the W3C `key` name to the External only when this widget owns
    /// focus (the roving-tabindex gate `focused == Some(Self::tag())`, so
    /// sibling controls keep their own keys), and returns the External's
    /// handled verdict so the shell swallow contract stays exact (an
    /// unhandled key like Tab falls through). The whole keymap is the
    /// External's `invoke("key", …)` — the same statechart the AT action
    /// layer and the §5.12 RPC client drive (§2 invariant #2).
    ///
    /// This is **not** the `apply_key` default (most widgets do not forward
    /// to an External `"key"` wire — buttons use `keybinding`, text fields
    /// run geometry-aware nav), so it stays an explicit opt-in a binding
    /// calls from its own `apply_key`. A binding with a richer keyboard
    /// model overrides `apply_key` directly instead of calling this.
    #[must_use]
    fn forward_key_to_external(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
        if focused != Some(Self::tag()) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        matches!(
            intro.invoke("key", IntrospectValue::Text(key.to_string())),
            Ok(IntrospectValue::Bool(true))
        )
    }

    /// R56.2.a §5.13 §5.38 — IME composition entry point. The shell
    /// calls this when the platform IME bridge dispatches a
    /// [`CompositionEvent`](crate::input::CompositionEvent), mirroring
    /// the W3C UI Events `CompositionEvent` (`compositionstart` /
    /// `compositionupdate` / `compositionend`) plus an explicit
    /// `Cancel` phase for IME cancel (Escape during preedit, blur
    /// with discarded composition, `WindowEvent::Ime::Disabled`
    /// mid-flight on winit 0.30).
    ///
    /// Symmetric with [`Self::apply_key`]: takes the authoritative
    /// state scene `&mut`, the focus manager's currently-focused tag
    /// at dispatch time, and the typed event. Widgets that own a
    /// focusable text-input surface walk the scene to the matching
    /// `Scene::External` (via [`Scene::find_external_with_tag_mut`])
    /// and call
    /// [`ExternalIntrospect::invoke`](crate::external::ExternalIntrospect::invoke)`("composition", Json{...})`
    /// — the same side door the R56.1.g.2 `scene/invoke` RPC path
    /// uses, so the AI client and the platform IME bridge funnel
    /// through one substrate contract.
    ///
    /// `focused != Some(<my tag>)` should short-circuit to `false`
    /// (mirroring the `apply_key` roving-tabindex pattern) so
    /// composition events do not broadcast to unfocused widgets.
    ///
    /// Returns `true` if the composition event was handled (the
    /// shell bumps the §5.34 revision, re-reads state, drains
    /// intents, and repaints on visible change). Returns `false` to
    /// defer (most widgets — only text-input widgets like
    /// `TextField` override this).
    ///
    /// Default returns `false` for every event — widgets without
    /// text-input affordances need no override. winit 0.30's
    /// [`WindowEvent::Ime`](https://docs.rs/winit/0.30/winit/event/enum.WindowEvent.html#variant.Ime)
    /// is the canonical cross-platform IME bridge (Wayland
    /// `text-input-v3` + X11 XIM + macOS `NSTextInputContext` +
    /// Windows TSF + GTK `IBus` all funnel through the four-variant
    /// `Ime` enum); pinion-shell's `app.rs` performs the
    /// `winit::Ime → CompositionEvent` mapping with `was_composing`
    /// state tracking so empty preedit triggers `Cancel` and
    /// `Disabled` cancels an in-flight session.
    #[must_use]
    fn apply_composition(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _event: &crate::input::CompositionEvent,
    ) -> bool {
        false
    }

    /// R56.2.e §5.13 §5.22 — middle-mouse-button paste entry point.
    /// The shell calls this when the platform mouse bridge dispatches
    /// a middle-button press (winit `MouseButton::Middle`, crossterm
    /// `MouseButton::Middle`, W3C `MouseEvent.button == 1`). On X11
    /// and Wayland desktops the canonical convention is "middle-click
    /// pastes the PRIMARY selection at the cursor" — `TextField` and
    /// other text-input widgets override this to read the PRIMARY
    /// clipboard via
    /// [`Clipboard::paste_from`](crate::clipboard::Clipboard::paste_from)`(`[`ClipboardSelection::Primary`](crate::clipboard::ClipboardSelection)`)`
    /// and insert at the caret. Non-text widgets accept the default
    /// `false` (the keystroke / pointer-event broadcasts to nobody).
    ///
    /// Symmetric with [`Self::apply_key`] / [`Self::apply_composition`]:
    /// takes the authoritative state scene `&mut`, the focus
    /// manager's currently-focused tag at dispatch time, and the
    /// W3C `MouseEvent` four-bit modifier surface. Position is *not*
    /// passed because pinion follows the [`Self::apply_key`] roving-
    /// tabindex pattern — widgets handle middle-click only when
    /// their own tag is the focused one, and the shell's
    /// [`InputRouter`](crate::scene::HitPath) cache holds the cursor
    /// position for any hit-test the widget needs (the same channel
    /// `mouse_pressed` consumes).
    ///
    /// `focused != Some(<my tag>)` should short-circuit to `false`
    /// so middle-click does not broadcast to unfocused widgets. The
    /// modifier gate stays widget-private: macOS / Linux convention
    /// is "plain middle-click pastes PRIMARY; modifier-prefixed
    /// middle-click is unspecified", so most widgets just check the
    /// focused tag and ignore the modifier bits.
    ///
    /// Returns `true` if the middle-click was handled (the shell
    /// bumps the §5.34 revision, re-reads state, drains intents, and
    /// repaints on visible change). Returns `false` to defer.
    ///
    /// Default returns `false` for every widget — only text-input
    /// widgets (`TextField`-class) override to wire the PRIMARY
    /// paste path the X11 / Wayland desktop convention expects.
    /// On macOS / Windows the [`Clipboard::paste_from`] default impl
    /// returns `None` for `Primary` so the widget override harmlessly
    /// produces a no-op (matching the OS-level absence of a parallel
    /// selection clipboard).
    #[must_use]
    fn apply_middle_click(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _modifiers: crate::input::Modifiers,
    ) -> bool {
        false
    }

    /// R772 §5.53 §5.38 — route a secondary-button (right-click) press
    /// into the widget tree at the window-space point `(x, y)`.
    ///
    /// Unlike [`Self::apply_middle_click`] this hook *does* carry the
    /// press position. A right-click opens a context menu **wherever the
    /// cursor landed**, independent of which widget owns keyboard focus,
    /// so the roving-tabindex "handle only when my tag is focused"
    /// contract does not apply here. Position-bearing pointer hooks are
    /// the established pattern: [`Self::position_caret_for_point`] takes
    /// the same window-space `(x, y)` to hit-test a text caret. The
    /// binding override walks the scene for its
    /// [`ContextMenuExternal`](crate::widgets::context_menu::ContextMenuExternal)
    /// and `invoke("open_at", "<x>,<y>")` to anchor the popup.
    ///
    /// Returns `true` if the secondary-click was handled (the shell
    /// bumps the §5.34 revision, re-reads state, drains intents, and
    /// repaints on visible change). Returns `false` to defer — the
    /// default for every widget that has no context menu.
    #[must_use]
    fn apply_secondary_click(_scene: &mut Scene, _x: f32, _y: f32) -> bool {
        false
    }

    // (R1020 §5.39) `focusable_tags()` was REMOVED. Keyboard focus
    // enumeration is no longer a hand-maintained binding-side list; it is
    // DERIVED from the paint scene by
    // [`Scene::collect_focusable_tags`](crate::Scene::collect_focusable_tags),
    // a depth-first walk collecting the tags of nodes marked
    // `.with_layout(LayoutStyle::new().with_focusable(true))`. The shell
    // re-runs that walk over the freshly produced paint scene every frame,
    // so a node that appears / disappears (a dynamic pane, a
    // conditionally-painted inline editor) joins / leaves the Tab order
    // automatically. This restores the ratified §5.39 design (the spec
    // always specified "depth-first traversal of the focusable tagged
    // subset" and rejected manual tabindex); the pre-R1020 flat list was an
    // unratified drift. A widget declares a focus stop where it paints the
    // node, via the node's `with_layout` builder — exactly as it declares
    // `pointer_transparent`.

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
    /// `Update(Model, Intent) -> Vec<Command<Intent>>`: a pure
    /// reducer that reads the application-side `Model`/state snapshot
    /// and returns a declarative list of IO/async work for the
    /// framework (or registered `Handler`) to execute. Commands are
    /// *described* here and *executed* outside reducer purity —
    /// preserving the §6.3 `dry_run` invariant (the reducer is
    /// replayable, only the `Command` dispatch is the side-effecting
    /// boundary).
    ///
    /// ## Why `Self::State` by-value, not `&mut Model`?
    ///
    /// R51.173 §5.23 R27 — the spec text "`Update(&mut Model, Intent)
    /// -> Vec<Command>`" reads as if the reducer mutates the
    /// application-side Model in place. In pinion the Model is the
    /// SCXML statechart wired through [`Scene::External`] (§5.15),
    /// not the cached projection: `Self::State` is the
    /// `Copy + Debug + PartialEq` snapshot [`Self::read_state`]
    /// extracts on every paint cycle, and the next paint's
    /// `read_state` re-derives it from the live `Scene`. Mutating the
    /// snapshot has no observable effect on the authoritative state.
    ///
    /// Passing `Self::State` by value (not `&mut`) makes that design
    /// choice explicit: the reducer reads its widget's slice as a
    /// `Copy` snapshot and returns Commands. State changes flow
    /// through `Command` → registered `Handler` → produced `Intent`
    /// → SCXML `invoke("send", …)` channel → statechart transition
    /// → next-frame `read_state`, not through reducer assignment.
    /// See [[scxml-as-model-update-transient]] for the
    /// design rationale.
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
    ///
    /// ## Cascade discipline
    ///
    /// R51.177 §5.23 R27 — the substrate calls `update` twice per
    /// dispatch cycle (once for the incoming carrier intent through
    /// `ShellCore::dispatch_intent`, once for each drained widget
    /// intent through `handle_tail`). If a registered `Handler`
    /// emits an [`Intent`] whose tag the same reducer also matches,
    /// the substrate re-enters the dispatch loop and the reducer
    /// re-emits — a self-referential cascade.
    ///
    /// pinion follows the Elm / Iced / Redux convention here: the
    /// framework does **not** install a cascade guard. Reducer
    /// authors carry the discipline:
    ///
    /// 1. Match on **specific** intent tags via
    ///    `match intent.tag_str() { "main_btn.click" => …, _ => Vec::new() }`.
    ///    A wildcard-emit reducer (every intent returns the same
    ///    command) is acceptable in tests (see
    ///    [`crate::test_fixtures::EchoButtonFixture`]) but produces
    ///    a guaranteed infinite loop in production whenever a
    ///    handler echoes any intent back through the SCXML send
    ///    channel.
    /// 2. If a handler's produced `Intent` tag overlaps with an
    ///    intent the same reducer emits a `Command` for, namespace
    ///    the handler's response (`hello-commands` uses `echo.<kind>`
    ///    for handler-produced intents so the reducer's
    ///    `<tag>.click` match arm never aliases) or use
    ///    [`Owner::cache`](crate::reactive::Owner::cache) to gate
    ///    the second emission.
    /// 3. The §5.7 `scene/commands` RPC method surfaces the pending
    ///    and in-flight Command queue at any time — a runaway
    ///    reducer accumulates visible queue depth that AI clients
    ///    can introspect mid-flight (the pinion-unique
    ///    observability advantage over Elm / Iced).
    ///
    /// Framework-level enforcement (kind whitelist, scope-id
    /// reentry counter) remains a future axis once a concrete
    /// widget consumer demonstrates the need; the cost of
    /// instrumenting every dispatch step on every cycle exceeds
    /// the prevention benefit against a discipline failure no
    /// current binding exhibits.
    #[must_use]
    fn update(_state: Self::State, _intent: &Intent) -> Vec<Command> {
        Vec::new()
    }
}

/// R643 §5.16 — bidirectional `Self ↔ &'static str` mapping for
/// [`WidgetCore::State`] enums.
///
/// Every visual binding hand-wrote two symmetric match arms before
/// R643 — a `parse_X_state(name: &str) -> XState` helper that the
/// [`WidgetCore::read_state`] body called after pulling the SCXML
/// state name through the §5.15 introspect channel, plus the same
/// `name → variant` table inverted for any other call site. The trait
/// captures both directions in one impl + one `from_name_or_default`
/// fallback (defensive default for unknown names — matches the every-
/// binding "first variant on failure" convention).
///
/// vendor/sce templates emit the [`WidgetCore::State`] enum without
/// any pinion-side derives ([[sce-priority-over-pinion]] keeps
/// `vendor/sce` untouched), so the impl is wired pinion-side in
/// `pinion-core/src/widgets/<widget>.rs` via the
/// [`widget_state_name!`](crate::widget_state_name) declarative macro
/// next to the `pub use sm::*;` re-export. Authors of new SCE-emitted
/// state enums add one macro invocation; the bindings then opt into
/// the derived [`WidgetCore::read_state`] body via the
/// `state_name_derive` flag on [`#[widget]`](pinion_derive::widget).
pub trait WidgetStateName: Sized {
    /// Map `self` to its `PascalCase` SCXML state id (1:1 with the
    /// `<state id="...">` attribute in the source `.scxml`).
    fn as_name(&self) -> &'static str;

    /// Parse `name` back to the corresponding variant. Returns the
    /// default variant when `name` is empty or unknown — matches the
    /// pre-R643 hand-written defensive fallback (`_ => Self::Idle`
    /// for every Button-class widget).
    fn from_name_or_default(name: &str) -> Self;
}

/// R643 §5.16 — bidirectional `Self ↔ &'static str` mapping for
/// [`WidgetCore::Event`] enums.
///
/// Mirror of [`WidgetStateName`] for the event side, but the two
/// directions cover *different* variant sets — the asymmetry that kept
/// the reverse off the trait until R699:
///
/// * [`as_name`](Self::as_name) is **total** — every variant emitted
///   by the SCE template (including the internal-only `*Activate`
///   raise events the winit handler never produces, plus the SCXML
///   3.13 `Null` sentinel) maps to its canonical `PascalCase` name so
///   AI-side introspection observes the full event surface.
/// * [`from_name`](Self::from_name) is **fallible + partial** — it
///   accepts only the *externally-drivable* variant subset (the names
///   the RPC `invoke("send", name)` path is allowed to inject), and
///   returns `None` for unknown names *and* for the internal /`Null`
///   variants. This matches the pre-R699 hand-written `parse_*_event`
///   contract exactly (rejecting an internal name as
///   `InvokeError::Rejected`), so an AI agent cannot drive a widget
///   into a state the real winit/keyboard handler would never reach.
///
/// The split is why the event side cannot reuse the state side's
/// total [`WidgetStateName::from_name_or_default`] — there is no
/// "default event", and silently coercing an unknown event name to
/// some fallback would let RPC callers desync the statechart. The
/// [`widget_event_name!`](crate::widget_event_name) macro therefore
/// takes two variant groups (`external` / `internal`) and emits the
/// total `as_name` over both while restricting `from_name` to the
/// `external` group.
pub trait WidgetEventName: Sized {
    /// Map `self` to its `PascalCase` SCXML event name (1:1 with the
    /// `<transition event="...">` attribute in the source `.scxml`).
    /// Total over every variant (external + internal + `Null`).
    fn as_name(&self) -> &'static str;

    /// Parse `name` back to the corresponding **externally-drivable**
    /// variant. Returns `None` when `name` is unknown or names an
    /// internal-only / `Null` variant — the RPC `invoke("send", …)`
    /// path surfaces that `None` as `InvokeError::Rejected`, exactly
    /// as the pre-R699 hand-written `parse_*_event` did.
    fn from_name(name: &str) -> Option<Self>;
}

/// R643 §5.16 — declarative impl emitter for [`WidgetStateName`].
///
/// Invoke once next to each vendor/sce-generated `pub use sm::<State>;`
/// re-export inside `pinion-core/src/widgets/<widget>.rs`. Variant
/// list must enumerate every variant emitted by the SCE template (a
/// missing variant produces a non-exhaustive match compile error at
/// macro expansion time).
///
/// ```rust,ignore
/// widget_state_name!(ButtonState, default = Idle, [
///     Idle, Hover, Pressed, Disabled,
/// ]);
/// ```
#[macro_export]
macro_rules! widget_state_name {
    ($ty:ident, default = $default:ident, [$($variant:ident),+ $(,)?]) => {
        impl $crate::WidgetStateName for $ty {
            fn as_name(&self) -> &'static str {
                match self {
                    $($ty::$variant => stringify!($variant),)+
                }
            }
            fn from_name_or_default(name: &str) -> Self {
                match name {
                    $(stringify!($variant) => $ty::$variant,)+
                    _ => $ty::$default,
                }
            }
        }
    };
}

/// R643 §5.16 / R699 §5.16 — declarative impl emitter for
/// [`WidgetEventName`].
///
/// Invoke once next to each vendor/sce-generated `pub use sm::<Event>;`
/// re-export inside `pinion-core/src/widgets/<widget>.rs`. The two
/// variant groups partition every variant the SCE template emits:
///
/// * `external` — the names the RPC `invoke("send", name)` path may
///   inject (pointer / keyboard / enable-disable events the real
///   winit handler also produces). These appear in **both**
///   [`WidgetEventName::as_name`] and [`WidgetEventName::from_name`].
/// * `internal` — the `*Activate` raise events the statechart fires
///   internally + the SCXML 3.13 `Null` sentinel. These appear in
///   `as_name` only (so AI-side introspection sees their canonical
///   names), and `from_name` rejects them — an RPC caller cannot
///   forge an internal raise.
///
/// Together the groups must enumerate **every** variant: the
/// `as_name` match is exhaustive, so a missing variant is a
/// compile error at macro-expansion time (the safety net that
/// catches a stale list after an SCE template change).
///
/// ```rust,ignore
/// widget_event_name!(ButtonEvent,
///     external = [
///         PointerEnter, PointerLeave, PointerDown, PointerUp,
///         PointerCancel, KeyboardActivate, Disable, Enable,
///     ],
///     internal = [ButtonActivate, Null],
/// );
/// ```
#[macro_export]
macro_rules! widget_event_name {
    ($ty:ident,
     external = [$($ext:ident),+ $(,)?],
     internal = [$($int:ident),* $(,)?] $(,)?) => {
        impl $crate::WidgetEventName for $ty {
            fn as_name(&self) -> &'static str {
                match self {
                    $($ty::$ext => stringify!($ext),)+
                    $($ty::$int => stringify!($int),)*
                }
            }
            fn from_name(name: &str) -> ::core::option::Option<Self> {
                match name {
                    $(stringify!($ext)
                        => ::core::option::Option::Some($ty::$ext),)+
                    _ => ::core::option::Option::None,
                }
            }
        }
    };
}

/// R644 §5.16 — type-safe single-source-of-truth tag identifier.
///
/// Pre-R644 every binding spelled its tag as a bare `&'static str`
/// literal in two or three places — the `#[widget(tag = "main_btn")]`
/// attribute, every `.with_tag("main_btn")` in the view fn, and
/// every test assertion against `"main_btn"`. A typo at any site
/// landed silently and was only surfaced when the input router
/// failed to hit-test the widget at runtime (or when a test missed
/// the new spelling).
///
/// R644 lifts the tag into a typed unit-variant enum that the
/// binding declares once + `#[derive(WidgetTag)]` to get
/// [`Self::as_tag`] (variant → `PascalCase`→`snake_case` string)
/// and [`Self::from_tag`] (string → variant) automatically. The
/// `#[widget(tag = Tags::MainBtn)]` form (R644 extension to the R641
/// attribute) emits `<Tags as WidgetTag>::as_tag(&Tags::MainBtn)` as
/// the `WidgetCore::tag` body so every site references the same
/// single source.
///
/// The trait carries no default-impl methods to keep the derive
/// surface narrow; future composite-tag enums (`Tags::MainBtn` +
/// `Tags::ScrollBar` + …) get the same impl shape for free.
///
/// [`Self::as_tag`]: WidgetTag::as_tag
/// [`Self::from_tag`]: WidgetTag::from_tag
pub trait WidgetTag: Sized + 'static {
    /// Map `self` to its canonical `snake_case` string. The
    /// [`#[derive(WidgetTag)]`](pinion_derive::WidgetTag) macro
    /// generates this by `PascalCase` → `snake_case` conversion on
    /// the variant ident (`MainBtn` → `"main_btn"`,
    /// `FigmaButtonM3` → `"figma_button_m3"`).
    fn as_tag(&self) -> &'static str;

    /// Parse `tag` back to the corresponding variant. Returns
    /// `None` when `tag` does not match any variant — unlike
    /// [`WidgetStateName::from_name_or_default`] there is no
    /// "default tag" convention (a missing tag at runtime is an
    /// AI-driven input-routing bug to surface, not silently
    /// substitute).
    fn from_tag(tag: &str) -> ::core::option::Option<Self>;
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
    use crate::Frame;
    use crate::command::Command;
    use crate::external::{External, IntrospectValue};
    use crate::intent::Intent;
    use crate::scene::Scene;
    use crate::test_fixtures::ButtonFixture;
    use crate::widgets::button::ButtonState;

    #[test]
    fn default_update_returns_empty_vec() {
        // R51.173 §5.23 R27 — by-value snapshot. `ButtonState: Copy`
        // makes the call site identical to a borrow at the source
        // level (no `&mut` taken), and the default impl returns an
        // empty `Vec<Command>` so the no-override path is observably
        // inert.
        let state = ButtonState::Idle;
        let intent = Intent::new_static("test_btn.click", IntrospectValue::Null);
        let commands = <ButtonFixture as WidgetCore>::update(state, &intent);
        assert!(commands.is_empty());
        // Caller's state binding is unaffected (by-value copy did
        // not move out of the local).
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

        fn update(state: Self::State, intent: &Intent) -> Vec<Command> {
            // R51.173 §5.23 R27 — by-value snapshot read. The
            // reducer derives the next counter value from the
            // snapshot and bakes it into the emitted Command's
            // `scope_id`. Any persistence of the new counter lives
            // in the authoritative model (the SCXML statechart on
            // `Scene::External`), not in the local `state` binding —
            // see [[scxml-as-model-update-transient]].
            let next = state.0.saturating_add(1);
            vec![Command::new_static(
                "echo.reply",
                IntrospectValue::Text(intent.tag_str().to_string()),
                u64::from(next),
            )]
        }
    }

    #[test]
    fn custom_update_reads_state_snapshot_and_emits_command() {
        // R51.173 §5.23 R27 — reducer reads the snapshot by value
        // and bakes the derived value into the Command. Caller's
        // `state` binding remains unchanged because the reducer
        // never observed a mutable reference to it.
        let state = CounterState(0);
        let intent = Intent::new_static("echo_reducer.tick", IntrospectValue::Null);
        let commands = <EchoReducerFixture as WidgetCore>::update(state, &intent);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind_str(), "echo.reply");
        assert_eq!(
            commands[0].payload,
            IntrospectValue::Text("echo_reducer.tick".to_string())
        );
        // Derived next value: 0 → 1 (snapshot + 1 baked into scope_id).
        assert_eq!(commands[0].scope_id, 1);
        // Caller's binding unaffected (Copy snapshot).
        assert_eq!(state, CounterState(0));
    }

    #[test]
    fn update_borrows_intent_without_consuming() {
        // Reducers commonly want to inspect / log the intent without
        // consuming it; the framework owns the wire-form value. The
        // borrow signature lets the same intent feed multiple
        // observers on the dispatch path (R51.167 routes it to the
        // SCXML send channel AFTER the reducer runs).
        //
        // R51.173 §5.23 R27 — by-value state snapshot means each
        // call reads the local independently; the reducer's derived
        // value reflects the snapshot at call time. Caller's
        // bindings remain unchanged.
        let state_a = CounterState(0);
        let state_b = CounterState(10);
        let intent = Intent::new_static("echo_reducer.shared", IntrospectValue::Null);
        let cmds_a = <EchoReducerFixture as WidgetCore>::update(state_a, &intent);
        let cmds_b = <EchoReducerFixture as WidgetCore>::update(state_b, &intent);
        // Derived next values baked into scope_id: 0+1 / 10+1.
        assert_eq!(cmds_a[0].scope_id, 1);
        assert_eq!(cmds_b[0].scope_id, 11);
        // Caller's snapshots unaffected (Copy).
        assert_eq!(state_a, CounterState(0));
        assert_eq!(state_b, CounterState(10));
        // Intent is still usable after both calls — confirms borrow,
        // not move.
        assert_eq!(intent.tag_str(), "echo_reducer.shared");
    }
}
