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

/// (R1306 PR-51 §5.41) The primary interactive surface a binding
/// contributes — the value [`WidgetCore::primary_surface`] returns when a
/// binding has one. Pairs the factory that builds the binding's root
/// [`External`] with the tag that routes input to it.
///
/// A binding with no single canonical primary (every routable surface is a
/// dynamic [`ExtraExternal`]) returns `None` from `primary_surface`
/// instead. Because the substrate reads the primary *only* through
/// `primary_surface`, "there is no primary" is an `Option::None` the type
/// system forces every substrate site to handle — never a bare
/// `Self::tag()` call a site can forget to gate.
#[derive(Clone, Copy)]
pub struct PrimarySurface {
    /// Stable routing tag — matches the paint-side hit-test target the
    /// view fn attaches (R55.G.17). The input router forwards pointer /
    /// key events to the `Scene::External` carrying this tag.
    pub tag: &'static str,
    /// Factory building a fresh primary [`External`] at boot (and on each
    /// dynamic reconcile). A `fn` pointer — not an eagerly built value — so
    /// a site that needs only [`Self::tag`] (e.g. the TUI focus target)
    /// reads it without constructing an `External`.
    pub factory: fn() -> Box<dyn External>,
}

impl core::fmt::Debug for PrimarySurface {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PrimarySurface")
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
///
/// # Most bindings should not write this block by hand
///
/// The `#[widget]` attribute macro in the `pinion-derive` crate emits the
/// mechanical half of the impl — [`Self::tag`], [`Self::title`],
/// [`Self::create_external`], and (with the `event_name_derive` /
/// `state_name_derive` flags) [`Self::read_state`] and [`Self::event_name`]:
///
/// ```ignore
/// #[widget(tag = "button", title = "hello-button", state_name_derive)]
/// struct HelloButton;
/// ```
///
/// 57 of the bindings in `examples/` use it. It is a separate crate, so a
/// consumer must add `pinion-derive` to its own `[dependencies]` — and until
/// R1641 nothing on this trait said the macro existed, so the first consumer
/// outside this workspace hand-wrote the whole impl and reported the required
/// [`Self::event_name`] / [`Self::title`] pair as boilerplate that ought to
/// have defaults. It does have them; the type that leads you here did not say
/// so. (A default [`Self::event_name`] is not currently available either way:
/// [`Self::Event`] is bound by `Copy` alone, and deriving a name from a value
/// would need [`WidgetEventName`] in that bound.)
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

    /// (R1306 PR-51 §5.41 §5.45 §5.49) The binding's **primary interactive
    /// surface**, if it has one — the factory that builds its root
    /// [`External`] plus the tag that routes to it, composed at state-scene
    /// index 0.
    ///
    /// This is the substrate's **sole accessor** for the primary: the boot
    /// compose ([`CoreShell::new`](../../pinion_runtime/struct.CoreShell.html#method.new)),
    /// the reconcile, `send_to_primary`, and the TUI key-focus dispatch all
    /// read `primary_surface()` and **never** call [`Self::create_external`]
    /// / [`Self::tag`] directly. Centralising "is there a primary + what
    /// builds it + what tag routes to it" in one [`Option`] accessor means a
    /// substrate site cannot silently skip the no-primary case — it is
    /// forced by the type. (The R1303→R1304 TUI panic was exactly a site
    /// that re-derived the tag as an unconditional `Self::tag()` instead of
    /// going through here; routing every site through this accessor makes
    /// that class of bug structural, not a matter of remembering to gate.)
    ///
    /// Default `Some(...)`, delegating to [`Self::create_external`] /
    /// [`Self::tag`] — the overwhelming majority of bindings have a natural
    /// primary (a button, a text field, an editor viewport, a list) and
    /// need no override. The substrate composes the primary at index 0: a
    /// bare [`Scene::External`] when there are no extras, or
    /// `Scene::Container([primary, ...extras])` when
    /// [`Self::create_extra_externals`] is non-empty — exactly as every
    /// pre-R1303 binding does. **No existing binding changes** (the default
    /// wraps the required `create_external`/`tag` they already provide).
    ///
    /// Override to `None` **only** for a binding whose interactive surfaces
    /// are *all* dynamic extras with no single canonical primary: every
    /// routable surface is a per-item / per-pane [`ExtraExternal`] from
    /// [`Self::create_extra_externals`]. The motivating consumer is sprag's
    /// multi-pane GUI (topology B) — its scrollbars / splitters / dock
    /// panels are all per-pane dynamic extras
    /// (`external_set_is_dynamic() == true`), so it has no natural primary.
    /// Before this opt-out such a binding had to satisfy the mandatory-
    /// primary contract with an **inert sentinel** External tagged with a
    /// name nothing ever paints: technically correct (never painted, never
    /// routed) but a deliberate violation of the R55.G.17 painted-primary-
    /// tag convention and a phantom node in the state scene. Returning
    /// `None` lets the binding declare the absence explicitly.
    ///
    /// When `None`:
    /// - [`Self::create_external`] / [`Self::tag`] are **never reached**
    ///   (the default that would call them is overridden away). A
    ///   no-primary binding leaves both as `unreachable!` markers. They
    ///   stay required on the trait so the has-primary majority keeps the
    ///   compile-time guarantee that a primary factory + tag is provided;
    ///   `Option`-typing *those two methods* instead would pollute the ~150
    ///   existing impls and their internal `Self::tag()` uses — one
    ///   delegating `Option` descriptor here achieves the opt-out with zero
    ///   migration. The binding must correspondingly NOT call a helper that
    ///   references its own [`Self::tag`] — e.g.
    ///   [`Self::forward_key_to_external`] — from its `apply_key`, and its
    ///   [`Self::keybinding`] must stay empty (a keybinding event would
    ///   reach the no-op `send_to_primary` and be dropped; a no-primary
    ///   binding drives its extras by tag).
    /// - The substrate composes the state scene from the extras alone:
    ///   `Scene::Container([...extras])`, or an empty container when the
    ///   extra set is momentarily empty (a dynamic binding before its first
    ///   pane exists). There is no index-0 primary.
    /// - The R55.G.17 painted-primary-tag convention on [`Self::tag`] /
    ///   [`Self::view`] does not apply. Keyboard focus is not
    ///   [`Self::tag`]-derived on either backend: the GUI focus manager
    ///   derives focus stops from the paint scene
    ///   ([`Scene::collect_focusable_tags`]) and the TUI `dispatch_key`
    ///   passes `None`.
    /// - [`CoreShell::send_to_primary`](../../pinion_runtime/struct.CoreShell.html#method.send_to_primary)
    ///   is a no-op. Over the §5.12 RPC wire the binding addresses each
    ///   extra by its explicit tag path; the bare `/external` shorthand has
    ///   no distinguished primary to name, so (R1307) the composed container
    ///   is marked no-primary-head and [`Scene::primary_external`] returns
    ///   `None` — a bare `/external` query / invoke / intervene rejects with
    ///   `NoExternalAtPath`, self-describing the absence (§2 #7) rather than
    ///   silently resolving an arbitrary extra as the primary.
    #[must_use]
    fn primary_surface() -> Option<PrimarySurface> {
        Some(PrimarySurface {
            tag: Self::tag(),
            factory: Self::create_external,
        })
    }

    /// Build a fresh state scene root. Called once at shell boot (and on
    /// each dynamic reconcile) — should return
    /// `Scene::External(ExternalNode::new(<my widget>)
    /// .with_tag(Self::tag()))` so the input router's hit-test on the
    /// paint-side tag routes to this node.
    ///
    /// (R1306 PR-51 §5.41) Reached **only** through the default
    /// [`Self::primary_surface`]. A binding that overrides `primary_surface`
    /// to `None` has no primary and this factory is never invoked — its body
    /// may be an `unreachable!` marker.
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
    /// `create_extra_externals` walks a `Signal<Option<DockTopology>>` and
    /// mints a `SplitterExternal` per split, so a reorganize gesture that
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
    /// (R1306 PR-51 §5.41) Reached **only** through the default
    /// [`Self::primary_surface`]. A binding with no primary surface
    /// (`primary_surface()` overridden to `None`) never has this called and
    /// the R55.G.17 convention below does not apply to it — its body may be
    /// an `unreachable!` marker.
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
    /// R55.G.17 §5.49 — composite paint-root tag convention. When the
    /// binding has a primary surface ([`Self::primary_surface`] is
    /// `Some`, the default), the returned scene must contain a node
    /// tagged [`Self::tag`]
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
    /// and the shell falls through to [`Self::apply_key`].
    ///
    /// R1364 — this used to say `Esc` / `Tab` / `Shift+Tab` "are shell-reserved
    /// and never reach this hook". That is true of the WINIT path only. The RPC
    /// `scene/key` method has no allowlist, so an injected `Escape` / `Tab` does
    /// reach [`Self::apply_key`] (pinned by pinion-shell's
    /// `r1364_shell_reserved_keys_are_injectable`). Write these hooks for the key
    /// they name, not for the path you expect it to arrive by.
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
    /// `End`, `PageUp`, `PageDown`, `Enter`, `Space`).
    ///
    /// R1364 §5.55 — `Escape` and `Tab` / `Shift+Tab` are shell-reserved ON THE
    /// WINIT PATH: `AppShell::handle_key_press` raises Escape as an app QUIT
    /// (through the `app_quit_requested` veto — R1363 §5.55; it does not "quit
    /// the window", and nothing about it is a window operation) and routes Tab to
    /// the focus manager. Both statements used to be written here as absolutes,
    /// and both were false for the RPC path, which has no allowlist: an injected
    /// `Escape` / `Tab` DOES reach this hook. Pinned by pinion-shell's
    /// `r1364_shell_reserved_keys_are_injectable`. R693 §5.39 additionally routes
    /// a physical `Escape` here while a modal focus trap is active, so a dialog
    /// can map Escape → cancel rather than the app ending.
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
    /// `Ctrl+Q` / platform-quit) are consumed upstream on the winit path — with
    /// the same R1364 caveat as above: "upstream" is `AppShell`, so the RPC
    /// injection path does not consume them.
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

    /// R1071 PR-27 §5.39 §5.35 — repeat-aware key entry point. The shell
    /// dispatches every keyboard press through this hook, carrying the
    /// platform `KeyEvent.repeat` flag (winit `KeyEvent::repeat`,
    /// crossterm `KeyEventKind::Repeat`): `false` for the leading press,
    /// `true` for every OS auto-repeat re-send of a held key.
    ///
    /// The default delegates to [`Self::apply_key`] **ignoring** the flag —
    /// the repeat-agnostic behaviour every existing binding already has, so
    /// the 100+ `apply_key` impls stay byte-unchanged and a held arrow key
    /// keeps scrolling / a held character keeps inserting. Override this
    /// (instead of, or in addition to, `apply_key`) only when a key's
    /// effect must NOT auto-repeat — a *toggle-class* shortcut. The
    /// motivating consumer is sprag's dock/undock `Ctrl+Shift+Enter`: a
    /// single physical press must toggle exactly once, so its binding
    /// returns `false` (defer / no-op) for `repeat == true` while still
    /// letting plain text / nav keys repeat. Auto-repeat re-dispatch of a
    /// toggle is what bounced a re-docked pane straight back out (the
    /// pre-R1071 shell offered every `Pressed`, repeat or not, to the
    /// binding with no way to tell them apart).
    ///
    /// `scene` / `focused` / `modifiers` carry the same authoritative
    /// surfaces as [`Self::apply_key`] — see that hook's contract. The
    /// shell's a11y activation path (Click → `apply_key("Enter")`) and the
    /// §5.49 RPC `scene/key` injection both dispatch with `repeat == false`
    /// (a synthesised activation is never an auto-repeat).
    #[must_use]
    fn apply_key_repeat(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: crate::input::Modifiers,
        repeat: bool,
    ) -> bool {
        let _ = repeat;
        Self::apply_key(scene, focused, key, modifiers)
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
    /// On macOS / Windows the [`Clipboard::paste_from`](crate::clipboard::Clipboard::paste_from) default impl
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
    /// the established pattern: `Self::position_caret_for_point` takes
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

    /// R1045 §5.45 §5.49 §5.38 — GUI-side wheel seam. The shell offers
    /// every wheel / touchpad-pan event to this hook **before** the
    /// [`InputRouter`](crate::input)'s two-stage default routing
    /// (the hover-`External` wheel offer, then the
    /// [`Scene::Scroll`] pixel-clip
    /// fallback). Returning `true` consumes the event — the router's
    /// two-stage never runs, so the producing `External` stays
    /// uncontaminated. Returning `false` (the default) defers to the
    /// router exactly as every pre-R1045 binding does.
    ///
    /// Unlike [`Self::apply_key`] / [`Self::apply_composition`] /
    /// [`Self::apply_middle_click`] (focus-gated, roving-tabindex), this
    /// is a **position-bearing** hook like [`Self::apply_secondary_click`]:
    /// a wheel scrolls whatever sits *under the pointer*, independent of
    /// keyboard focus (the W3C / desktop convention), so `cursor` — not a
    /// focused tag — is the discriminator, resolved against `paint`.
    /// `cursor` is the pointer's last window-local logical-pixel position.
    ///
    /// `paint` (R1048) is the **laid-out paint scene** of the addressed
    /// window — the [`InputRouter`](crate::input)'s `last_paint_scene`, the
    /// post-layout tree with resolved rects that the router *itself*
    /// hit-tests against. It is deliberately NOT the un-laid-out
    /// state / model scene (the compose-root of input `External`s, tagged
    /// by `External` tag and never laid out), on which
    /// [`rect_for_tag_absolute`](crate::Scene::rect_for_tag_absolute)
    /// resolves a paint-side pane tag to `None` — the pane tags are
    /// paint-tree constructs absent from the model scene, and even a node
    /// that *is* present there carries only its default zero rect. A
    /// multi-pane binding maps
    /// `cursor` to the pane it owns by hit-testing the pane's rect:
    /// `paint.rect_for_tag_absolute(pane_tag)` resolves the pane's
    /// window-absolute rect, which the binding tests `cursor` against —
    /// the same laid-out basis the router's `External` offer normalises
    /// against. A wheel with no stored cursor, or before the first paint
    /// (no `paint` scene yet), never reaches this hook (the shell
    /// short-circuits, matching the router's own no-op).
    ///
    /// This exists because a binding whose scroll authority is a
    /// **row-granular view-state it owns** — a virtualized terminal /
    /// log pane that re-projects scrollback by `offset_lines` rather than
    /// pixel-clipping a `Scene::Scroll` subtree — cannot be served by
    /// *either* router stage: stage 1 would push the wheel into the
    /// hovered `External` (embedding the human-facing viewport offset in
    /// a producer engine that "carries no scene state of its own" — a
    /// layering violation), and stage 2 requires a `Scene::Scroll`
    /// pixel-clip node the row-reprojected grid deliberately has none of
    /// (a row offset is not a pixel offset). The binding overrides this
    /// hook instead: it hit-tests `cursor` against its pane rects in
    /// `paint` and advances that pane's own
    /// [`ScrollState`](crate::widgets::scroll::ScrollState) (or equivalent
    /// reactive authority) at row granularity, keeping the keyboard /
    /// drag / wheel writers on one SSOT. The wheel action mutates that
    /// reactive authority (a `use_*` hook), not `paint` — hence `paint` is
    /// shared read-only, not `&mut`.
    ///
    /// `delta` is the typed [`WheelDelta`](crate::event::WheelDelta)
    /// verbatim ([`Pixels`](crate::event::WheelDelta::Pixels) for
    /// high-resolution touchpads, [`Lines`](crate::event::WheelDelta::Lines)
    /// for notched wheels), so the binding owns the unit conversion. To
    /// turn a pixel delta into row steps without re-hardcoding the magic
    /// constant, consume the exported
    /// [`LINE_HEIGHT_PX`](crate::event::LINE_HEIGHT_PX) (the W3C 16-px
    /// line basis the router's own `Lines` path uses) or divide by the
    /// binding's measured cell height. `modifiers` carries the held W3C
    /// four-bit surface for `Shift`-axis-remap / `Ctrl`-zoom conventions.
    ///
    /// Implementations run inside the binding's
    /// [`Owner`](crate::reactive::Owner) root scope (the shell wraps the
    /// call in `root_owner.run`, mirroring [`Self::apply_key`]), so
    /// `use_*` reactive hooks resolve to the same instances the view fn
    /// shares.
    ///
    /// Returns `true` if the wheel was handled (the shell bumps the
    /// §5.34 revision, drains intents, and repaints on visible change);
    /// `false` to defer to the router default. Default returns `false`
    /// for every binding — widgets backed by a `Scene::Scroll` subtree
    /// or a hover-`External` need no override and keep the router path.
    #[must_use]
    fn apply_wheel(
        _paint: &Scene,
        _cursor: (f64, f64),
        _delta: crate::event::WheelDelta,
        _modifiers: crate::input::Modifiers,
    ) -> bool {
        false
    }

    /// R1047 §5.23 §5.22 §6.3 — per-paint binding **reconcile** pass. The
    /// shell runs this once at the top of every *real* paint cycle,
    /// BEFORE the pure view fn, inside the binding's root
    /// [`Owner`](crate::reactive::Owner) scope (so `use_*` reactive hooks
    /// resolve to the same instances the view fn shares — the same wrap
    /// [`Self::apply_key`] uses). The default is a no-op; almost every
    /// binding needs no override.
    ///
    /// This is the sanctioned non-view-fn place for a binding to write
    /// its own reactive view-state. It exists because the view fn is
    /// **pure** (§6.3 R51.27: same `(state, frame)` ⇒ same `Scene`, the
    /// `dry_run` guarantee), so a `Signal` write inside `V::view` — e.g.
    /// [`ScrollState::set_max`](crate::widgets::scroll::ScrollState::set_max)
    /// / [`scroll_to`](crate::widgets::scroll::ScrollState::scroll_to) to
    /// grow a scroll bound and tail-follow — is a purity violation.
    /// Moving that write here keeps `V::view` a pure read of
    /// already-reconciled state.
    ///
    /// The motivating gap: a binding whose scroll authority is an
    /// **offset-projection** rather than a [`Scene::Scroll`]
    /// pixel clip — a terminal grid ([`Scene::TextGrid`])
    /// that re-projects scrollback by `offset_lines` — has no clip node
    /// for the post-layout reducer
    /// (`update_scroll_state_bounds`) to measure, AND its content extent
    /// (`scrollback_len`) lives in an **off-thread producer** (a PTY
    /// thread) that only calls `request_repaint` with no reactive
    /// `Signal` for an [`Effect`](crate::reactive::Effect) to subscribe.
    /// So neither the declarative layout reducer (no clip node, extent
    /// not layout-derived) nor an Effect (no dep) can reconcile the
    /// bound — this is a substrate-integration gap, not a missed
    /// refactor. The binding reconciles here instead: read the current
    /// producer extent + [`at_bottom`](crate::widgets::virtual_list::at_bottom)
    /// tail-follow predicate, then
    /// [`follow_tail`](crate::widgets::virtual_list::follow_tail) its own
    /// [`ScrollState`](crate::widgets::scroll::ScrollState).
    ///
    /// Runs **pre-view** (the post-layout sibling is the R774 scroll-dirty
    /// re-pass + the R1012 pane-viewport publish): the producer-derived
    /// extent needs no measured viewport, so reconciling before the view
    /// fn lets the view project against the reconciled state in one pass —
    /// no re-run. A consumer that genuinely needs a *layout-measured*
    /// viewport for its reconcile uses the post-layout
    /// [`use_pane_viewport_size`](crate::reactive::use_pane_viewport_size)
    /// Effect (R1012) instead.
    ///
    /// Runs **only on the real paint path**, never the side-effect-free
    /// introspection / `dry_run` paint mirror — so a `scene/snapshot` or a
    /// dry-run never mutates state (§6.3 / §2 #3 preserved), exactly like
    /// the R1006 pre-view `set_viewport_size` publish it sits beside.
    /// Writes should be loop-safe (`set_max` / `scroll_to`
    /// equality-skip), so a steady producer reconciles to a no-op and
    /// does not schedule a paint.
    fn reconcile_frame() {}

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

    /// (R1363 §5.55 §2 #6) App-level QUIT veto — the app-lifecycle peer of
    /// `WidgetView::window_close_requested`.
    ///
    /// The shell calls this when something asks the APP to end: `Escape` (the
    /// standalone convention), the last window closing under
    /// `WidgetView::quit_on_last_window_closed`, a binding's own
    /// [`QuitSink::request_quit`](crate::QuitSink::request_quit), or the
    /// `app/quit` RPC. Return `true` if the BINDING handled it — pop a "Save
    /// changes?" dialog, start an async flush — and the app stays alive. Return
    /// `false` (the default) to end it.
    ///
    /// # Why this is on `WidgetCore`, not `WidgetView`
    ///
    /// Quitting is not a window operation, so it is not the GUI's private
    /// business: `pinion-tui` deps this crate and no further, and its `Esc`
    /// routes through this same veto. Putting it here is what makes the §2 #6
    /// dual true for app lifecycle — pre-R1363 BOTH shells hard-coded a bare
    /// exit on `Escape`, bypassing every binding veto, because neither had a
    /// verb for "quit" distinct from "close this window".
    ///
    /// Called inside the shell's reactive owner, so it may read / write
    /// `Signal`s (raise a modal, set a flag).
    #[must_use]
    fn app_quit_requested() -> bool {
        false
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
    ///    `crate::test_fixtures::EchoButtonFixture`) but produces
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
/// SCE-002 §5.16 — the sce codegen emits the [`WidgetCore::State`] enum
/// carrying a `#[default]` marker on its SCXML-initial variant, and
/// `pinion-core`'s `build.rs` injects `#[derive(WidgetStateName)]` (from
/// `pinion-derive`) onto it via `compile_scxml_with_derives`. The derive
/// maps each variant to its ident string for [`as_name`](Self::as_name)
/// and falls through to that `#[default]` state for an unknown name in
/// [`from_name_or_default`](Self::from_name_or_default), replacing the per-widget
/// `widget_state_name!` declarative macro that used to be hand-written
/// next to each `pub use sm::*;` re-export. Bindings then opt into the
/// derived [`WidgetCore::read_state`] body via the `state_name_derive`
/// flag on `#[widget]`.
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
/// some fallback would let RPC callers desync the statechart. SCE-002
/// §5.16 — the sce codegen emits an `EXTERNALLY_DRIVABLE_EVENTS`
/// associated const on the [`WidgetCore::Event`] enum (exactly the
/// externally-drivable variants, excluding internal `<raise>` events
/// and the `Null` sentinel), and `build.rs` injects
/// `#[derive(WidgetEventName)]` (from `pinion-derive`) onto it. The
/// derive emits the total `as_name` over every variant while
/// restricting `from_name` to the members of that const, replacing the
/// hand-written `widget_event_name!` declarative macro and its
/// `external` / `internal` variant groups.
pub trait WidgetEventName: Sized {
    /// Map `self` to its `PascalCase` SCXML event name (1:1 with the
    /// `<transition event="...">` attribute in the source `.scxml`).
    /// Total over every variant (external + internal + `Null`).
    fn as_name(&self) -> &'static str;

    /// Parse `name` back to the corresponding **externally-drivable**
    /// variant. Returns `None` when `name` is unknown or names an
    /// internal-only / `Null` variant — the RPC `invoke("send", …)`
    /// path surfaces that `None` as a
    /// [`RefusalReason`](crate::external::RefusalReason)-carrying
    /// [`InvokeError::Rejected`](crate::external::InvokeError::Rejected)
    /// (R1564), exactly as the pre-R699 hand-written `parse_*_event` did.
    fn from_name(name: &str) -> Option<Self>;

    /// R1564 §5.15 §2 #2 — every name [`from_name`](Self::from_name) admits,
    /// in declaration order.
    ///
    /// The set was already known — the sce codegen emits it as
    /// `EXTERNALLY_DRIVABLE_EVENTS` and the derive's `from_name` tests
    /// membership against it — and it was **unreachable**, so a refused
    /// `send` could say that a name was wrong and not what would have been
    /// right. That is the difference between a refusal an operator reads and
    /// one an operator can act on, which is the whole subject of
    /// [`RefusalReason`](crate::external::RefusalReason).
    ///
    /// The toolkit's floor: `invokeMethod` with an unknown member answers
    /// `false` and, in a debug build, prints `No such method` to stderr — the
    /// meta-object holds every method's signature and the failure path
    /// enumerates none of them.
    ///
    /// A `Vec` rather than a `&'static [&'static str]` because the const holds
    /// *variants*, and mapping them to names is what the derive would have to
    /// do anyway; this runs on a refusal, never on a hot path.
    fn drivable_names() -> Vec<&'static str>;
}

/// R1564 §5.15 §2 #2 (PINION-PR82) — decode a `send` action's event name, or
/// refuse with a sentence naming both the name that arrived and the vocabulary
/// that would have been accepted.
///
/// Sixteen widget `invoke` arms wrote `X::from_name(name).ok_or(Rejected)?`,
/// and every one of them threw away two facts it was holding: the offending
/// name, and — through [`WidgetEventName::drivable_names`] — the closed set it
/// failed to be a member of. An operator reading `InvokeRejected` learned
/// neither.
///
/// `widget` names the surface, because a refusal is read out of context: it is
/// the wire word for the widget kind (`"button"`, `"checkbox"`), not the paint
/// tag, which the caller already has in the path it sent.
///
/// ```
/// # use pinion_core::widget_core::require_event;
/// # use pinion_core::widgets::button::ButtonEvent;
/// let refusal = require_event::<ButtonEvent>("button", "Bogus").unwrap_err();
/// let said = refusal.reason().expect("a rejection states why").as_str().to_owned();
/// assert!(said.contains("\"Bogus\""), "{said}");
/// assert!(said.contains("PointerDown"), "{said}");
/// ```
///
/// # Errors
///
/// [`InvokeError::Rejected`](crate::external::InvokeError::Rejected) when
/// `name` is not an externally-drivable event of `E`.
pub fn require_event<E: WidgetEventName>(
    widget: &str,
    name: &str,
) -> Result<E, crate::external::InvokeError> {
    E::from_name(name).ok_or_else(|| {
        crate::external::InvokeError::rejected(format!(
            "{widget}.send: {name:?} is not an event this widget accepts (accepts: {})",
            E::drivable_names().join(", ")
        ))
    })
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
    /// `#[derive(WidgetTag)]` macro
    /// generates this by `PascalCase` → `snake_case` conversion on
    /// the variant ident (`MainBtn` → `"main_btn"`,
    /// `DesignButtonM3` → `"design_button_m3"`).
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

#[cfg(test)]
mod r1071_apply_key_repeat_tests {
    //! R1071 PR-27 §5.39 §5.35 — the default [`WidgetCore::apply_key_repeat`]
    //! contract: it forwards to [`WidgetCore::apply_key`] for BOTH repeat
    //! values, so the 100+ bindings that only override `apply_key` are
    //! repeat-agnostic (a held key keeps acting) with zero per-impl change.
    use super::WidgetCore;
    use crate::Frame;
    use crate::external::External;
    use crate::input::Modifiers;
    use crate::scene::{ContainerNode, Scene};

    /// Minimal fixture: `apply_key` returns `true` only for `"Enter"`, so a
    /// test can prove the repeat variant routes through it verbatim.
    struct EnterFixture;

    impl WidgetCore for EnterFixture {
        type State = ();
        type Event = ();

        fn create_external() -> Box<dyn External> {
            unreachable!("repeat-delegation test does not paint")
        }
        fn tag() -> &'static str {
            "enter_fixture"
        }
        fn read_state(_: &Scene) -> Self::State {}
        fn view((): Self::State, _: &Frame) -> Scene {
            unreachable!("repeat-delegation test does not paint")
        }
        fn event_name((): Self::Event) -> &'static str {
            "__internal__"
        }
        fn title() -> &'static str {
            "EnterFixture"
        }
        fn apply_key(_: &mut Scene, _: Option<&str>, key: &str, _: Modifiers) -> bool {
            key == "Enter"
        }
    }

    #[test]
    fn default_repeat_delegates_to_apply_key_for_both_flags() {
        let mut scene = Scene::Container(ContainerNode::new(Vec::new()));
        // repeat == false (the leading press) and repeat == true (an OS
        // auto-repeat re-send) must yield the SAME verdict as apply_key —
        // the default impl ignores the flag entirely.
        for repeat in [false, true] {
            assert!(
                <EnterFixture as WidgetCore>::apply_key_repeat(
                    &mut scene,
                    Some("enter_fixture"),
                    "Enter",
                    Modifiers::empty(),
                    repeat,
                ),
                "Enter delegates to apply_key regardless of repeat={repeat}",
            );
            assert!(
                !<EnterFixture as WidgetCore>::apply_key_repeat(
                    &mut scene,
                    Some("enter_fixture"),
                    "Space",
                    Modifiers::empty(),
                    repeat,
                ),
                "unhandled key stays unhandled regardless of repeat={repeat}",
            );
        }
    }
}

#[cfg(test)]
mod r1564_advertised_vocabulary {
    //! R1564 §5.15 (PINION-PR82) — the vocabulary a refusal advertises is the
    //! vocabulary the surface accepts.
    //!
    //! This module exists because a counterfactual **passed**. The claim was
    //! being checked one refusal message at a time, by substring, and the
    //! substring asked about a name's *position* rather than its membership —
    //! so a derive that advertised every variant (internal `<raise>` events and
    //! the SCXML `Null` sentinel included) satisfied every assertion in the
    //! tree.
    //!
    //! R1564.1 — the first fix reached for the message and **parsed the list
    //! back out of it**, which closed the hole and opened a smaller one: a
    //! sentence whose format drifted would answer with an empty list, and an
    //! empty list satisfies a membership test vacuously. The property does not
    //! need the message at all. `require_event` composes the sentence *from*
    //! `drivable_names`, so asking that function directly is asking the source
    //! rather than its rendering — and the one thing the message must be
    //! checked for (that the names actually reach it) is a `contains` per name
    //! with nothing to parse. The accessor is gone.
    //!
    //! Both directions are asserted and neither alone is enough. Without the
    //! first, `drivable_names` could return the whole variant list. Without the
    //! second, it could return an empty one — vacuously "accurate", and useless
    //! to the operator the sentence exists for.

    use super::{WidgetEventName, require_event};
    use crate::widgets::button::ButtonEvent;
    use crate::widgets::disclosure::DisclosureEvent;
    use crate::widgets::listbox_item::ListboxItemEvent;
    use crate::widgets::radio::RadioEvent;
    use crate::widgets::text_field::TextFieldEvent;
    use crate::widgets::toggle::ToggleEvent;

    /// The advertised vocabulary is exactly the accepted one, and it reaches
    /// the sentence.
    fn advertises_exactly_what_it_accepts<E: WidgetEventName + std::fmt::Debug>(widget: &str) {
        let names = E::drivable_names();
        assert!(
            !names.is_empty(),
            "{widget}: a refusal that lists nothing tells the operator nothing",
        );
        let refusal = require_event::<E>(widget, "\u{0}definitely-not-an-event")
            .expect_err("a NUL-led name is not an event of any statechart");
        let said = refusal
            .reason()
            .expect("a rejection states why")
            .as_str()
            .to_owned();
        for name in &names {
            // (1) Advertised implies accepted. This is the direction the
            // counterfactual breaks, and it needs no message: `drivable_names`
            // IS what the sentence is built from.
            assert!(
                E::from_name(name).is_some(),
                "{widget}: advertises {name:?}, which from_name then refuses",
            );
            // (2) …and the name reaches the operator, which is the only thing
            // the rendered sentence is responsible for.
            assert!(
                said.contains(name),
                "{widget}: accepts {name:?} without saying so: {said}",
            );
        }
    }

    #[test]
    fn six_widgets_advertise_exactly_what_they_accept() {
        advertises_exactly_what_it_accepts::<ButtonEvent>("button");
        advertises_exactly_what_it_accepts::<DisclosureEvent>("disclosure");
        advertises_exactly_what_it_accepts::<ListboxItemEvent>("listbox_item");
        advertises_exactly_what_it_accepts::<RadioEvent>("radio");
        advertises_exactly_what_it_accepts::<TextFieldEvent>("text_field");
        advertises_exactly_what_it_accepts::<ToggleEvent>("toggle");
    }

    #[test]
    fn the_scxml_null_sentinel_is_advertised_by_nobody() {
        // `Null` is in every generated Event enum and is drivable by none of
        // them — the sharpest single witness that the advertised list comes
        // from `EXTERNALLY_DRIVABLE_EVENTS` and not from the variant list.
        assert!(!ButtonEvent::drivable_names().contains(&"Null"));
        assert!(!ToggleEvent::drivable_names().contains(&"Null"));
        assert!(!RadioEvent::drivable_names().contains(&"Null"));
    }

    #[test]
    fn r1564_1_every_invoke_failure_declares_how_a_transport_renders_it() {
        // R1564.1 — the compile-time guard `pinion-rpc` cannot have.
        //
        // `InvokeError` is `#[non_exhaustive]`, so the transport's
        // `From<InvokeError>` needs a `_` arm; R1564 made that arm answer
        // `UnmappedSurfaceError` rather than fabricate a reason, which is the
        // honest RUNTIME answer and leaves the decision unmade until someone
        // notices. `#[non_exhaustive]` constrains other crates and not this
        // one, so an exhaustive match HERE is a compile error the day a variant
        // is added — which is the point, and why this lives in the defining
        // crate ([`ArgDomain::to_wire`](crate::external::ArgDomain::to_wire)'s
        // own argument).
        use crate::external::InvokeError;
        fn renders_as(err: &InvokeError) -> &'static str {
            match err {
                InvokeError::UnknownPath | InvokeError::TypeMismatch => "a framework word",
                InvokeError::Rejected(_) => "the producer's sentence",
            }
        }
        // …and the classification is not free-floating: it agrees with what the
        // value can actually supply.
        for (err, expected) in [
            (InvokeError::UnknownPath, "a framework word"),
            (InvokeError::TypeMismatch, "a framework word"),
            (
                InvokeError::rejected("no pane 999"),
                "the producer's sentence",
            ),
        ] {
            assert_eq!(renders_as(&err), expected);
            assert_eq!(
                err.reason().is_some(),
                expected == "the producer's sentence",
                "a failure renders as a sentence iff it carries one: {err:?}",
            );
        }
    }
}
