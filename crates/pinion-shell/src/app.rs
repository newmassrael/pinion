//! R51.92.1 §5.40 — surface (winit / wgpu / `accesskit_winit`) module
//! split from `lib.rs`.
//!
//! Houses the framework-side application surface ([`AppShell`]) and
//! its `winit::application::ApplicationHandler` implementation, plus
//! the two surface-only helpers (`named_key_str` for the winit↔W3C
//! `KeyboardEvent.key` bridge and `spawn_stdin_rpc_reader` for the
//! background JSON-RPC reader thread) and the public [`run`]
//! entry-point every visual binary's `fn main()` collapses to.
//!
//! R51.92 (substrate.rs) extracted [`crate::ShellCore`] so the
//! dispatch substrate is module-private (14 fields + the
//! [`crate::AccessEmitDecision`] body genuinely private to
//! `substrate.rs`). R51.92.1 completes the textbook 3-module split:
//! every `AppShell` field — including the `core: ShellCore<V>`
//! borrow — is private to `app.rs`, so the surface boundary is now
//! enforced at the module level as well. Any future addition to
//! `lib.rs` cannot accidentally reach across the boundary to touch
//! either the substrate's fields or `AppShell`'s winit surface
//! state.
//!
//! See `claim-accuracy-self-audit` and `substrate-incompleteness-signal`.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use pinion_core::reactive::{Effect, Signal};

use pinion_a11y::AccessTreeBuilder;
use pinion_core::event::WheelDelta;
use pinion_core::scene::BoxNode;
use pinion_core::style::CursorHint;
use pinion_rpc::{ConnId, FnEgress, RpcEgress, RpcFrame, RpcIngress, try_async_wait_for};
use pinion_runtime::instant_delta_us;
use pinion_runtime::{CommandExecutor, HandlerRegistry, PointerId, image_cache, paint_adapter};
use vello::Scene as VelloScene;
use vello::kurbo::Affine;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::monitor::MonitorHandle;
use winit::window::{
    CursorIcon, ResizeDirection, Window, WindowId, WindowLevel as WinitWindowLevel,
};

use crate::executor::build_executor_and_sink;
use crate::substrate::ShellCore;
use crate::{
    AppEvent, RenderState, SizeStrategy, VelloContext, VelloRenderer, WidgetRenderer, WidgetView,
    WindowPlacement, WindowSpec,
};
use pinion_core::display::{Anchored, DisplayId, DisplayInfo, DisplayRect, DisplayTopology};
use pinion_core::window_level::{WindowLevel, WindowingBackend};

/// R1576 §5.16 §5.41 — one winit monitor as a [`DisplayInfo`], field for field.
///
/// This function and [`topology_from`] are the **entire** untestable surface of
/// the display axis: everything a `MonitorHandle` can answer is moved across
/// with no arithmetic, and every derivation on top of the result lives in
/// `pinion_core::display`, where an arrangement is an argument a test writes.
/// A monitor farm is not a thing CI has, so keeping this seam to a move is what
/// makes the rest of the axis provable.
///
/// `refresh_rate_millihertz()` is already an `Option` in winit — the honesty the toolkit lacks (`refreshRate()` returns
/// `qreal`, so "unknown" arrives as a real-looking `0`) — and it is carried across
/// as one.
fn display_info_from(monitor: &MonitorHandle, primary: bool) -> DisplayInfo {
    let position = monitor.position();
    let size = monitor.size();
    DisplayInfo {
        label: monitor.name(),
        bounds: DisplayRect::new(position.x, position.y, size.width, size.height),
        scale_factor: monitor.scale_factor(),
        refresh_mhz: monitor.refresh_rate_millihertz(),
        primary,
    }
}

/// R1576 §5.16 §5.41 — the desk, from a winit monitor enumeration.
///
/// `primary` is compared by winit's own `MonitorHandle` equality rather than by
/// name, because a name is exactly the thing that is not unique — the reason
/// [`pinion_core::display::DisplayId`] exists.
fn topology_from(
    monitors: impl Iterator<Item = MonitorHandle>,
    primary: Option<&MonitorHandle>,
) -> DisplayTopology {
    DisplayTopology::new(
        monitors
            .map(|m| {
                let is_primary = primary.is_some_and(|p| *p == m);
                display_info_from(&m, is_primary)
            })
            .collect(),
    )
    // R1621 — the work area is a SEPARATE platform call from the monitor
    // enumeration, so it is applied as a separate step. A topology this was
    // not called on reports `Unprobed`, which is what a TUI or a unit-test
    // desk honestly is.
    .with_work_area(crate::work_area::probe_work_area())
}

/// R670.B §5.16 §5.41 — per-window state cluster.
///
/// Lifts the 5 single-window `AppShell` fields (`render` +
/// `vello_scene` + `accesskit` + IME state + `pending_intrinsic_resize`)
/// into one struct so [`AppShell`] can hold
/// `HashMap<WindowId, WindowSlot<R>>` and stay disjoint across
/// multi-window dispatch. R670.A landed the
/// `WidgetView::windows() -> Vec<WindowSpec>` trait foundation; this
/// struct is the runtime side that the foundation enables — each
/// `WindowSpec` in the binding's list owns one `WindowSlot` once
/// [`AppShell::resumed`] creates the winit `Window` + GPU renderer +
/// AccessKit adapter for that spec.
///
/// The pre-R670.B single-window `AppShell` carried these fields
/// directly on the struct. R670.B preserves bit-identical
/// single-window lifecycle by anchoring the canonical primary spec
/// (`WindowSpec::main(...)` from R670.A default impl) as the only
/// window the 15+ existing bindings ever create — the `HashMap`
/// holds exactly one entry for single-window bindings and the
/// per-window dispatch code paths are no-op-per-spec for the missing
/// secondaries.
/// (R1121 §5.16 §5.39) A client-side window-chrome control a left-press
/// resolved to (a borderless window's title-bar buttons / drag grip). Mapped
/// from the [`pinion_overlay`] chrome control tag in [`AppShell::try_chrome_press`].
///
/// R1188 §5.16 §5.49 §2 #2 — the discrete button actions collapsed into
/// [`Control`](Self::Control) (`pinion_overlay::WindowControl`), the vocabulary
/// the headless RPC click drain shares: both input paths detect through the one
/// `window_control_for_tag` mapping and execute through the one
/// [`AppShell::apply_window_control`] arm. The shared DETECTION vocabulary is
/// these two tag-driven paths; the shared EXECUTION arm serves every
/// [`ControlProducer`], including the two that detect from no tag at all. `Move`
/// / `Resize` stay winit-local — pointer-session gestures with no RPC-click
/// semantic.
#[derive(Clone, Copy)]
enum ChromeAction {
    /// A discrete control button — minimize / maximize-toggle / close.
    Control(pinion_overlay::WindowControl),
    /// `Window::drag_window()` — OS-driven interactive move from the grip.
    Move,
    /// `Window::drag_resize_window(dir)` — OS-driven interactive resize from a
    /// client-side resize edge / corner (R1122). A borderless window has no OS
    /// frame, so the chrome supplies the resize border.
    Resize(ResizeDirection),
}

/// (R1364 §5.16 §5.55) Which producer asked for a window control — the roster of
/// [`AppShell::apply_window_control`], as a type.
///
/// It exists because the count did not. The arm has always been ONE, but "how
/// many reach it" lived only as prose — nine sentences across five files, of
/// which THREE still said "third" / "three" at R1363's HEAD. R1362 authored them
/// under three reviewers, in the very round that added the fourth producer:
/// prose does not recount itself, and nothing failed when it was wrong.
///
/// As a parameter the roster has exactly one home. The variants ARE the count,
/// every call site names itself at the call, and the `tracing` warn on an
/// unregistered window can say WHO asked — which for [`Self::Binding`] (a
/// `String` built off the UI thread, possibly a typo or an id whose window
/// `reconcile_windows` already dropped) is the difference between a diagnosable
/// drop and a silent one.
///
/// This is R1190's lesson applied one layer over. R1190 fused a split tag
/// vocabulary by giving the tags ONE authority; the producers already shared one
/// arm, but had no name, so their number was only ever countable by grep — and
/// the grep was wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlProducer {
    /// The winit pointer path — a physical left-press on a client-side chrome
    /// control tag ([`AppShell::try_chrome_press`]).
    ChromePress,
    /// The OS window manager — the X on a decorated window, or Alt+F4
    /// (`WindowEvent::CloseRequested`). Unified into the arm by R1362, having
    /// hand-copied the `Close` body since R1170.
    OsCloseRequested,
    /// R1188 §2 #2 — an RPC `scene/click` on a control tag, which the headless
    /// `ShellCore` drain detects and queues for
    /// [`ShellCore::take_pending_window_controls`], drained in
    /// [`AppShell::user_event`] right after `dispatch_rpc`.
    RpcClick,
    /// R1362 PR-65 — the BINDING itself, via [`crate::WindowControlSink`],
    /// delivered as [`AppEvent::WindowControlRequested`].
    Binding,
}

/// (R1121 §5.16 §5.39) Map a hit-test tag to the window-chrome action it names,
/// or `None` when the tag is not a chrome tag. R1190 — the tag→semantic decision
/// is the overlay [`chrome_tag_semantic`](pinion_overlay::chrome_tag_semantic)
/// SSOT; this fn only lifts that shell-neutral meaning into the winit-typed
/// [`ChromeAction`] (`WindowResizeEdge`→[`ResizeDirection`] here, the sole place
/// the winit resize type is introduced). The `ChromeTag` match is exhaustive, so
/// a new overlay chrome-tag meaning fails to compile until this handles it —
/// cross-crate exhaustiveness by the type system. Pure (no `self` / live
/// `Window`), so unit-tested without a window.
fn chrome_action_for_tag(tag: &str) -> Option<ChromeAction> {
    pinion_overlay::chrome_tag_semantic(tag).map(chrome_action_for_semantic)
}

/// (R1701 §5.16) The winit-typed conversion of a resolved
/// [`ChromeTag`](pinion_overlay::ChromeTag), split
/// out of [`chrome_action_for_tag`] so the ordinal-aware resolution
/// ([`pinion_overlay::chrome_press_intent`]) and the ordinal-free one share it.
///
/// Exhaustive on purpose — a ninth chrome region fails to compile here, which is
/// the cross-crate guarantee `chrome_tag_semantic`'s own doc claims.
fn chrome_action_for_semantic(semantic: pinion_overlay::ChromeTag) -> ChromeAction {
    use pinion_overlay::ChromeTag;
    match semantic {
        ChromeTag::Control(control) => ChromeAction::Control(control),
        ChromeTag::MoveGrip => ChromeAction::Move,
        ChromeTag::Resize(edge) => ChromeAction::Resize(resize_edge_to_direction(edge)),
    }
}

/// (R1190 §5.16 §5.39) The trivial 1:1 lift of the overlay's shell-neutral
/// [`WindowResizeEdge`](pinion_overlay::WindowResizeEdge) to the winit
/// [`ResizeDirection`] `drag_resize_window` needs — the ONE place the winit
/// resize type is named. Exhaustive, so an added edge fails to compile here.
fn resize_edge_to_direction(edge: pinion_overlay::WindowResizeEdge) -> ResizeDirection {
    use pinion_overlay::WindowResizeEdge as E;
    match edge {
        E::North => ResizeDirection::North,
        E::South => ResizeDirection::South,
        E::West => ResizeDirection::West,
        E::East => ResizeDirection::East,
        E::NorthWest => ResizeDirection::NorthWest,
        E::NorthEast => ResizeDirection::NorthEast,
        E::SouthWest => ResizeDirection::SouthWest,
        E::SouthEast => ResizeDirection::SouthEast,
    }
}

/// (R1189 §5.16 §5.39) The hover CURSOR a client-side chrome control requests, or
/// `None` when it commands no special cursor (the OS default arrow).
///
/// Pure (maps the `ResizeDirection` the [`chrome_action_for_tag`] SSOT already
/// resolved from the hovered tag to the matching CSS-standard resize cursor), so
/// the hover→cursor contract is unit-tested without a live window — the read side
/// of the same tag vocabulary the press side drives through `drag_resize_window`.
/// Only the eight resize regions map: a `Move` grip keeps the default arrow (the
/// GTK / Win11 caption convention — a title bar is dragged, not shown a move
/// cursor), and a `Control` button is a normal click target.
fn resize_cursor_for_action(action: ChromeAction) -> Option<CursorIcon> {
    let ChromeAction::Resize(direction) = action else {
        return None;
    };
    Some(match direction {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::NsResize,
        ResizeDirection::West | ResizeDirection::East => CursorIcon::EwResize,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::NwseResize,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::NeswResize,
    })
}

/// (R1196 §5.16 §5.39) Map a shell-neutral [`CursorHint`] a scene node declared
/// (via [`LayoutStyle::cursor`](pinion_core::style::LayoutStyle::cursor)) to the
/// winit [`CursorIcon`] — the ONE place a node's cursor hint meets winit, the
/// generic peer of [`resize_cursor_for_action`]'s chrome-tag→cursor role. The
/// generic node-hint path (a splitter divider, any future hinted widget) and the
/// R1189 chrome-resize path are two producers of the same hover cursor; this
/// maps the node-hint half. Exhaustive, so a new `CursorHint` variant fails to
/// compile until it is handled here — cross-crate exhaustiveness by the type
/// system (the R1190 pattern). Pure, unit-tested without a live window.
fn cursor_icon_for_hint(hint: CursorHint) -> CursorIcon {
    match hint {
        CursorHint::ColResize => CursorIcon::EwResize,
        CursorHint::RowResize => CursorIcon::NsResize,
        // R1405 — the pointing hand over a clickable target (an OSC-8 link).
        CursorHint::Pointer => CursorIcon::Pointer,
        // R1609 — a corner handle moves two edges at once, so it asks for the
        // diagonal cursor the toolkit spells `SizeFDiagCursor` / `SizeBDiagCursor`. The same two icons `resize_cursor_for_action`
        // has commanded for a window corner since R1189: the icons were
        // reachable and the node vocabulary was not, which is why these two
        // arms are one line each.
        CursorHint::NwseResize => CursorIcon::NwseResize,
        CursorHint::NeswResize => CursorIcon::NeswResize,
    }
}

/// (R1189 §5.16 §5.39) The min-change LATCH decision for a window's hover cursor:
/// given the icon currently commanded (`last`, `None` = the OS default arrow) and
/// the newly `desired` one, return `Some(icon)` to command winit with — the
/// desired resize icon, or the default arrow when LEAVING a region — or `None`
/// when `desired` already matches what is shown (no winit call). Pure, so the
/// redundant-suppression + the region→default reset (fired once on the
/// region→non-region transition, incl. the `CursorLeft` arm's explicit `None`)
/// are unit-tested without a live `Window`.
fn next_cursor_command(
    last: Option<CursorIcon>,
    desired: Option<CursorIcon>,
) -> Option<CursorIcon> {
    (last != desired).then(|| desired.unwrap_or(CursorIcon::Default))
}

struct WindowSlot<R: VelloRenderer> {
    /// R46.5 §5.16 suspend / resume lifecycle (R46.3.4 pattern).
    /// Lifted from the single-window `AppShell` field of the same name.
    render: RenderState<R>,
    /// Reusable Vello scene buffer — reset at the start of each
    /// frame rather than reallocated. Vello's Scene API expects this
    /// pattern (Linebender Vello examples / Xilem). Per-window because
    /// multi-window paint would otherwise serialise the same buffer
    /// across windows (race on `reset` between secondary + primary
    /// paint cycles).
    vello_scene: VelloScene,
    /// R51.62 §5.40 — per-window `accesskit_winit::Adapter`. `None`
    /// while the window is `Suspended`; populated once the winit
    /// `Window` exists. AccessKit canonical: 1 adapter = 1 window
    /// (multi-window = multi-adapter, NOT auto-merged), mirroring
    /// macOS `NSAccessibility` / GTK Atk semantics.
    accesskit: Option<accesskit_winit::Adapter>,
    /// R56.2.a §5.13 §5.38 — per-window IME composition state.
    /// Per-window because the IME session belongs to the focused
    /// window (Wayland `text-input-v3` / X11 XIM / macOS
    /// `NSTextInputContext` all scope by window); a multi-window
    /// binding's main + inspector windows could each carry their
    /// own composition session if both contained text-input widgets.
    ime_was_composing: bool,
    /// R56.2.c §5.13 §5.38 — per-window last
    /// `Window::set_ime_cursor_area` rect. Per-window so dedup against
    /// winit boundary calls works correctly when different windows
    /// publish different caret positions.
    last_ime_cursor_area: Option<(f32, f32, f32, f32)>,
    /// R668 §5.16 — per-window pending
    /// [`SizeStrategy::IntrinsicAfterFirstPaint`] resize. Per-window
    /// because the `IntrinsicAfterFirstPaint` contract is one-shot
    /// per-window-lifetime (not per-binding); mixed-strategy bindings
    /// (main: `Fixed` + inspector: `IntrinsicAfterFirstPaint`) need
    /// independent resize queues.
    pending_intrinsic_resize: Option<((u32, u32), (u32, u32))>,
    /// R670.B §5.16 — spec id (the canonical `&'static str` AI
    /// clients address the window by). Cached here so per-window
    /// dispatch code (RPC scope resolution, paint scene producer,
    /// future `view_for_window` plumbing) can resolve the spec id from
    /// the winit `WindowId` without re-walking
    /// [`AppShell::spec_id_to_window_id`]. Canonical primary spec is
    /// `"main"`; secondary specs pick their own non-conflicting
    /// names.
    ///
    /// Read by [`AppShell::render_window`] (per-window redraw drain
    /// keyed on `spec_id`) + [`AppShell::dispatch_rpc`] window-scope
    /// resolution.
    ///
    /// R683 §5.16 — `Cow<'static, str>` so dock + tear-off can mint
    /// runtime ids (`Cow::Owned(format!("torn-panel-{n}"))`)
    /// alongside the canonical static literals
    /// (`Cow::Borrowed("main")` / `Cow::Borrowed("inspector")`). All
    /// downstream `spec_id: &str` parameter sites stay unchanged —
    /// Cow's `Deref<Target = str>` covers the read API.
    spec_id: Cow<'static, str>,
    /// R682 §5.16 atomic 1 — per-window paint-fragment cache. Lives
    /// per `WindowSlot` because the cached `vello::Scene` fragments
    /// reference the same backend coordinate space as
    /// [`Self::vello_scene`]; sharing a cache across windows would
    /// require encoding fragments with explicit transform metadata
    /// for cross-window replay, which is out of scope for the
    /// 4-axis paint-pipeline rewrite series.
    ///
    /// The cache is consulted by [`paint_adapter::to_vello_cached`]
    /// (R682 atomic 1) at every cacheable [`Scene::Container`](pinion_core::Scene::Container)
    /// boundary the encoder reaches with [`Affine::IDENTITY`]
    /// accumulated transform. A cache hit appends the previously
    /// encoded fragment via [`vello::Scene::append`] without
    /// re-walking the subtree; a miss encodes the subtree fresh,
    /// installs the fragment, and replays from the install slot.
    ///
    /// Mark-and-sweep eviction (handled inside
    /// [`FragmentCache::end_paint`](pinion_runtime::paint_adapter::FragmentCache::end_paint)) keeps the cache bounded to the
    /// set of cacheable Containers actually painted in the most
    /// recent frame — no fixed-cap LRU; no manual reset between
    /// frames.
    fragment_cache: paint_adapter::FragmentCache,
    /// R740 §5.16 — per-window decoded-image cache. Sits beside the
    /// fragment cache (both are vello render state) and feeds
    /// [`paint_adapter::to_vello_cached`] so `Scene::Image` sources are
    /// decoded once and reused every frame. Per-window for now (a
    /// multi-window image consumer that wants one shared decode is an
    /// additive app-level move, [[abstraction-needs-second-consumer]]).
    ///
    /// R1404 §5.16 — built with [`image_cache::with_store`](image_cache::ImageCache::with_store)
    /// off the shell's seeded [`image_cache::IMAGE_STORE`], so a
    /// `memory://<key>` source resolves to a producer-registered in-memory
    /// image (the mutable, filesystem-free path the sprag terminal
    /// inline-graphics consumer needs). The store is process-shared; only the
    /// decode-once map for filesystem sources is per-window.
    image_cache: image_cache::ImageCache,
    /// R1027 §5.16 — the window's current winit `scale_factor` (device
    /// pixels per logical pixel). Seeded from `Window::scale_factor()` at
    /// slot construction and refreshed on
    /// `WindowEvent::ScaleFactorChanged`. The paint scene is laid out in
    /// logical pixels; this factor is applied only at the GPU raster
    /// boundary (the scaled vello append in `render_window`), the
    /// pointer-input boundary (`CursorMoved` / `Touch` physical ->
    /// logical), and the AccessKit root transform. `1.0` on a non-`HiDPI`
    /// display, which keeps the byte-identical pre-R1027 render path.
    scale_factor: f64,
    /// R1027 §5.16 — reusable scratch scene for the `HiDPI` scaled append.
    /// When `scale_factor` is non-identity, `render_window` appends the
    /// logical `vello_scene` into this buffer under an
    /// `Affine::scale(scale_factor)` so the logical scene rasterizes at
    /// device resolution; the renderer then submits this scene. At `1.0`
    /// the renderer submits `vello_scene` directly and this buffer stays
    /// untouched (no extra append, so non-`HiDPI` performance is
    /// unchanged). Per-window for the same reason `vello_scene` is (no
    /// cross-window buffer race on `reset`).
    scaled_scene: VelloScene,
    /// R1060 §5.16 §5.12 — one-shot live-surface capture request, whose
    /// set+clear lifecycle [`AppShell::capture_window_screenshot`] OWNS
    /// (R1062): it sets the flag, drives one [`AppShell::render_window`]
    /// pass, then clears it unconditionally. `render_window` only READS it
    /// and, when set, submits the frame through
    /// [`VelloRenderer::capture_rgba8`] (which reads back the presented
    /// swapchain texture) instead of the normal `render` present. The
    /// unconditional caller-side clear means an early-return `render_window`
    /// path cannot leave it stale, so it is `false` on every
    /// event-loop-driven paint and the 60-144fps hot path is byte-unchanged.
    pending_capture: bool,
    /// R1060 §5.16 §5.12 — the most recent capture result, written by
    /// `render_window` when `pending_capture` was set and drained by
    /// `capture_window_screenshot` immediately after. `None` between
    /// captures.
    last_capture: Option<crate::vello_capture::CapturedFrame>,
    /// R1088 §5.16 §5.41 §2 #7 PR-31 — the last outer position the SHELL
    /// commanded for this window via `Window::set_outer_position` (the
    /// reconcile move pass or the `Suspended`-resume re-apply), in logical
    /// px, that has not yet been observed echoing back as a
    /// `WindowEvent::Moved`. [`AppShell::note_window_moved`] suppresses a
    /// `Moved` equal to it (our own command echoing) and writes back only
    /// a DIVERGENT one (a user / WM title-bar drag), so a shell-driven
    /// move does not loop and a user move converges the declared
    /// `windows_signal` on the actual position. `None` once the echo is
    /// consumed, or when the shell has commanded no position (every
    /// WM-placed window — the typical `"main"`).
    last_commanded_position: Option<(i32, i32)>,
    /// (R1189 §5.16 §5.39) The client-side resize cursor currently commanded for
    /// this window, or `None` when the pointer is over no resize region (the OS
    /// default arrow). A borderless (CSD) window owns its hover affordance — the
    /// OS gives a decorated window a resize cursor over its frame for free, but a
    /// borderless one has no frame, so [`AppShell::command_resize_cursor`] maps a
    /// hover over a `WINDOW_RESIZE_*` region to the matching `CursorIcon`. This is
    /// the min-change LATCH (like [`Self::last_commanded_position`]): winit's
    /// `set_cursor` is called only when the desired icon actually changes, so the
    /// per-`CursorMoved` hot path stays a hover-target read + an equality compare.
    /// A window that never enters a resize region keeps this `None` and is never
    /// commanded a cursor at all (so a decorated window's client area is
    /// untouched — the OS keeps managing it).
    last_resize_cursor: Option<CursorIcon>,
}

impl<R: VelloRenderer> WindowSlot<R> {
    /// R682 §5.16 — collapse the per-slot field defaults that the
    /// `resume_spec` code path repeats at both the suspended-init and
    /// active-init sites. Only `render`, `accesskit`, `spec_id`,
    /// `pending_intrinsic_resize`, and `scale_factor` (R1027) vary
    /// between the two sites; every other field starts at its canonical
    /// empty-state value.
    fn build(
        render: RenderState<R>,
        accesskit: Option<accesskit_winit::Adapter>,
        spec_id: Cow<'static, str>,
        pending_intrinsic_resize: Option<((u32, u32), (u32, u32))>,
        scale_factor: f64,
        image_store: image_cache::MemoryImageStore,
    ) -> Self {
        Self {
            render,
            vello_scene: VelloScene::new(),
            accesskit,
            ime_was_composing: false,
            last_ime_cursor_area: None,
            pending_intrinsic_resize,
            spec_id,
            fragment_cache: paint_adapter::FragmentCache::new(),
            // R1404 §5.16 — the window's cache is wired to the shell's seeded
            // producer store, so `memory://<key>` sources paint the images a
            // producer registered.
            image_cache: image_cache::ImageCache::with_store(image_store),
            scale_factor,
            scaled_scene: VelloScene::new(),
            pending_capture: false,
            last_capture: None,
            last_commanded_position: None,
            last_resize_cursor: None,
        }
    }
}

/// (R1147 §5.51 §5.16) The shell-private cross-desktop drag preview window —
/// the toolkit-ADS `CFloatingDragPreview` model. A small, opaque, borderless, always-on-top
/// window that *is* the drag chip; it follows the desktop cursor during a dock
/// drag so the chip can escape the source window (the R1113 in-window overlay
/// is clipped to its surface, the gap the user found after R1146).
///
/// Created **once and reused** (hidden via `set_visible(false)` between drags —
/// no per-gesture window creation, so no R1144-class surface-churn freeze),
/// **moved by a direct `set_outer_position`** from the DESKTOP cursor (never the
/// reactive `Signal → reconcile_windows` path, and the cursor — not the window's
/// own moved position — is read, so the R1119 feedback oscillation is
/// structurally impossible), and **repainted only when the dragged label
/// changes** (render-once, move-many: zero per-move GPU work).
///
/// Deliberately ABSENT from [`AppShell::windows`] / `spec_id_to_window_id` / the
/// substrate window registry / `windows_signal`, so it never appears in
/// `scene/windows`: a transient shell affordance like the R1113 overlay + focus
/// ring, NOT declared scene-as-data (§2 #7). The during-drag chip stays
/// AI-introspectable through the R1113 in-window overlay in `scene/snapshot`,
/// which this window mirrors visually for the human.
struct DragPreviewWindow<R: VelloRenderer> {
    /// The OS window — the chip itself (opaque, borderless, always-on-top).
    window: Arc<Window>,
    /// Cached winit id so the `RedrawRequested` / `Resized` arms route a preview
    /// frame here without a `windows` lookup (the preview is not in that map).
    window_id: WindowId,
    /// Per-preview Vello renderer (its own GPU surface). Tiny fixed scene, so no
    /// fragment / image cache, no AccessKit, no IME — unlike a [`WindowSlot`].
    renderer: Box<R>,
    /// Reusable encode buffer (reset per repaint, like `WindowSlot::vello_scene`).
    vello_scene: VelloScene,
    /// OS DPI scale for the preview's monitor (seeded at create; the chip is
    /// laid out logical and rastered at device resolution like every window).
    scale_factor: f64,
    /// Whether the window is currently mapped (`set_visible(true)`). Shown on a
    /// preview-eligible drag, hidden on release.
    visible: bool,
    /// The label the chip is currently painted with (`None` until first paint).
    /// The shell repaints + resizes-to-fit only when the active drag's label
    /// differs, so a steady cursor move is a pure `set_outer_position`.
    painted_label: Option<String>,
}

/// The framework-side shell. Generic over a widget binding
/// [`WidgetView`]; concrete examples instantiate via `run::<V>()`.
///
/// R51.76 §5.40 — every piece of testable dispatch state lives in
/// [`ShellCore`]; this struct only owns the winit / wgpu / AccessKit
/// surface so headless tests can target [`ShellCore`] directly.
///
/// R51.92.1 §5.40 — moved from `lib.rs` to its own module so every
/// field below (including the `core: ShellCore<V>` substrate borrow
/// added in R51.76) is genuinely private to `app.rs`. The lib.rs
/// entry point reaches the shell only through [`run`] / the
/// re-exported `AppShell::new` constructor; private state is now
/// module-bound rather than file-bound.
pub struct AppShell<V: WidgetView> {
    /// R51.76 §5.40 — extracted dispatch substrate (scene, cached
    /// state, focus, intents, previews, revision, router, modifiers,
    /// text cache, last paint snapshot, AT caches, redraw flag).
    ///
    /// R51.83 §5.40 — private. All surface-side access happens
    /// through the substrate's typed methods + accessors so the
    /// boundary stays one-way.
    ///
    /// R51.92.1 §5.40 — module-private. Even `lib.rs` (the entry
    /// crate root) cannot touch this field directly.
    ///
    /// R670.B §5.16 — single `ShellCore` + multi-window (Approach A):
    /// the binding's state lives here once; every window's view fn
    /// reads the same `cached_state`. Multi-binding (different
    /// `ShellCore` per window) is R750+ widget catalog territory
    /// (Approach B).
    core: ShellCore<V>,
    /// R670.B §5.16 §5.41 — per-window state cluster, keyed by winit
    /// `WindowId`. R670.A landed `WidgetView::windows() -> Vec<WindowSpec>`
    /// with default `vec![WindowSpec::main(...)]`; R670.B walks the
    /// list in `resumed()` + creates one `WindowSlot` per spec.
    ///
    /// Pre-R670.B this struct carried 5 single-window fields
    /// directly (`render` + `vello_scene` + `accesskit` + IME +
    /// intrinsic resize). The cluster lift preserves bit-identical
    /// single-window behaviour by anchoring the canonical primary
    /// spec (`WindowSpec::main`) as the only window the 15+ existing
    /// bindings ever create — the hashmap holds exactly one entry
    /// for them.
    windows: HashMap<WindowId, WindowSlot<V::Renderer>>,
    /// R670.B §5.16 — spec-id → `WindowId` reverse lookup for RPC
    /// `{window: "<id>"}` scope resolution (R670.B atomic 1). AI
    /// clients address windows by the `&'static str` id declared in
    /// `WindowSpec::new(id, ..)`; the dispatcher resolves the id to a
    /// winit `WindowId` here before looking up the per-window slot
    /// in [`Self::windows`]. Default single-window bindings carry
    /// exactly `"main" → primary_id`.
    spec_id_to_window_id: HashMap<Cow<'static, str>, WindowId>,
    /// ★★★★★ R1701 §5.16 §5.49 — the consecutive-click ordinal for presses on
    /// this window's CLIENT-SIDE CHROME, per window.
    ///
    /// The widget router keeps its own such window per pointer, and a chrome
    /// press never reaches it: [`Self::try_chrome_press`] consumes the press and
    /// returns before `pointer_button_for_window` runs, deliberately, because a
    /// title bar is not a widget. So a title bar could not tell a second click
    /// from a first, and double-clicking it started the OS move drag twice —
    /// measured at R1701, and below the floor, whose in-application window kinds
    /// maximise on exactly that gesture.
    ///
    /// The RULE is not re-derived here: [`pinion_core::input::DoubleClickWindow`]
    /// owns the time and distance thresholds that the router reads too, which is
    /// what keeps a title bar's idea of a double click and a widget's from
    /// drifting apart.
    chrome_click_window: HashMap<Cow<'static, str>, pinion_core::input::DoubleClickWindow>,
    /// R670.B §5.16 — primary window's [`WindowId`]. The first spec
    /// in `V::windows()` (canonically `WindowSpec::main(..)`); RPC
    /// frames that omit `{window: "..."}` default-scope to this id.
    /// `None` until `resumed()` creates the first window; populated
    /// once per binding lifetime + never cleared by `suspended`
    /// (which keeps the windows cached for the next `resumed`).
    primary_window_id: Option<WindowId>,
    /// R51.62 §5.40 — winit [`EventLoopProxy`] cached so the shell
    /// can construct the per-window `accesskit_winit::Adapter` on
    /// `resumed` (the constructor requires both the active event
    /// loop and a proxy that produces `Adapter`-routed user events).
    proxy: EventLoopProxy<AppEvent>,
    /// R683 §5.16 §5.41 — runtime window-list reactive lift.
    ///
    /// Populated on the first [`Self::resumed`] when the binding's
    /// [`WidgetView::windows_signal`] returns `Some(..)`. Kept here so
    /// [`Self::reconcile_windows`] (the
    /// [`AppEvent::WindowsDirty`] handler) can re-read the latest
    /// `Signal<Vec<WindowSpec>>` snapshot without going back through
    /// the trait (which would re-evaluate the binding's
    /// `Owner::cache` factory each call — the trait impl pattern
    /// memoises but the shell-side cache makes the contract
    /// explicit).
    ///
    /// `None` for every pre-R683 single + multi-window binding —
    /// they inherit the default `fn windows_signal() -> None` and the
    /// reconcile Effect is never installed.
    windows_signal: Option<Rc<Signal<Vec<WindowSpec>>>>,
    /// R683 §5.16 §5.41 — lifetime anchor for the reconcile Effect.
    ///
    /// The Effect is registered against `self.core.root_owner()` so
    /// its cleanup queue holds a strong reference for the binding's
    /// lifetime — but the AppShell-side `Option<Effect>` keeps the
    /// handle live + accessible for diagnostics (test asserts the
    /// Effect was actually installed). `None` when
    /// `WidgetView::windows_signal()` returned `None` (no opt-in,
    /// no Effect, no reactive subscription).
    ///
    /// The handle is read in [`Self::resumed`] (the
    /// `self.reconcile_effect.is_none()` install gate) and never
    /// otherwise — its real job is to extend the [`Effect`]'s
    /// lifetime past the install scope so the Effect closure stays
    /// alive across event-loop iterations. The Owner cleanup queue
    /// also holds a `Weak<EffectInner>` so the cleanup-on-shutdown
    /// path is correct independent of this handle.
    reconcile_effect: Option<Effect>,
    /// R683 §5.16 §5.41 — last spec list snapshot the reconcile
    /// Effect observed, used to compute the add/drop diff.
    ///
    /// Initialised to `V::windows_signal().get()` on the first
    /// `resumed()` when the Effect is installed; updated to the
    /// freshly-emitted `Signal::get()` snapshot at the end of each
    /// `reconcile_windows` call. Wrapped in `Rc<RefCell<..>>`
    /// because the Effect closure cannot capture `&mut self` (it
    /// lives past the `AppShell` constructor scope); the `AppShell`
    /// + the closure share the same `Rc<RefCell<..>>` handle.
    ///
    /// An empty `Vec` sentinel for the pre-install state — when
    /// [`AppEvent::WindowsDirty`] fires before the install completes
    /// (a degenerate corner case) the diff fires "no adds, no drops"
    /// against the empty baseline.
    last_known_specs: Rc<RefCell<Vec<WindowSpec>>>,
    /// R1147 §5.51 §5.16 — the shell-private cross-desktop drag preview window
    /// ([`DragPreviewWindow`]). `None` until the first preview-eligible drag
    /// lazily creates it; then persisted (hidden between drags). Kept OUTSIDE
    /// `windows` / `spec_id_to_window_id` so it is invisible to `scene/windows`
    /// (§2 #7) and untouched by the reconcile / dispatch paths.
    drag_preview: Option<DragPreviewWindow<V::Renderer>>,
}

impl<V: WidgetView> AppShell<V> {
    /// R51.76 §5.40 — construct the shell with a freshly-built state
    /// scene and the initial cached state read through the §5.15
    /// introspect channel. Delegates the dispatch substrate to
    /// [`ShellCore::new`]; this constructor only adds the winit /
    /// wgpu / AccessKit surface (`render`, `vello_scene`, `proxy`,
    /// `accesskit`) which lives on `AppShell` itself.
    ///
    /// `proxy` is the same `EventLoopProxy<AppEvent>` the
    /// `spawn_stdin_rpc_reader` background thread holds; the shell
    /// retains a clone so it can hand a fresh copy to the
    /// `accesskit_winit::Adapter` on `resumed` (R51.62 §5.40).
    #[must_use]
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self::new_with_fonts(proxy, Vec::new(), None)
    }

    /// R1448 §5.36 — [`Self::new`] plus the faces the application declared
    /// through [`ShellConfig::with_application_font`]. They are registered into
    /// the shell's render cache before the binding's root owner is seeded, so
    /// the first `view` both can select them and can read the resulting
    /// [`FontSourceReport`](pinion_core::reactive::FontSourceReport).
    ///
    /// R1472 §5.36 — `default_family` is the family unset text resolves to
    /// ([`ShellConfig::with_default_font_family`], the toolkit's
    /// `setFont`); `None` keeps the platform stack.
    #[must_use]
    pub fn new_with_fonts(
        proxy: EventLoopProxy<AppEvent>,
        app_fonts: Vec<Vec<u8>>,
        default_family: Option<pinion_core::style::FontFamily>,
    ) -> Self {
        // R999 §5.23 / R1362 PR-65 — seed the binding's root Owner with the live
        // `EventLoopProxy`-backed boundary handles before `ShellCore::new_with_seed`
        // runs the binding factories, so a binding's `create_extra_externals` can
        // capture them (`use_repaint_sink()` / `use_window_control_sink()`) for an
        // off-thread producer. All three ride the ONE seeding window, and the
        // R1366.x migrations have now moved every one of them to a `ProviderSlot`:
        // `REPAINT_SINK` (R1366.1), `QUIT_SINK` (R1366.2) and `WINDOW_CONTROL_SINK`
        // (R1366.4), so a seed after this point PANICS rather than being silently
        // dropped to hand the binding a dead handle. `new_with_seed` is what makes
        // "before any read" structural, so that panic is never reached here.
        let seed_proxy = proxy.clone();
        Self {
            core: ShellCore::new_with_seed_and_fonts(
                app_fonts,
                default_family,
                move |root_owner| {
                    pinion_core::REPAINT_SINK.provide(
                        root_owner,
                        std::sync::Arc::new(crate::ProxyRepaintSink::new(seed_proxy.clone())),
                    );
                    crate::window_control::WINDOW_CONTROL_SINK.provide(
                        root_owner,
                        std::sync::Arc::new(crate::ProxyWindowControlSink::new(seed_proxy.clone())),
                    );
                    // R1363 §5.55 — the app-lifecycle sink rides the same seeding
                    // window as its window-lifecycle peer.
                    pinion_core::QUIT_SINK.provide(
                        root_owner,
                        std::sync::Arc::new(crate::ProxyQuitSink::new(seed_proxy)),
                    );
                    // R1576 §5.16 §5.41 §5.23 — the desk a binding reads to
                    // NAME a display in a `WindowSpec`. Rides the same one
                    // seeding window as the sinks, so a binding's
                    // `create_extra_externals` can hold the handle; it starts
                    // empty and the surface stamps it at every window create
                    // and RPC dispatch.
                    crate::displays::DISPLAYS
                        .provide(root_owner, std::sync::Arc::new(crate::DisplayHandle::new()));
                },
            ),
            windows: HashMap::new(),
            spec_id_to_window_id: HashMap::new(),
            chrome_click_window: HashMap::new(),
            primary_window_id: None,
            proxy,
            windows_signal: None,
            reconcile_effect: None,
            last_known_specs: Rc::new(RefCell::new(Vec::new())),
            drag_preview: None,
        }
    }

    /// R670.B §5.16 — borrow the primary window's slot, if any.
    /// `None` until `resumed()` has created the primary window. The
    /// primary spec is `V::windows()[0]` (canonically
    /// `WindowSpec::main`); RPC scope default + single-window legacy
    /// paths reach the window through this accessor.
    fn primary_slot(&self) -> Option<&WindowSlot<V::Renderer>> {
        self.primary_window_id.and_then(|id| self.windows.get(&id))
    }

    /// R670.B §5.16 §5.41 — pull the `RenderState` Window arc out of
    /// a slot (active or suspended-with-cached-window). Both render
    /// state variants may carry a `Window` arc; this helper
    /// canonicalises the lookup so callers don't repeat the match.
    fn slot_window(slot: &WindowSlot<V::Renderer>) -> Option<&Arc<Window>> {
        match &slot.render {
            RenderState::Active { window, .. } => Some(window),
            RenderState::Suspended(maybe) => maybe.as_ref(),
        }
    }

    /// R1148 §5.51 §5.16 → R1151 — stamp every live window's ACTUAL client origin
    /// (logical px) into the core so the LIVE cross-window redock resolution maps
    /// the desktop cursor against real desktop positions, not the DECLARED ones — a
    /// WM-placed `"main"` declares position `None` → `(0,0)` but sits at a real WM
    /// offset, which put every floater→main redock off by that offset (the
    /// user-found "좌표 안 맞아" bug). Gated on an active drag this window owns, so
    /// idle hovers + single-window apps skip the `inner_position()` queries. Runs
    /// each `CursorMoved` BEFORE `cursor_moved_for_window` resolves the drop.
    fn stamp_live_window_origins(&self, window_id: WindowId) {
        let Some(spec_id) = self.windows.get(&window_id).map(|s| &*s.spec_id) else {
            return;
        };
        if !self
            .core
            .drag_session_active_for_window(spec_id, PointerId::MOUSE)
        {
            return;
        }
        self.core
            .set_live_window_origins(self.collect_window_origins());
    }

    /// R1149 §5.51 §2 #7 §2 #2 — stamp every live window's ACTUAL outer origin
    /// UNCONDITIONALLY (no drag gate), for the RPC dispatch path. The winit cursor
    /// path stamps in [`Self::stamp_live_window_origins`] (gated on an active
    /// drag), but an RPC `scene/drag` opens the drag DURING its own drain, so the
    /// gate would skip the stamp and the cross-window resolution would fall back to
    /// DECLARED origins (a WM-placed `"main"` at `(0,0)`) — diverging from the live
    /// winit path and making an AI's RPC drive unable to reproduce / diagnose the
    /// live coordinate behavior. Origins are static during a dispatch, so one
    /// unconditional stamp at dispatch entry covers the whole RPC drag drain.
    fn stamp_all_window_origins(&self) {
        self.core
            .set_live_window_origins(self.collect_window_origins());
    }

    /// R1149 §5.51 → R1151 — collect every live window's ACTUAL CLIENT-area origin
    /// in logical pixels (`Window::inner_position()` → logical). Shared by the
    /// winit-path [`Self::stamp_live_window_origins`] and the RPC-path
    /// [`Self::stamp_all_window_origins`] so both resolve cross-window drops
    /// against the same real desktop positions.
    ///
    /// R1151 — `inner_position` (CLIENT top-left), NOT `outer_position` (the
    /// decorated FRAME top-left). The dock scene a cross-window drop hit-tests
    /// against is CLIENT-relative, so a decorated host (e.g. gnome adds a 37px
    /// title bar: `_NET_FRAME_EXTENTS` top=37, so `outer.y = client.y − 37`) made
    /// the hit-test land a title-bar's-worth off — resolving the wrong panel
    /// (a toolbar / the panel's own slot) so the redock fired but did not relocate
    /// ("dropped on the preview, didn't dock"). A borderless floater has no frame,
    /// so `inner == outer` there; this only corrects the decorated host.
    fn collect_window_origins(&self) -> Vec<(String, (f64, f64))> {
        self.windows
            .values()
            .filter_map(|slot| {
                let window = Self::slot_window(slot)?;
                let phys = window.inner_position().ok()?;
                let logical = PhysicalPosition::new(f64::from(phys.x), f64::from(phys.y))
                    .to_logical::<f64>(slot.scale_factor);
                Some((slot.spec_id.to_string(), (logical.x, logical.y)))
            })
            .collect()
    }

    /// R1149 / R1576 §5.51 §5.16 §2 #7 §2 #2 — push the two facts about the
    /// **desktop outside this process** that an RPC dispatch will read, before
    /// it reads them.
    ///
    /// Both exist because `ShellCore` is backend-agnostic by construction (no
    /// winit, no wgpu) while these facts belong to the window system, so the
    /// surface pushes and the substrate reads. Grouped because they are one
    /// obligation at one moment — "the desktop as it is at the start of this
    /// dispatch" — and because two separate stamps in the dispatch body is how
    /// a third one gets added at the wrong point later.
    ///
    /// * **Window origins** (R1149): every window's ACTUAL outer origin, so a
    ///   cross-window resolution during this RPC (a `scene/drag` redock) uses
    ///   the same real positions the live winit path does. Without it the RPC
    ///   drain fell back to DECLARED origins (a WM-placed `"main"` at
    ///   `(0, 0)`), so an agent's RPC drive could not reproduce — let alone
    ///   diagnose — a live coordinate divergence. Multi-window only: a single
    ///   window never resolves cross-window.
    /// * **The display topology** (R1576): the monitors attached right now, for
    ///   `scene/displays` and for the placement `scene/windows` reports.
    ///   Stamped per dispatch rather than cached at boot because winit 0.30
    ///   emits no monitor-change event — a cached desk would have no
    ///   invalidation signal and would answer confidently with yesterday's
    ///   arrangement, which is the failure this axis exists to remove.
    fn stamp_desktop_facts(&self) {
        if self.windows.len() > 1 {
            self.stamp_all_window_origins();
        }
        self.stamp_window_homes();
    }

    /// R1617 §5.16 §5.41 §2 #7 — publish the desk and, against that same
    /// reading, where every live window is according to both answerers.
    ///
    /// Called at every RPC dispatch (through [`Self::stamp_desktop_facts`]) and
    /// at the two winit moments the answer can actually change: a window being
    /// created and a window being moved. The second matters for the
    /// binding-facing [`crate::use_window_home`] rather than for the wire — a
    /// GUI-only session issues no dispatches at all, so a home stamped only
    /// there would be permanently absent in exactly the sessions a painted
    /// readout lives in.
    ///
    /// The cost is proportionate and was measured against the backend rather
    /// than assumed: the monitor enumeration is cached behind a lock there, and
    /// what is left is one outer-position read per window, which is what
    /// [`Self::stamp_live_window_origins`] already pays on the far hotter
    /// cursor path.
    fn stamp_window_homes(&self) {
        // ONE reading of the desk feeds both products. The topology's ids ARE
        // positions in this enumeration, so resolving a window's own monitor
        // against a second, later call would index into a topology it was not
        // built from — and a hot-plug landing between the two calls would
        // manufacture a divergence this process invented and then report it as
        // if the window system had disagreed.
        let (topology, monitors) = self.desk_reading();
        let homes = self.collect_window_homes(&topology, &monitors);
        self.publish_display_topology(topology, homes);
    }

    /// R1617 §5.16 §5.41 §2 #7 — per live window, its **actual** outer
    /// rectangle and the display the window system itself says it is on.
    ///
    /// The third and last member of the display axis's untestable surface,
    /// beside [`display_info_from`] and [`topology_from`], and it is kept to
    /// the same standard: nothing here decides anything. The rectangle is a
    /// field-for-field move, the platform's opinion is resolved to an id by its
    /// position in the enumeration `topology` was built from, and the whole
    /// judgment — do the two answers agree, and what is it called when they do
    /// not — lives in [`pinion_core::display::DisplayHome`], where an
    /// arrangement is an argument a test writes.
    ///
    /// A window whose outer position the platform declines to report is
    /// **omitted**, so `scene/windows` publishes `null` for it rather than a
    /// home derived from a rectangle nobody supplied. Nobody looked, so nothing
    /// is claimed — the rule `anchored` and `level_outcome` already use.
    ///
    /// Resolving the monitor by enumeration POSITION rather than by name is
    /// deliberate: a monitor's reported name is optional and routinely repeats
    /// across identical panels, which is the whole reason
    /// [`pinion_core::display::DisplayId`] is derived rather than taken. The
    /// position is exact, and it is valid because `monitors` is the very list
    /// `topology` was built from — which is why the two arrive together from
    /// [`Self::desk_reading`] rather than being enumerated here.
    fn collect_window_homes(
        &self,
        topology: &DisplayTopology,
        monitors: &[MonitorHandle],
    ) -> Vec<(String, DisplayRect, Option<DisplayId>)> {
        self.windows
            .values()
            .filter_map(|slot| {
                let window = Self::slot_window(slot)?;
                let position = window.outer_position().ok()?;
                let size = window.outer_size();
                let rect = DisplayRect::new(position.x, position.y, size.width, size.height);
                let platform = window
                    .current_monitor()
                    .and_then(|current| monitors.iter().position(|m| *m == current))
                    .and_then(|index| topology.nth(index))
                    .map(|display| display.id().clone());
                Some((slot.spec_id.to_string(), rect, platform))
            })
            .collect()
    }

    /// R1610 §5.16 §2 #7 — stamp which windowing system this process is talking
    /// to, once, from the live event loop.
    ///
    /// Deliberately NOT part of [`Self::stamp_desktop_facts`], which re-reads the
    /// desk on every dispatch because a monitor can be plugged in mid-session.
    /// A process does not migrate from X11 to Wayland while running, so re-asking
    /// would be pure cost — and asking once at the one place that holds an
    /// `ActiveEventLoop` is what makes it a read rather than a build-target guess.
    fn stamp_windowing_backend(&self, event_loop: &ActiveEventLoop) {
        let backend = detect_windowing_backend(event_loop);
        self.core.set_windowing_backend(backend);
        tracing::debug!(
            target: "pinion::shell",
            backend = backend.as_str(),
            "windowing backend detected",
        );
    }

    /// R1576 §5.16 §5.41 — publish one reading of the desk to BOTH readers:
    /// the substrate (which answers `scene/displays` and resolves
    /// `scene/windows`' placements) and the binding-facing
    /// [`crate::use_displays`] handle.
    ///
    /// One function because two stamp sites with two readers is four places for
    /// them to disagree about what the desk is, and "the wire says one thing
    /// and the binding sees another" is the exact class of defect the whole
    /// declared-placement design exists to prevent.
    ///
    /// R1617 — `homes` travels with the topology for the same reason and one
    /// stronger: a window rectangle is only interpretable against the desk it
    /// was measured on, so publishing the two separately would let a hot-plug
    /// between the calls produce a divergence this process invented. Both
    /// readers get one reading.
    fn publish_display_topology(
        &self,
        topology: DisplayTopology,
        homes: Vec<(String, DisplayRect, Option<DisplayId>)>,
    ) {
        self.core.set_display_topology(topology.clone());
        self.core.set_window_homes(homes.clone());
        self.core.root_owner().run(|| {
            let handle = crate::displays::use_display_handle();
            handle.set(topology);
            handle.set_homes(homes);
        });
    }

    /// R1576 §5.16 §5.41 §2 #7 — the monitors attached **right now**.
    ///
    /// Read live from any window rather than cached on the shell, so a monitor
    /// plugged in mid-session is simply seen. winit 0.30 emits no
    /// monitor-change event, so a cache here would have no invalidation signal
    /// at all and would answer confidently with yesterday's desk — the failure
    /// mode this whole axis exists to remove.
    ///
    /// A shell with no window yet (pre-`Resumed`, or a suspended mobile state)
    /// answers with the empty topology, which every derivation is total on.
    /// `resume_spec` does not come through here: at create time there is no
    /// window and the `ActiveEventLoop` is the enumeration source.
    fn display_topology(&self) -> DisplayTopology {
        self.desk_reading().0
    }

    /// R1617 — the desk, and the monitor enumeration it was built from.
    ///
    /// The two travel together because a [`pinion_core::display::DisplayId`] is
    /// derived from a display's POSITION in this list, so the list is the only
    /// thing that can turn a window system's monitor handle back into one of
    /// those ids. Handing back the topology alone would leave a caller with no
    /// way to do that except to enumerate again — against which the positions
    /// are no longer guaranteed to line up.
    fn desk_reading(&self) -> (DisplayTopology, Vec<MonitorHandle>) {
        let Some(window) = self.windows.values().find_map(Self::slot_window) else {
            return (DisplayTopology::empty(), Vec::new());
        };
        let monitors: Vec<MonitorHandle> = window.available_monitors().collect();
        let primary = window.primary_monitor();
        (
            topology_from(monitors.iter().cloned(), primary.as_ref()),
            monitors,
        )
    }

    /// R1576 §5.16 §5.41 — drive a live window to a declared placement, and
    /// answer with what became of it.
    ///
    /// The two arms differ in the frame they command in, and that is the only
    /// difference: an absolute placement goes out as a `LogicalPosition`
    /// exactly as it has since R1087 (byte-identical for every pre-R1576
    /// binding), while a display-relative one is resolved to an absolute
    /// **physical** point first, because that is the space the resolution
    /// happened in and re-dividing it by the window's own scale would
    /// reintroduce the per-window guess the display axis exists to retire.
    ///
    /// The returned [`Anchored`] is what the caller latches and what
    /// `scene/windows` publishes, so the fact that a named display was
    /// substituted travels with the move instead of being recomputed by
    /// someone else later.
    ///
    /// The DECISION — which frame, which numbers — is
    /// [`placement_command`], a pure function, so what this body does is
    /// exactly one `set_outer_position` call. That split is not tidiness: on a
    /// single-monitor 1x desk the two frames produce identical numbers, so a
    /// bug that ignored the display entirely would be invisible to any test
    /// that could run on this host. Against a fabricated two-panel high-DPI
    /// desk it is not, and that is where the decision is pinned.
    fn apply_placement(
        window: &Window,
        scale: f64,
        topology: &DisplayTopology,
        placement: &WindowPlacement,
    ) -> (Anchored, (i32, i32)) {
        let (anchored, command, latch) = placement_command(placement, topology, scale);
        match command {
            Some(PlacementCommand::Logical(x, y)) => {
                window.set_outer_position(LogicalPosition::new(f64::from(x), f64::from(y)));
            }
            Some(PlacementCommand::Physical(x, y)) => {
                window.set_outer_position(PhysicalPosition::new(x, y));
            }
            // No displays at all: there is nowhere to put it, and commanding a
            // position would be inventing one.
            None => {}
        }
        (anchored, latch)
    }

    /// R51.38 / R1027 / R1120 §5.35 §5.16 §5.51 — the `CursorMoved` body.
    ///
    /// winit mouse events are single-source on every desktop platform pinion
    /// supports, so the shell threads `PointerId::MOUSE` unconditionally.
    /// `position` is physical; convert to logical so it shares the router's
    /// coordinate space (the same logical space the paint scene lays out in) —
    /// without this a small hit target (splitter handle, slider thumb) is
    /// unreachable on `HiDPI`.
    ///
    /// [`Self::stamp_live_window_origins`] (R1148) runs BEFORE the cursor is
    /// forwarded so an in-flight cross-window redock hit-tests against every live
    /// window's ACTUAL client origin, not the declared one. The `DEFAULT_WINDOW`
    /// fallback mirrors the other pointer arms (an untracked window — a `Resumed`
    /// event landing before the slot is inserted).
    /// R1147 §5.16 — initialise a Vello renderer against `window`'s surface at
    /// its current physical inner size. Shared by [`Self::resume_spec`] (declared
    /// windows) and the R1147 drag-preview window so both cross the §6.3
    /// `pollster::block_on` boundary identically. Returns the boxed renderer or
    /// the backend init error (surface / adapter / device).
    fn build_renderer(
        window: &Arc<Window>,
    ) -> Result<Box<V::Renderer>, <V::Renderer as WidgetRenderer>::Error> {
        let size = window.inner_size();
        pollster::block_on(<V::Renderer as VelloRenderer>::new(
            Arc::clone(window),
            size.width.max(1),
            size.height.max(1),
        ))
        .map(Box::new)
    }

    /// R1147 §5.51 §5.16 — lazily create the shell-private cross-desktop drag
    /// preview window ([`DragPreviewWindow`]), sized to the chip for `label` at
    /// `style`. Borderless + always-on-top + opaque (the window IS the chip),
    /// created HIDDEN so the caller maps it explicitly. Idempotent — a no-op once
    /// built (the window is reused across drags; a later label's resize-to-fit is
    /// [`Self::update_drag_preview`]'s job). Created OUTSIDE `windows` /
    /// `spec_id_to_window_id` / the substrate window registry, so it never
    /// reaches `scene/windows` (§2 #7). The first-ever eligible drag is the only
    /// window creation; subsequent drags reuse it.
    fn ensure_drag_preview_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        label: &str,
        style: pinion_overlay::DragImageStyle,
    ) {
        if self.drag_preview.is_some() {
            return;
        }
        let (w, h) = pinion_overlay::chip_size(label, style);
        let attrs = Window::default_attributes()
            .with_title("pinion-drag-preview")
            .with_decorations(false)
            // R1610 — through the same map every declared window uses, so the
            // shell has ONE spelling of "on top" rather than a private one here
            // and a vocabulary elsewhere.
            .with_window_level(winit_window_level(WindowLevel::AlwaysOnTop))
            .with_visible(false)
            .with_inner_size(LogicalSize::new(f64::from(w.max(1)), f64::from(h.max(1))));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::warn!(target: "pinion::shell", error = %e, "drag-preview window create failed");
                return;
            }
        };
        let scale_factor = window.scale_factor();
        let renderer = match Self::build_renderer(&window) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(target: "pinion::shell", error = %e, "drag-preview renderer init failed");
                return;
            }
        };
        let window_id = window.id();
        self.drag_preview = Some(DragPreviewWindow {
            window,
            window_id,
            renderer,
            vello_scene: VelloScene::new(),
            scale_factor,
            visible: false,
            painted_label: None,
        });
    }

    /// R1147 §5.51 §5.16 — paint the drag-preview chip for its current
    /// `painted_label`. The chip scene carries explicit rects (no layout pass),
    /// so it submits straight through [`paint_adapter::to_vello`]. A no-op until a
    /// label is set. Routed from the `RedrawRequested` arm for the preview's
    /// window id; also called directly on a label change / first show so the chip
    /// appears without waiting on a redraw round-trip.
    fn render_drag_preview(&mut self) {
        let Some(preview) = self.drag_preview.as_mut() else {
            return;
        };
        let Some(label) = preview.painted_label.clone() else {
            return;
        };
        let style = V::drag_image_style(&label).unwrap_or_default();
        let (scene, _size) = pinion_overlay::drag_chip_scene(&label, style);
        let base = paint_adapter::root_background(&scene);
        preview.vello_scene.reset();
        // Render-once-per-drag, so a fresh `LayoutCache` per repaint is fine (this
        // is not a per-frame hot path); the chip is single-line single-style text.
        let mut text_cache = pinion_text::LayoutCache::new();
        paint_adapter::to_vello(
            &scene,
            &|_b: &BoxNode| None,
            &mut text_cache,
            &mut preview.vello_scene,
        );
        // R1027 §5.16 — chip is logical; raster at device resolution like every
        // window. The preview repaints rarely, so a local scaled buffer is cheap.
        let scale = preview.scale_factor;
        let _ = if scale_is_non_identity(scale) {
            let mut scaled = VelloScene::new();
            scaled.append(&preview.vello_scene, Some(Affine::scale(scale)));
            submit_frame(&mut *preview.renderer, &scaled, base, false)
        } else {
            submit_frame(&mut *preview.renderer, &preview.vello_scene, base, false)
        };
    }

    /// R1147 §5.51 §5.16 — resize the preview window's GPU surface after a winit
    /// `Resized` (the OS applying the `request_inner_size` a label change issued).
    /// Routed from the `Resized` arm for the preview's window id.
    fn note_preview_resized(&mut self, size: PhysicalSize<u32>) {
        if let Some(preview) = self.drag_preview.as_mut() {
            preview
                .renderer
                .resize(size.width.max(1), size.height.max(1));
            preview.window.request_redraw();
        }
    }

    /// R1147 §5.51 §5.16 — drive the cross-desktop drag preview from a cursor
    /// move. On a preview-eligible drag (the binding opted the dragged label in
    /// via [`WidgetView::drag_image_style`]) this ensures the window, repaints the
    /// chip iff the label changed (render-once), positions it at the DESKTOP
    /// cursor (the SOURCE window's ACTUAL client origin + the window-local cursor —
    /// the live origin, not the lagging declared one; the source window is the
    /// dragger, not the moved one, so there is no R1119 feedback loop), shows it,
    /// and toggles the in-window-overlay suppression. Skipped under
    /// `PINION_HIDDEN_WINDOW` (headless RPC runs keep the R1113 in-window overlay
    /// as the introspection chip rather than flashing a real window).
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "logical-pixel desktop coords + chip dims -> integer window geometry; sub-pixel rounding + sign are irrelevant (origin/cursor desktop coords, chip dims clamped >= 1)"
    )]
    fn update_drag_preview(
        &mut self,
        window_id: WindowId,
        cursor: (f64, f64),
        event_loop: &ActiveEventLoop,
    ) {
        if hidden_window_requested() {
            return;
        }
        let Some(spec_id) = self.windows.get(&window_id).map(|s| s.spec_id.clone()) else {
            return;
        };
        let Some((label, _)) = self
            .core
            .active_drag_label_for_window(&spec_id, PointerId::MOUSE)
        else {
            // No active drag for this window: the release path hides any preview.
            return;
        };
        if label.is_empty() {
            return;
        }
        let Some(style) = V::drag_image_style(&label) else {
            // The binding wants no follower for this label.
            return;
        };
        // Desktop pointer = the SOURCE window's ACTUAL CLIENT origin + the
        // window-local (client-relative) cursor, trailed by the style offset so the
        // chip sits beside the pointer (matching the in-window R1113 follower).
        // Reads the SAME live-window-origin SSOT the cross-window redock stamps each
        // move (R1148/R1151): `stamp_live_window_origins` runs just above in
        // `handle_cursor_moved`, so the source's origin is fresh. CLIENT (not outer)
        // is the correct base — winit reports the cursor client-relative, so
        // `client_origin + client_cursor` is the true desktop pointer even on a
        // decorated source (a borderless floater has client == outer).
        let Some((ox, oy)) = self.core.live_window_origin(&spec_id) else {
            return;
        };
        let off = i32::try_from(style.cursor_offset).unwrap_or(i32::MAX);
        let pos = (
            (ox + cursor.0).round() as i32 + off,
            (oy + cursor.1).round() as i32 + off,
        );
        self.ensure_drag_preview_window(event_loop, &label, style);
        let repaint = {
            let Some(preview) = self.drag_preview.as_mut() else {
                return;
            };
            let label_changed = preview.painted_label.as_deref() != Some(label.as_str());
            if label_changed {
                let (w, h) = pinion_overlay::chip_size(&label, style);
                let _ = preview
                    .window
                    .request_inner_size(LogicalSize::new(f64::from(w.max(1)), f64::from(h.max(1))));
                // Resize the surface NOW so this frame rasters at the new size;
                // the matching `Resized` event re-applies idempotently.
                let s = preview.scale_factor;
                let pw = (f64::from(w.max(1)) * s).round() as u32;
                let ph = (f64::from(h.max(1)) * s).round() as u32;
                preview.renderer.resize(pw.max(1), ph.max(1));
                preview.painted_label = Some(label.clone());
            }
            preview
                .window
                .set_outer_position(LogicalPosition::new(f64::from(pos.0), f64::from(pos.1)));
            let became_visible = !preview.visible;
            if became_visible {
                preview.window.set_visible(true);
                preview.visible = true;
            }
            label_changed || became_visible
        };
        // Suppress the in-window overlay (one chip, not two) + repaint the source
        // window once so the in-window ghost is stripped now.
        self.core.set_desktop_drag_preview_active(true);
        if repaint {
            self.render_drag_preview();
            self.core.request_redraw();
        }
    }

    /// R1147 §5.51 §5.16 — hide the drag preview on drag end (release / cancel).
    /// `set_visible(false)` keeps the window for reuse; clears the suppression
    /// flag so the in-window overlay resumes (e.g. for headless introspection).
    fn hide_drag_preview(&mut self) {
        if let Some(preview) = self.drag_preview.as_mut()
            && preview.visible
        {
            preview.window.set_visible(false);
            preview.visible = false;
        }
        self.core.set_desktop_drag_preview_active(false);
    }

    /// R1147 §5.51 §5.16 — whether `window_id` is the shell-private drag-preview
    /// window (so `window_event` routes its `RedrawRequested` / `Resized` to the
    /// preview's own render path instead of the declared-window `windows` map).
    fn is_drag_preview_window(&self, window_id: WindowId) -> bool {
        self.drag_preview
            .as_ref()
            .is_some_and(|p| p.window_id == window_id)
    }

    fn handle_cursor_moved(
        &mut self,
        window_id: WindowId,
        position: PhysicalPosition<f64>,
        scale: f64,
        event_loop: &ActiveEventLoop,
    ) {
        let (lx, ly) = winit_pointer_to_logical(position, scale);
        // R1148/R1151 §5.51 — stamp every live window's ACTUAL client origin
        // BEFORE the resolution below, so a floater→main redock hit-tests against
        // real desktop positions (a WM-placed `"main"` has no declared position).
        self.stamp_live_window_origins(window_id);
        let spec_id: &str = self
            .windows
            .get(&window_id)
            .map_or(pinion_runtime::DEFAULT_WINDOW, |s| &*s.spec_id);
        self.core
            .cursor_moved_for_window(spec_id, PointerId::MOUSE, lx, ly);
        // R1196 §5.16 §5.39 — hover cursor affordance, two producers resolved in
        // precedence: (1) the generic node-hint path — the deepest scene node
        // under the pointer that declares a `LayoutStyle::cursor` (a splitter
        // divider, any future hinted widget), mapped by `cursor_icon_for_hint`;
        // else (2) the R1189 chrome-resize path — a client-side window resize
        // region, via the press-side `chrome_action_for_tag` SSOT. The two never
        // overlap in practice (a resize region carries no cursor hint, a hinted
        // widget is not a `WINDOW_RESIZE_*` tag), so the order only sets the
        // tie-break. `spec_id`'s borrow (self.windows) + the self.core reads all
        // end here before the latch's `get_mut`.
        let desired_cursor = self
            .core
            .cursor_hint_for_window(spec_id, PointerId::MOUSE)
            .map(cursor_icon_for_hint)
            .or_else(|| {
                self.core
                    .hover_target_for_window(spec_id, PointerId::MOUSE)
                    .and_then(chrome_action_for_tag)
                    .and_then(resize_cursor_for_action)
            });
        self.command_resize_cursor(window_id, desired_cursor);
        // R1147 §5.51 — drive the cross-desktop drag preview window (a no-op
        // unless a preview-eligible drag is in flight in live mode).
        self.update_drag_preview(window_id, (lx, ly), event_loop);
    }

    /// (R1189 §5.16 §5.39) Command `window_id`'s live cursor to `desired` (a
    /// resize icon, or `None` = the OS default arrow), through the min-change
    /// latch [`WindowSlot::last_resize_cursor`]: winit's `set_cursor` is called
    /// ONLY when the desired icon differs from what is already shown, so the
    /// per-`CursorMoved` cost is a hover read + an equality compare, and a window
    /// that never enters a resize region is never commanded a cursor at all (its
    /// client area stays OS-managed — the decorated-window case). The reset to
    /// the default arrow on leaving a region is itself latched: it fires once, on
    /// the region→non-region transition, not on every subsequent move.
    fn command_resize_cursor(&mut self, window_id: WindowId, desired: Option<CursorIcon>) {
        let Some(slot) = self.windows.get_mut(&window_id) else {
            return;
        };
        let Some(icon) = next_cursor_command(slot.last_resize_cursor, desired) else {
            return;
        };
        slot.last_resize_cursor = desired;
        if let Some(window) = Self::slot_window(&*slot) {
            window.set_cursor(icon);
        }
    }

    /// R51.76 §5.40 — drain the [`ShellCore::redraw_requested`] flag
    /// and forward to every live winit `Window`.
    ///
    /// R670.B §5.16 — pre-R670.B forwarded to the single window only;
    /// post-R670.B forwards to every active window slot so a
    /// multi-window binding gets all windows repainted on a single
    /// state change (the canonical "main button click → inspector
    /// state mirror updates" arc requires this). Steady-state
    /// single-window bindings are unaffected (one window in the
    /// hashmap = one `request_redraw` call).
    ///
    /// Called at the end of every `window_event` / `user_event`
    /// `ApplicationHandler` arm so all redraw requests collapse into a
    /// single `Window::request_redraw` per window per
    /// event-loop iteration.
    ///
    /// R1023.1 §5.16 — the two redraw-request idioms, and when each applies:
    /// - **Ledger route** (this drain): triggers that cannot name their window
    ///   up front — state mutations, RPC dispatch, animation ticks, external
    ///   repaints — set [`ShellCore::request_redraw`] /
    ///   [`ShellCore::request_redraw_for_window`] and rely on this chokepoint to
    ///   coalesce + forward. This is the default and the coalescing SSOT.
    /// - **Direct poke**: winit-event arms that already hold the `Arc<Window>`
    ///   and need to repaint exactly that surface — the `WindowEvent::Resized`
    ///   arm (R1023) and the `request_inner_size` sites — call
    ///   `window.request_redraw()` directly. winit itself coalesces repeated
    ///   `request_redraw` before the next `RedrawRequested`, so the direct poke
    ///   and the ledger converge to one paint per frame; the direct form just
    ///   skips a ledger round-trip when the window is already in hand.
    fn drain_redraw_to_winit(&mut self) {
        // R680 atomic 2 §5.16 §5.41 — two-tier redraw drain:
        // - Binding-wide [`ShellCore::redraw_requested`] flag fans
        //   out to every active window slot (the pre-R680 contract,
        //   safe default for state mutations that cannot reliably
        //   attribute themselves to a single window).
        // - Per-window [`ShellCore::redraw_requested_per_window`]
        //   flags target individual slots (R680 atomic 3+ RPC
        //   dispatch follow-ups, R681 immediate-mode subtree
        //   polling, R683 dock-panel local layout reactions). The
        //   binding-wide drain takes priority — if the fan-out
        //   flag was set, every slot's per-window flag is also
        //   considered "satisfied" by the same `request_redraw`
        //   call, so we drain (clear) the per-window flag too to
        //   avoid a spurious follow-up wake-up in the next
        //   event-loop iteration.
        let fan_out = self.core.take_redraw_request();
        // Collect spec_ids first so the per-window drain doesn't
        // hold a `&self.windows` borrow across the
        // `self.core.take_redraw_request_for_window` `&mut self.core`
        // mutation.
        //
        // R683 §5.16 — `spec_id: Cow<'static, str>` because the dock
        // + tear-off arc can mint runtime-generated ids alongside
        // static literals; the `.clone()` is `Rc`-cheap for
        // `Cow::Borrowed` (no heap alloc) and a `String::clone` for
        // `Cow::Owned`.
        let active: Vec<(Cow<'static, str>, std::sync::Arc<Window>)> = self
            .windows
            .values()
            .filter_map(|slot| {
                if let RenderState::Active { window, .. } = &slot.render {
                    Some((slot.spec_id.clone(), std::sync::Arc::clone(window)))
                } else {
                    None
                }
            })
            .collect();
        for (spec_id, window) in active {
            // Always drain the per-window flag so a stale `true`
            // does not survive a binding-wide fan-out drain.
            let per_window = self.core.take_redraw_request_for_window(&spec_id);
            if fan_out || per_window {
                window.request_redraw();
            }
        }
    }

    /// R51.76 §5.40 — thin wrapper around [`ShellCore::dispatch_rpc`]
    /// that builds the production `resize_request` closure from the
    /// live winit `Window` and routes the JSON-RPC response through the
    /// frame's [`RpcReply`](pinion_rpc::RpcReply) sink. Headless tests call
    /// `ShellCore::dispatch_rpc` directly with a no-op closure.
    ///
    /// R-PR47 §5.7 — the response used to be hard-written to
    /// `std::io::stdout` here; now it goes to `frame.reply`, so a frame
    /// that arrived over a socket is answered on that socket. The
    /// built-in stdin producer supplies a stdout-writing reply
    /// ([`stdout_egress`]), keeping the `stdin → stdout` path
    /// byte-identical.
    ///
    /// R1188 §5.16 §5.49 §2 #2 — a `scene/click` on a window-control tag queues
    /// a control on `ShellCore` (winit handles + the event-loop exit live HERE,
    /// not in the headless core). R1190 §5.16 §5.49 — this method now DRAINS
    /// [`ShellCore::take_pending_window_controls`] itself (below, after the
    /// response write), executing each queued control through
    /// [`Self::apply_window_control`] with the `event_loop` parameter. R1188
    /// left the drain to the caller with a "future callers MUST drain" rustdoc
    /// contract; the session audit flagged that comment-enforced coupling — so
    /// the drain is now INSIDE the one method that owns the window-control RPC
    /// path, and taking `event_loop` makes the requirement compiler-enforced
    /// (a caller cannot invoke this without the handle the close/app-exit needs).
    /// R1550 §5.16 §5.7 — every arena this shell's windows hold, as census
    /// rows.
    ///
    /// One `paint-fragments` row and one `images` row per window, because the
    /// caches are per-window and can differ by orders of magnitude (a DCC
    /// viewport against a palette), plus one shell-wide `images` row for the
    /// producer store a `memory://` source resolves through. The store is
    /// counted here and nowhere else: every window's `ImageCache` holds a
    /// handle to it, so counting it per window would report one registered
    /// image once per window.
    fn arena_footprints(&self) -> Vec<pinion_core::memory_census::ArenaFootprint> {
        use pinion_core::memory_census::MeasuredArena;
        let mut rows = Vec::with_capacity(self.windows.len() * 2 + 1);
        for slot in self.windows.values() {
            rows.push(
                slot.fragment_cache
                    .arena_footprint()
                    .in_window(&slot.spec_id),
            );
            rows.push(slot.image_cache.arena_footprint().in_window(&slot.spec_id));
        }
        rows.push(image_cache::resolve_image_store(self.core.root_owner()).arena_footprint());
        rows
    }

    fn dispatch_rpc(&mut self, frame: RpcFrame, event_loop: &ActiveEventLoop) {
        // R-PR47 §5.7 — split the frame into its raw request + the reply
        // sink that routes the response back to the originating
        // transport. `reply` is consumed on exactly one path below: the
        // parse-error short-circuit, or the post-dispatch response write.
        // R-PR67 — `conn` is carried on the frame but unused by the GUI
        // shell (its `ProxyRpcIngress` keeps the default no-op lifecycle
        // hooks); a stateful ingress reads it instead.
        // R1552 §5.7 PINION-PR83 — `conn` and `egress` are no longer
        // discarded: together they are what lets a handler on this frame keep
        // writing to this client after the response (`scene/subscribe`).
        let RpcFrame {
            request,
            reply,
            conn,
            egress,
        } = frame;
        // R671 §5.7 §5.16 — single-parse per-window RPC dispatch.
        // Pre-R671 (R670.B) AppShell parsed the JSON-RPC envelope
        // *twice*: once to sniff `params.window` (the per-window
        // scope) + once inside `pinion_rpc::dispatch` for actual
        // routing. R671 parses once via `pinion_rpc::parse_request`
        // + extracts the window scope from `Request.params` + hands
        // the same `Request` to the substrate which forwards to
        // `pinion_rpc::dispatch_parsed`. Parse errors short-circuit
        // here + we return the canonical -32700 frame through the reply.
        let parsed_request = match pinion_rpc::parse_request(&request) {
            Ok(r) => r,
            Err(err_resp) => {
                reply.send(err_resp);
                return;
            }
        };
        // R1269 PR-50 §6.3 — async `scene/waitFor`: park or answer the frame
        // before normal dispatch. The registry lives in ONE slot, seeded on
        // `root_owner` and resolved through `Owner::cache_inherited` (R1365), so
        // a wait parked here and the `notify_changed` that wakes it share a
        // single instance. R1365.1 — this said a binding's producer resolves it
        // via `use_waiter_registry`. There is no such function, and R1269's
        // ledger froze the same claim ("pinion-shell re-exports it"; it
        // re-exports only `use_scene_revision`). A producer needs the REVISION,
        // which is what it bumps; the registry is the shell's own park side. `try_async_wait_for` claims only a `scene/waitFor`
        // carrying a numeric `since` — every other frame (and the v0 busy-poll
        // waitFor) hands the reply back for the normal path below, byte-unchanged.
        let reply = {
            let registry = crate::waiter::resolve_waiter_registry(self.core.root_owner());
            match try_async_wait_for(
                &parsed_request,
                self.core.revision_token(),
                &registry,
                reply,
            ) {
                // Parked (fires later when a bump wakes it) or answered
                // immediately — a waitFor dispatches nothing and queues no
                // window controls, so there is nothing further to do this frame.
                std::ops::ControlFlow::Break(()) => return,
                std::ops::ControlFlow::Continue(reply) => reply,
            }
        };
        self.stamp_desktop_facts();
        // R890.1 §5.16 §5.49 — no window extraction here at all: the
        // substrate's windowed entry derives the dispatch scope from
        // the request's own `{window: "<id>"}` param through the ONE
        // extraction home (`pinion_rpc::Request::window_scope`), and
        // the dispatcher gates unknown ids with `-32602
        // unknown_window` before method routing. Pre-R889 this site
        // hosted `resolve_spec_id` (silent alias of unknown ids onto
        // the primary); pre-R890.1 it still hand-rolled a second copy
        // of the param extraction that had to agree with the gate's
        // forever.
        // R670.B §5.16 — primary-window-scoped `scene/resize` (the
        // resize closure still targets the primary window; per-window
        // `scene/resize` is a follow-up axis once a real consumer
        // surfaces — typical multi-window app resizes the main
        // window, not the inspector). Holding the Arc<Window> across
        // the substrate call keeps the closure's `&Arc<Window>`
        // borrow alive without re-borrowing `self.windows` from
        // inside the closure.
        let primary_window_arc: Option<Arc<Window>> =
            self.primary_slot().and_then(Self::slot_window).cloned();
        let mut resize_req = |w: u32, h: u32| {
            // R47.7.4.2 — `scene/resize` reaches winit through this
            // closure: `request_inner_size` queues a size change that
            // winit emits as a `Resized` event on the next loop pass,
            // and the explicit `request_redraw` shortens the gap to
            // the new paint scene observation.
            if let Some(window) = &primary_window_arc {
                let _ = window.request_inner_size(LogicalSize::new(w, h));
                window.request_redraw();
            }
        };
        // R890 §5.12 §5.16 — no slot-layout threading either: the
        // dispatcher projects the addressed window's layout on demand
        // from the stored paint scene it already threads for
        // `scene/snapshot from: paint`, so `scene/layout {viewport:
        // null}` answers with the named window's own geometry or the
        // honest `NoLastPaintLayout`.
        // R1060 §5.12 §5.16 — a `scene/screenshot` request reads the
        // live presented surface, which only AppShell (not ShellCore)
        // can reach. Capture it here, before the `&mut self.core`
        // dispatch borrow, when the method matches; every other method
        // skips the GPU readback (the render_fidelity snapshot pattern,
        // lazily gated by method so non-screenshot dispatches pay
        // nothing). `scope` borrows `parsed_request`, so the capture runs
        // before `parsed_request` moves into the dispatch.
        let screenshot = if parsed_request.method == "scene/screenshot" {
            let scope = parsed_request.window_scope().ok().flatten();
            // R1061 §5.12 — optional `{out_path: "….png"}` switches the
            // wire to file output (small response) vs the inline
            // `pixels_rgba8` array (default).
            let out_path = parsed_request
                .params
                .as_ref()
                .and_then(|p| p.get("out_path"))
                .and_then(serde_json::Value::as_str);
            self.capture_window_screenshot(scope, out_path)
        } else {
            None
        };
        // R1550 §5.16 §5.7 — the arenas only AppShell can reach: each window
        // slot's paint-fragment and decoded-image caches, plus the shell-wide
        // producer image store. Method-gated like the screenshot readback
        // above, and for a stronger reason — pricing an arena WALKS it, so an
        // ungated census would put an O(fragments + glyphs) traversal on every
        // `scene/click`. `ShellCore` adds its own shape-cache row and the
        // process total; see `ShellCore::dispatch_rpc_inner`.
        let window_arenas =
            (parsed_request.method == "scene/memory").then(|| self.arena_footprints());
        // R1557 §5.16 §5.18 §5.7 — the frame's draw work attributed per subtree,
        // which only AppShell can produce: the attribution re-encodes the
        // retained paint scene through the vello walk, and that needs the
        // window's decoded-image cache (a slot field here) alongside the shape
        // cache and paint scene `ShellCore` owns. Method-gated for the same
        // reason `window_arenas` above is, and a stronger one — this is a whole
        // encode, not a traversal of what is already encoded.
        let draw_profile = (parsed_request.method == "scene/draw_profile")
            .then(|| {
                // R1558 — the request's params decide BOTH which window is
                // re-encoded and which subtree of it, so they are parsed once,
                // here, and `DrawProfileParams::window` answers the first
                // question with the same call the dispatcher uses to render
                // every row's address. A malformed `path` yields `None`; the
                // dispatcher parses the same params and answers with the typed
                // reason, so the failure is named in one place rather than
                // guessed at in two.
                let params = pinion_rpc::draw_profile::DrawProfileParams::parse(
                    parsed_request.params.as_ref(),
                )
                .ok()?;
                let window_id = params
                    .window(parsed_request.window_scope().ok().flatten())
                    .to_owned();
                let Some(slot) = self.windows.values_mut().find(|s| s.spec_id == window_id) else {
                    // R1558 — a window named by the ADDRESS and not open is
                    // judged here, because this is where the live registry is.
                    // `crate::path::resolve` would have judged it against the
                    // SCE topology, which a binding that opens a second
                    // `WindowSpec` without a second `AppState` differs from —
                    // and under that rule this method published
                    // `/window[inspector]/…` rows it then refused to read back.
                    // A window named by `{window: …}` instead is already
                    // refused upstream by `unknown_window_verdict`, so this
                    // arm answers only for the address.
                    return params.scope.as_ref().and_then(|s| s.window.as_ref()).map(
                        |requested| {
                            Err(pinion_rpc::draw_profile::DrawProfileError::UnknownWindow {
                                requested: requested.clone(),
                                valid: self
                                    .windows
                                    .values()
                                    .map(|s| s.spec_id.to_string())
                                    .collect(),
                            })
                        },
                    );
                };
                self.core.draw_profile_for_window(
                    &window_id,
                    &mut slot.image_cache,
                    params.scope.as_ref(),
                )
            })
            .flatten();
        let resp = self.core.dispatch_rpc_scoped_from(
            parsed_request,
            &mut resize_req,
            screenshot,
            window_arenas,
            Some((conn, &egress)),
            draw_profile,
        );
        // R-PR47 §5.7 — route the response (if any) back through the
        // frame's reply sink. A JSON-RPC notification produces `None`:
        // `reply` is then dropped unused, sending nothing — identical to
        // the pre-PR47 `if let Some` guard that skipped the stdout write.
        if let Some(resp) = resp {
            reply.send(resp);
        }
        // R1552 §5.7 PINION-PR83 — a subscription opened by THIS frame becomes
        // eligible only now, after its own response has gone out, so a client
        // can never receive `scene/changed {subscription: N}` before the answer
        // that told it `N`. The publish is gated on there having BEEN one:
        // ordinary scene advances are delivered by the `SceneRevision` observer
        // (see `ShellCore::with_core`), and this call exists only to hand a
        // fresh subscription the catch-up a stale `since` is owed. Publishing
        // unconditionally here would deliver those advances too — which is
        // harmless, and hid the observer path from every test that could have
        // exercised it.
        let subscriptions = pinion_rpc::process_registry();
        if subscriptions.arm_pending() > 0 {
            subscriptions.publish(self.core.revision_token().current());
        }
        // R1190 §5.16 §5.49 §2 #2 — execute the window-control presses the RPC
        // click drain queued during this dispatch (`ShellCore` is headless, so
        // it queued them; the winit handles + the event-loop exit live here).
        // AFTER the response write, so a `scene/click {close}` client sees its
        // `result` before the window closes. Same execution arm as a physical
        // press on the same tag (`try_chrome_press` → `apply_window_control`).
        for (spec_id, control) in self.core.take_pending_window_controls() {
            self.apply_window_control(&spec_id, control, ControlProducer::RpcClick, event_loop);
        }
        // R1364 §5.55 §2 #2 — an `app/quit` the drain recorded. AFTER the
        // response write, for the same reason as the controls above and more
        // sharply: this one may end the process, so a client that never saw its
        // `result` could not tell success from a crash.
        //
        // Lands on `request_quit`, the ONE arm, so `app_quit_requested` refuses
        // it exactly as it refuses Escape. An AI gets the peer of the user's
        // Escape and the OS X — not a privileged exit past the unsaved-changes
        // gate, which is what R1362's caveat feared and got backwards.
        if self.core.take_pending_quit() {
            self.request_quit(event_loop);
        }
    }

    /// R1060 §5.12 §5.16 — capture the addressed window's live presented
    /// surface for a `scene/screenshot` RPC. Resolves the request's
    /// `{window: "<id>"}` scope (absent → the primary window) to a slot,
    /// flags it for capture, drives ONE `Self::render_window` pass (which
    /// then submits through [`VelloRenderer::capture_rgba8`], reading back
    /// the swapchain texture instead of presenting blind), then drains +
    /// converts the frame to the wire [`pinion_rpc::Screenshot`].
    ///
    /// Returns `None` when the window is unknown or the GPU capture
    /// failed; the dispatcher then surfaces `unknown_window` (its own
    /// gate) or `RenderBackendUnavailable` (the screenshot handler's
    /// absent-snapshot path). This is the ONE site that can read live
    /// pixels — the dispatch runs in `ShellCore`, which holds no renderer.
    /// R1459 §5.16 §5.36 — attach the paint's WORK counts to a duration sample
    /// and record it.
    ///
    /// The counts are read from the window the paint just wrote, so both halves
    /// describe the same frame. They live on one sample rather than a second
    /// surface because they answer the same question from two sides:
    /// `build_us` is the whole settle loop, so a 4ms frame that ran one heavy
    /// pass and a 4ms frame whose four cheap passes disagree are identical by
    /// time alone — and they want opposite fixes.
    ///
    /// R1537 — the GPU report lands here too, and in one call, because its
    /// two halves belong in different places: the fresh measurement is a
    /// per-frame value that rides the ring sample, while the capability and
    /// the drop count are properties of the backend that ride the read.
    /// Splitting the call site would let a future paint record one without
    /// the other.
    ///
    /// R1538 — the node census arrives split the same way and is reassembled
    /// here for the same reason. Its build half is the producer's
    /// ([`pinion_runtime::PaintWork`], read back from the window like the
    /// settle counts); its paint half is `encode_nodes`, which only the
    /// surface's own encode scope can know. This is the one place both are in
    /// scope, so it is the one place the sample can describe a whole frame.
    ///
    /// R1556 — `draw` joins them, and is the only one of the counts that is not
    /// a size of a *tree*: it is the size of the drawing the frame submitted.
    /// Like `encode_nodes` it can only be read inside the render scope, because
    /// the scene it censuses is the one that scope hands to the renderer.
    fn record_frame_sample(
        &mut self,
        window: &str,
        timing: pinion_runtime::FrameTiming,
        gpu: GpuFrameReport,
        encode_nodes: u32,
        access_nodes: u32,
        draw: pinion_runtime::DrawWork,
    ) {
        let work = self.core.last_frame_work_for_window(window);
        self.core.record_frame_timing(
            window,
            timing
                .with_work(work.passes, work.settled, work.shape_misses)
                .with_census(work.scene_nodes, work.layout_nodes, encode_nodes)
                .with_access_census(access_nodes)
                .with_draw_census(draw)
                .with_gpu(gpu.us),
        );
        self.core
            .set_gpu_timing_state(window, gpu.supported, gpu.dropped);
    }

    fn capture_window_screenshot(
        &mut self,
        window_scope: Option<&str>,
        out_path: Option<&str>,
    ) -> Option<pinion_rpc::Screenshot> {
        let window_id = match window_scope {
            Some(spec_id) => self.spec_id_to_window_id.get(spec_id).copied(),
            None => self.primary_window_id,
        }?;
        self.windows.get_mut(&window_id)?.pending_capture = true;
        self.render_window(window_id);
        // R1062 §5.12 — clear the one-shot flag UNCONDITIONALLY: if
        // `render_window` early-returned (minimized / 0-size or suspended
        // window) it never consumed the flag, and a stale `true` would
        // make the NEXT event-loop paint capture by surprise — a
        // multi-MB readback + a blocking `device.poll` on the 60-144fps
        // hot path, plus an orphaned frame. The capture still fails
        // honestly via the `last_capture.take()?` below.
        let slot = self.windows.get_mut(&window_id)?;
        slot.pending_capture = false;
        let frame = slot.last_capture.take()?;
        // R1061 §5.12 — `{out_path}` mode: write the captured frame to the
        // file as PNG (the RGBA8 -> PNG SSOT shared with the headless
        // path) and return just the path, so the wire stays small for
        // large windows. A create / encode failure fails the capture
        // (`None` -> `RenderBackendUnavailable`) rather than returning a
        // wrong frame. This filesystem write is the ONE side-effect on
        // the otherwise-Read `scene/screenshot` method — scene + OCC
        // state are untouched, so the `HandlerKind::Read` classification
        // holds; the write is the client's explicitly-requested output
        // (same trust model as the `PINION_SCREENSHOT` env path).
        if let Some(path) = out_path {
            let file = std::fs::File::create(path).ok()?;
            crate::vello_capture::encode_rgba8_png(frame.width, frame.height, &frame.rgba8, file)
                .ok()?;
            return Some(pinion_rpc::Screenshot::new_file(
                frame.width,
                frame.height,
                path.to_owned(),
            ));
        }
        Some(pinion_rpc::Screenshot::new(
            frame.width,
            frame.height,
            frame.rgba8,
        ))
    }

    /// R670.B §5.16 — per-window AccessKit emit helper. Extracted
    /// from `render_window` so the parent fn stays under the
    /// workspace `clippy::too_many_lines = 100` ceiling after the
    /// per-window cluster lift R670.B added. The body is unchanged
    /// from the pre-R670.B inline version; only the slot lookup +
    /// disjoint-borrow split (slot.accesskit, slot vs self.core)
    /// differ.
    ///
    /// R1538 §5.40 — returns how many nodes the AT-tree walk produced, for the
    /// frame's accessibility census. `0` when this window has no adapter, which
    /// is the honest count: the walk did not run.
    fn emit_accesskit_for_window(
        &mut self,
        window_id: WindowId,
        spec_id: &str,
        paint_scene: &pinion_core::Scene,
        size_w: u32,
        size_h: u32,
        scale_factor: f64,
    ) -> u32 {
        let Some(slot) = self.windows.get_mut(&window_id) else {
            return 0;
        };
        if slot.accesskit.is_none() {
            return 0;
        }
        // R813 §5.40 — thread the resolved spec id so the substrate calls
        // `V::access_node_for_window(spec_id, ...)`: each window's AT tree
        // carries only its own nodes (no cross-window ghost nodes).
        let (nodes, at_focus) = self.core.collect_access_emit_inputs(spec_id, paint_scene);
        let window_bounds = pinion_core::scene::Rect::new(0, 0, size_w, size_h);
        let decision = self.core.plan_access_emit(&nodes, at_focus.as_ref());
        // Re-acquire the slot mutable borrow now that the substrate
        // borrows released (collect_access_emit_inputs +
        // plan_access_emit take `&mut self.core`).
        // R1538 — the census is the size of the tree the WALK produced, taken
        // before the emit decision: a frame whose tree matched the last one
        // emits nothing, and it still did the work of finding that out.
        let access_nodes = u32::try_from(nodes.len()).unwrap_or(u32::MAX);
        let Some(slot) = self.windows.get_mut(&window_id) else {
            return access_nodes;
        };
        if decision.should_emit
            && let Some(adapter) = slot.accesskit.as_mut()
        {
            let initial = decision.initial;
            let dirty = decision.dirty;
            let at_focus_ref = at_focus.as_ref();
            let nodes_ref = &nodes;
            adapter.update_if_active(|| {
                let mut builder = AccessTreeBuilder::new();
                if !initial {
                    builder.initial(false);
                }
                for node in nodes_ref {
                    builder.add(node);
                }
                if !initial {
                    builder.dirty_tags(dirty);
                }
                if let Some(f) = at_focus_ref {
                    builder.focused(Some(&f.focus_tag));
                    if let Some(child) = &f.active_descendant {
                        builder.active_descendant(&f.focus_tag, child);
                    }
                } else {
                    builder.focused(None);
                }
                // R1027 §5.40 — `window_bounds` + every AccessNode rect
                // are logical pixels (the paint scene is logical since
                // R1027). The root-node `Affine::scale(scale_factor)`
                // re-expresses the whole tree in the physical-pixel space
                // AccessKit expects; identity scale leaves it byte-identical.
                builder.build_with_scale(Some(window_bounds), scale_factor)
            });
        }
        // R51.77 / R51.79 §5.40 — commit step. By-value Vec move
        // into the cache; nodes consumed here. Idempotent on
        // !should_emit so the next frame's plan diffs against the
        // post-emit baseline.
        self.core.commit_access_emit(nodes, at_focus.as_ref());
        access_nodes
    }

    /// Build the paint scene for the current cached state, run layout,
    /// hand it to the framework-side `paint_adapter` walker, and submit
    /// the resulting `vello::Scene` to the renderer. No-op while
    /// suspended (R46.3.4 lifecycle).
    ///
    /// R51.76 §5.40 — the AccessKit emit decision is delegated to
    /// [`ShellCore::plan_access_emit`](crate::ShellCore::plan_access_emit) so the same diff logic is
    /// exercised by headless tests; the AppShell-side responsibility
    /// is just to feed the plan to `Adapter::update_if_active`.
    /// R670.B §5.16 §5.41 — render one window's paint scene.
    ///
    /// Pre-R670.B was the single-window `fn render(&mut self)`; the
    /// body is unchanged structurally — only the field accesses now
    /// resolve through `self.windows[&window_id]` instead of
    /// `self.render` / `self.accesskit` / `self.vello_scene` /
    /// `self.last_ime_cursor_area` / `self.pending_intrinsic_resize`.
    /// The §5.16 §5.36 substrate ownership of the paint scene
    /// pipeline (`ShellCore::compute_paint_scene`) is unchanged — the
    /// surface still only handles the vello/wgpu submit per window.
    ///
    /// No-op when the slot is missing (window already dropped) or
    /// when its [`RenderState`] is `Suspended` (GPU released, mobile
    /// platform cycle). Mirrors the pre-R670.B `let RenderState::
    /// Active { .. } = &mut self.render else { return; }` guard.
    // R1538 — the frame pipeline: build, encode, acquire, submit, present,
    // close. Its length is the sequence, not accumulated incidental work —
    // each phase must stay inside the `Instant` bracket that measures it and
    // inside the slot borrow that owns the renderer, so the phases cannot be
    // reordered or hoisted into helpers without breaking one or the other.
    // The one part of it with a single job of its own, the frame close, is
    // `close_frame`; the rest is the pipeline itself.
    #[allow(clippy::too_many_lines, reason = "the frame pipeline is a sequence")]
    fn render_window(&mut self, window_id: WindowId) {
        // R670.B §5.16 — read the slot's spec id + inner size first
        // without holding a long-lived `&mut self.windows` borrow,
        // so the subsequent `self.core.compute_paint_scene_for_window`
        // call (which takes `&mut self.core`) does not conflict with
        // the slot borrow needed to call back into vello_scene /
        // accesskit / last_ime_cursor_area / pending_intrinsic_resize
        // after the paint scene is computed.
        // R683 §5.16 — `spec_id: Cow<'static, str>` so the dock +
        // tear-off arc can mint runtime ids; `.clone()` here is
        // `Cow`-cheap for `Borrowed` (no heap alloc) and a
        // `String::clone` for `Owned`. The clone detaches from the
        // `&slot` borrow so the substrate's `&mut self.core` call
        // below does not conflict.
        let (spec_id, scale, w, h) = {
            let Some(slot) = self.windows.get(&window_id) else {
                return;
            };
            let RenderState::Active { window, .. } = &slot.render else {
                return;
            };
            // R1027 §5.16 — lay out in logical pixels. winit reports the
            // surface in physical pixels; the window `scale_factor` maps
            // physical -> logical so app-authored dimensions render at
            // their intended size on `HiDPI` (the scale is re-applied at the
            // GPU raster boundary below). `logical_layout_size` may return a
            // `0` dimension for a degenerate (minimized / sub-pixel) window;
            // the `NonZeroU32` guards below then early-return with no paint —
            // the load-bearing 0-size skip pre-R1027 got from feeding the raw
            // physical `inner_size` to `NonZeroU32::new`.
            let scale = slot.scale_factor;
            let (lw, lh) = logical_layout_size(window.inner_size(), scale);
            let Some(w) = core::num::NonZeroU32::new(lw) else {
                return;
            };
            let Some(h) = core::num::NonZeroU32::new(lh) else {
                return;
            };
            (slot.spec_id.clone(), scale, w, h)
        };
        // R51.80 §5.16 §5.36 — ShellCore owns the paint scene
        // pipeline; AppShell only handles the vello/wgpu submit.
        // R670.B §5.16 — `compute_paint_scene_for_window` is the
        // per-window variant that routes through
        // `V::view_for_window(spec_id, state, frame)`; default impl
        // forwards to `V::view` so single-window bindings remain
        // bit-identical.
        // R683 §5.16 — `&spec_id` auto-derefs from
        // `&Cow<'static, str>` to `&str` through `Cow`'s `Deref`
        // impl, so the substrate signature (which takes `&str`)
        // stays unchanged.
        // R907 §5.16 §5.7 — frame-timing profiler: bracket the whole
        // productive frame (build → finalize) and each named phase with
        // `Instant` spans. `build_us` is the `view` + layout pass; the
        // scope below measures `encode_us` (to_vello_cached), `acquire_us`
        // (the vsync block) and `render_us` (GPU submit) into these vars;
        // `total_us` closes
        // after finalize. `total >= build + encode + acquire + render` holds by
        // construction (disjoint sub-intervals); R1361.1 split `acquire_us`
        // out of `render_us`, so the inequality now spans four phases.
        let frame_start = Instant::now();
        let paint_scene = self
            .core
            .compute_paint_scene_for_window(&spec_id, w.get(), h.get());
        let build_us = instant_delta_us(frame_start, Instant::now());
        // R668 §5.16 / R1072.1 — `IntrinsicAfterFirstPaint` resize hook, extracted
        // into `apply_pending_intrinsic_resize`: walk the first painted scene for
        // its content size, clamp to `[min, max]`, request the new winit inner-size
        // (applied next frame via `Resized`). Runs BEFORE the encode so the encode
        // uses the current size; a no-op for `Fixed` strategy + steady-state paints.
        self.apply_pending_intrinsic_resize(window_id, &paint_scene, w, h);
        // R1426 §5.41 §5.28 — the live winit surface animates the cursor blink:
        // read this window's clock phase (armed by
        // `compute_paint_scene_for_window` above). `grid_cursor_blink_on` reads
        // the phase OUTSIDE any reactive scope, so it does not fold into the
        // scene; a steady / hidden cursor resolves to `true` (always drawn).
        // Computed here (before the slot-borrow scope below) so the immutable
        // `self.core` read does not overlap the `self.windows` mutable borrow.
        // The produce / screenshot path passes the steady default instead, so a
        // golden PNG never flakes on the wall-clock phase.
        let cursor_blink_on = self.core.grid_cursor_blink_on(&spec_id);
        // R1427 §5.41 §5.39 — whether THIS window holds the OS keyboard focus,
        // via the same fails-open predicate the key-dispatch gate uses
        // (`is_key_dispatch_window`: unknown focus → `true`). An unfocused
        // window renders its cursor HOLLOW (paint-time, never scene data — OS
        // focus is already introspectable via `scene/input_state`). Read here
        // (before the slot borrow) alongside `cursor_blink_on`, and it is the
        // SAME fact the blink-arm gates on, so the two can never disagree (no
        // blinking-hollow contradiction).
        let cursor_focused = self.core.is_key_dispatch_window(&spec_id);
        // Assigned exactly once inside the paint scope below; the
        // scope's `else { return; }` arms diverge, so the fall-through
        // path that reaches `record_frame_timing` always assigns both.
        //
        // R1537 §5.16 — `gpu` (what this paint learned about the GPU's own
        // clock) joins them, and is declared WITH them because it obeys the
        // same rule: one assignment, inside that scope, on every path that
        // reaches the sample below.
        //
        // R1538 §5.16 — `encode_nodes` (how many scene nodes the encode walk
        // entered) joins them under the same rule. It is the paint-side half
        // of the frame's node census, and it can only be read here: the
        // fragment cache that counts it is the window slot's, borrowed inside
        // the scope below.
        //
        // R1556 §5.16 — `draw` (what the submitted scene will ask the renderer
        // to draw) joins them under the same rule, and for `encode_nodes`'
        // reason: the scene it censuses is the render target the scope below
        // builds, so nowhere else can see it.
        let (encode_us, encode_nodes, acquire_us, render_us, gpu, draw);
        // R1036 PR-17 — the `renderer.render` outcome for this frame, fed into
        // the per-window render-fidelity record so `scene/render_fidelity`
        // surfaces a failed present (the present-staleness signature).
        let present_ok;
        // Re-acquire the slot mutable borrow now that the substrate
        // borrow released, then bind window + renderer for the
        // intrinsic-resize hook + vello submit. Scope the borrow
        // so it drops before the post-paint helpers
        // (emit_accesskit_for_window, publish_ime_for_window) which
        // take `&mut self`.
        // R1027 §5.16 — this scope no longer yields the physical
        // `inner_size`: AccessKit bounds are now logical (`w`/`h`) and the
        // GPU surface is sized by the `Resized` arm, so nothing past the
        // paint needs the physical size.
        {
            let Some(slot) = self.windows.get_mut(&window_id) else {
                return;
            };
            let RenderState::Active { renderer, .. } = &mut slot.render else {
                return;
            };
            slot.vello_scene.reset();
            let base = paint_adapter::root_background(&paint_scene);
            // R682 §5.16 atomic 1 — cached path. The per-window
            // `FragmentCache` skips re-encoding cacheable Container
            // subtrees whose `paint_hash` matches the previous frame
            // (the §2 #4 immediate-mode coexistence enabler: a sibling
            // `ImmediateModeNode` triggers `V::view` re-runs every paint,
            // but retained widget subtrees with stable structure replay
            // their encoded `vello::Scene` via `append` instead of fresh
            // walk). `&|_b| None` is the canonical no-override fill hook
            // every production shell call site passes — the cache's
            // structurally-derived contract holds trivially.
            // R705 §5.39 — the focus ring is no longer stroked here. It is
            // injected upstream as a pointer-transparent overlay
            // `Scene::Box` by `WindowOverlayInputs::apply` (the final step
            // of every paint-scene producer), so `to_vello_cached` paints it
            // via the generic box path and `scene/snapshot from: paint`
            // observes it (§2 #1 + #7). The pre-R705 opaque
            // `paint_adapter::paint_focus_ring` vello stroke is retired.
            let encode_start = Instant::now();
            // R1072 §5.37 — engine-aware cached paint: cache (mut) + opt-in engine
            // (shared) from one disjoint-field borrow. `None` = pre-R1072 path.
            let (text_cache, text_engine) = self.core.text_cache_and_engine();
            paint_adapter::to_vello_cached_with_text_engine(
                &paint_scene,
                &|_b: &BoxNode| None,
                text_cache,
                &mut slot.image_cache,
                &mut slot.fragment_cache,
                text_engine,
                &mut slot.vello_scene,
                cursor_blink_on,
                cursor_focused,
            );
            encode_us = instant_delta_us(encode_start, Instant::now());
            // R1538 §5.16 — published by `end_paint`, which the call above
            // just made, so this reads THIS frame's walk.
            encode_nodes = slot.fragment_cache.nodes_walked_last_paint();
            // R51.109.1 §5.41 — call through the backend-agnostic
            // `WidgetRenderer` trait. `VelloContext::base_color` carries
            // the window background sampled from
            // `paint_adapter::root_background`; the renderer's macro impl
            // forwards to the inherent `<R>::render(frame, base_color)`.
            // `renderer.render` auto-derefs through `Box<R>` because the
            // `WidgetRenderer` trait is in scope.
            let render_start = Instant::now();
            // R1027 §5.16 — the paint scene (and thus `vello_scene`) is in
            // logical pixels; the GPU surface is physical. At non-identity
            // scale, append the logical scene into `scaled_scene` under
            // `Affine::scale(scale)` so it rasterizes at device resolution.
            // The `FragmentCache` stays IDENTITY-keyed and fully intact —
            // the scale is one top-level transform applied AFTER the cached
            // walk, not threaded through it, so cache hits are unaffected.
            // At `1.0` the renderer submits `vello_scene` directly: zero
            // extra append, byte-identical to the pre-R1027 output.
            let render_target: &VelloScene = if scale_is_non_identity(scale) {
                slot.scaled_scene.reset();
                slot.scaled_scene
                    .append(&slot.vello_scene, Some(Affine::scale(scale)));
                &slot.scaled_scene
            } else {
                &slot.vello_scene
            };
            // R1556 §5.16 — census what this frame will actually ask the
            // renderer to DRAW, off the scene that is about to be submitted.
            //
            // Here, and not after the encode above, for two reasons that are
            // really one: this is the scene that runs. The DPI append copies the
            // streams verbatim so the counts are the same either way, and
            // taking them from the submitted value is what makes "this is what
            // ran" true by construction rather than by review — a future
            // round that adds a step between the encode and the submit cannot
            // silently leave its work out of the census.
            draw = paint_adapter::draw_work_of(render_target);
            // R1060 §5.16 §5.12 — submit the frame: the normal present,
            // or — when an RPC `scene/screenshot` flagged this slot via
            // `capture_window_screenshot` — `capture_rgba8` reading back
            // the presented swapchain. The flag is false on every
            // event-loop-driven paint, so the hot path is byte-identical.
            // R1062 — read-only here: `capture_window_screenshot` OWNS the
            // flag's set+clear lifecycle (it clears unconditionally after
            // this call, so an early-return path above cannot leave it
            // stale and make a later event-loop paint capture by surprise).
            let wants_capture = slot.pending_capture;
            let captured;
            (present_ok, captured) =
                submit_frame(&mut **renderer, render_target, base, wants_capture);
            if let Some(frame) = captured {
                slot.last_capture = Some(frame);
            }
            // R1361.1 §5.16 — split the swapchain acquire out of the render
            // phase. `render` brackets `get_current_texture()`, which BLOCKS on
            // vsync (`PresentMode::AutoVsync`), so the raw span is
            // "work + wait-for-image". Only the backend can see the split, so it
            // reports the block and we subtract: `render_us` becomes work, and
            // `acquire_us` becomes the idle wait a profiler needs to tell
            // "I am slow" from "I am merely waiting". Saturating because the two
            // are measured by different clocks reads and must never underflow.
            let render_span_us = instant_delta_us(render_start, Instant::now());
            acquire_us = renderer.last_acquire_us().min(render_span_us);
            render_us = render_span_us.saturating_sub(acquire_us);
            // R1537 §5.16 — and what the GPU took, which none of the above
            // can be. Every span here is CPU wall-clock around a `submit`
            // that returns before the GPU has started, so a window can be
            // entirely GPU-bound with all three of these reading fast.
            // Read HERE because this is the only scope that holds a
            // renderer, which is also why the capability rides along.
            gpu = GpuFrameReport::read(&mut **renderer);
        };
        // R1036 PR-17 §2 #7 — record the uncontaminated fidelity fingerprint of
        // the frame just ENCODED + presented for this window (per-TextGrid
        // used-row count + content hash + present outcome). Written ONLY here on
        // the winit paint path, never by an RPC recompute, so
        // `scene/render_fidelity` can answer "what is actually displayed"
        // without the `last_paint_scene` post-dispatch-finalize contamination.
        self.core
            .record_presented_frame(&spec_id, present_ok, (w.get(), h.get()), &paint_scene);
        // The post-paint helpers (`emit_accesskit_for_window`,
        // `publish_ime_for_window`) each re-acquire their own slot
        // borrow internally and take `&mut self.core` — the scope
        // above released the long-held slot borrow so this is safe.
        // R1027 §5.16 §5.40 — AccessKit window bounds are the LOGICAL
        // (`w`, `h`) dims (matching the logical paint scene the AccessNode
        // rects are collected from); `scale` rides through to the root
        // node transform so the AT side still sees physical-pixel coords.
        let access_nodes = self.emit_accesskit_for_window(
            window_id,
            &spec_id,
            &paint_scene,
            w.get(),
            h.get(),
            scale,
        );
        // R56.2.c §5.13 §5.38 — push IME candidate window position
        // to the platform IME (per-window since R670.B).
        self.publish_ime_for_window(window_id, &paint_scene);
        // R890 §5.12 §5.16 — no per-frame [`pinion_rpc::LayoutNode`]
        // build any more: the paint scene `finalize_frame_for_window`
        // stores in the addressed window's router IS the layout
        // source; the substrate projects it on demand at the AI-paced
        // RPC read (`ShellCore::last_paint_layout_for_window`). The
        // R671-era per-frame build + per-slot clone paid an O(painted
        // tree) walk on EVERY winit frame for data only RPC consumed.
        // R683 §5.16 — `spec_id` is `Cow<'static, str>`; clone for
        // the post-borrow `finalize_frame_for_window` call (clones
        // are `Cow`-cheap for `Borrowed`, `String::clone` for
        // `Owned`).
        let spec_id_for_finalize = self.windows.get(&window_id).map(|s| s.spec_id.clone());
        // R682 §5.16 atomic 3 — capture the post-paint cache
        // snapshot before publishing. `to_vello_cached` brackets its
        // walk with begin_paint / end_paint internally, so the
        // counters / damage region read here are the post-sweep
        // publishable snapshot. Computed before the optional slot
        // borrow below + the publish call so neither the slot
        // borrow nor the `&mut self.core` publish call conflict.
        let cache_stats = self
            .windows
            .get(&window_id)
            .map(|slot| slot.fragment_cache.stats());
        // R681 §2 #4 atomic 2 — capture the immediate-mode pacing flag
        // before `paint_scene` is moved into `finalize_frame_for_window`
        // below. Published to the substrate after finalize (keyed by the
        // finalize target window, like `record_frame_timing`), so pacing
        // and the §5.16 jank profiler read it from one home.
        let has_immediate_subtree = paint_scene.has_immediate_mode_subtree();
        // R682 §5.16 atomic 3 — publish the snapshot into the
        // GUI-agnostic substrate so RPC + tests can introspect cache
        // observability without depending on the
        // vello::Scene-bearing FragmentCache directly. Skip when the
        // spec_id is unknown (defensive — the slot borrow above
        // would have already returned in that case).
        //
        // R683 §5.16 — `spec_id_for_finalize` is now
        // `Option<Cow<'static, str>>`; `as_deref()` projects to
        // `Option<&str>` so both substrate signatures (which take
        // `&str`) consume the same borrow shape they had under the
        // pre-R683 `&'static str` field type. `unwrap_or` returns
        // `&str` directly because `pinion_runtime::DEFAULT_WINDOW`
        // is still a `&'static str`.
        let target_window: &str = spec_id_for_finalize
            .as_deref()
            .unwrap_or(pinion_runtime::DEFAULT_WINDOW);
        self.close_frame(
            target_window,
            paint_scene,
            cache_stats,
            FrameClose {
                frame_start,
                build_us,
                encode_us,
                acquire_us,
                render_us,
                gpu,
                encode_nodes,
                access_nodes,
                draw,
                has_immediate_subtree,
            },
        );
    }
}

/// (R1538 §5.16) Everything one painted frame measured about itself, handed
/// from [`AppShell::render_window`] to [`AppShell::close_frame`].
///
/// A struct and not eight arguments: six of the nine are same-typed numbers
/// measured hundreds of lines above the call that consumes them, which is the
/// transposition [`pinion_runtime::PaintWork`] refused for the same reason.
/// Every round that adds a per-frame observable adds a field here rather than
/// another positional `u64` nobody can check at the call site.
#[derive(Clone, Copy)]
struct FrameClose {
    /// When the productive frame opened; `total_us` closes against it.
    frame_start: Instant,
    /// `view` + layout pass.
    build_us: u64,
    /// Structured-scene to `vello` fragment encode.
    encode_us: u64,
    /// The vsync block (idle, not work).
    acquire_us: u64,
    /// GPU command-buffer record + submit, CPU-side.
    render_us: u64,
    /// What this paint learned about the GPU's own clock (R1537).
    gpu: GpuFrameReport,
    /// Scene nodes the encode walk entered (R1538).
    encode_nodes: u32,
    /// Nodes the accessibility walk produced (R1538).
    access_nodes: u32,
    /// What the submitted scene will ask the renderer to draw (R1556).
    draw: pinion_runtime::DrawWork,
    /// Whether the painted scene carried an immediate-mode subtree.
    has_immediate_subtree: bool,
}

impl FrameClose {
    /// The phase durations as a [`pinion_runtime::FrameTiming`], with the
    /// total the caller just closed.
    fn timing(&self, total_us: u64) -> pinion_runtime::FrameTiming {
        pinion_runtime::FrameTiming::new(
            self.build_us,
            self.encode_us,
            self.acquire_us,
            self.render_us,
            total_us,
        )
    }
}

impl<V: WidgetView + 'static> AppShell<V> {
    /// R1538 §5.16 — close the frame: publish what the paint measured, hand
    /// the scene to the substrate, record the sample, and publish the pacing
    /// signal the next `about_to_wait` reads.
    ///
    /// Lifted out of [`Self::render_window`] because it is the one part of
    /// that function with a single job, and because every round that adds a
    /// per-frame observable adds a line here — R1537 the GPU report, R1538 two
    /// censuses. The parts arrive as [`FrameClose`] rather than as loose
    /// arguments for [`pinion_runtime::PaintWork`]'s reason: they are same-typed
    /// numbers, and this is a call made once, far from where they were measured.
    fn close_frame(
        &mut self,
        target_window: &str,
        paint_scene: pinion_core::Scene,
        cache_stats: Option<pinion_runtime::FragmentCacheStats>,
        close: FrameClose,
    ) {
        if let Some(stats) = cache_stats {
            self.core.publish_fragment_cache_stats(target_window, stats);
        }
        // R51.80 §5.12 §5.35 — hand the rendered scene to the
        // substrate. `finalize_frame` refreshes the input router +
        // intent drain in one method; the stored scene doubles as the
        // `scene/layout` source (R890).
        //
        // R672 §5.35 §5.41 — route through `finalize_frame_for_window`
        // so the addressed window's [`pinion_runtime::InputRouter`]
        // (not the binding-wide single router pre-R672) sees the
        // paint scene. Each window's pointer state stays isolated;
        // cross-window paint cycles no longer flip-flop hover state.
        self.core
            .finalize_frame_for_window(target_window, paint_scene);
        // R907 §5.16 §5.7 — close the total-frame span (after finalize)
        // and record the sample into the per-window rolling profiler
        // window. The O(window) aggregate fold is deferred to the
        // AI-paced `scene/frame_timings` read, never run here.
        let total_us = instant_delta_us(close.frame_start, Instant::now());
        // R1459 §5.16 §5.36 — the frame's WORK counts ride the same sample as
        // its durations. `build_us` is the whole settle loop, so a 4ms frame
        // that ran one heavy pass and a 4ms frame whose four cheap passes
        // disagree are indistinguishable by time alone — and they want
        // opposite fixes. Read from the window the paint just wrote, so the
        // counts and the spans describe the same frame.
        self.record_frame_sample(
            target_window,
            close.timing(total_us),
            close.gpu,
            close.encode_nodes,
            close.access_nodes,
            close.draw,
        );
        // R1361 §5.16 §5.22 — hand the freshly-recorded history to any
        // in-app profiler HUD (`use_frame_timings`). Immediately after
        // the record, so the next paint charts a window that includes
        // this frame; demand-gated, so a binding that does not chart
        // itself pays nothing here.
        //
        // The one-frame lag is inherent, not a defect: a frame's cost is
        // only known once it is painted, so no HUD can plot the frame it
        // is drawing. It plots the frames behind it.
        self.core.publish_frame_timings(target_window);
        // R681 §2 #4 atomic 2 — publish the sticky immediate-mode flag
        // into the substrate (one home with `target_fps`). The next
        // `about_to_wait` reads it to choose `ControlFlow::Wait` vs
        // `WaitUntil(deadline)`, and the §5.16 jank profiler derives the
        // same frame budget from it — pacing and observability cannot
        // disagree because they read this one signal.
        self.core
            .set_immediate_subtree_for_window(target_window, close.has_immediate_subtree);
    }

    /// R668 §5.16 / R1072.1 — the `IntrinsicAfterFirstPaint` post-first-paint
    /// resize hook, extracted from `Self::render_window` (the same discipline
    /// `publish_ime_for_window` / `emit_accesskit_for_window` follow to keep the
    /// parent under the `clippy::too_many_lines = 100` ceiling — preferred over a
    /// lint suppression).
    ///
    /// The first painted scene carries layout-computed rects on every node, so
    /// walking the tree ([`Scene::intrinsic_content_size`](pinion_core::Scene::intrinsic_content_size)) gives the tight
    /// `(width, height)` the content wants; clamp to `[min, max]` and forward to
    /// [`Window::request_inner_size`]. winit emits a `WindowEvent::Resized` on
    /// acceptance which re-enters the layout pass at the new viewport next paint.
    /// Self-draining: `Fixed`-strategy paints and every steady-state paint after
    /// the first take the `None` branch and no-op. A vanished slot / non-`Active`
    /// render state is also a no-op.
    fn apply_pending_intrinsic_resize(
        &mut self,
        window_id: WindowId,
        paint_scene: &pinion_core::Scene,
        w: core::num::NonZeroU32,
        h: core::num::NonZeroU32,
    ) {
        let Some(slot) = self.windows.get_mut(&window_id) else {
            return;
        };
        let RenderState::Active { window, .. } = &mut slot.render else {
            return;
        };
        let Some((min, max)) = slot.pending_intrinsic_resize.take() else {
            return;
        };
        let (content_w, content_h) = paint_scene.intrinsic_content_size();
        let target_w = content_w.clamp(min.0, max.0);
        let target_h = content_h.clamp(min.1, max.1);
        if (target_w, target_h) != (w.get(), h.get()) {
            let _ = window
                .request_inner_size(LogicalSize::new(f64::from(target_w), f64::from(target_h)));
            // Force-request a redraw so the next event-loop pass re-enters
            // `render` against the updated inner_size and paints the final
            // layout immediately rather than idling on the now-undersized
            // first-paint frame.
            window.request_redraw();
        }
    }

    /// R670.B §5.16 — per-window IME candidate publish helper.
    /// Extracted from `render_window` so the parent fn stays under
    /// the workspace `clippy::too_many_lines = 100` ceiling after
    /// the per-window cluster lift R670.B added.
    ///
    /// Runs `V::ime_caret_rect` inside the substrate's root-owner
    /// scope so application hooks (`use_text_edit_state(tag)` /
    /// `use_layout_cache(key)`) resolve through `Owner::current()`
    /// inside the trait call. Dedups against the slot's
    /// `last_ime_cursor_area` so an unchanged caret (most frames —
    /// caret moves only on key press or text mutation) skips the
    /// `winit` boundary call.
    fn publish_ime_for_window(&mut self, window_id: WindowId, paint_scene: &pinion_core::Scene) {
        let cached_state = *self.core.cached_state();
        let focused_owned = self.core.focus().focused().map(str::to_owned);
        let owner = self.core.root_owner().clone();
        let ime_rect =
            owner.run(|| V::ime_caret_rect(&cached_state, paint_scene, focused_owned.as_deref()));
        let Some(rect) = ime_rect else { return };
        let rect_tuple = (rect.x, rect.y, rect.width, rect.height);
        let Some(slot) = self.windows.get_mut(&window_id) else {
            return;
        };
        if slot.last_ime_cursor_area == Some(rect_tuple) {
            return;
        }
        let RenderState::Active { window, .. } = &slot.render else {
            return;
        };
        let pos = LogicalPosition::new(f64::from(rect.x), f64::from(rect.y));
        let size = LogicalSize::new(
            f64::from(rect.width.max(1.0)),
            f64::from(rect.height.max(1.0)),
        );
        window.set_ime_cursor_area(pos, size);
        slot.last_ime_cursor_area = Some(rect_tuple);
    }

    /// R770 §5.15 — route the three winit file-DnD events to the
    /// binding's `WidgetView` file hooks via [`ShellCore`]. winit
    /// normalises the platform file-DnD (X11 `XdndDrop` / Wayland
    /// data-device / macOS `NSDraggingDestination` / Windows
    /// `IDropTarget`) into window-scoped events (path, no drop
    /// coordinate); the `scene/hover_file` / `scene/hover_file_cancel` /
    /// `scene/drop_file` RPC peers reach the same `ShellCore` methods
    /// (§2 invariant #2). Split out of `window_event` so that dispatch
    /// stays under the line cap; the caller's arm guarantees one of these
    /// three variants, so the wildcard is unreachable in practice.
    fn handle_file_dnd(&mut self, spec_id: &str, event: &WindowEvent) {
        match event {
            WindowEvent::HoveredFile(path) => {
                self.core
                    .file_hover_for_window(spec_id, &path.to_string_lossy());
            }
            WindowEvent::HoveredFileCancelled => {
                self.core.file_hover_cancel_for_window(spec_id);
            }
            WindowEvent::DroppedFile(path) => {
                self.core
                    .file_drop_for_window(spec_id, &path.to_string_lossy());
            }
            _ => {}
        }
    }

    /// R51.78 §5.39 — pressed-key routing surface. Translates the
    /// winit-specific [`Key`] enum into the winit-free
    /// [`ShellCore::handle_focus_traverse`] /
    /// [`ShellCore::handle_character_key`] /
    /// [`ShellCore::handle_named_key`] triple, keeping `Escape`
    /// shell-side because it terminates the event loop.
    ///
    /// Pre-R51.78 this helper did the full dispatch (focus traversal,
    /// `V::keybinding` lookup, `V::apply_key`) inline and was untestable
    /// without a winit `ActiveEventLoop`. R51.78 pushes the substrate
    /// logic into [`ShellCore`] and leaves only the winit↔substrate
    /// adapter shape here.
    fn handle_key_press(&mut self, event_loop: &ActiveEventLoop, logical_key: &Key, repeat: bool) {
        match logical_key.as_ref() {
            // R693 §5.39 — while a modal focus trap is active, Escape
            // dismisses the modal, not the window: route it to the
            // widget's `apply_key` (the dialog binding maps Escape →
            // cancel) instead of `event_loop.exit`. WAI-ARIA modal
            // contract: you cannot Escape past an open dialog to quit;
            // you dismiss the dialog first. With no modal up, Escape
            // keeps the standalone-app convention of closing the window.
            Key::Named(NamedKey::Escape) => {
                // R695 §5.35 — offer Escape to the focused widget first
                // (the Tooltip's WCAG 1.4.13 dismiss, the Dialog's modal
                // cancel). Only fall back to the standalone-app
                // close-window convention when no widget consumes it AND
                // no modal trap is up — you cannot Escape past an open
                // modal to quit (WAI-ARIA modal contract).
                // R1071 PR-27 §5.39 §5.35 — carry the OS auto-repeat flag to
                // the binding (a modal dialog's Escape→cancel is idempotent,
                // but the binding owns the policy uniformly across keys).
                let handled = self.core.try_apply_key_inner("Escape", repeat);
                if !handled && !self.core.focus_is_modal() {
                    // R1363 §5.55 — Escape means QUIT, not "close this window",
                    // so it now routes through the app veto. Pre-R1363 it called
                    // `event_loop.exit()` right here and consulted NO binding
                    // hook — a veto bypass this shell and the TUI shell had each
                    // hard-coded independently, for want of a Quit verb.
                    self.request_quit(event_loop);
                }
            }
            Key::Named(NamedKey::Tab) => {
                // R938 §5.22 §5.39 — offer Tab to the focused widget first (a
                // multi-line code editor with `tab_indents` on indents the
                // selection; Shift+Tab dedents — the shell tracks the Shift
                // bit in `self.core.modifiers`). Only fall back to focus
                // traversal when no widget consumes it — the mirror of the
                // Escape offer-first arm above (a focused Tooltip / Dialog
                // gets first refusal before the shell default). Every
                // non-editor widget reports Tab unhandled, so traversal is
                // byte-unchanged for them.
                if !self.core.try_apply_key_inner("Tab", repeat) {
                    self.core
                        .handle_focus_traverse(self.core.modifiers_shift_key());
                }
            }
            Key::Character(c) => self.core.handle_character_key_inner(c, repeat),
            Key::Named(named) => {
                if let Some(key_str) = named_key_str(named) {
                    self.core.handle_named_key_inner(key_str, repeat);
                }
            }
            _ => {}
        }
    }

    /// R1073 PR-27.4 §5.39 §5.16 §5.35 — body of the
    /// `WindowEvent::KeyboardInput` arm, extracted to keep `window_event`
    /// under the workspace `clippy::too_many_lines` ceiling (the app.rs split
    /// convention shared with [`Self::handle_mouse_button`] /
    /// `handle_file_dnd`). Takes the `Copy` [`WindowId`] (not a borrowed
    /// `spec_id`) and re-resolves the canonical [`WindowSpec::id`](crate::WindowSpec)
    /// internally, so the high-frequency keyboard / auto-repeat path allocates
    /// nothing (the sibling mouse / file arms `to_owned()` because they are
    /// low-frequency).
    fn handle_keyboard_input(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: &winit::event::KeyEvent,
        is_synthetic: bool,
    ) {
        // R683 §5.16 — re-resolve `WindowId` → canonical spec id (the same
        // fallback `window_event` uses); borrows `self.windows`, disjoint from
        // the `self.core` calls below.
        let spec_id: &str = self
            .windows
            .get(&window_id)
            .map_or(pinion_runtime::DEFAULT_WINDOW, |s| &*s.spec_id);
        let pressed = event.state == ElementState::Pressed;
        // R1073.1 PR-27.4 §5.39 §5.16 §5.35 — resolve TWO vocabularies in one
        // match. `chord_key` (R882 §5.41 `named_key_str`) feeds the chord cache
        // on BOTH edges — auto-repeat re-sends `Pressed`, idempotent against the
        // cache. `gate_key` (`dispatch_named_key_str`, a superset adding the
        // shell-reserved `Escape` / `Tab`) feeds the press-owner gate, so EVERY
        // key `handle_key_press` dispatches is covered, not just chord keys.
        let (chord_key, gate_key): (Option<&str>, Option<&str>) = match event.logical_key.as_ref() {
            Key::Named(named) => (named_key_str(named), dispatch_named_key_str(named)),
            Key::Character(c) => (Some(c), Some(c)),
            _ => (None, None),
        };
        // Passive chord cache tracks PHYSICAL held state on both edges — for
        // synthetic events too, so a key held across a focus transition stays
        // armed (the R882 pan chord's focus continuity). Auto-repeat re-sends
        // `Pressed`, idempotent against the cache.
        if let Some(key_str) = chord_key {
            self.core.note_key_state(key_str, pressed);
        }
        // R1076 PR-28 §5.39 §5.16 §5.35 — gate the key-edge decision through the
        // synthetic-aware ShellCore seam. winit emits SYNTHETIC key events (a
        // `Pressed` for every held key when a window GAINS OS focus, a `Released`
        // when it LOSES focus; `is_synthetic` on X11 / Windows) only to sync key
        // state to a newly-(un)focused window — they are NOT user intent.
        // Dispatching them as real presses self-sustains a dock-toggle flap: a
        // held shortcut's dispatch moves OS focus, the focus change emits a
        // synthetic `Pressed`, which fires the toggle again. `apply_key_edge`
        // excludes synthetic events from the gate, the press-owner lifecycle
        // (R1071 pin / R1073.1 clear), and dispatch; the physical key is unchanged
        // across the transition, so the owner a real keydown pinned survives to
        // the matching physical keyup. For a physical edge it is byte-identical to
        // the pre-R1076 inline gate (R1071 OS-focus + R1073 press-owner snapshot,
        // `gate_key` `None` = a media / dead key the shell does not dispatch).
        // R1078 PR-28.2 — `apply_key_edge` returns `Some(repeat)` for a dispatched
        // edge with the auto-repeat flag DERIVED from the press-owner gate, not
        // winit's `event.repeat`. winit resets its own repeat detector on every
        // focus transition (`x11/event_processor.rs`: the first non-synthetic event
        // after a focus gain is never flagged a repeat), so a held shortcut whose
        // dispatch bounces OS focus would see `event.repeat == false` on each
        // auto-repeat and a repeat-dropping toggle would never drop (the residual
        // sprag dock flap after R1076). The press-owner survives focus transitions
        // by design, so it is the focus-robust repeat source.
        if let Some(repeat) = self
            .core
            .apply_key_edge(spec_id, gate_key, pressed, is_synthetic)
        {
            self.handle_key_press(event_loop, &event.logical_key, repeat);
        }
    }

    /// R51.62 §5.40 — relay one winit `WindowEvent` to the
    /// `accesskit_winit::Adapter` (if attached).
    ///
    /// Every winit event must reach the adapter before the shell
    /// processes it so the AT-side mirror of window bounds / focus
    /// state stays consistent. The adapter handles `Moved` /
    /// `Resized` / `Focused` internally (`set_root_window_bounds` +
    /// `update_window_focus_state`) and ignores everything else, so
    /// the relay is unconditional.
    /// R670.B §5.16 — per-window accesskit relay. Looks up the
    /// `WindowSlot` by `window_id` and forwards the event to that
    /// slot's adapter if attached. AccessKit canonical: 1 adapter
    /// per window (multi-window = multi-adapter, NOT auto-merged) so
    /// the event MUST reach the same window's adapter that the
    /// dispatch is targeting; routing through the primary's adapter
    /// would leak winit coordinates between windows.
    fn forward_to_accesskit(&mut self, window_id: WindowId, event: &WindowEvent) {
        if let Some(slot) = self.windows.get_mut(&window_id)
            && let (Some(adapter), RenderState::Active { window, .. }) =
                (slot.accesskit.as_mut(), &slot.render)
        {
            adapter.process_event(window, event);
        }
    }

    /// Dispatch a winit mouse-button transition to the substrate's
    /// per-window input arcs. Split out of [`Self::window_event`] to keep
    /// that dispatcher under the workspace `clippy::too_many_lines` (100)
    /// ceiling (the app.rs extract convention).
    ///
    /// - **Left Pressed / Released** — pointer down / up
    ///   ([`ShellCore::mouse_pressed_for_window`] /
    ///   [`ShellCore::mouse_released_for_window`]).
    /// - **Middle Pressed / Released** — R881 §5.35 §5.49 middle-button
    ///   gesture pair ([`ShellCore::middle_pressed_for_window`] /
    ///   [`ShellCore::middle_released_for_window`]). The router's
    ///   `DragLatch` resolves the press: a drag past the dead zone pans
    ///   the pinned scrollable / canvas (the DCC / the engine middle-drag),
    ///   a release-in-place runs the R56.2.e `apply_middle_click` paste
    ///   funnel (X11 PRIMARY at the focused text widget — paste moved
    ///   from press to release, the xterm / the toolkit convention).
    /// - **Right Pressed** — R772 §5.53 `apply_secondary_click`, the
    ///   own-renderer context-menu open path (R771.1: pinion draws its own
    ///   menu on every platform). `secondary_click_for_window` reads the
    ///   cached cursor position for `spec_id` and dispatches through
    ///   [`CoreShell::apply_secondary_click`](pinion_runtime::CoreShell::apply_secondary_click).
    ///
    /// winit normalises each platform's button events (X11 `ButtonEvent` /
    /// Wayland `wl_pointer` button / macOS `NSEvent` / Windows
    /// `WM_*BUTTONDOWN`) under one enum, so these arms cover every backend.
    /// Back / forward (and `Other`) buttons have no pinion semantics yet and
    /// are ignored.
    ///
    /// R1416 §5.35 §5.15 — every left / middle / right EDGE now routes through
    /// the unified [`ShellCore::pointer_button_for_window`](crate::ShellCore::pointer_button_for_window)
    /// seam (which the RPC `scene/pointer_button` drain also reaches), so a
    /// widget that owns the raw multi-button stream receives the button verbatim
    /// while a non-raw widget keeps the per-button GUI arc unchanged. The right
    /// RELEASE arm — absent before R1416 (`_ => {}` swallowed it) — now exists so
    /// a raw sink sees the release edge; for a non-raw widget it is still a no-op
    /// inside the seam (the context menu is a press-edge one-shot). The
    /// chrome-press interception and drag-preview teardown stay here, at the
    /// winit / window-handle layer, around the seam call.
    fn handle_mouse_button(
        &mut self,
        spec_id: &str,
        button: MouseButton,
        state: ElementState,
        event_loop: &ActiveEventLoop,
    ) {
        use pinion_core::{PointerButton, PointerEdge};
        let pbutton = match button {
            MouseButton::Left => PointerButton::Left,
            MouseButton::Middle => PointerButton::Middle,
            MouseButton::Right => PointerButton::Right,
            // Back / Forward / Other — no pinion semantics yet.
            MouseButton::Back | MouseButton::Forward | MouseButton::Other(_) => return,
        };
        let edge = match state {
            ElementState::Pressed => PointerEdge::Down,
            ElementState::Released => PointerEdge::Up,
        };
        // R1121 §5.16 §5.39 — a left PRESS on a client-side window-chrome control
        // (borderless title bar) is consumed by the shell, not forwarded to
        // widget routing / the button seam.
        if pbutton == PointerButton::Left
            && edge == PointerEdge::Down
            && self.try_chrome_press(spec_id, event_loop)
        {
            return;
        }
        self.core.pointer_button_for_window(spec_id, pbutton, edge);
        // R1147 §5.51 — a left release ends the drag session, so hide the
        // cross-desktop drag preview (kept for reuse) + clear suppression.
        if pbutton == PointerButton::Left && edge == PointerEdge::Up {
            self.hide_drag_preview();
        }
    }

    /// (R1121 §5.16 §5.39) Resolve `spec_id`'s live winit `Window` handle, or
    /// `None` when the window has no active render slot.
    fn window_arc_for_spec(&self, spec_id: &str) -> Option<&Arc<Window>> {
        let window_id = self.spec_id_to_window_id.get(spec_id).copied()?;
        Self::slot_window(self.windows.get(&window_id)?)
    }

    /// (R1121 §5.16 §5.39 §2 #2) Intercept a left-press that landed on a
    /// client-side window-chrome control (a borderless window's title bar).
    /// Returns `true` when the press hit a control and was consumed (the
    /// normal widget routing is then skipped).
    ///
    /// The control is read from the live hover target the `CursorMoved` arm
    /// already recorded, so the SAME hit-test the router uses drives it — an AI
    /// `scene/click` on the introspectable control tag reaches this identical
    /// path (§2 #2). `minimize` / `maximize` / move map straight to the winit
    /// `Window` (pinion owns the handle); `close` routes through
    /// [`Self::apply_window_control`] as [`ControlProducer::ChromePress`], the
    /// same arm `WindowEvent::CloseRequested` (the OS X on a decorated window)
    /// takes. R1170 §5.16 — a close is offered to the
    /// [`WidgetView::window_close_requested`] binding seam first (a torn-off panel
    /// docks back). R1363 §5.55 — and it can no longer exit: this sentence used
    /// to end "an unhandled close exits", which stopped being true one round
    /// after it was written.
    fn try_chrome_press(&mut self, spec_id: &str, event_loop: &ActiveEventLoop) -> bool {
        let Some(tag) = self.core.hover_target_for_window(spec_id, PointerId::MOUSE) else {
            return false;
        };
        if pinion_overlay::chrome_tag_semantic(tag).is_none() {
            return false;
        }
        // ★★ R1701 — the chrome's own click ordinal, because this press is about
        // to be CONSUMED and the widget router's detector will never see it.
        // The cursor comes from the core rather than from a field of this
        // struct: the same position the hit test above resolved with, so the
        // ordinal is measured against the point the press actually landed on.
        let (cx, cy) = self
            .core
            .cursor_position_for_window(spec_id, PointerId::MOUSE)
            .unwrap_or_default();
        let tag = tag.to_owned();
        let count = self
            .chrome_click_window
            .entry(Cow::Owned(spec_id.to_owned()))
            .or_default()
            .press(std::time::Instant::now(), cx, cy, &tag);
        let Some(action) =
            pinion_overlay::chrome_press_intent(&tag, count).map(chrome_action_for_semantic)
        else {
            return false;
        };
        match action {
            // R1188 — the discrete buttons execute through the arm the RPC
            // click drain also reaches (one execution path for both inputs).
            ChromeAction::Control(control) => {
                self.apply_window_control(
                    spec_id,
                    control,
                    ControlProducer::ChromePress,
                    event_loop,
                );
            }
            ChromeAction::Move => {
                // OS-driven interactive move; a borderless window has no OS
                // title bar, so the chrome grip is the move handle.
                if let Some(window) = self.window_arc_for_spec(spec_id) {
                    let _ = window.drag_window();
                }
            }
            ChromeAction::Resize(direction) => {
                // OS-driven interactive resize; a borderless window has no OS
                // frame, so a chrome resize edge / corner is the grab handle.
                if let Some(window) = self.window_arc_for_spec(spec_id) {
                    let _ = window.drag_resize_window(direction);
                }
            }
        }
        true
    }

    /// (R1188 §5.16 §5.49 §2 #2) Execute one discrete window control against
    /// `spec_id`'s window — the ONE execution arm EVERY producer shares, so no
    /// two ways of asking for the same thing can drift.
    ///
    /// The roster is [`ControlProducer`] (R1364), and deliberately not a list
    /// here: this sentence used to carry the count, and so did eight others, and
    /// three of them were wrong.
    ///
    /// * `Close` — R1170 §5.16 §5.39 per-window close: offered to the binding
    ///   first (a torn-off panel docks back via
    ///   [`WidgetView::window_close_requested`]); an unhandled close of the
    ///   PRIMARY window becomes a quit REQUEST, and of a secondary window is
    ///   declined. Because [`ControlProducer::Binding`] lands HERE, a binding
    ///   that closes itself still passes its own veto — the seam grants no
    ///   privileged exit.
    /// * `Minimize` / `Maximize` — straight to the winit `Window` (pinion owns
    ///   the handle); a missing render slot (window already closing) is a no-op.
    ///
    /// The app-termination map — every way this process can end — lives on
    /// [`Self::request_quit`] (R1364). R1362 wrote it here because this arm's
    /// unhandled `Close` WAS an exit; §5.55 severed that, so a census of app
    /// terminations no longer belongs in the rustdoc of a window operation.
    /// This arm can no longer end the app at all: it routes a primary-window
    /// `Close` to `request_quit` and declines a secondary one.
    fn apply_window_control(
        &mut self,
        spec_id: &str,
        control: pinion_overlay::WindowControl,
        producer: ControlProducer,
        event_loop: &ActiveEventLoop,
    ) {
        // R1362 PR-65 — an unregistered `spec_id` names no window, so there is
        // nothing to control: drop the request, the contract
        // [`WindowControlSink::request_window_control`] documents.
        //
        // `Minimize` / `Maximize` have always had this for free (they resolve
        // through `Self::window_arc_for_spec`, which returns `None` on a miss);
        // `Close` did not, and would instead offer an unknown id to
        // `V::window_close_requested`, get the default `false` back, and EXIT THE
        // APP — a catastrophic answer to a stale id. Unreachable before R1362:
        // every other `ControlProducer` derives `spec_id` from a live window (a
        // hit-test, the winit `WindowId` map, a resolved click target).
        // `ControlProducer::Binding` is the first that can name an arbitrary
        // window — a `String` built off the UI thread, possibly a typo or an id
        // whose window `reconcile_windows` already dropped. R1364 — which is why
        // the warn carries the `producer`: the one path that can reach here is
        // also the one whose author is not looking at the window.
        //
        // Gated on REGISTRATION (`spec_id_to_window_id`), not on a live `Window`
        // handle: `window_arc_for_spec` also misses a `RenderState::Suspended(None)`
        // slot, and a suspended (mobile) window must stay closable.
        if !self.spec_id_to_window_id.contains_key(spec_id) {
            tracing::warn!(
                target: "pinion::shell",
                window = %spec_id,
                ?control,
                ?producer,
                "window control requested for an unregistered window; dropped",
            );
            return;
        }
        match control {
            pinion_overlay::WindowControl::Close => {
                if self
                    .core
                    .root_owner()
                    .run(|| V::window_close_requested(spec_id))
                {
                    // The binding handled it (a torn-off panel docked back by
                    // dropping its WindowSpec); the reconcile pass closes the OS
                    // window and the empty-set policy is checked there.
                    return;
                }
                if spec_id == pinion_runtime::DEFAULT_WINDOW {
                    // R1363 §5.55 — the standalone-app convention, now routed
                    // through the APP veto instead of a bare exit. The primary
                    // window IS the app: its scope is the binding-wide reactive
                    // anchor, which is why `remove_window` refuses to tear it
                    // down. So an unhandled close of it is a QUIT request, not a
                    // window op — and `app_quit_requested` gets to refuse.
                    self.request_quit(event_loop);
                } else {
                    // R1363 §5.55 — pre-R1363 THIS EXITED THE APP: an unhandled
                    // close of any window fell through to `event_loop.exit()`.
                    // A secondary window is not the app, and pinion's window set
                    // is declarative (the binding owns `windows_signal`), so the
                    // shell cannot remove it unilaterally — the next reconcile
                    // would re-create it. Declining to remove it IS the answer.
                    tracing::warn!(
                        target: "pinion::shell",
                        window = %spec_id,
                        "close requested for a secondary window, but the binding                          did not remove it from `windows_signal`; ignoring                          (pre-R1363 this exited the app)",
                    );
                }
            }
            pinion_overlay::WindowControl::Minimize => {
                if let Some(window) = self.window_arc_for_spec(spec_id) {
                    window.set_minimized(true);
                }
            }
            pinion_overlay::WindowControl::Restore => {
                if let Some(window) = self.window_arc_for_spec(spec_id) {
                    window.set_minimized(false);
                }
            }
            pinion_overlay::WindowControl::Maximize => {
                if let Some(window) = self.window_arc_for_spec(spec_id) {
                    window.set_maximized(!window.is_maximized());
                }
            }
            pinion_overlay::WindowControl::Show => {
                if let Some(window) = self.window_arc_for_spec(spec_id) {
                    window.set_visible(true);
                }
            }
            pinion_overlay::WindowControl::Hide => {
                if let Some(window) = self.window_arc_for_spec(spec_id) {
                    window.set_visible(false);
                }
            }
        }
    }

    /// (R1363 §5.55 §2 #6) End the app on purpose — the ONE arm every quit
    /// producer reaches.
    ///
    /// Producers: `Escape` (the standalone convention), an unhandled `Close` of
    /// the PRIMARY window, the last window closing under
    /// [`WidgetView::quit_on_last_window_closed`], a binding's own
    /// [`QuitSink`](pinion_core::QuitSink) (via [`AppEvent::QuitRequested`]), and
    /// R1364's `app/quit` RPC (§2 #2 — the AI's peer of Escape, drained here
    /// after the response write). Every one is offered to
    /// [`pinion_core::WidgetCore::app_quit_requested`] first, so none of them —
    /// not the binding's own request, not an AI's — grants a privileged exit past
    /// the binding's unsaved-changes gate.
    ///
    /// Pre-R1363 there was no such arm: `Escape` called `event_loop.exit()`
    /// inline (bypassing every binding veto, in BOTH shells independently) and
    /// an unhandled `WindowControl::Close` fell through to an exit. That is the
    /// conflation §5.55 splits.
    ///
    /// # Every way this app can terminate (R1362 PR-65 R-65.2; R1364 enforced)
    ///
    /// Enumerated because it is otherwise discoverable only by reading every
    /// call site — which is what let "a binding cannot request its own exit" go
    /// unnoticed until sprag hit it. The map lives on this method, not on
    /// [`Self::apply_window_control`] where R1362 wrote it: that arm's
    /// unhandled `Close` used to BE an exit, and §5.55 severed it.
    ///
    /// Two families, and BOTH live in this file. `event_loop.exit()` needs an
    /// `&ActiveEventLoop`, which in this crate reaches only `ApplicationHandler`
    /// callbacks on [`AppShell`] (winit also passes one to the deprecated
    /// `EventLoop::run` closure, which pinion does not use — it runs `run_app`),
    /// so `ShellCore` — winit-free for the §2 #6 GUI/TUI dual — can never reach
    /// one. `std::process::exit` needs nothing, so it is enumerated on its own
    /// evidence, by grep, not by an argument from types.
    ///
    /// The table is MACHINE-CHECKED against this file's source text by
    /// `r1364_termination_map_tests`, so each row reads `` `fn` ×N → `family` ``
    /// rather than prose. R1362 published it as prose and R1363 rewired three of
    /// its six rows one round later — `Close` and `handle_key_press` stopped
    /// exiting inline, this arm became THE exit — and every gate stayed green
    /// over a map describing a world that no longer existed.
    ///
    /// | Termination | Trigger | Binding-drivable? |
    /// |---|---|---|
    /// | `Self::request_quit` ×1 → `event_loop.exit()` | this arm: every producer above, once `app_quit_requested` declines to handle it | **yes**, and vetoable — that is the point |
    /// | `Self::resumed` ×1 → `event_loop.exit()` | the spec list is empty on ANY resume | **yes**, and NOT vetoable — see below |
    /// | `Self::resume_spec` ×2 → `event_loop.exit()` | `create_window` / renderer init failed | no — error paths |
    /// | `try_headless_screenshot` ×3 → `std::process::exit(1)` | `PINION_SCREENSHOT` set + screenshot init / file create / render failed | no — error paths, and only under that env var |
    ///
    /// Two rows earn their footnotes:
    ///
    /// * **`resumed` is NOT boot-only, and does NOT pass this arm.** winit
    ///   re-issues `resumed` across the mobile suspend/resume lifecycle (which is
    ///   why [`AppShell::suspended`] exists and why slots cache their `Window`),
    ///   and each one re-reads the binding's [`WidgetView::windows_signal`]. A
    ///   binding that empties that signal and is later resumed therefore DOES
    ///   drive an exit. So "empty window list" already means "quit" — but only on
    ///   a resume, which is a platform event the binding does not choose. That
    ///   accidental, unschedulable exit is precisely why R1362 gave the binding an
    ///   explicit request instead of widening it: an exit must be a statement of
    ///   intent, not a side effect of a list length observed at a moment the
    ///   binding cannot predict.
    /// * **`try_headless_screenshot` also ends the process by RETURNING.** With
    ///   `PINION_SCREENSHOT` set and the PNG written it returns `true`, and
    ///   `run_with_config` returns without ever building an event loop. That is
    ///   not a call site, so it cannot be a row; it is recorded here because
    ///   "every way this app can terminate" would otherwise be a lie by omission.
    ///
    /// Note what is NOT here: `Self::reconcile_windows` never exits. Dropping
    /// every spec from `windows_signal` at runtime closes the OS windows and
    /// leaves the loop parked in `about_to_wait` — a UI-less process (until the
    /// next `resumed`, per above), not an exit.
    fn request_quit(&mut self, event_loop: &ActiveEventLoop) {
        if self.core.root_owner().run(V::app_quit_requested) {
            // The binding handled it (raised a "Save changes?" modal, started an
            // async flush). It stays alive and owns the follow-up.
            return;
        }
        eprintln!(
            "shell: final state = {}",
            V::fmt_state_log(self.core.cached_state()),
        );
        event_loop.exit();
    }

    /// R51.62 §5.40 — dispatch one AT-side event reported by
    /// `accesskit_winit`.
    ///
    /// * `InitialTreeRequested` (AT attaches): trigger a redraw — the
    ///   next `render` will see `Adapter::update_if_active` as active
    ///   and emit the full tree. No state mutation here.
    /// * `ActionRequested(req)`: routed through
    ///   [`ShellCore::handle_action_request`] which lifts the
    ///   AccessKit action into the same dispatch path the winit
    ///   keyboard arm uses.
    /// * `AccessibilityDeactivated`: log only — the adapter stays in
    ///   place so a subsequent AT reconnect can reuse it without
    ///   recreating the per-window state.
    fn handle_accesskit_event(&mut self, event: accesskit_winit::Event) {
        use accesskit_winit::WindowEvent as AccessEvent;
        match event.window_event {
            AccessEvent::InitialTreeRequested => {
                self.core.request_redraw();
            }
            AccessEvent::ActionRequested(req) => {
                self.core.handle_action_request(&req);
            }
            AccessEvent::AccessibilityDeactivated => {
                tracing::debug!(target: "pinion::shell", "accesskit deactivated");
            }
        }
    }

    /// R670.B §5.16 — create one window+renderer+accesskit slot for
    /// the given [`WindowSpec`]. Extracted from `resumed()` because
    /// the single-resumed→N-windows fan-out would otherwise blow the
    /// `clippy::too_many_lines` (100) ceiling on the parent fn. Each
    /// spec gets its own winit `Window`, GPU renderer,
    /// `IntrinsicAfterFirstPaint` queue, and
    /// `accesskit_winit::Adapter`.
    ///
    /// `cached_window_id` is the `WindowId` from a prior suspend
    /// cycle if any (mobile platform suspend / resume reuse path).
    /// `make_primary` is `true` for the very first spec only — that
    /// spec's `WindowId` is recorded as `primary_window_id` so RPC
    /// frames omitting `{window: "..."}` default to it (R670.B
    /// atomic 1 wire).
    /// R683 §5.16 §5.41 — install the reconcile [`Effect`] that
    /// subscribes the binding's
    /// [`WidgetView::windows_signal`] `Signal<Vec<WindowSpec>>` and
    /// wakes the shell via [`AppEvent::WindowsDirty`] on every
    /// value-changing emit.
    ///
    /// Idempotent: never installs a second Effect on the same
    /// [`AppShell`] (gated by `self.reconcile_effect.is_some()` at
    /// the call site in [`Self::resumed`]). The shell only installs
    /// the
    /// Effect the very first time `resumed()` sees a non-`None`
    /// `windows_signal()`; subsequent suspend / resume cycles reuse
    /// the existing Effect because the same signal handle is
    /// memoised on the binding side via [`Owner::cache`](pinion_core::reactive::Owner::cache).
    ///
    /// The Effect closure captures three values by move:
    ///
    /// 1. `Rc<Signal<Vec<WindowSpec>>>` — `.get()` on every rerun
    ///    establishes the dependency on the signal so future
    ///    mutations fire `rerun` again. The closure does NOT use
    ///    the snapshot — diff happens in
    ///    [`Self::reconcile_windows`] where `&mut self` +
    ///    [`ActiveEventLoop`]
    ///    are available.
    /// 2. `EventLoopProxy<AppEvent>` — sends
    ///    [`AppEvent::WindowsDirty`] to wake the shell from any
    ///    blocking wait state. The proxy clone is `'static` and
    ///    survives the closure's lifetime (winit guarantees the
    ///    proxy is valid as long as the event loop is running).
    /// 3. `Rc<RefCell<Vec<WindowSpec>>>` — not currently captured;
    ///    reserved for a follow-up where the closure pre-computes
    ///    add/drop diffs to avoid wake / re-read costs on identical
    ///    re-emits. v1 keeps the closure minimal so the wake-up cost
    ///    is just `signal.get()` + `proxy.send_event(..)`.
    ///
    /// `send_event` failure (the event loop already exited) is
    /// silently ignored — the Effect closure has no recovery path
    /// at that point.
    ///
    /// The Effect's eager initial run fires `WindowsDirty` once
    /// immediately; the subsequent `reconcile_windows` call no-ops
    /// because [`Self::last_known_specs`] is initialised to the
    /// same snapshot the Effect just observed.
    fn install_reconcile_effect(&mut self, signal: Rc<Signal<Vec<WindowSpec>>>) {
        let signal_for_closure = Rc::clone(&signal);
        let proxy = self.proxy.clone();
        let effect = self.core.root_owner().run(|| {
            let owner = pinion_core::Owner::current()
                .expect("install_reconcile_effect runs inside root_owner.run wrap");
            Effect::new(&owner, move || {
                // Subscribe by reading the signal value — the Effect
                // re-runs whenever a `Signal::set` actually changes
                // the inner `Vec<WindowSpec>` (R26 equality-skip).
                let _ = signal_for_closure.get();
                // Wake the shell so the user_event handler can call
                // `reconcile_windows` with `&mut self` + the active
                // event loop. Failure means winit already exited;
                // the closure has no recovery path at that point.
                let _ = proxy.send_event(AppEvent::WindowsDirty);
            })
        });
        self.windows_signal = Some(signal);
        self.reconcile_effect = Some(effect);
    }

    /// R683 §5.16 §5.41 — diff the freshly-emitted
    /// `Signal<Vec<WindowSpec>>` snapshot against
    /// [`Self::last_known_specs`] and reconcile.
    ///
    /// **Add pass**: every spec id in the new snapshot that is not
    /// in the last-known cache resumes via the existing
    /// [`Self::resume_spec`] helper (one winit `Window` + GPU
    /// renderer + `accesskit_winit::Adapter` per spec, plus the
    /// per-window `WindowSlot` cluster + `spec_id_to_window_id`
    /// reverse-map entry).
    ///
    /// **Drop pass**: every spec id in the last-known cache that is
    /// not in the new snapshot drops the matching `WindowSlot` (the
    /// `RenderState::Active`'s `Arc<Window>` releases; winit closes
    /// the OS window when the last ref drops). The shell-side
    /// [`crate::ShellCore::remove_window`] call drains every
    /// per-window substrate state map
    /// (`redraw_requested_per_window` / `last_paint_instants` /
    /// `target_fps_per_window` / `fragment_cache_stats_per_window`),
    /// then forwards into the runtime-side
    /// [`pinion_runtime::CoreShell::remove_window`] which drops the
    /// `routers` + `window_owners` entries (the per-window `Owner`
    /// drop fires the cleanup queue for every animation / command /
    /// cache slot registered on that scope).
    ///
    /// **Idempotency**: returns immediately when
    /// `new_specs == old_specs` (`Vec` element-wise `PartialEq`); the
    /// Effect's eager initial run lands here on the very first
    /// install and the snapshot equality short-circuit keeps the
    /// no-op path cheap.
    ///
    /// **Primary protection**: the canonical primary spec
    /// (`WindowSpec::main`, id `"main"`) is the binding's reactive
    /// substrate anchor; the shell-side substrate refuses to remove
    /// it (`ShellCore::remove_window` returns `false` for
    /// `DEFAULT_WINDOW`). A binding that drops `"main"` from its
    /// `windows_signal()` list will see the AppShell-side
    /// `WindowSlot` drop but the substrate state survives —
    /// canonical behaviour for the dock + tear-off arc (the main
    /// dock surface stays alive as the "host" for every torn-off
    /// panel).
    /// R1610 §5.16 §5.41 — apply one live declared axis to the windows whose
    /// value on it changed.
    ///
    /// The apply half of the family whose diff half is [`window_axis_changes`],
    /// and lifted for the same reason at the same moment: the level pass would
    /// have been the THIRD hand-written copy of "look the spec id up, reach the
    /// live window, set the value, trace it". Three copies of a lookup is three
    /// chances for one of them to skip a window shape the others reach.
    ///
    /// The lookup is the subtle part and it is now stated once. The apply
    /// reaches every window with a live arc, INCLUDING a `Suspended(Some)` one
    /// ([`Self::slot_window`] returns its cached arc); only a `Suspended(None)`
    /// slot — no arc at all — is skipped, and that one is rebuilt by
    /// [`Self::resume_spec`] from the CURRENT spec, so nothing is lost.
    ///
    /// `what` is the trace message. winit offers no read-back for any of these
    /// axes, so the trace is what an out-of-process observer can assert the
    /// apply on; it is not a claim that the OS accepted it.
    fn apply_axis_pass<T, F>(&self, changes: Vec<(String, T)>, what: &'static str, apply: F)
    where
        T: std::fmt::Debug,
        F: Fn(&Window, T),
    {
        for (spec_id, value) in changes {
            if let Some(window_id) = self.spec_id_to_window_id.get(spec_id.as_str()).copied()
                && let Some(slot) = self.windows.get(&window_id)
                && let Some(window) = Self::slot_window(slot)
            {
                tracing::debug!(
                    target: "pinion::shell",
                    window = %spec_id,
                    value = ?value,
                    "{what}",
                );
                apply(window, value);
            }
        }
    }

    fn reconcile_windows(&mut self, event_loop: &ActiveEventLoop) {
        let Some(signal) = self.windows_signal.as_ref() else {
            // No opt-in, no reconcile — defensive against a
            // WindowsDirty arriving before install (degenerate corner
            // case).
            return;
        };
        let new_specs = signal.get();
        let old_specs: Vec<WindowSpec> = self.last_known_specs.borrow().clone();
        if new_specs == old_specs {
            // Idempotent fast-path — identical re-emit, no add / drop
            // work to do. The Effect's eager initial run lands here
            // because `install_reconcile_effect` seeds
            // `last_known_specs` from the same snapshot.
            return;
        }
        // Build the id sets once so the difference walks below are
        // O(N) instead of O(N²). `HashSet<&str>` so the lookups are
        // alloc-free (Cow derefs to str through the iterator).
        let new_ids: HashSet<&str> = new_specs.iter().map(|s| s.id.as_ref()).collect();
        let old_ids: HashSet<&str> = old_specs.iter().map(|s| s.id.as_ref()).collect();
        // Drop pass: in old, not in new. Materialise the to-drop
        // list as owned `String`s so the subsequent
        // `&mut self.windows` mutation does not conflict with the
        // `&self.last_known_specs` borrow chain above.
        let to_drop: Vec<String> = old_ids
            .difference(&new_ids)
            .map(|s| (*s).to_owned())
            .collect();
        // R1363 §5.55 — did this pass actually CLOSE a window? The
        // `quit_on_last_window_closed` policy keys off a real removal, never off
        // a merely-empty snapshot: an exit must be a statement of intent, not a
        // side effect of a list length (sprag PR-65's correct objection to
        // "reconcile exits on empty").
        let mut removed_any = false;
        for spec_id in to_drop {
            // Look up + remove the spec_id → WindowId reverse-map
            // entry. The Cow<str> key resolves through `&str` via
            // `Borrow<str>` for an alloc-free lookup.
            if let Some(window_id) = self.spec_id_to_window_id.remove(spec_id.as_str()) {
                // Drop the WindowSlot — the OS window closes when
                // the last `Arc<Window>` ref drops (the slot's
                // RenderState::Active::window + the
                // accesskit_winit::Adapter both held strong refs;
                // dropping the slot releases the AppShell-side ref,
                // and the adapter drops with the slot since it's a
                // field of the slot).
                self.windows.remove(&window_id);
                if self.primary_window_id == Some(window_id) {
                    self.primary_window_id = None;
                }
                // Drain the per-window substrate state.
                // `ShellCore::remove_window` refuses DEFAULT_WINDOW
                // — the primary scope stays alive even if the
                // binding drops `"main"` from the signal (the
                // primary's reactive substrate is the
                // binding-wide anchor and tearing it down would
                // orphan every `Owner::cache` slot on root_owner).
                let _ = self.core.remove_window(&spec_id);
                tracing::debug!(target: "pinion::shell", window = %spec_id, "closed window");
                removed_any = true;
            }
        }
        // Add pass: in new, not in old. `resume_spec` creates one
        // winit Window + GPU renderer + accesskit_winit::Adapter
        // per spec and inserts the matching `WindowSlot` +
        // `spec_id_to_window_id` entry.
        for spec in &new_specs {
            if !old_ids.contains(spec.id.as_ref()) {
                // `make_primary == false` — the primary was assigned
                // during the initial `resumed()` and survives
                // reconcile passes; runtime-added windows are always
                // secondary.
                self.resume_spec(event_loop, spec, None, false);
            }
        }
        // R1087 §5.16 PR-31 — move pass: a spec present in BOTH old and
        // new whose declared position changed drives the live OS window to
        // the new logical-pixel position. `window_placement_moves` is a TOTAL
        // diff (every same-id position change appears; without it the
        // id-keyed add/drop passes would silently swallow it). The apply
        // here is best-effort: a window with no live arc — a
        // `Suspended(Some)` mobile state, `slot_window` → `None` — is skipped
        // and reconciles on its next create (mobile-lifecycle, deferred with
        // mobile). The silent skip on a missing `spec_id_to_window_id` entry
        // mirrors the drop pass's identical pattern (absence ⇒ creation
        // already failed + the loop is exiting). The drag-follow PR-31 builds
        // on top writes the position signal each pointer move; this is where
        // each write lands on the real window. (The live move is HW-gated;
        // the diff is unit-tested in `r1087_window_position_move_diff_tests`.)
        // R1576 — the topology is read ONCE for the whole pass rather than per
        // window: it is a platform round-trip, and two windows reconciled in
        // one pass must not be resolved against two different desks.
        let placement_moves = window_placement_moves(&old_specs, &new_specs);
        let topology = if placement_moves.is_empty() {
            DisplayTopology::empty()
        } else {
            self.display_topology()
        };
        for (spec_id, placement) in placement_moves {
            if let Some(window_id) = self.spec_id_to_window_id.get(spec_id.as_str()).copied() {
                if let Some(slot) = self.windows.get_mut(&window_id) {
                    let scale = slot.scale_factor;
                    // Clone the arc first so the immutable `slot_window`
                    // borrow ends before the `last_commanded_position`
                    // mutation below.
                    if let Some(window) = Self::slot_window(slot).cloned() {
                        let (_anchored, commanded) =
                            Self::apply_placement(&window, scale, &topology, &placement);
                        // R1088 §5.16 PR-31 — latch the commanded position
                        // so the OS `Moved` echo this `set_outer_position`
                        // triggers is recognised + suppressed by
                        // `note_window_moved`, not mistaken for a user drag.
                        slot.last_commanded_position = Some(commanded);
                    }
                }
            }
        }
        // R1319 §5.16 §5.41 PR-52 — title pass: a spec present in BOTH old and new
        // whose declared title changed drives the live OS window's title (alt-tab,
        // taskbar, window list). Structurally the twin of the move pass above — total
        // pure diff (`window_title_changes`, unit-tested) + best-effort apply.
        //
        // Pre-R1319 `title` was create-time-only, so a binding could not rename a live
        // window at all — the terminal-multiplexer convention (the OS title follows the
        // focused pane, which its child renames every prompt) was unreachable, and
        // `WindowSpec::title`'s rustdoc claimed a `set_title` forwarding that only ever
        // happened once.
        //
        // The apply reaches every window with a live arc, INCLUDING a `Suspended(Some)`
        // one (`slot_window` returns its cached arc — R1320 correction: the R1087 move
        // pass's comment claims such a window is skipped and re-applied at create; it is
        // not skipped, and `resume_spec`'s cached-arc branch does NOT re-apply, which is
        // why that branch now re-applies the title explicitly, mirroring R1088's
        // position re-apply). Only a `Suspended(None)` slot — no arc at all — is skipped;
        // it is rebuilt by `resume_spec`, whose `with_title` reads the CURRENT spec, so
        // the title cannot be lost.
        // winit exposes no OS title read-back (its X11 `Window::title()` is a stub
        // returning ""), so the trace each apply emits is what an out-of-process
        // observer (the `hello-dock-panels-editor` demo) asserts on. It is NOT a
        // claim that the OS accepted it — that is winit's contract.
        self.apply_axis_pass(
            window_title_changes(&old_specs, &new_specs),
            "window title updated",
            Window::set_title,
        );
        // R1320 §5.16 §5.41 — decorations pass. R1118 made a same-id `decorations` flip
        // a WARN ("create-time-only; recreate the window to change chrome") justified by
        // "no `Window::set_decorations` call exists". That justification was FALSE:
        // winit 0.30 has `Window::set_decorations` (`window.rs:1160`), implemented on
        // X11 / Wayland / macOS / Windows (a no-op only on iOS / Android / Web). A warn
        // that tells a consumer to destroy and recreate a window, on the strength of an
        // invented platform limit, is worse than no warn — so it becomes an apply, the
        // same shape as the title + position passes.
        self.apply_axis_pass(
            window_decoration_changes(&old_specs, &new_specs),
            "window decorations updated",
            Window::set_decorations,
        );
        // R1610 §5.16 §5.41 — level pass, the third live declared axis. A window
        // level is a thing the USER toggles, so the create-time-only shape the
        // `decorations` axis wore until R1320 would not have been the feature.
        // `Window::set_window_level` applies to a live window on every desktop
        // backend — where a flags-word encoding costs a hide, and on one backend a
        // destroy-and-recreate of the native window. See `window_level_changes`.
        self.apply_axis_pass(
            window_level_changes(&old_specs, &new_specs),
            "window level updated",
            |window, level| window.set_window_level(winit_window_level(level)),
        );
        // Update the cache so the next `reconcile_windows` call
        // diffs against the snapshot the shell just acted on.
        let now_empty = new_specs.is_empty();
        *self.last_known_specs.borrow_mut() = new_specs;
        // R1363 §5.55 — the ONE bridge from the window lifecycle to the app
        // lifecycle: this pass CLOSED a window and none is left. Gated on a real
        // removal (`removed_any`), never on a merely-empty snapshot, so a
        // binding rebuilding its window list cannot be mistaken for a user
        // quitting. Even then this only REQUESTS a quit —
        // `app_quit_requested` may refuse.
        //
        // This is what retires the R1362 zombie: pre-R1363 dropping every spec
        // closed every OS window and parked the loop forever with no window, no
        // `CloseRequested` source and no Escape target, because "the window set
        // is empty" meant nothing to anyone.
        if removed_any && now_empty && self.core.root_owner().run(V::quit_on_last_window_closed) {
            tracing::debug!(
                target: "pinion::shell",
                "last window closed; quit_on_last_window_closed policy raises a quit",
            );
            self.request_quit(event_loop);
            return;
        }
        // Re-request paint on every active window so the next event
        // loop iteration renders the new topology. drain dispatches
        // a Window::request_redraw per active slot.
        self.core.request_redraw();
        self.drain_redraw_to_winit();
    }

    #[allow(
        clippy::too_many_lines,
        reason = "cohesive single-window creation routine — winit Window + GPU renderer + AccessKit adapter + slot assembly are one transactional unit; the R1088 Suspended-resume position re-apply belongs inline beside the create-time with_position it mirrors"
    )]
    fn resume_spec(
        &mut self,
        event_loop: &ActiveEventLoop,
        spec: &WindowSpec,
        cached_window_id: Option<WindowId>,
        make_primary: bool,
    ) {
        // Pull the cached window arc if any (suspend-resume reuse).
        let cached_window = cached_window_id
            .and_then(|id| self.windows.remove(&id))
            .and_then(|slot| match slot.render {
                RenderState::Active { window, .. } | RenderState::Suspended(Some(window)) => {
                    Some(window)
                }
                RenderState::Suspended(None) => None,
            });
        // R668 §5.16 — read the spec's window-creation policy.
        // `Fixed` opens at exactly `(width, height)`;
        // `IntrinsicAfterFirstPaint` opens at `min` and queues a
        // post-first-paint resize request via
        // `pending_intrinsic_resize`. Either way the renderer
        // initialises against `window.inner_size()`.
        let strategy = spec.strategy;
        let (init_w, init_h) = strategy.initial_logical_size();
        let window = if let Some(w) = cached_window {
            // R1088 §5.16 §5.41 PR-31 — re-apply the declared position on a
            // `Suspended(Some)` resume. `with_position` only applies at
            // CREATE (the `else` branch); a cached window reused across a
            // suspend/resume cycle would otherwise keep its pre-suspend OS
            // position, drifting the live window from the declared
            // `windows_signal` (R1087.1 finding ②). This closes the
            // move-pass apply gap for the mobile-lifecycle resume path. The
            // matching `last_commanded_position` latch is stamped on the
            // rebuilt slot below (suppresses the resulting `Moved` echo).
            if let Some(placement) = spec.placement() {
                // R1576 — a cached window already knows its own scale; a fresh
                // one does not, hence the two branches. The commanded position
                // is discarded here because the slot below is rebuilt with its
                // own latch (unchanged from R1088).
                let scale = w.scale_factor();
                let topology = topology_from(w.available_monitors(), w.primary_monitor().as_ref());
                let _ = Self::apply_placement(&w, scale, &topology, &placement);
            }
            // R1320 §5.16 §5.41 PR-52 — re-apply the declared TITLE + DECORATIONS on a
            // `Suspended(Some)` resume, mirroring the position re-apply above (R1088).
            // `with_title` / `with_decorations` only run on the CREATE branch below, so a
            // cached window reused across a suspend/resume cycle would otherwise keep its
            // pre-suspend chrome while the binding's spec says otherwise — the same
            // drift R1088 closed for position. (The live-window passes in
            // `reconcile_windows` cover a window that never suspended.)
            w.set_title(&spec.title);
            w.set_decorations(spec.decorations);
            // R1610 — and the declared LEVEL, for the same reason: a cached window
            // reused across a suspend/resume cycle would otherwise keep whatever
            // stacking it had before while the binding's spec says otherwise.
            w.set_window_level(winit_window_level(spec.level));
            w
        } else {
            let mut attrs = Window::default_attributes()
                .with_title(spec.title.clone())
                .with_inner_size(LogicalSize::new(f64::from(init_w), f64::from(init_h)));
            // R1087 §5.16 PR-31 — honour the declared logical-pixel outer
            // position when the binding pins one (the floating dock-panel
            // tear-off opens its window under the cursor at detach).
            // `None` — every pre-R1087 spec — leaves placement to the
            // window manager exactly as before (byte-identical). winit
            // applies the per-monitor DPI scale to the logical coords.
            // R1576 §5.16 §5.41 — a spec naming a display re-reads that same
            // pair as an offset INTO it, resolved against the monitors
            // attached now, and the window is created at the resulting
            // absolute PHYSICAL point. A spec naming no display keeps the
            // logical absolute reading byte-identical.
            match spec.placement() {
                Some(placement) if placement.display.is_some() => {
                    let topology = topology_from(
                        event_loop.available_monitors(),
                        event_loop.primary_monitor().as_ref(),
                    );
                    if let Some((x, y)) = placement.resolve(&topology).at() {
                        attrs = attrs.with_position(PhysicalPosition::new(x, y));
                    }
                }
                Some(placement) => {
                    attrs = attrs.with_position(LogicalPosition::new(
                        f64::from(placement.offset.0),
                        f64::from(placement.offset.1),
                    ));
                }
                None => {}
            }
            // R1115 §5.16 §5.51 PR-38 — honour the declared OS chrome. A
            // torn-off dock panel declares `decorations: false` so the OS
            // draws no title bar/border and pinion owns the panel's chrome
            // (its own header + drag-grip). winit's default is `true`, so
            // setting it to the spec value (`true` for every pre-R1115
            // binding) is byte-identical. Create-time only — like
            // `strategy`, not re-applied on a same-id spec change (a
            // dock-back destroys the floating window, never re-decorates).
            attrs = attrs.with_decorations(spec.decorations);
            // R1610 §5.16 §5.41 — honour the declared window LEVEL at create.
            // `WindowLevel::Normal` (every pre-R1610 binding) is winit's own
            // default, so this is byte-identical for them. Unlike
            // `decorations` this is also re-applied on a same-id change (the
            // level pass in `reconcile_windows`) — see `window_level_changes`.
            attrs = attrs.with_window_level(winit_window_level(spec.level));
            // R835 §5.16 — windowless test mode. `PINION_HIDDEN_WINDOW`
            // creates the shell window UNMAPPED (`visible = false`): Vello
            // still renders to the GPU surface and `scene/snapshot` /
            // `scene/query` work unchanged, but no window flashes on the
            // developer's real display. Headless RPC demos set this so a
            // local verification run does not seize focus / flicker.
            // Unset (the default) keeps the window visible for interactive
            // `run` / `verify` sessions.
            if hidden_window_requested() {
                attrs = attrs.with_visible(false);
            }
            // R668 §5.16 / R1059 — anchor the user-driven OS-resize
            // floor via the single-source-of-truth policy on
            // `SizeStrategy`. `Fixed`/`IntrinsicAfterFirstPaint` pin
            // the floor at their open size / `min`; `OpenResizable`
            // forwards its independent `min`, and `None` skips
            // `with_min_inner_size` entirely so the window stays at the
            // OS-native minimum (~100×30 desktop) — the freely
            // shrinkable case.
            if let Some(min_floor) = strategy.min_inner_floor() {
                attrs = attrs.with_min_inner_size(LogicalSize::new(
                    f64::from(min_floor.0),
                    f64::from(min_floor.1),
                ));
            }
            match event_loop.create_window(attrs) {
                Ok(w) => Arc::new(w),
                Err(e) => {
                    tracing::error!(target: "pinion::shell", window = %spec.id, error = %e, "window create failed");
                    event_loop.exit();
                    return;
                }
            }
        };
        // R889 §5.16 §5.49 — register the window in the substrate's
        // window-known registry the moment the OS window exists
        // (before renderer init + before the first paint). From here
        // on `CoreShell::is_window_known(spec.id)` is true, so the
        // dispatch-entry unknown-window gate admits RPC scoped to this
        // window and the per-window READ axes (`scene/input_state` /
        // `scene/pacing_state`) answer honestly for the
        // registered-but-unpainted phase (R683 tear-off arc). One
        // creation edge for both `resumed` and `reconcile_windows`
        // paths; the matching removal edge is the reconcile drop
        // pass's `ShellCore::remove_window`.
        self.core.register_window(spec.id.as_ref());
        let pending_intrinsic_resize = match strategy {
            // R1059 — `OpenResizable` opens at `size` with no
            // post-first-paint resize, exactly like `Fixed`; only
            // `IntrinsicAfterFirstPaint` queues the measure-and-resize.
            SizeStrategy::Fixed { .. } | SizeStrategy::OpenResizable { .. } => None,
            SizeStrategy::IntrinsicAfterFirstPaint { min, max } => Some((min, max)),
        };
        // R56.2.a §5.13 §5.38 — opt into the winit IME bridge per
        // window so `WindowEvent::Ime` events flow into
        // `Self::window_event` and reach `ShellCore::apply_composition`
        // → `WidgetCore::apply_composition`. Per-window because IME
        // sessions are window-scoped on every platform.
        window.set_ime_allowed(true);
        // R57.1 §5.50 — push the initial OS `prefers-color-scheme`
        // reading into the global signal (only needed on the primary
        // window; the signal is process-global). Secondary window
        // re-pushes are idempotent so the gate is omitted for
        // simplicity.
        if let Some(theme) = window.theme() {
            pinion_core::set_system_color_scheme(winit_theme_to_pinion_scheme(theme));
        }
        // R1027 §5.16 — seed the per-slot scale factor from the OS so the
        // first paint already lays out logical and rasters at the device
        // resolution (refreshed later by `WindowEvent::ScaleFactorChanged`).
        // The renderer surface is sized in physical pixels (unchanged).
        let scale_factor = window.scale_factor();
        // R1147 §5.16 — renderer init via the shared `build_renderer` helper
        // (also used by the drag-preview window), so both window paths cross the
        // §6.3 `pollster::block_on` boundary the same way.
        let renderer = match Self::build_renderer(&window) {
            Ok(r) => *r,
            Err(e) => {
                tracing::error!(target: "pinion::shell", window = %spec.id, error = %e, "renderer init failed");
                // Cache the window for a subsequent retry; renderer
                // init failed but the OS window survives.
                let window_id = window.id();
                // R683 §5.16 — `spec.id` is `Cow<'static, str>` so
                // `.clone()` produces a fresh owned handle for the
                // per-slot copy + the spec_id_to_window_id map key.
                // `Cow::Borrowed` clones are pointer-cheap; runtime
                // ids (`Cow::Owned`) pay one `String::clone`.
                let image_store = image_cache::resolve_image_store(self.core.root_owner());
                self.windows.insert(
                    window_id,
                    WindowSlot::build(
                        RenderState::Suspended(Some(window)),
                        None,
                        spec.id.clone(),
                        pending_intrinsic_resize,
                        scale_factor,
                        image_store,
                    ),
                );
                self.spec_id_to_window_id.insert(spec.id.clone(), window_id);
                event_loop.exit();
                return;
            }
        };
        // R51.62 §5.40 — construct the per-window accesskit_winit
        // Adapter. AccessKit canonical: 1 adapter = 1 window. The
        // proxy is cloned because Adapter consumes one internally
        // for each of its three handler hooks (activation / action /
        // deactivation).
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        );
        let window_id = window.id();
        let image_store = image_cache::resolve_image_store(self.core.root_owner());
        let mut slot = WindowSlot::build(
            RenderState::Active {
                window,
                renderer: Box::new(renderer),
            },
            Some(adapter),
            spec.id.clone(),
            pending_intrinsic_resize,
            scale_factor,
            image_store,
        );
        // R1088 §5.16 §5.41 §2 #7 PR-31 — latch the declared position the
        // create (`with_position`) or `Suspended`-resume re-apply just
        // commanded, so the window's first `WindowEvent::Moved` (the OS
        // echo of that placement) is suppressed by `note_window_moved`
        // rather than written back as if it were a user drag. `None` for a
        // WM-placed window leaves the latch empty.
        slot.last_commanded_position = spec.position;
        self.windows.insert(window_id, slot);
        self.spec_id_to_window_id.insert(spec.id.clone(), window_id);
        if make_primary {
            self.primary_window_id = Some(window_id);
        }
        tracing::debug!(
            target: "pinion::shell",
            window = %spec.id,
            title = %spec.title,
            init_w,
            init_h,
            "window resumed",
        );
    }

    /// R670.B / R1023 / R1123 §5.16 §5.39 — per-window resize. Forwards the
    /// surface resize to the live GPU renderer, repaints THIS window (winit
    /// does not guarantee a `RedrawRequested` after a `Resized`, so without
    /// this the reconfigured swapchain presents a stale backbuffer until some
    /// unrelated redraw arrives — the live drag-resize ghosting on full-bleed
    /// content), and syncs the per-window maximized cache from winit's actual
    /// `Window::is_maximized()`. The cache feeds the client-side chrome glyph
    /// (maximize vs restore) and the resize-border suppression on both the live
    /// paint and the pure mirror, so `scene/snapshot` matches the painted glyph
    /// (§2 #7). Reading winit's report (not just the chrome-button intent)
    /// keeps the cache correct when a tiling WM maximizes the window.
    fn note_window_resized(&mut self, window_id: WindowId, size: PhysicalSize<u32>) {
        let spec_id = self
            .windows
            .get(&window_id)
            .map_or(pinion_runtime::DEFAULT_WINDOW, |s| &*s.spec_id)
            .to_owned();
        let mut maximized = None;
        let mut resized = false;
        if let Some(slot) = self.windows.get_mut(&window_id)
            && let RenderState::Active {
                renderer, window, ..
            } = &mut slot.render
        {
            renderer.resize(size.width.max(1), size.height.max(1));
            maximized = Some(window.is_maximized());
            resized = true;
        }
        if let Some(m) = maximized {
            self.core.set_maximized_for_window(&spec_id, m);
        }
        // R1219 §5.16 §5.41 — paint the resized surface SYNCHRONOUSLY, in-band
        // with the `Resized` event, instead of scheduling an async
        // `request_redraw`. During an interactive OS resize the platform runs a
        // modal resize loop that can withhold `RedrawRequested` until the drag
        // ends; an async redraw then leaves the newly-exposed region unpainted
        // for the whole drag — a flash at the grow edge (the bottom when
        // growing vertically). An immediate paint fills the new size before the
        // compositor composites the resized frame, so the surface never shows a
        // stale/uncleared band. `render_window` early-returns on a 0-size
        // (minimize) window, so the un-guarded call is safe.
        if resized {
            self.render_window(window_id);
        }
    }

    /// R1027 §5.16 — the window moved to a display with a different DPI,
    /// or the OS scale changed. Refresh the cached factor so the next paint
    /// lays out logical -> rasters at the new device resolution and pointer
    /// events convert against the new scale. winit pairs this event with a
    /// `Resized` (the new physical inner size) that reconfigures the GPU
    /// surface; the explicit redraw here makes the rescaled frame appear
    /// immediately rather than waiting on an unrelated redraw.
    /// `inner_size_writer` is left untouched — pinion accepts winit's
    /// recommended physical size.
    fn note_scale_factor_changed(&mut self, window_id: WindowId, scale_factor: f64) {
        if let Some(slot) = self.windows.get_mut(&window_id) {
            slot.scale_factor = scale_factor;
            if let RenderState::Active { window, .. } = &slot.render {
                window.request_redraw();
            }
        }
    }

    /// R1088 §5.16 §5.41 §2 #7 PR-31 — feed a winit `WindowEvent::Moved`
    /// back into `windows_signal` so the DECLARED position converges on the
    /// actual one (the architecture-A ideal: a user native title-bar drag
    /// writes the position SSOT, and `scene/windows` then reads the live
    /// placement, declared == actual).
    ///
    /// **Echo suppression.** The reconcile move pass and the
    /// `Suspended`-resume re-apply record every position they command in
    /// [`WindowSlot::last_commanded_position`]. A `Moved` equal to that
    /// latch is the shell's OWN `set_outer_position` echoing back — it is
    /// consumed (the latch clears) and NOT written, so a shell- or
    /// RPC-driven move does not loop (`command -> Moved -> signal write ->
    /// reconcile -> command -> ...`). A `Moved` that DIVERGES is a user /
    /// WM drag and is written back.
    ///
    /// **Conservative scope.** Only a window that ALREADY declares a
    /// position (the floating tear-off panels) is updated; a `None`
    /// WM-placed window (the typical `"main"`) is left WM-managed — one
    /// user drag must not silently convert it into a pinned window.
    ///
    /// The write syncs [`Self::last_known_specs`] to the same snapshot
    /// before emitting, so the signal write fires the reconcile effect but
    /// hits its `new == old` fast path: the OS window is NOT re-commanded
    /// back to where the user just put it.
    fn note_window_moved(&mut self, window_id: WindowId, position: PhysicalPosition<i32>) {
        let Some(slot) = self.windows.get(&window_id) else {
            return;
        };
        let scale = slot.scale_factor;
        let spec_id = slot.spec_id.clone();
        let commanded = slot.last_commanded_position;
        // winit reports the outer position in PHYSICAL px; the signal +
        // `scene/windows` are LOGICAL px (§5.21). `to_logical::<i32>`
        // divides by the per-window scale and rounds (winit's i32 `Pixel`),
        // matching the `set_outer_position(LogicalPosition)` the move pass
        // issues (winit multiplies logical -> physical on the way out).
        let logical: LogicalPosition<i32> = position.to_logical(scale);
        let logical = (logical.x, logical.y);
        // Echo of our own command? Consume the latch (clear it) without
        // writing. `moved_is_command_echo` is the pure, unit-tested core.
        if moved_is_command_echo(commanded, logical) {
            if let Some(slot) = self.windows.get_mut(&window_id) {
                slot.last_commanded_position = None;
            }
            return;
        }
        // A user / WM move. Compute the write-back via the pure, unit-tested
        // `user_move_writeback` (the conservative-scope filter + idempotency
        // skip + spec lookup live there); `None` = nothing to write.
        let Some(signal) = self.windows_signal.as_ref() else {
            return;
        };
        // R1576 — the topology is read only when a spec actually measures its
        // position from a display; `user_move_writeback` ignores it otherwise,
        // so an absolute-placement binding (every pre-R1576 one) pays nothing
        // per drag event.
        let needs_topology = signal
            .get()
            .iter()
            .any(|s| s.id.as_ref() == spec_id.as_ref() && s.display.is_some());
        let topology = if needs_topology {
            self.display_topology()
        } else {
            DisplayTopology::empty()
        };
        let Some(new_specs) = user_move_writeback(
            signal.get(),
            spec_id.as_ref(),
            logical,
            (position.x, position.y),
            &topology,
        ) else {
            return;
        };
        // Sync the reconcile cache so the signal write does NOT re-command the OS
        // window to where the user just dragged it (the move pass would otherwise emit
        // a redundant `set_outer_position`).
        //
        // R1320 §5.16 §5.41 — patch ONLY THIS WINDOW'S POSITION, not the whole cached
        // vector. Pre-R1320 this overwrote the cache with the signal's CURRENT value,
        // which silently ACKNOWLEDGED every other pending change in it: a title (R1319)
        // or decorations write that `reconcile_windows` had not yet drained was
        // swallowed — the next reconcile saw `new == old`, took the fast path, and the
        // OS window kept the stale title FOREVER (until the next rename). The forcing
        // consumer makes that reachable: a terminal renames its panes on every prompt
        // while their floating windows are being dragged. The echo suppression only
        // ever needed the position, so only the position is acknowledged.
        cache_moved_position(
            &mut self.last_known_specs.borrow_mut(),
            spec_id.as_ref(),
            logical,
        );
        signal.set(new_specs);
    }
}

impl<V: WidgetView> ApplicationHandler<AppEvent> for AppShell<V> {
    /// R1658 §5.13 §5.39 — open a keystroke **delivery** for this event-loop
    /// iteration.
    ///
    /// winit calls this once at the top of every iteration, *before* it
    /// dispatches any of that iteration's events, which makes it the exact
    /// boundary the capability needs: every key winit hands over without an
    /// intervening wait shares one arrival, and the instant is stamped before
    /// the first binding handler of the iteration runs.
    ///
    /// That ordering is the whole point. Stamping inside the keyboard arm
    /// instead would date the second key of a burst at *after the first key's
    /// handler returned* — and a handler that blocks (an embedder doing a
    /// round trip per keystroke) is precisely the case where a repeat window
    /// judged on that clock silently collapses.
    ///
    /// `cause` is deliberately unread: a poll wake, a timer wake and a
    /// platform wake are all the start of a new delivery, and discriminating
    /// them would make the batch mean something different on each platform.
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: winit::event::StartCause) {
        self.core.open_key_delivery();
    }

    /// R46.3.4 — winit may fire `resumed` more than once on platforms
    /// that suspend (Android, Wayland-compositor focus changes). The
    /// Vello canonical pattern caches the previous `Window` across
    /// the drop-and-recreate cycle so the OS-side handle survives,
    /// while the GPU renderer is freshly constructed each time.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // R670.B §5.16 — single-window legacy behaviour was "skip if
        // already Active". Multi-window equivalent: skip if every
        // declared spec has already been created. We resume per-spec
        // (suspended slots keep their cached `Window` Arc) so the
        // mobile suspend-resume lifecycle still works per slot.
        //
        // R683 §5.16 §5.41 — read the binding's optional
        // [`WidgetView::windows_signal`] first. When `Some(signal)`,
        // the signal's snapshot replaces `V::windows()` as the
        // initial spec list AND the reconcile Effect will subscribe
        // to subsequent mutations (dock tear-off / dock-back). When
        // `None` (the pre-R683 default for every single + multi-
        // window binding) the compile-time `V::windows()` list is
        // the source of truth + window topology is frozen for the
        // binding's lifetime — bit-identical to the pre-R683
        // contract.
        //
        // The trait call is wrapped in `root_owner.run(..)` so the
        // binding impl can reach `Owner::current()` + use
        // `Owner::cache` to memoise the returned signal across
        // shell-side re-entries (suspend / resume cycles read the
        // same memoised `Rc<Signal<..>>` so the Effect install gate
        // below sees the same handle).
        // R1576 §5.16 §5.41 — publish the desk BEFORE the first window is
        // created, so a binding that names a display in its own `WindowSpec`
        // factory (`use_displays()` inside `windows_signal`) has one to read.
        // The `ActiveEventLoop` is the enumeration source here because there is
        // no window yet to ask; every later refresh goes through
        // `stamp_desktop_facts`.
        // R1617 — no windows exist yet, so there are no homes to stamp. The
        // empty list is the honest value: `display_home` reads `null` until a
        // window is there to have one, which is the same
        // nobody-looked-so-nothing-is-claimed rule the rest of this axis uses.
        self.publish_display_topology(
            topology_from(
                event_loop.available_monitors(),
                event_loop.primary_monitor().as_ref(),
            ),
            Vec::new(),
        );
        // R1610 §5.16 §2 #7 — and which windowing system that desk belongs to,
        // for the level outcome `scene/windows` reports. Same reason it goes
        // first: a binding declaring `level: AlwaysOnTop` in its own factory
        // should be able to learn straight away whether that will be honoured.
        self.stamp_windowing_backend(event_loop);
        let opt_signal = self.core.root_owner().run(V::windows_signal);
        let specs: Vec<WindowSpec> = match opt_signal.as_ref() {
            Some(signal) => signal.get(),
            None => V::windows(),
        };
        if specs.is_empty() {
            tracing::warn!(target: "pinion::shell", "V::windows() returned empty list; nothing to create");
            event_loop.exit();
            return;
        }
        let mut primary_assigned = false;
        for spec in &specs {
            // Resolve any cached window from a prior suspend cycle
            // (look up by spec id). The first `resumed()` after boot
            // never has cached entries; subsequent post-suspended
            // resumes can re-attach the cached `Window` to a fresh
            // GPU renderer.
            //
            // R683 §5.16 — `spec.id` is `Cow<'static, str>`; the
            // HashMap `.get(K: Borrow<Q>)` resolves through Cow's
            // `Borrow<str>` impl, so the `.get(&*spec.id)` form
            // passes a plain `&str` and avoids cloning the Cow.
            let cached_window_id = self.spec_id_to_window_id.get(&*spec.id).copied();
            // Skip specs that already have an Active slot. A spec is
            // either fully Active or fully Suspended (no mid-state).
            if let Some(window_id) = cached_window_id
                && let Some(slot) = self.windows.get(&window_id)
                && matches!(slot.render, RenderState::Active { .. })
            {
                if !primary_assigned {
                    self.primary_window_id = Some(window_id);
                    primary_assigned = true;
                }
                continue;
            }
            self.resume_spec(event_loop, spec, cached_window_id, !primary_assigned);
            if !primary_assigned && self.spec_id_to_window_id.contains_key(&*spec.id) {
                primary_assigned = true;
            }
        }
        // R1617 §5.16 §5.41 — the windows exist NOW, so stamp where each of
        // them is before the first paint reads it. The desk was published
        // above, before any window existed, with an empty home list — which
        // was the honest value then and would be a permanent one in a
        // GUI-only session if nothing re-stamped it here.
        self.stamp_window_homes();
        // R47.7.5 — winit does not auto-emit `RedrawRequested` on
        // `resumed` (platform-dependent). Explicitly request the
        // first redraw so every active window's first paint commits
        // before the first AI client `scene/layout {viewport: null}`
        // lands. drain_redraw_to_winit walks all windows.
        self.core.request_redraw();
        self.drain_redraw_to_winit();
        // R683 §5.16 §5.41 — install the reconcile Effect after the
        // initial spec resume completes. Gated by
        // `self.reconcile_effect.is_none()` so subsequent
        // `resumed()` calls (mobile suspend / resume cycles) do not
        // install a second Effect on top of the existing
        // subscription.
        if let Some(signal) = opt_signal
            && self.reconcile_effect.is_none()
        {
            // Seed `last_known_specs` to the same snapshot the
            // Effect's eager initial run will observe — the diff
            // short-circuits to a no-op on the first
            // [`AppEvent::WindowsDirty`] arrival.
            *self.last_known_specs.borrow_mut() = specs;
            self.install_reconcile_effect(signal);
        }
    }

    /// R46.3.4 — release the GPU-side renderer on suspend so the OS
    /// can reclaim the wgpu surface. The winit window itself is
    /// cached for the next `resumed` so its handle / OS state
    /// survives.
    ///
    /// R670.B §5.16 — per-window suspend. Walks every `WindowSlot` +
    /// drops its GPU renderer + keeps the `Window` Arc cached. The
    /// `accesskit_winit::Adapter` stays attached so AT-side state
    /// survives the suspend (the adapter only forwards events; the
    /// GPU surface is what mobile platforms reclaim).
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        for slot in self.windows.values_mut() {
            if let RenderState::Active { window, .. } =
                core::mem::replace(&mut slot.render, RenderState::Suspended(None))
            {
                slot.render = RenderState::Suspended(Some(window));
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // R670.B §5.16 — route the AccessKit relay to the matching
        // window's adapter. AccessKit canonical: 1 adapter = 1
        // window; forwarding to the wrong adapter would leak winit
        // event coordinates between windows.
        self.forward_to_accesskit(window_id, &event);
        // R672 §5.35 §5.41 — resolve winit WindowId to canonical
        // [`crate::WindowSpec::id`] before dispatching pointer events
        // so each window's [`pinion_runtime::InputRouter`] handles
        // its own cursor + hover state. Slot lookup is O(1) on the
        // HashMap; spec_id falls back to
        // [`pinion_runtime::DEFAULT_WINDOW`] when the WindowId is not
        // tracked (a Resumed event that has not landed yet — winit
        // can emit some events between AppShell::resumed creating the
        // window and the slot being inserted into the map).
        // R683 §5.16 — `s.spec_id` is `Cow<'static, str>`; `&*s.spec_id`
        // re-borrows as `&str` so the downstream substrate signatures
        // (which take `&str`) stay unchanged. The `&'static str`
        // fallback (`DEFAULT_WINDOW`) coerces to `&str` trivially.
        let spec_id: &str = self
            .windows
            .get(&window_id)
            .map_or(pinion_runtime::DEFAULT_WINDOW, |s| &*s.spec_id);
        // R1027 §5.16 §5.35 — the addressed window's scale factor, copied
        // out (so it does not extend the `self.windows` borrow into the
        // arms). Used to map physical pointer coordinates -> logical for
        // `CursorMoved` / `Touch`. `1.0` for an untracked window (a
        // Resumed event landing before the slot is inserted), which is the
        // pre-R1027 behaviour.
        let scale = self.windows.get(&window_id).map_or(1.0, |s| s.scale_factor);
        // R1434 §5.35 §5.15 — the native trackpad gestures (the toolkit native
        // gesture event: pinch / rotation / pan) dispatch ahead of the main
        // match through one sub-dispatcher, so this function stays under the
        // line cap as the gesture set grows — the same shape the TUI drain's
        // `try_drain_native_gesture` takes (§2 #6), and an extract rather than an `#[allow(too_many_lines)]`. `&event` ends its
        // borrow before the match takes ownership below.
        if try_forward_native_gesture(&mut self.core, spec_id, &event, scale) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                // R1170 §5.16 — per-window close seam: offer the close to the
                // binding first (a torn-off dock panel docks BACK by dropping its
                // WindowSpec — the `windows_signal` → `reconcile_windows` pass then
                // removes this OS window). Only an UNHANDLED close (a single-window
                // app, or a multi-window binding's PRIMARY window) exits the app.
                //
                // R1362 PR-65 — through the ONE arm every other producer reaches
                // (`Self::apply_window_control`). Pre-R1362 this arm hand-copied
                // that Close body, leaving the OS X as the single producer that
                // could silently drift from the chrome press / RPC click drain /
                // the binding's own `WindowControlSink` — the exact split R1190
                // removed from the tag vocabulary, still present in the
                // execution. `spec_id` is copied out because the arm needs
                // `&mut self` while `spec_id` borrows `self.windows`: one
                // allocation, once, on a close.
                let spec_id = spec_id.to_owned();
                self.apply_window_control(
                    &spec_id,
                    pinion_overlay::WindowControl::Close,
                    ControlProducer::OsCloseRequested,
                    event_loop,
                );
            }
            // R48 / R51.80 §5.35: all pointer routing flows through
            // the framework `InputRouter` via [`ShellCore`] wrapper
            // methods. The handler arms only translate winit events
            // into the substrate's pinion-native shape.
            WindowEvent::CursorMoved { position, .. } => {
                // R1148/R1147 §5.51 — body lives in `handle_cursor_moved` (stamp
                // live window origins for cross-window redock + forward the
                // logical cursor + drive the cross-desktop drag preview).
                // Delegating keeps `window_event` under the line cap.
                self.handle_cursor_moved(window_id, position, scale, event_loop);
            }
            WindowEvent::CursorLeft { .. } => {
                self.core.cursor_left_for_window(spec_id, PointerId::MOUSE);
                // R1189 §5.16 §5.39 — reset the resize cursor on leave. winit
                // stores the cursor per-window, so a pointer that leaves over a
                // resize edge would otherwise strand the resize icon on the
                // window's OS attribute (and the latch), showing it again on
                // re-entry before the first `CursorMoved` corrects it. Commanding
                // `None` resets both the latch and the OS attribute to the default
                // arrow (a no-op via the latch when the pointer left over content).
                self.command_resize_cursor(window_id, None);
            }
            // R770 §5.15 — OS file drag-drop (winit normalises the
            // platform file-DnD into three window-scoped events). One
            // delegating arm keeps `window_event` under the line cap; the
            // body lives in `handle_file_dnd`.
            ev @ (WindowEvent::HoveredFile(_)
            | WindowEvent::HoveredFileCancelled
            | WindowEvent::DroppedFile(_)) => {
                // `spec_id` borrows `self.windows`; the `&mut self`
                // helper needs an owned id (file events are rare).
                let sid = spec_id.to_owned();
                self.handle_file_dnd(&sid, &ev);
            }
            // R56.2.e / R772 §5.13 §5.22 §5.53 — Left / Middle / Right
            // mouse-button presses + the Left release. Extracted to
            // `handle_mouse_button` to keep this dispatcher under the
            // workspace `clippy::too_many_lines` (100) ceiling (the
            // app.rs split convention). `spec_id` borrows `self.windows`,
            // so the `&mut self` helper needs an owned id (the file-event
            // arc above does the same `to_owned()`).
            WindowEvent::MouseInput { state, button, .. } => {
                let sid = spec_id.to_owned();
                self.handle_mouse_button(&sid, button, state, event_loop);
            }
            // (R51.186 §5.45 R55.C.2) winit `MouseWheel` events do
            // not carry a position field — winit follows the same
            // W3C / iOS / Android contract pinion's router reads:
            // the wheel applies to the surface under the last
            // cursor position. The substrate's `InputRouter`
            // remembers that position, so this arm only needs to
            // convert the unit-tagged `MouseScrollDelta` into the
            // matching pinion-native [`WheelDelta`] and forward.
            // ★ R1703 — the `phase` field is no longer discarded. winit reports
            // where a wheel event sits in a continuous gesture (a trackpad's
            // two-finger scroll starts, moves and ends; a notched mouse wheel
            // only ever moves), and a stepped consumer needs the end: without
            // it the sub-notch remainder a flick banks is spent by the NEXT
            // gesture, possibly minutes later and aimed at something else.
            WindowEvent::MouseWheel { delta, phase, .. } => {
                // R1027 §5.16 §5.45 — `scale` converts a `PixelDelta`
                // (physical) to logical, mirroring the `CursorMoved` arm.
                let pinion_delta = winit_wheel_to_pinion(delta, scale);
                self.core.wheel_phase_for_window(
                    spec_id,
                    PointerId::MOUSE,
                    pinion_delta,
                    winit_gesture_phase_to_pinion(phase),
                );
            }
            // R51.45 §5.35 — winit `WindowEvent::Touch` closes the
            // R51.38 multi-pointer first-design substrate arc.
            // R51.108 §5.41 — convert at the winit boundary so the
            // substrate sees only the abstract `pinion_runtime::Touch`.
            WindowEvent::Touch(touch) => {
                // R1027 §5.16 §5.35 — pass `scale` so the touch's physical
                // location maps to logical (mirrors `CursorMoved`).
                self.core
                    .touch_event_for_window(spec_id, winit_touch_to_pinion(touch, scale));
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                // R51.53 §5.39 — winit emits `KeyEvent` without
                // modifier state, so cache the most-recent value
                // out-of-band for Shift+Tab detection.
                // R51.108 §5.41 — convert at the winit boundary.
                self.core
                    .set_modifiers(winit_modifiers_to_pinion(modifiers.state()));
            }
            WindowEvent::Focused(focused) => {
                // R1071 PR-27 §5.39 §5.16 §5.35 — track which window holds the
                // OS keyboard focus for the key-dispatch gate (separate from
                // the FocusManager save/restore below: that owns the focused-
                // widget snapshot, this owns the OS-focus identity keyboard
                // routing keys on).
                self.core.note_os_focus(spec_id, focused);
                // R51.59 §5.39 — Window blur / refocus. ARIA Focus
                // Order asks the framework to reinstate the focused
                // widget when the user returns to the window.
                if focused {
                    self.core.window_focused();
                } else {
                    self.core.window_blurred();
                }
                // R1427 §5.41 §5.39 — a focus edge changes the terminal cursor's
                // render (filled+blinking <-> hollow+steady) with NO other event,
                // and an unfocused window intentionally idles its blink clock, so
                // nothing else would schedule the frame. Request a repaint on BOTH
                // edges so the enable/disable + fill/hollow transition lands on the
                // very next frame instead of latching until an unrelated event.
                self.core.request_redraw_for_window(spec_id);
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                // R1073 PR-27.4 §5.39 — extracted to `handle_keyboard_input` to
                // keep this dispatcher under the workspace
                // `clippy::too_many_lines` (100) ceiling (the app.rs split
                // convention). Passes the `Copy` `window_id` (the helper
                // re-resolves `spec_id` itself) so the keyboard / auto-repeat
                // hot path allocates nothing — unlike the rarer mouse / file
                // arms that `to_owned()` the borrowed `spec_id`.
                // R1076 PR-28 §5.39 — `is_synthetic` (winit focus-transition
                // key-state sync, dropped pre-R1076) gates the dock-toggle flap.
                self.handle_keyboard_input(event_loop, window_id, &event, is_synthetic);
            }
            // R56.2.a §5.13 §5.38 — IME composition events from the
            // platform input method (Wayland `text-input-v3`, X11
            // XIM, macOS `NSTextInputContext`, Windows TSF, GTK
            // IBus — winit 0.30 abstracts all four under one `Ime`
            // enum). Map to the pinion-native
            // [`pinion_core::CompositionEvent`] surface via the
            // `was_composing` state machine and dispatch through
            // `ShellCore::apply_composition` (R56.2.a substrate).
            // Multiple `CompositionEvent`s can fan out of a single
            // `Ime` event (e.g. first non-empty `Preedit` produces
            // `Start + Update`); see [`winit_ime_to_composition`]
            // for the mapping table.
            WindowEvent::Ime(ime) => {
                // R670.B §5.16 — per-window IME state machine. The
                // composition session belongs to the focused window;
                // tracking `was_composing` per-window means a
                // multi-window binding's main + inspector can each
                // carry an independent composition session.
                if let Some(slot) = self.windows.get_mut(&window_id) {
                    let (events, next_state) =
                        winit_ime_to_composition(&ime, slot.ime_was_composing);
                    slot.ime_was_composing = next_state;
                    for event in events {
                        self.core.apply_composition(&event);
                    }
                }
            }
            // R670.B / R1023 / R1123 §5.16 — per-window resize. Extracted to
            // `note_window_resized` to keep `window_event` under the 100-line
            // ceiling (the app.rs split convention) and to let the helper own
            // the slot borrow the maximized-cache sync needs.
            // R1147 §5.51 — the shell-private drag-preview window is not in
            // `windows`, so route its resize to its own surface; otherwise the
            // declared-window resize path.
            WindowEvent::Resized(size) if self.is_drag_preview_window(window_id) => {
                self.note_preview_resized(size);
            }
            WindowEvent::Resized(size) => self.note_window_resized(window_id, size),
            // R1088 §5.16 §5.41 §2 #7 PR-31 — OS window move. Feed the new
            // outer position back into `windows_signal` so the DECLARED
            // position converges on the actual one (a user title-bar drag
            // writes the position SSOT; `scene/windows` then reads it).
            // Echo of the shell's own `set_outer_position` is suppressed
            // inside `note_window_moved`. Extracted (like the scale arm) to
            // keep `window_event` under the 100-line ceiling.
            WindowEvent::Moved(position) => {
                self.note_window_moved(window_id, position);
                // R1617 — a move is the moment a window's display can change,
                // and a GUI-only session issues no RPC dispatch to re-stamp it
                // at. Without this the in-process `use_window_home` would
                // answer with wherever the window was at boot, forever.
                self.stamp_window_homes();
            }
            // R1027 §5.16 — DPI / scale change. Extracted to
            // `note_scale_factor_changed` to keep this dispatcher under the
            // workspace `clippy::too_many_lines` (100) ceiling (the app.rs
            // split convention, as `handle_mouse_button` / `handle_file_dnd`).
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.note_scale_factor_changed(window_id, scale_factor);
            }
            // R57.1 §5.50 — OS `prefers-color-scheme` change. winit
            // fires this on every desktop platform pinion supports
            // (macOS observes `NSApp.effectiveAppearance`, GNOME /
            // KDE observe `gsettings color-scheme`, Windows observes
            // `ImmersiveColorSet`). Forward to the global
            // `pinion_core::SystemColorScheme` signal so every
            // [`ThemeProvider`] in [`ThemeMode::System`] re-resolves
            // its palette in the next frame.
            WindowEvent::ThemeChanged(theme) => {
                pinion_core::set_system_color_scheme(winit_theme_to_pinion_scheme(theme));
            }
            // R1147 §5.51 — the shell-private drag-preview window paints a fixed
            // chip via its own render path (it is not in `windows`, so
            // `render_window` would early-return for it).
            WindowEvent::RedrawRequested if self.is_drag_preview_window(window_id) => {
                self.render_drag_preview();
            }
            WindowEvent::RedrawRequested => self.render_window(window_id),
            _ => {}
        }
        self.drain_redraw_to_winit();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            // R1190 §5.16 §5.49 §2 #2 — `dispatch_rpc` now drains the window-control
            // queue itself (it takes `event_loop`), so the drive-half is
            // compiler-enforced, not a caller-remembered step.
            AppEvent::RpcRequest(frame) => self.dispatch_rpc(frame, event_loop),
            AppEvent::AccessKit(ak) => self.handle_accesskit_event(ak),
            // R51.159 §5.23 — re-feed an Intent produced by a resolved
            // Command future back into the SCXML `send` channel via
            // `ShellCore::dispatch_intent`. The closing step of the
            // §5.23 R27 dispatch loop.
            AppEvent::IntentArrived(intent) => self.core.dispatch_intent(&intent),
            // R683 §5.16 §5.41 — the reconcile Effect (installed in
            // [`Self::resumed`] when `WidgetView::windows_signal()`
            // returned `Some(..)`) fired this user-event on every
            // value-changing emit of the binding's
            // `Signal<Vec<WindowSpec>>`. The handler re-reads the
            // signal snapshot + diffs against the cached last-known
            // spec list + resumes added specs + drops removed specs.
            // Idempotent on identical re-emits (Vec PartialEq
            // short-circuit inside `reconcile_windows`).
            AppEvent::WindowsDirty => self.reconcile_windows(event_loop),
            // R999 §5.23 — an off-thread producer wrote fresh data into a
            // shared handle the binding's `view` reads. Arm a binding-wide
            // redraw; the `drain_redraw_to_winit` tail below collapses it (and
            // any other wakes this iteration) into one `Window::request_redraw`,
            // and the next frame re-runs `view` which re-reads the handle.
            AppEvent::ExternalRepaint => self.core.request_redraw(),
            // R1363 §5.55 — a binding asked the APP to end through its
            // `QuitSink` (`hello-tray`'s Quit; sprag's poll thread on a dead
            // daemon socket). Same arm as Escape and the last-window policy, so
            // `app_quit_requested` still gets to refuse.
            AppEvent::QuitRequested => self.request_quit(event_loop),
            // R1362 PR-65 §5.16 §5.49 §2 #2 — the BINDING requested a WINDOW
            // control on its own behalf (`hello-tray`'s Show/Hide), delivered
            // through `ProxyWindowControlSink`. Execute it through the arm every
            // other `ControlProducer` reaches, so a `Close` is still offered to
            // `WidgetView::window_close_requested` first.
            //
            // Executing HERE (a later UI-thread turn) rather than inside the
            // binding's own call is what lets a tray `invoke` return its RPC
            // result before the window goes away — the same ordering
            // `dispatch_rpc` buys by draining its queue after the response
            // write.
            AppEvent::WindowControlRequested { window_id, control } => {
                self.apply_window_control(
                    &window_id,
                    control,
                    ControlProducer::Binding,
                    event_loop,
                );
            }
        }
        self.drain_redraw_to_winit();
    }

    /// R681 §2 #4 atomic 2 §5.16 §5.28 — per-window game-loop pacing.
    /// winit calls `about_to_wait` after every batch of pending events
    /// has been drained and immediately before the event loop blocks
    /// for more input. This is the canonical hook to configure the
    /// next [`ControlFlow`]:
    ///
    /// - Any active window slot whose published
    ///   [`ShellCore::immediate_subtree_for_window`] is `true` arms the
    ///   §2 #4 game-loop branch: compute that slot's
    ///   next-paint deadline as
    ///   `slot.last_paint_instant + frame_budget`
    ///   (`frame_budget` = 1/60s; per-window override lands in
    ///   atomic 3 via
    ///   [`pinion_runtime::frame_pacing::frame_budget_for_window`]).
    /// - The earliest deadline across every immediate-mode slot wins
    ///   — winit has a single global control-flow setting per loop
    ///   iteration, so the tightest deadline determines when winit
    ///   wakes up; the per-window paint clock survives because each
    ///   window's `Self::render_window` still reads its own
    ///   `last_paint_instants` slot for `dt`, and the
    ///   `redraw_requested_for_window` flag fires only the affected
    ///   window's redraw.
    /// - When no slot has an immediate-mode subtree, fall back to
    ///   [`ControlFlow::Wait`] — the input-driven retained-tree
    ///   semantics every Phase A binding already relies on.
    ///
    /// Each immediate-mode slot also re-arms its per-window redraw
    /// flag here so the next event-loop iteration's
    /// `Self::drain_redraw_to_winit` dispatches one
    /// `Window::request_redraw` per slot — that delivers the
    /// `WindowEvent::RedrawRequested` event the slot's
    /// `Self::render_window` consumes to drive frame N+1. Without
    /// this re-arm the §2 #4 game-loop would stall after the first
    /// paint (the substrate's compute-paint-scene wire arms the flag
    /// on first paint, but only one drain consumes it; subsequent
    /// frames need the re-arm here).
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let mut earliest_deadline: Option<Instant> = None;
        // R683 §5.16 — `slot.spec_id` is `Cow<'static, str>` which
        // can wrap a runtime-generated id (`Cow::Owned`) for the dock
        // tear-off arc. The per-iteration `slot_ids` buffer carries
        // owned `Cow`s so the substrate calls below survive the
        // `self.windows.values()` borrow release.
        let mut immediate_slot_ids: Vec<Cow<'static, str>> = Vec::new();
        for slot in self.windows.values() {
            if !matches!(slot.render, RenderState::Active { .. }) {
                continue;
            }
            // R681 atomic 3 — derive the per-window frame budget from
            // the substrate signals (both homed on `ShellCore`): the
            // sticky `immediate_subtree` flag `render_window` publishes
            // + the binding's optional `set_target_fps_for_window`
            // override. `None` budget means "this slot does not
            // contribute a deadline" (idle policy, the default for
            // retained-tree windows). The §5.16 jank profiler reads the
            // same two signals through the same helper, so its budget
            // equals this pacing budget for every window.
            let override_fps = self.core.target_fps_for_window(&slot.spec_id);
            let budget = pinion_runtime::frame_pacing::frame_budget_for_window(
                self.core.immediate_subtree_for_window(&slot.spec_id),
                override_fps,
            );
            let Some(budget) = budget else { continue };
            immediate_slot_ids.push(slot.spec_id.clone());
            let deadline = match self.core.last_paint_instant_for_window(&slot.spec_id) {
                Some(prev) => prev + budget,
                // No prior paint — schedule ASAP (now + 0). winit
                // clamps WaitUntil(past_or_present) to "wake
                // immediately" semantics.
                None => now,
            };
            earliest_deadline = Some(match earliest_deadline {
                Some(d) => d.min(deadline),
                None => deadline,
            });
        }
        for spec_id in immediate_slot_ids {
            // Re-arm so the next iteration's
            // `drain_redraw_to_winit` dispatches the per-window
            // `Window::request_redraw`. Idempotent: only one redraw
            // per drain regardless of how many times we set the flag.
            self.core.request_redraw_for_window(&spec_id);
        }
        // R924 §5.22 — keep the event loop awake while the owner-scoped
        // `LocalTaskPump` has async work in flight (a `Resource` fetch
        // spawned by a reactive `Effect` — e.g. lazy page loading driven by
        // a scroll). `compute_paint_scene_internal` polls the pump and
        // re-arms `redraw_requested` while pending, so the loop self-sustains
        // *once a frame renders*; this requests the frame that bootstraps it.
        // It is needed because some RPCs mutate reactive state — and so spawn
        // pump work — without arming a redraw (e.g. `scene/scroll`, whose
        // offset write fires the prefetch `Effect`): absent this, the loop
        // would sleep at `ControlFlow::Wait` (the deadline above is computed
        // only from animations + immediate-mode subtrees, not the pump) and
        // the fetch would stall. This is the "stay awake while active"
        // contract the `LocalTaskPump` doc names. NOTE the cost: while a task
        // is pending the scene re-renders every frame (the v1 `Waker::noop`
        // poll model — see `LocalTaskPump` docs); a wake-channel waker that
        // re-renders only on task progress is the documented forward
        // refinement (R761.1 carry) for genuinely long-running fetches.
        if pinion_core::LOCAL_TASK_PUMP
            .resolve(self.core.root_owner())
            .has_pending()
        {
            self.core.request_redraw();
            earliest_deadline = Some(earliest_deadline.map_or(now, |d| d.min(now)));
        }
        match earliest_deadline {
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
        // Drain the per-window flags we just armed so the
        // immediate-mode windows receive their next redraw without
        // waiting on another event-loop cycle.
        self.drain_redraw_to_winit();
    }
}

/// R51.37 §5.35 R1009 §5.13 — bridge from winit's [`NamedKey`] enum to the
/// W3C-aligned `KeyboardEvent.key` strings the
/// [`WidgetView::apply_key`](crate::WidgetView) contract speaks.
///
/// Surfaces the keys with an established cross-platform **widget** meaning:
/// navigation (arrows / Home / End / Page), activation (Enter / Space), the
/// **editing** keys (Backspace / Delete / Insert) and the **function** row
/// (F1–F12). R1009 added the editing + function keys for the content-surface
/// consumer — a terminal / code-editor / canvas forwards every one of them to
/// its child (sprag's PTY pane is the forcing case); the earlier curation
/// dropped them as "no widget cares", which is true only of the device-control
/// keys (`Browser*` / `Media*` / `Launch*` / `Audio*`). Those stay `None`: no
/// widget interprets them, and one that did would key off `apply_key` returning
/// `false` either way, so surfacing them buys nothing.
///
/// `NamedKey::Escape` / `NamedKey::Tab` are filtered **upstream** of this
/// bridge (the shell-reserved quit / `FocusManager` traverse arms in
/// [`AppShell::handle_key_press`] offer them to the focused widget first), so
/// they intentionally return `None` here.
///
/// The TUI backend's peer bridge `pinion_tui::input::key_str_from_event`
/// (crossterm `KeyCode` → the same W3C strings) must surface the same
/// content-surface vocabulary. The two map different platform enums, so the
/// tables are **not** folded (the R773 pin-don't-fold rule); their shared canon
/// is the W3C `KeyboardEvent.key` spec — keep both pinned to it. R1009 widened
/// this winit table to the editing + function coverage the TUI peer already had
/// (the drift that absence proved).
///
/// R51.92.1 §5.40 — module-local helper (sole caller is
/// [`AppShell::handle_key_press`] above).
fn named_key_str(named: NamedKey) -> Option<&'static str> {
    match named {
        NamedKey::ArrowLeft => Some("ArrowLeft"),
        NamedKey::ArrowRight => Some("ArrowRight"),
        NamedKey::ArrowUp => Some("ArrowUp"),
        NamedKey::ArrowDown => Some("ArrowDown"),
        NamedKey::Home => Some("Home"),
        NamedKey::End => Some("End"),
        NamedKey::PageUp => Some("PageUp"),
        NamedKey::PageDown => Some("PageDown"),
        NamedKey::Enter => Some("Enter"),
        NamedKey::Space => Some("Space"),
        // R1009 §5.13 — editing keys: a content-surface widget forwards these to
        // its child (Backspace = 0x7f, Delete = ESC[3~ in a PTY).
        NamedKey::Backspace => Some("Backspace"),
        NamedKey::Delete => Some("Delete"),
        NamedKey::Insert => Some("Insert"),
        // R1009 §5.13 — the function row F1–F12 (each an xterm escape). winit
        // also exposes F13–F35, left out until a consumer needs them.
        NamedKey::F1 => Some("F1"),
        NamedKey::F2 => Some("F2"),
        NamedKey::F3 => Some("F3"),
        NamedKey::F4 => Some("F4"),
        NamedKey::F5 => Some("F5"),
        NamedKey::F6 => Some("F6"),
        NamedKey::F7 => Some("F7"),
        NamedKey::F8 => Some("F8"),
        NamedKey::F9 => Some("F9"),
        NamedKey::F10 => Some("F10"),
        NamedKey::F11 => Some("F11"),
        NamedKey::F12 => Some("F12"),
        _ => None,
    }
}

/// R1073.1 PR-27.4 §5.39 — the DISPATCH key vocabulary for the press-owner gate
/// ([`crate::ShellCore::admit_key_press`]): a superset of [`named_key_str`] that
/// adds the two shell-reserved keys [`named_key_str`] deliberately excludes from
/// its R1009 content-surface / chord vocabulary — `Escape` and `Tab`. Those are
/// not forwarded to a content surface and are not chord keys, but
/// [`AppShell::handle_key_press`] DOES dispatch them (Escape → modal-cancel /
/// `event_loop.exit`, Tab → focus traverse), so the close-during-dispatch gate
/// must see them too: a future `Escape`-closes-a-window binding gets the same
/// one-press-one-action protection as `Enter`. Used ONLY for the owner gate +
/// its release ([`crate::ShellCore::note_key_release`]); the chord cache keeps
/// the narrower [`named_key_str`] vocabulary. `None` only for keys the shell
/// does not dispatch at all (media / browser keys, dead keys).
fn dispatch_named_key_str(named: NamedKey) -> Option<&'static str> {
    match named {
        NamedKey::Escape => Some("Escape"),
        NamedKey::Tab => Some("Tab"),
        other => named_key_str(other),
    }
}

/// R-PR47 §5.7 — build the egress for the connection that is the process's
/// own stdin: every frame it carries — a response, or (R1552) a
/// `scene/changed` notification nobody asked for — is written to stdout,
/// one line, exactly as the pre-PR47 inline `AppShell::dispatch_rpc` write
/// did (so the built-in `stdin → stdout` transport stays byte-identical — a
/// broken pipe silently skips rather than aborting the loop).
///
/// R1552 — built once for the stdin connection rather than once per frame,
/// because a subscription outlives the frame that opened it. `false` reports
/// a stdout that has gone away, which is what prunes such a subscription;
/// `writeln!` is the one call that can see it, since stdout is not checked
/// anywhere else.
fn stdout_egress() -> Arc<dyn RpcEgress> {
    FnEgress::new(|frame: String| {
        let mut out = std::io::stdout().lock();
        // stdout closed (downstream consumer gone) — silently skip; do
        // not abort the GUI loop on a broken pipe.
        writeln!(out, "{frame}").is_ok()
    })
}

/// The built-in stdin transport: a background thread that reads JSON-RPC
/// 2.0 lines from stdin and submits each through the [`RpcIngress`] seam
/// with a [`stdout_egress`]. Blank lines are skipped; EOF or any read
/// error terminates the thread quietly (the GUI loop keeps running).
/// [`RpcIngress::submit`] becomes a no-op after the event loop has shut
/// down, so a post-shutdown line is simply dropped.
///
/// R-PR47 — this is now one *producer* over the same seam an injected
/// transport uses (the socket adapter, a test harness, ...), not a
/// privileged stdin-only path. Sole in-crate caller is
/// [`run_with_config`], which builds it from the GUI backend's
/// [`ProxyRpcIngress`].
fn spawn_stdin_rpc_reader(ingress: Arc<dyn RpcIngress>) {
    thread::spawn(move || {
        // R-PR67 — stdin is a single logical connection for the process
        // lifetime: one stable id, its open announced before the first
        // frame and its close on EOF. A stateless ingress ignores the
        // lifecycle hooks (default no-op), so the pipe workflow is unchanged.
        let conn = ConnId::allocate();
        ingress.on_connect(conn);
        // R1552 — one egress for this one logical connection; every frame's
        // reply is derived from it, so a response and an unsolicited
        // notification reach stdout through the same writer.
        let egress = stdout_egress();
        let stdin = std::io::stdin();
        let handle = stdin.lock();
        for line in handle.lines() {
            let Ok(text) = line else {
                break;
            };
            if text.trim().is_empty() {
                continue;
            }
            ingress.submit(RpcFrame::new(conn, text, Arc::clone(&egress)));
        }
        // EOF (or a read error): the stdin peer closed. Balance on_connect.
        ingress.on_disconnect(conn);
    });
}

/// R-PR47 §5.7 §2 #6 — the GUI backend's [`RpcIngress`] implementation:
/// wraps the winit [`EventLoopProxy`] so a frame becomes an
/// [`AppEvent::RpcRequest`] user event on the UI thread. This is the ONE
/// place the winit proxy is adapted to the winit-free seam — the raw
/// `EventLoopProxy` is never handed to a consumer (that would leak winit
/// across the transport boundary and be un-implementable for the TUI
/// backend, breaking §2 #6).
struct ProxyRpcIngress {
    proxy: EventLoopProxy<AppEvent>,
}

impl ProxyRpcIngress {
    fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self { proxy }
    }
}

impl RpcIngress for ProxyRpcIngress {
    fn submit(&self, frame: RpcFrame) {
        // `send_event` errors only once the event loop has shut down;
        // the frame (and its reply) are then dropped — matching the old
        // reader's "break on send failure" behaviour.
        let _ = self.proxy.send_event(AppEvent::RpcRequest(frame));
    }

    /// R1552 §5.7 PINION-PR83 — release this connection's change streams.
    ///
    /// Done **here, synchronously on the transport's own reader thread**,
    /// rather than queued to the UI thread as frames are: a subscription
    /// holds a clone of the connection's
    /// [`RpcEgress`], the transport drops its own
    /// clone immediately after this returns, and it then joins the writer
    /// thread. Deferring the release would park that join behind the next
    /// UI turn — and behind *nothing at all* if the event loop has already
    /// shut down, which is exactly when connections are being torn down.
    ///
    /// This is the contract `on_disconnect` already states: release the
    /// connection's state however the client went away. An egress clone is
    /// that state.
    fn on_disconnect(&self, conn: ConnId) {
        pinion_rpc::process_registry().close_connection(conn);
    }
}

/// R51.108 §5.41 — convert a `winit::event::Touch` to the
/// substrate-local [`pinion_runtime::Touch`] at the winit boundary so
/// `ShellCore` stays winit-free for the §2 #6 GUI/TUI dual invariant.
/// `winit::event::Touch::id` already matches the abstract `id: u64`;
/// `location: PhysicalPosition<f64>` decomposes to `(x, y)`; the
/// four-variant `TouchPhase` enum maps 1:1.
/// R1027 §5.16 — whether a window `scale_factor` is non-identity.
///
/// Gates the scaled vello append (`render_window`) + the AccessKit root
/// transform so a `1.0` (non-`HiDPI`) window stays on the byte-identical
/// pre-R1027 render path. winit reports exact factors (`1.0` / `1.5` /
/// `2.0` …), but a bare `!= 1.0` would trip `clippy::float_cmp`; the
/// `f64::EPSILON` margin excludes only a literal `1.0`.
fn scale_is_non_identity(scale: f64) -> bool {
    (scale - 1.0).abs() > f64::EPSILON
}

/// R1060 §5.16 §5.12 — submit one encoded frame to a window's renderer.
///
/// Normally this is the `render` present; when `capture` is set (an RPC
/// `scene/screenshot` flagged the slot via
/// [`AppShell::capture_window_screenshot`]) it is `capture_rgba8`, which
/// renders AND reads back the presented swapchain texture. Returns the
/// present outcome (fed to the per-window render-fidelity record) plus
/// the captured frame when one was requested. Extracted from
/// [`AppShell::render_window`] so that paint method stays under the
/// workspace `clippy::too_many_lines` ceiling; the normal (`capture ==
/// false`) path is byte-identical to the pre-R1060 inline present.
/// R1537 §5.16 — what one paint learned about the GPU's own clock.
///
/// Three values read from the renderer in one place because only one scope
/// in `render_window` holds a renderer at all, and because they are easy
/// to record partially: the fresh measurement belongs on the frame sample,
/// while the capability and the drop count belong to the backend and ride
/// the read. A struct makes "all three, or none" the only shape a call
/// site can express.
#[derive(Debug, Clone, Copy, Default)]
struct GpuFrameReport {
    /// A measurement harvested for this frame, if one arrived. `None` is
    /// *no measurement* — never a zero, which would read as a free frame.
    us: Option<u64>,
    /// Whether this backend can time the GPU at all.
    supported: bool,
    /// Measurements taken and discarded since boot.
    dropped: u64,
}

impl GpuFrameReport {
    /// Read the backend's clock. Consuming on the renderer's side — each
    /// measurement is reported to exactly one frame sample, so the ring
    /// holds distinct measurements rather than one value re-stamped.
    fn read<R: VelloRenderer>(renderer: &mut R) -> Self {
        let clock = renderer.gpu_clock();
        Self {
            us: clock.measured(),
            supported: clock.is_supported(),
            dropped: renderer.gpu_dropped_samples(),
        }
    }
}

fn submit_frame<R: VelloRenderer>(
    renderer: &mut R,
    target: &VelloScene,
    base: vello::peniko::Color,
    capture: bool,
) -> (bool, Option<crate::vello_capture::CapturedFrame>) {
    if capture {
        match renderer.capture_rgba8(target, base) {
            Ok(frame) => (true, Some(frame)),
            Err(e) => {
                tracing::warn!(target: "pinion::shell", error = %e, "vello capture failed");
                (false, None)
            }
        }
    } else {
        let ok = match renderer.render(target, VelloContext { base_color: base }) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(target: "pinion::shell", error = %e, "vello render failed");
                false
            }
        };
        (ok, None)
    }
}

/// R1027 §5.16 — convert a winit **physical** `inner_size` to the
/// **logical** layout size the paint scene is built in.
///
/// Since R1027 the whole pinion scene / layout / introspection world is
/// logical; physical pixels appear only at the GPU surface size, the
/// vello raster scale, and pointer input. Delegates to winit's own DPI
/// math ([`PhysicalSize::to_logical`]) rather than hand-rolling the
/// division + rounding.
///
/// May return `0` for a degenerate (minimized / sub-logical-pixel)
/// dimension — e.g. a 1-physical-px width at scale 4 rounds to `0`
/// logical. The caller's `NonZeroU32` guard in `render_window`
/// early-returns on a `0` dimension (no paint), exactly as pre-R1027
/// did when it fed the raw physical `inner_size` to `NonZeroU32::new`.
/// This is deliberately NOT clamped to `>= 1` here: clamping would make
/// that guard unreachable and paint a wasted 1px frame for a 0-size
/// window.
///
/// `scale` must be a valid winit factor (positive + normal); winit's
/// `to_logical` asserts this. Always satisfied here — the only callers
/// pass `WindowSlot::scale_factor`, sourced from `Window::scale_factor()`
/// / `ScaleFactorChanged`.
fn logical_layout_size(physical: PhysicalSize<u32>, scale: f64) -> (u32, u32) {
    let logical: LogicalSize<u32> = physical.to_logical(scale);
    (logical.width, logical.height)
}

/// R1027 §5.16 §5.35 — convert a winit **physical** pointer position to
/// the **logical** coordinate space the [`pinion_runtime::InputRouter`]
/// hit-tests in (the same space the logical paint scene lays out in).
///
/// Without this, a 4-logical-px hit target (e.g. a splitter handle) on a
/// 2x display would be a 4-device-px target the router never resolves at
/// the cursor's true logical position. Delegates to winit's
/// [`PhysicalPosition::to_logical`] (the canonical platform DPI math).
/// `scale` must be a valid winit factor (positive + normal), which the
/// per-slot `scale_factor` always is.
fn winit_pointer_to_logical(pos: PhysicalPosition<f64>, scale: f64) -> (f64, f64) {
    let logical: LogicalPosition<f64> = pos.to_logical(scale);
    (logical.x, logical.y)
}

fn winit_touch_to_pinion(touch: winit::event::Touch, scale: f64) -> pinion_runtime::Touch {
    // R1027 §5.16 §5.35 — touch location is physical; map to logical so
    // it shares the router's coordinate space (mirrors `CursorMoved`).
    let (x, y) = winit_pointer_to_logical(touch.location, scale);
    // R1423 §5.35 — the pen / touch FORCE (W3C `PointerEvent.pressure` source),
    // normalised to `0.0..=1.0`; `None` on a platform that reports no force.
    // `Force::normalized` folds the iOS `Calibrated` / `Normalized` variants into
    // one 0..1 scale.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a normalised pen force 0.0..=1.0 loses no meaningful precision as f32"
    )]
    let force = touch.force.map(|f| f.normalized() as f32);
    pinion_runtime::Touch {
        id: touch.id,
        x,
        y,
        phase: match touch.phase {
            winit::event::TouchPhase::Started => pinion_runtime::TouchPhase::Started,
            winit::event::TouchPhase::Moved => pinion_runtime::TouchPhase::Moved,
            winit::event::TouchPhase::Ended => pinion_runtime::TouchPhase::Ended,
            winit::event::TouchPhase::Cancelled => pinion_runtime::TouchPhase::Cancelled,
        },
        force,
    }
}

/// (R51.186 §5.45 R55.C.2) Convert a winit `MouseScrollDelta` to
/// the substrate-local [`WheelDelta`] at the winit boundary.
///
/// winit reports wheel input in one of two unit modes —
/// `LineDelta(f32, f32)` for legacy notched mouse wheels and
/// `PixelDelta(PhysicalPosition<f64>)` for trackpad inertia /
/// high-resolution scroll. Both narrow into pinion's unit-tagged
/// variants.
///
/// R1027 §5.16 §5.45 — `PixelDelta` is in **physical** device pixels
/// (winit `MouseScrollDelta` doc), so it is divided by `scale` to the
/// logical coordinate space the scene + `ScrollState` live in — the
/// pointer-input boundary (#3) of the R1027 logical-coordinate policy.
/// Without this a 2x display would over-scroll the logical content 2x.
/// `LineDelta` is unitless notch counts (not pixels), so it is
/// scale-independent and left unscaled. The `f64` delta narrows to
/// `f32` after the divide (the substrate consumes the delta at `f32` —
/// `External::wheel` directly, the `ScrollState` fallback after an
/// `i32` round — so the wider `f64` carries no information past here).
///
/// (R51.192 §5.45 R55.C.2) Both axes flip sign at this boundary.
/// winit's [`MouseScrollDelta`] convention is "positive = content
/// being scrolled should move right and down (revealing more
/// content left and up)" — i.e. winit `(dx, dy) > 0` means the
/// scroll is toward the content origin, the user wants to *see*
/// what is above/left of the current viewport. pinion's substrate
/// (and the [`ScrollState::scroll_by`](pinion_core::widgets::scroll::ScrollState::scroll_by)
/// it forwards into) follows the W3C `WheelEvent.deltaY` /
/// `deltaX` convention — positive scrolls *away* from the
/// content origin, exposing what is below/right. The two
/// conventions are opposite-signed, so the boundary flip lands
/// here exactly once. The TUI sibling
/// `pinion_tui::shell::dispatch_mouse` already emits W3C-signed
/// `WheelDelta` values (`ScrollUp` → `dy = -1.0`,
/// `ScrollDown` → `dy = +1.0`), so the substrate stays
/// crossterm + W3C agreed and only winit needs the flip.
#[allow(clippy::cast_possible_truncation)]
fn winit_wheel_to_pinion(delta: MouseScrollDelta, scale: f64) -> WheelDelta {
    match delta {
        MouseScrollDelta::LineDelta(dx, dy) => WheelDelta::Lines { dx: -dx, dy: -dy },
        MouseScrollDelta::PixelDelta(pos) => {
            // R1027 §5.16 §5.45 — physical device pixels -> logical, same
            // conversion as `CursorMoved` / `Touch` (reuses the shared
            // `winit_pointer_to_logical` helper), so high-res / trackpad
            // scrolling moves the logical content the intended distance.
            let (lx, ly) = winit_pointer_to_logical(pos, scale);
            WheelDelta::Pixels {
                dx: -(lx as f32),
                dy: -(ly as f32),
            }
        }
    }
}

/// R1432 §5.35 — convert a winit `TouchPhase` (which brackets a native
/// gesture's lifecycle) to the pinion-native
/// [`GesturePhase`](pinion_core::GesturePhase). The four variants map 1:1:
/// `Started -> Begin`, `Moved -> Update`, `Ended -> End`, `Cancelled -> Cancel`.
/// Shared by every native-gesture arm (R1432 pinch, R1433 rotation) — the phase
/// bracket is the gesture-agnostic half, so the name is too.
///
/// ★ R1703 — and the **wheel** is now one of them. winit brackets a trackpad
/// scroll with the same `TouchPhase` it brackets a pinch with, and the arm that
/// took the wheel discarded it (`MouseWheel { delta, .. }`) for the whole life
/// of this shell. A second enum for the wheel's copy of these four arms would
/// have been the second spelling this tree keeps deleting.
fn winit_gesture_phase_to_pinion(phase: winit::event::TouchPhase) -> pinion_core::GesturePhase {
    match phase {
        winit::event::TouchPhase::Started => pinion_core::GesturePhase::Begin,
        winit::event::TouchPhase::Moved => pinion_core::GesturePhase::Update,
        winit::event::TouchPhase::Ended => pinion_core::GesturePhase::End,
        winit::event::TouchPhase::Cancelled => pinion_core::GesturePhase::Cancel,
    }
}

/// R1434 §5.35 §5.15 — dispatch the winit native trackpad gestures (the
/// toolkit native gesture event: pinch / rotation / pan) off `window_event`'s main match:
/// `true` when `event` was one of them (the caller is done), `false` otherwise (the
/// caller falls through to the general match). Mirrors the TUI drain's `try_drain_native_gesture`
/// sub-dispatcher, so both backends group the gesture family the same way (§2
/// #6) and each backend's giant dispatcher stays under the line ceiling as the
/// family grows — an extract, never an `#[allow(too_many_lines)]`.
///
/// Every gesture's payload fields are `Copy`, so matching through the reference
/// moves nothing out of `event` and the caller keeps ownership for its match.
fn try_forward_native_gesture<V: WidgetView>(
    core: &mut ShellCore<V>,
    spec_id: &str,
    event: &WindowEvent,
    scale: f64,
) -> bool {
    match *event {
        WindowEvent::PinchGesture { delta, phase, .. } => {
            forward_pinch_gesture(core, spec_id, delta, phase);
        }
        WindowEvent::RotationGesture { delta, phase, .. } => {
            forward_rotation_gesture(core, spec_id, delta, phase);
        }
        WindowEvent::PanGesture { delta, phase, .. } => {
            forward_pan_gesture(core, spec_id, delta, phase, scale);
        }
        // R1435 §5.35 §5.15 — winit `DoubleTapGesture` (macOS smart-magnify /
        // iOS), the family's phase-less member: no delta and no `TouchPhase` to
        // convert, so the arm forwards nothing but the window.
        WindowEvent::DoubleTapGesture { .. } => {
            core.smart_zoom_gesture_for_window(spec_id, PointerId::MOUSE);
        }
        _ => return false,
    }
    true
}

/// R1432 §5.35 §5.15 — forward a winit `PinchGesture` (macOS / iOS trackpad
/// magnify) into the addressed window's `ShellCore` seam. Lifted off
/// `window_event`'s dispatch match so that giant function stays under the line
/// ceiling (extract, not `#[allow]`); a free fn over `&mut core` (not `&mut
/// self`) so it borrows the disjoint field the live `spec_id` borrow leaves
/// alone. Like `MouseWheel` a pinch carries no position — it applies to the
/// surface under the last cursor position the router remembers — so this just
/// forwards the incremental `delta` (positive = zoom in) with the converted
/// phase. On non-Apple platforms the variant never fires; the
/// `scene/pinch_gesture` RPC is the sole driver there (§2 #2).
fn forward_pinch_gesture<V: WidgetView>(
    core: &mut ShellCore<V>,
    spec_id: &str,
    delta: f64,
    phase: winit::event::TouchPhase,
) {
    core.pinch_gesture_for_window(
        spec_id,
        PointerId::MOUSE,
        delta,
        winit_gesture_phase_to_pinion(phase),
    );
}

/// R1433 §5.35 §5.15 — forward a winit `RotationGesture` (macOS / iOS trackpad
/// twist), the [`forward_pinch_gesture`] sibling with rotation in place of
/// scale. Forwards the incremental `delta` in degrees (positive =
/// counter-clockwise, winit's convention) with the converted phase. On non-Apple
/// platforms the variant never fires; the `scene/rotation_gesture` RPC is the
/// sole driver there (§2 #2).
fn forward_rotation_gesture<V: WidgetView>(
    core: &mut ShellCore<V>,
    spec_id: &str,
    delta: f32,
    phase: winit::event::TouchPhase,
) {
    core.rotation_gesture_for_window(
        spec_id,
        PointerId::MOUSE,
        f64::from(delta),
        winit_gesture_phase_to_pinion(phase),
    );
}

/// R1434 §5.35 §5.15 — forward a winit `PanGesture` (N-finger trackpad / touch
/// pan), the [`forward_pinch_gesture`] sibling with a 2D delta. Unlike the
/// unitless magnification and the degrees of a rotation, a pan delta is in
/// PIXELS, and winit reports physical ones — so this converts to logical
/// through the shared `winit_pointer_to_logical`, exactly as the `MouseWheel`
/// pixel path and `CursorMoved` do, and the widget's offset math stays in the
/// one logical coordinate world the rest of the scene lives in. The sign is
/// forwarded as the platform reports it (a pan is direct manipulation), NOT
/// flipped the way `winit_wheel_to_pinion` flips a scroll command. On non-iOS
/// platforms the variant never fires; the `scene/pan_gesture` RPC is the sole
/// driver there (§2 #2).
#[allow(
    clippy::cast_possible_truncation,
    reason = "a logical-pixel pan delta loses no meaningful precision as f32, the unit the External hook carries"
)]
fn forward_pan_gesture<V: WidgetView>(
    core: &mut ShellCore<V>,
    spec_id: &str,
    delta: PhysicalPosition<f32>,
    phase: winit::event::TouchPhase,
    scale: f64,
) {
    let (lx, ly) = winit_pointer_to_logical(delta.cast(), scale);
    core.pan_gesture_for_window(
        spec_id,
        PointerId::MOUSE,
        lx as f32,
        ly as f32,
        winit_gesture_phase_to_pinion(phase),
    );
}

/// R51.108 §5.41 — convert a `winit::keyboard::ModifiersState` to the
/// substrate-local [`pinion_runtime::Modifiers`] at the winit boundary.
/// The four W3C DOM Level 3 modifier bits map 1:1 (winit's `super_key`
/// is the Meta / Cmd / Win key in the abstract vocabulary).
fn winit_modifiers_to_pinion(
    modifiers: winit::keyboard::ModifiersState,
) -> pinion_runtime::Modifiers {
    pinion_runtime::Modifiers {
        shift: modifiers.shift_key(),
        ctrl: modifiers.control_key(),
        alt: modifiers.alt_key(),
        meta: modifiers.super_key(),
    }
}

/// R57.1 §5.50 — translate winit's two-state
/// [`winit::window::Theme`] (`Light` / `Dark`)
/// into the W3C-aligned three-state
/// [`pinion_core::SystemColorScheme`]. winit itself never surfaces a
/// "no preference" reading on `Window::theme()` or
/// `WindowEvent::ThemeChanged` (every desktop backend — macOS
/// `NSApp.effectiveAppearance`, GNOME / KDE `gsettings color-scheme`,
/// Windows `ImmersiveColorSet` — resolves the OS signal to one of
/// the two), so this helper maps 1:1 onto `Light` / `Dark`. The
/// `NoPreference` variant remains the fresh-thread default; the
/// platform side never *clears* the signal back to it.
fn winit_theme_to_pinion_scheme(theme: winit::window::Theme) -> pinion_core::SystemColorScheme {
    match theme {
        winit::window::Theme::Light => pinion_core::SystemColorScheme::Light,
        winit::window::Theme::Dark => pinion_core::SystemColorScheme::Dark,
    }
}

/// R56.2.a §5.13 §5.38 — map a `winit::event::Ime` (cross-platform IME
/// abstraction covering Wayland `text-input-v3` + X11 XIM + macOS
/// `NSTextInputContext` + Windows TSF + GTK `IBus`) onto the pinion-
/// native [`pinion_core::CompositionEvent`] sequence the substrate
/// consumes (`Start` / `Update` / `Commit` / `Cancel`).
///
/// The mapping is deterministic given the current `was_composing`
/// boolean (the shell-side state machine):
///
/// | winit event                        | `was_composing` in  | Dispatched `CompositionEvent`s          | `was_composing` out |
/// |------------------------------------|---------------------|-----------------------------------------|---------------------|
/// | `Ime::Enabled`                     | any                 | (none — just a notification)            | unchanged           |
/// | `Ime::Preedit(text, _)` non-empty  | `false`             | `Start`, `Update(text)`                 | `true`              |
/// | `Ime::Preedit(text, _)` non-empty  | `true`              | `Update(text)`                          | `true`              |
/// | `Ime::Preedit("", _)` empty        | `false`             | (none — idempotent)                     | `false`             |
/// | `Ime::Preedit("", _)` empty        | `true`              | `Update("")` (visual clear, stay open)  | `true`              |
/// | `Ime::Commit(text)`                | `false`             | `Start`, `Commit(text)`                 | `false`             |
/// | `Ime::Commit(text)`                | `true`              | `Commit(text)`                          | `false`             |
/// | `Ime::Disabled`                    | `false`             | (none — idempotent)                     | `false`             |
/// | `Ime::Disabled`                    | `true`              | `Cancel`                                | `false`             |
///
/// Key invariants behind the table:
///
/// 1. **Empty `Preedit` is `Update("")`, not `Cancel`** — winit
///    documents "Right before `Commit` event winit will send empty
///    `Ime::Preedit` event" as a synthetic clear. Treating empty
///    preedit as cancel would fire a spurious `Cancel + Commit` pair
///    on every pinyin / Hangul commit. Instead the visual clears and
///    the substrate stays composing; on the immediately-following
///    `Commit` the substrate (still in `Editing`) commits at the
///    caret with the `was_composing && !text.is_empty()` gate
///    intact. (R56.1.g.1 substrate.)
///
/// 2. **`Commit` from `was_composing=false` injects a synthetic
///    `Start`** — macOS dead-key sequences and trivial-IME paths
///    can emit `Commit` without a prior `Preedit`. Seeding `Start`
///    drives the SCXML through `Focused → Editing` so the substrate
///    is consistent (`preedit_buffer` switches `None → Some("")`)
///    before the immediate `Commit` lands the text. Without the
///    synthetic `Start` the text would still insert at caret (the
///    `preedit_commit` path runs unconditionally) but the SCXML
///    would never reach `Editing` and the `text_committed` intent
///    would not fire (`was_composing` gate evaluates to `false`).
///
/// 3. **`Disabled` mid-session dispatches `Cancel`, not just
///    state reset** — the substrate observes the cancel through
///    SCXML `CancelEdit` drive + `preedit_cancel`; without the
///    explicit `Cancel` the caret blink would stay paused (R56.1.j)
///    and the `Editing` SCXML state would linger.
///
/// 4. **`Enabled` is informational** — winit emits `Enabled` after
///    `set_ime_allowed(true)` succeeds, but the substrate's IME
///    state is driven entirely by the `Preedit` / `Commit` /
///    `Disabled` triplet. `Enabled` is a hook a future sub-round
///    could use for `set_ime_cursor_area` rebroadcast on session
///    start.
///
/// Returns the dispatched events as a `Vec` (caller iterates in
/// order) plus the next `was_composing` state for the caller to
/// store on the shell. Free function (not a method) so unit tests
/// can drive the mapping table without a winit `EventLoop`.
fn winit_ime_to_composition(
    ime: &Ime,
    was_composing: bool,
) -> (Vec<pinion_core::CompositionEvent>, bool) {
    use pinion_core::CompositionEvent;
    match ime {
        Ime::Enabled => (Vec::new(), was_composing),
        Ime::Preedit(text, _cursor) => {
            if text.is_empty() {
                if was_composing {
                    (vec![CompositionEvent::Update(String::new())], true)
                } else {
                    (Vec::new(), false)
                }
            } else if was_composing {
                (vec![CompositionEvent::Update(text.clone())], true)
            } else {
                (
                    vec![
                        CompositionEvent::Start,
                        CompositionEvent::Update(text.clone()),
                    ],
                    true,
                )
            }
        }
        Ime::Commit(text) => {
            if was_composing {
                (vec![CompositionEvent::Commit(text.clone())], false)
            } else {
                (
                    vec![
                        CompositionEvent::Start,
                        CompositionEvent::Commit(text.clone()),
                    ],
                    false,
                )
            }
        }
        Ime::Disabled => {
            if was_composing {
                (vec![CompositionEvent::Cancel], false)
            } else {
                (Vec::new(), false)
            }
        }
    }
}

/// R1087 §5.16 §5.41 PR-31, widened R1576 — the pure **move-pass** diff for
/// [`AppShell::reconcile_windows`]: which already-open windows must have
/// their OS position reconciled because the binding re-declared a
/// different [`WindowSpec::placement`].
///
/// A window appears here iff its `id` is present in **both** `old` and
/// `new` (so it is neither an add nor a drop — those passes key on id
/// alone) AND `new` declares a placement (`Some`) that differs from what
/// `old` declared (which also fires when `old` left placement to the window
/// manager — `None` → first declared placement). A `new` spec that drops back
/// to `None` is **not** a move: `set_outer_position` cannot hand a window back
/// to WM auto-placement, so the declared `None` simply leaves the window where
/// it is.
///
/// R1576 widened the compared value from the raw `position` pair to the whole
/// [`WindowPlacement`], so **re-declaring the display alone is a move** — the
/// signal write that sends a torn-off panel to the other monitor. Comparing
/// only the offset would have made that write a silent no-op, which is the
/// R1319 defect exactly (a declared axis the reconcile pass could not see).
///
/// Splitting this out of `reconcile_windows` keeps the genuinely-new logic
/// pure and unit-testable with no winit event loop (the apply —
/// `Window::set_outer_position` — stays in the imperative reconcile, its
/// live effect HW-gated like every other real-window behaviour).
///
/// **This DIFF is total** over the `position` field: every same-id position
/// change appears in the returned `Vec` (pre-R1087 the add/drop passes would
/// silently swallow it — the top-level `new_specs == old_specs` guard sees
/// the diff, then neither id-keyed pass acts on it). The *apply* the caller
/// then performs is best-effort per window: a window with a live arc moves
/// immediately; a window with no live arc — a `Suspended(Some)` mobile state
/// ([`crate::AppShell::slot_window`] returns `None`) — reconciles on its next
/// create instead, not here (a mobile-lifecycle gap deferred with mobile,
/// not a desktop one). So "total" describes the diff, not the OS effect.
///
/// `old.position != Some(new_pos)` also fires when `old` left placement to the
/// window manager (`None` → first declared position). A `new` spec that drops
/// back to `None` is **not** a move: `set_outer_position` cannot hand a window
/// back to WM auto-placement, so the declared `None` leaves the window where
/// it is.
///
/// O(N²) (`find` inside the `new` loop) unlike the `HashSet` add/drop passes:
/// deliberately accepted — N is the window count (a handful), so a map would
/// be premature; the add/drop passes go `HashSet` only because their set
/// difference can span many ids.
/// R1576 §5.16 §5.41 — the frame a declared placement is commanded in.
///
/// Two arms because the two placements were resolved in different spaces and
/// converting either into the other's would lose exactly the precision that
/// motivated the axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlacementCommand {
    /// Absolute logical pixels — what `WindowSpec::position` has meant since
    /// R1087, byte-identical for every window that declares no display. winit
    /// converts it with the window's own scale factor.
    Logical(i32, i32),
    /// Absolute physical pixels, already resolved against the desk. Commanded
    /// in physical because that is the space the resolution happened in;
    /// re-dividing by the window's own scale would reintroduce the per-window
    /// guess the display axis exists to retire.
    Physical(i32, i32),
}

/// R1576 §5.16 §5.41 — decide what to command for a declared placement, and
/// what to latch for the R1088 echo check.
///
/// Split out of [`AppShell::apply_placement`] so the decision is testable
/// without a live winit window, exactly as
/// [`window_placement_moves`] and [`moved_is_command_echo`] are — and for a
/// sharper reason than either: on a one-monitor 1x desk the two frames produce
/// the SAME numbers, so an implementation that ignored the display would pass
/// every test this machine can run against a real window. The arrangement has
/// to be an argument for the difference to be visible at all.
///
/// Answers `(what the placement resolved to, what to command, what to latch)`.
/// The command is `None` only on a headless desk, where a display-relative
/// placement has no position — commanding one would be inventing it.
fn placement_command(
    placement: &WindowPlacement,
    topology: &DisplayTopology,
    scale: f64,
) -> (Anchored, Option<PlacementCommand>, (i32, i32)) {
    let anchored = placement.resolve(topology);
    if placement.display.is_none() {
        return (
            anchored,
            Some(PlacementCommand::Logical(
                placement.offset.0,
                placement.offset.1,
            )),
            placement.offset,
        );
    }
    let Some((x, y)) = anchored.at() else {
        return (anchored, None, placement.offset);
    };
    // The echo latch is compared against `Moved`'s LOGICAL reading, so it is
    // stamped in the units that comparison uses.
    let logical: LogicalPosition<i32> = PhysicalPosition::new(x, y).to_logical(scale);
    (
        anchored,
        Some(PlacementCommand::Physical(x, y)),
        (logical.x, logical.y),
    )
}

fn window_placement_moves(
    old: &[WindowSpec],
    new: &[WindowSpec],
) -> Vec<(String, WindowPlacement)> {
    let mut moves = Vec::new();
    for spec in new {
        let Some(new_placement) = spec.placement() else {
            continue;
        };
        // Only an id present in `old` is a move — an id only in `new` is
        // an ADD (resume_spec applies its initial placement at create time,
        // not this pass).
        if let Some(old_spec) = old.iter().find(|o| o.id == spec.id) {
            if old_spec.placement().as_ref() != Some(&new_placement) {
                moves.push((spec.id.as_ref().to_owned(), new_placement));
            }
        }
    }
    moves
}

/// R1319 §5.16 §5.41 PR-52 — the same-id TITLE changes between two
/// [`WindowSpec`] lists: every window whose declared [`WindowSpec::title`] differs
/// from what it declared before, paired with the new title.
///
/// Pre-R1319 `title` was read exactly ONCE, at `Window::default_attributes()`
/// create time — a live window's OS title could never follow the binding's declared
/// spec, and (worse) [`WindowSpec::title`]'s own rustdoc CLAIMED that winit
/// `set_title`-forwards it, which was true only for the window's first frame of
/// existence. The forcing consumer is a terminal multiplexer (sprag PR-52): the
/// tmux / gnome-terminal convention is that the OS window title (alt-tab, taskbar)
/// follows the FOCUSED pane's title, which the child renames on every prompt. That
/// needs `title` to be a live, reconcilable axis — like `position` (R1087), not like
/// `strategy` / `decorations` (create-time intent).
///
/// A window appears here iff its `id` is in **both** lists (neither an add — whose
/// title `resume_spec` applies via `with_title` — nor a drop) AND its title changed.
///
/// Splitting this out of `reconcile_windows` keeps the logic pure and unit-testable
/// with no winit event loop, exactly as [`window_placement_moves`] does; the apply
/// (`Window::set_title`) stays in the imperative reconcile.
///
/// **No write-back twin.** Position closes its declared→actual loop (R1088:
/// `WindowEvent::Moved` from a user drag feeds the signal). Title has no such loop
/// and needs none: nothing but the binding can rename a window, so the declared
/// spec IS the truth. (winit offers no title read-back to converge on anyway — its
/// X11 `Window::title()` is a stub returning an empty string.)
///
/// O(N²) `find` for the same reason [`window_placement_moves`] accepts it: N is the
/// window count, a handful.
fn window_title_changes<'a>(
    old: &'a [WindowSpec],
    new: &'a [WindowSpec],
) -> Vec<(String, &'a str)> {
    window_axis_changes(old, new, |spec| spec.title.as_str())
}

/// R1610 §5.16 §5.41 — the same-id changes on ONE declared axis between two
/// [`WindowSpec`] lists: every window present in **both** lists whose value on that
/// axis differs, paired with the new value.
///
/// The three live axes ([`window_title_changes`], [`window_decoration_changes`],
/// [`window_level_changes`]) are this function with a different projection. R1610 was
/// about to write the third hand-rolled copy of the identical walk — a window absent
/// from `old` is an ADD, whose value `resume_spec` applies at create; a window absent
/// from `new` is a DROP; only the intersection can carry a change — and three
/// mechanical copies of a rule is the point at which the rule should have one home
/// (the R727 / R732 third-consumer mandate). Getting the ADD case wrong in one copy
/// and not the others is exactly the drift that costs.
///
/// `axis` returns the value to compare, which may borrow from the specs (hence the
/// shared lifetime) so the title projection stays allocation-free.
///
/// [`window_placement_moves`] is deliberately NOT expressed here: placement is not a
/// field comparison but a resolution against the live monitor topology, with an add
/// that CAN require an apply. A shape that only looks similar is not the same rule.
fn window_axis_changes<'a, T, F>(
    old: &'a [WindowSpec],
    new: &'a [WindowSpec],
    axis: F,
) -> Vec<(String, T)>
where
    T: PartialEq,
    F: Fn(&'a WindowSpec) -> T,
{
    let mut changes = Vec::new();
    for spec in new {
        if let Some(old_spec) = old.iter().find(|o| o.id == spec.id) {
            let next = axis(spec);
            if axis(old_spec) != next {
                changes.push((spec.id.as_ref().to_owned(), next));
            }
        }
    }
    changes
}

/// R1610 §5.16 — the one place a pinion [`WindowLevel`] becomes a winit
/// [`winit::window::WindowLevel`].
///
/// A total match rather than a `From` impl with a `_` arm: the two enums are the
/// same three arms today, and if either grows one the compiler is what says so.
const fn winit_window_level(level: WindowLevel) -> WinitWindowLevel {
    match level {
        WindowLevel::AlwaysOnBottom => WinitWindowLevel::AlwaysOnBottom,
        WindowLevel::Normal => WinitWindowLevel::Normal,
        WindowLevel::AlwaysOnTop => WinitWindowLevel::AlwaysOnTop,
    }
}

/// R1610 §5.16 §2 #7 — which windowing system this process is actually talking to.
///
/// A build target is not the answer on Linux: one binary runs on X11 or Wayland
/// depending on the session, and that is precisely the pair whose window-level
/// support differs (winit 0.30's Wayland backend implements `set_window_level` as an
/// empty body — core `xdg-shell` has no stacking protocol). winit answers it for the
/// live event loop through its per-platform extension traits, so this is a read, not
/// a guess.
///
/// [`WindowingBackend::Other`] is deliberate on any target this adapter has not
/// measured: reported as such rather than folded into a success or a failure, so a
/// caller can tell "cannot" from "nobody looked".
#[cfg(all(unix, not(target_os = "macos")))]
fn detect_windowing_backend(event_loop: &ActiveEventLoop) -> WindowingBackend {
    use winit::platform::wayland::ActiveEventLoopExtWayland;
    use winit::platform::x11::ActiveEventLoopExtX11;
    if event_loop.is_wayland() {
        WindowingBackend::Wayland
    } else if event_loop.is_x11() {
        WindowingBackend::X11
    } else {
        WindowingBackend::Other
    }
}

/// R1610 §5.16 §2 #7 — macOS has one windowing system; see the unix twin.
#[cfg(target_os = "macos")]
const fn detect_windowing_backend(_event_loop: &ActiveEventLoop) -> WindowingBackend {
    WindowingBackend::MacOs
}

/// R1610 §5.16 §2 #7 — Windows has one windowing system; see the unix twin.
#[cfg(windows)]
const fn detect_windowing_backend(_event_loop: &ActiveEventLoop) -> WindowingBackend {
    WindowingBackend::Windows
}

/// R1610 §5.16 §2 #7 — any other target: unmeasured, and said so.
#[cfg(not(any(unix, windows)))]
const fn detect_windowing_backend(_event_loop: &ActiveEventLoop) -> WindowingBackend {
    WindowingBackend::Other
}

/// R1320 §5.16 §5.41 — acknowledge a USER window move in the reconcile cache by
/// patching ONLY that window's `position`.
///
/// The write-back path ([`AppShell::note_window_moved`]) must tell the next
/// `reconcile_windows` "this window is already where the signal says", so the move pass
/// does not re-command the position the user just dragged to. Pre-R1320 it did that by
/// overwriting the WHOLE cached spec list with the signal's current value — which also
/// acknowledged every OTHER pending change in that list (a title, a decorations flip),
/// so the reconcile that had not yet drained them took its `new == old` fast path and
/// dropped them permanently. Patching one field of one window keeps every other diff
/// alive.
///
/// A spec id not in the cache is a no-op (the window is being created or dropped in the
/// same pass; its title/position come from the fresh spec either way).
fn cache_moved_position(cache: &mut [WindowSpec], spec_id: &str, position: (i32, i32)) {
    if let Some(spec) = cache.iter_mut().find(|s| s.id.as_ref() == spec_id) {
        spec.position = Some(position);
    }
}

/// R1320 §5.16 §5.41 — the same-id DECORATIONS changes between two [`WindowSpec`]
/// lists, the twin of [`window_title_changes`].
///
/// R1118 declared this axis create-time-only and warned on a runtime flip, on the
/// stated grounds that "no `Window::set_decorations` call exists". winit 0.30 HAS
/// [`winit::window::Window::set_decorations`], implemented for X11 / Wayland / macOS /
/// Windows (documented no-op on iOS / Android / Web). The limit was invented, so the
/// warn is replaced by an apply and a binding can now hide or restore a live window's
/// OS chrome by writing its spec — the declared spec stays the SSOT for chrome exactly
/// as it now is for title.
fn window_decoration_changes(old: &[WindowSpec], new: &[WindowSpec]) -> Vec<(String, bool)> {
    window_axis_changes(old, new, |spec| spec.decorations)
}

/// R1610 §5.16 §5.41 — the same-id LEVEL changes between two [`WindowSpec`] lists,
/// the third member of the live-axis family beside [`window_title_changes`] and
/// [`window_decoration_changes`].
///
/// This axis has to be live, which is what makes it different from `decorations`
/// arriving late (R1320 corrected an invented limit) — a window level is a thing the
/// USER turns on and off ("keep this readout above the app I am watching"), so a
/// create-time-only level would not be the feature at all. winit 0.30 has
/// [`winit::window::Window::set_window_level`] on every desktop backend, and it
/// applies to a live window: no recreate, no unmap, no re-show.
///
/// The reference encoding pays much more for the same capability. When the level
/// lives inside a window-flags word, changing it goes through the flags setter, which
/// reparents and therefore HIDES the widget — the caller has to show it again — and on
/// one platform backend the native window is destroyed and recreated, because a
/// changed stacking bit is recorded as a recreation reason and the next show rebuilds
/// the window. Pinning a panel there costs the window's native handle, its mapped
/// state, and a visible flash. Here it costs one `set_window_level`.
fn window_level_changes(old: &[WindowSpec], new: &[WindowSpec]) -> Vec<(String, WindowLevel)> {
    window_axis_changes(old, new, |spec| spec.level)
}

/// R1088 §5.16 §5.41 §2 #7 PR-31 — is this `WindowEvent::Moved` the echo of
/// a position the shell just commanded via `set_outer_position`? `true`
/// when `commanded` is set and equals the incoming `logical` position
/// within 1px.
///
/// R1091: the 1px tolerance is NOT for logical<->physical rounding — that
/// round-trip is EXACT for an integer-logical command (`round(round(L*s)/s)
/// == L` for every integer `L` across DPI scales). It absorbs **window-
/// manager placement imprecision**: `set_outer_position` is a REQUEST a WM
/// may honour off-by-a-pixel (edge-snapping, decoration insets, tiling
/// quantization), so the OS `Moved` echo can land 1px off our command
/// without being a user drag — and treating that as a user move would
/// re-write the signal and risk a command/echo nudge loop. Cost: a
/// deliberate 1px user nudge of a just-commanded window is misclassified as
/// an echo and swallowed, which is below intent resolution and acceptable.
///
/// The pure core of [`AppShell::note_window_moved`]'s echo suppression,
/// split out so the own-command-vs-user-drag classification is unit-testable
/// without a live winit event loop (the `Moved` delivery + `set_outer_position`
/// effect stay HW-gated, like every real-window behaviour).
fn moved_is_command_echo(commanded: Option<(i32, i32)>, logical: (i32, i32)) -> bool {
    matches!(
        commanded,
        Some((cx, cy)) if (logical.0 - cx).abs() <= 1 && (logical.1 - cy).abs() <= 1
    )
}

/// R1088/R1091 §5.16 §5.41 §2 #7 PR-31 — the pure write-back core of
/// [`AppShell::note_window_moved`]: given the current declared `specs`, the
/// moved window's `spec_id`, and its new `logical` position, return the
/// updated `specs` to emit, or `None` to skip.
///
/// Conservative scope: only a window that ALREADY declares a position is
/// written (a `None` WM-placed window — the typical `"main"` — is left
/// WM-managed; one user drag must not silently pin it). A missing id, an
/// id whose spec has no declared position, or an unchanged position all
/// return `None` (no emit). Split out (R1091, mirroring `moved_is_command_echo`
/// and `window_placement_moves`) so the conservative-scope filter + the
/// idempotency skip are unit-tested without a live `Moved` event — the OS
/// delivery and `signal.set` effect are the only HW-gated parts.
fn user_move_writeback(
    mut specs: Vec<WindowSpec>,
    spec_id: &str,
    logical: (i32, i32),
    physical: (i32, i32),
    topology: &DisplayTopology,
) -> Option<Vec<WindowSpec>> {
    let spec = specs
        .iter_mut()
        .find(|s| s.id.as_ref() == spec_id && s.position.is_some())?;
    // R1576 — a spec that measures its position from a display must have the
    // write-back stated in the SAME frame. Overwriting a display-relative
    // offset with an absolute desktop coordinate would silently change what
    // the declaration MEANS: the preset would still read "40 in from that
    // monitor" and would now be pointing at the desktop's origin. So the
    // absolute reading is kept for an absolute spec and the relative one is
    // re-derived for a relative spec, off the EXACT physical position winit
    // reported rather than the per-window logical rounding of it.
    let Some(declared) = spec.display.clone() else {
        if spec.position == Some(logical) {
            return None;
        }
        spec.position = Some(logical);
        return Some(specs);
    };
    // Dragging a window onto another monitor re-declares its display: the user
    // moved it there, and the loop that keeps the declaration true of the world
    // is the whole reason this write-back exists. A window dragged onto NO
    // display keeps its declared one, so the offset simply goes negative or
    // past the edge — which is the honest reading of where it is, and is
    // recoverable, where re-pointing it at a fallback would quietly lose the
    // monitor the user chose.
    let host = topology
        .display_at(physical.0, physical.1)
        .or_else(|| topology.get(&declared))?;
    let scale = host.scale_factor();
    let to_logical = |v: i64| -> i32 {
        let scaled = ratio_i64(v, scale);
        i32::try_from(scaled).unwrap_or(i32::MAX)
    };
    let offset = (
        to_logical(i64::from(physical.0) - host.bounds().left()),
        to_logical(i64::from(physical.1) - host.bounds().top()),
    );
    let host_id = host.id().clone();
    if spec.position == Some(offset) && spec.display.as_ref() == Some(&host_id) {
        return None;
    }
    spec.position = Some(offset);
    spec.display = Some(host_id);
    Some(specs)
}

/// Divide a physical pixel distance by a scale factor, rounding to the nearest
/// logical pixel. Split out so the two `as` conversions carry one exemption
/// with one reason rather than four.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "a desktop pixel distance is far below 2^53, and the quotient is clamped into i64's range before the conversion"
)]
fn ratio_i64(v: i64, scale: f64) -> i64 {
    if !scale.is_finite() || scale <= 0.0 {
        return v;
    }
    let q = (v as f64 / scale).round();
    if q.is_finite() {
        q.clamp(-(2f64.powi(62)), 2f64.powi(62)) as i64
    } else {
        0
    }
}

#[cfg(test)]
mod r1087_window_position_move_diff_tests {
    //! R1087 §5.16 §5.41 PR-31, widened R1576 — the pure
    //! `window_placement_moves` diff that drives `reconcile_windows`'s move
    //! pass. Forcing consumer for the move logic (the live drag-follow runtime
    //! consumer is the next PR-31 slice) per the test-as-forcing-consumer
    //! discipline.
    use pinion_core::display::DisplayId;

    use super::{WindowSpec, window_placement_moves};
    use crate::{SizeStrategy, WindowPlacement};

    /// An ABSOLUTE placement — what every pre-R1576 spec declares.
    fn abs(x: i32, y: i32) -> WindowPlacement {
        WindowPlacement {
            display: None,
            offset: (x, y),
        }
    }

    /// A placement measured from a named display.
    fn on(display: &str, x: i32, y: i32) -> WindowPlacement {
        WindowPlacement {
            display: Some(DisplayId::new(display)),
            offset: (x, y),
        }
    }

    fn fixed(id: &'static str) -> WindowSpec {
        WindowSpec::new(
            id,
            id,
            SizeStrategy::Fixed {
                width: 100,
                height: 100,
            },
        )
    }

    #[test]
    fn unchanged_specs_yield_no_moves() {
        let a = vec![fixed("main"), fixed("torn-x").with_position(40, 50)];
        // Same lists → the idempotent fast-path never reaches the move
        // pass, but the diff itself must also report nothing.
        assert_eq!(window_placement_moves(&a, &a), Vec::new());
    }

    #[test]
    fn same_id_position_change_is_a_move() {
        let old = vec![fixed("main"), fixed("torn-x").with_position(40, 50)];
        let new = vec![fixed("main"), fixed("torn-x").with_position(120, 80)];
        assert_eq!(
            window_placement_moves(&old, &new),
            vec![("torn-x".to_owned(), abs(120, 80))]
        );
    }

    #[test]
    fn first_declared_position_on_existing_window_is_a_move() {
        // Window existed WM-placed (None); binding now pins a position.
        let old = vec![fixed("main"), fixed("torn-x")];
        let new = vec![fixed("main"), fixed("torn-x").with_position(10, 10)];
        assert_eq!(
            window_placement_moves(&old, &new),
            vec![("torn-x".to_owned(), abs(10, 10))]
        );
    }

    #[test]
    fn add_with_position_is_not_a_move() {
        // `torn-x` is only in `new` → an ADD (resume_spec places it), not
        // a move. The move pass must leave it to the add pass.
        let old = vec![fixed("main")];
        let new = vec![fixed("main"), fixed("torn-x").with_position(10, 10)];
        assert_eq!(window_placement_moves(&old, &new), Vec::new());
    }

    #[test]
    fn dropping_back_to_none_is_not_a_move() {
        // A re-declared `None` cannot un-position a live window
        // (set_outer_position has no "hand back to WM" form), so it is
        // deliberately not reported as a move.
        let old = vec![fixed("torn-x").with_position(40, 50)];
        let new = vec![fixed("torn-x")];
        assert_eq!(window_placement_moves(&old, &new), Vec::new());
    }

    #[test]
    fn multiple_simultaneous_moves_all_reported_in_new_order() {
        let old = vec![
            fixed("a").with_position(0, 0),
            fixed("b").with_position(0, 0),
            fixed("c").with_position(5, 5),
        ];
        let new = vec![
            fixed("a").with_position(1, 1),
            fixed("b").with_position(2, 2),
            fixed("c").with_position(5, 5), // unchanged
        ];
        assert_eq!(
            window_placement_moves(&old, &new),
            vec![("a".to_owned(), abs(1, 1)), ("b".to_owned(), abs(2, 2))]
        );
    }

    // ── R1576 — the diff compares the whole PLACEMENT, so the display ──
    //    is a first-class part of "where this window is declared to be".

    #[test]
    fn r1576_re_declaring_only_the_display_is_a_move() {
        // ★The signal write that sends a torn-off panel to the other monitor.
        // Comparing only the offset would make it a silent no-op — the R1319
        // defect exactly, a declared axis the reconcile pass cannot see.
        let old = vec![
            fixed("torn-x")
                .with_position(40, 50)
                .with_display(DisplayId::new("left")),
        ];
        let new = vec![
            fixed("torn-x")
                .with_position(40, 50)
                .with_display(DisplayId::new("right")),
        ];
        assert_eq!(
            window_placement_moves(&old, &new),
            vec![("torn-x".to_owned(), on("right", 40, 50))]
        );
    }

    #[test]
    fn r1576_adopting_a_display_changes_what_the_same_offset_means_so_it_is_a_move() {
        // `(40, 50)` on both sides, but one is a desktop coordinate and the
        // other is an offset into a monitor. Same numbers, different place.
        let old = vec![fixed("torn-x").with_position(40, 50)];
        let new = vec![
            fixed("torn-x")
                .with_position(40, 50)
                .with_display(DisplayId::new("right")),
        ];
        assert_eq!(
            window_placement_moves(&old, &new),
            vec![("torn-x".to_owned(), on("right", 40, 50))]
        );
    }

    #[test]
    fn r1576_a_display_with_no_position_declares_that_displays_corner() {
        // "Open it on that monitor" is a complete declaration, and it is a move
        // for a window that was WM-placed before.
        let old = vec![fixed("torn-x")];
        let new = vec![fixed("torn-x").with_display(DisplayId::new("right"))];
        assert_eq!(
            window_placement_moves(&old, &new),
            vec![("torn-x".to_owned(), on("right", 0, 0))]
        );
    }

    #[test]
    fn r1576_an_unchanged_display_relative_spec_is_not_a_move() {
        let a = vec![
            fixed("torn-x")
                .with_position(40, 50)
                .with_display(DisplayId::new("right")),
        ];
        assert_eq!(window_placement_moves(&a, &a), Vec::new());
    }

    // ── R1576 — what is COMMANDED, against a desk this machine does ────
    //    not have. On a one-monitor 1x desk the two frames produce the
    //    same numbers, so these are the only tests that can tell an
    //    implementation ignoring the display from one honouring it.

    use pinion_core::display::{DisplayInfo, DisplayRect, DisplayTopology};

    use super::{PlacementCommand, placement_command};

    /// A 1x panel with a 2x panel to its right — the arrangement where a
    /// logical command and a physical one disagree twice over.
    fn mixed_dpi() -> DisplayTopology {
        DisplayTopology::new(vec![
            DisplayInfo::new("left", DisplayRect::new(0, 0, 1000, 1000)).as_primary(),
            DisplayInfo::new("right", DisplayRect::new(1000, 0, 2000, 2000)).with_scale(2.0),
        ])
    }

    #[test]
    fn r1576_an_absolute_placement_is_commanded_in_logical_pixels_as_it_always_was() {
        let (anchored, command, latch) = placement_command(&abs(40, 50), &mixed_dpi(), 1.0);
        assert_eq!(command, Some(PlacementCommand::Logical(40, 50)));
        assert_eq!(latch, (40, 50), "and latched in the units it was commanded");
        assert_eq!(
            anchored.name(),
            "on_declared",
            "it landed on the left panel"
        );
    }

    #[test]
    fn r1576_a_display_relative_placement_is_commanded_at_that_displays_own_scale() {
        // 40 LOGICAL pixels into a 2x panel that starts at x = 1000 is
        // (1000 + 80, 0 + 80) PHYSICAL — not (1040, 40), and not 40 scaled by
        // the WINDOW's own factor, which is what winit would do with a logical
        // command and is the guess this axis exists to retire.
        let (anchored, command, latch) = placement_command(
            &on("right", 40, 40),
            &mixed_dpi(),
            // The window is still on the 1x panel as this is issued, which is
            // exactly when the window's own scale is the wrong number to use.
            1.0,
        );
        assert_eq!(command, Some(PlacementCommand::Physical(1080, 80)));
        assert_eq!(anchored.at(), Some((1080, 80)));
        assert_eq!(latch, (1080, 80), "latched through the WINDOW's scale of 1");
    }

    #[test]
    fn r1576_the_latch_is_logical_because_the_echo_check_is() {
        // Same command, a window already on the 2x panel: the physical point is
        // unchanged and the latch halves, because `note_window_moved` compares
        // against `Moved`'s logical reading through that window's scale.
        let (_, command, latch) = placement_command(&on("right", 40, 40), &mixed_dpi(), 2.0);
        assert_eq!(command, Some(PlacementCommand::Physical(1080, 80)));
        assert_eq!(latch, (540, 40));
    }

    #[test]
    fn r1576_a_headless_desk_commands_nothing_rather_than_inventing_a_position() {
        let (anchored, command, latch) =
            placement_command(&on("right", 40, 40), &DisplayTopology::empty(), 1.0);
        assert_eq!(command, None);
        assert_eq!(anchored.name(), "no_display");
        assert_eq!(latch, (40, 40), "the latch degrades to the declared offset");
        // An ABSOLUTE placement still commands: it named no display, so a
        // headless desk takes nothing away from it.
        let (_, command, _) = placement_command(&abs(40, 50), &DisplayTopology::empty(), 1.0);
        assert_eq!(command, Some(PlacementCommand::Logical(40, 50)));
    }

    #[test]
    fn r1576_a_vanished_display_is_commanded_on_the_fallback_and_named() {
        let (anchored, command, _) = placement_command(&on("gone", 40, 40), &mixed_dpi(), 1.0);
        assert_eq!(anchored.name(), "substituted");
        assert_eq!(
            command,
            Some(PlacementCommand::Physical(40, 40)),
            "on the fallback (the 1x primary), so the window is reachable"
        );
    }
}

#[cfg(test)]
mod r1319_window_title_change_diff_tests {
    //! R1319 §5.16 §5.41 PR-52 — the pure `window_title_changes` diff that drives
    //! `reconcile_windows`'s title pass, the twin of R1087's move pass. Pre-R1319
    //! `title` was create-time-only: a binding could not rename a live window, so the
    //! tmux convention (the OS title follows the focused pane, renamed by its child on
    //! every prompt) was unreachable. The live `set_title` apply is HW-gated; this pins
    //! the logic that decides WHICH window gets renamed to WHAT.
    use super::{WindowSpec, window_title_changes};
    use crate::SizeStrategy;

    fn titled(id: &'static str, title: &str) -> WindowSpec {
        WindowSpec::new(
            id,
            title,
            SizeStrategy::Fixed {
                width: 100,
                height: 100,
            },
        )
    }

    #[test]
    fn unchanged_specs_yield_no_title_changes() {
        let a = vec![titled("main", "editor"), titled("torn-x", "console")];
        assert_eq!(window_title_changes(&a, &a), Vec::new());
    }

    #[test]
    fn same_id_title_change_is_reported() {
        // ★The PR-52 gesture: the pane's child renamed itself, so the window
        // hosting it must follow — same window id, new title.
        let old = vec![titled("main", "editor"), titled("torn-x", "console")];
        let new = vec![titled("main", "editor"), titled("torn-x", "vim README")];
        assert_eq!(
            window_title_changes(&old, &new),
            vec![("torn-x".to_owned(), "vim README")]
        );
    }

    #[test]
    fn an_added_window_is_not_a_title_change() {
        // An id only in `new` is an ADD — `resume_spec` applies its title via
        // `Window::with_title` at create; re-applying it here would be redundant
        // (and, for a window whose creation failed, a lookup miss).
        let old = vec![titled("main", "editor")];
        let new = vec![titled("main", "editor"), titled("torn-x", "console")];
        assert_eq!(window_title_changes(&old, &new), Vec::new());
    }

    #[test]
    fn a_dropped_window_is_not_a_title_change() {
        let old = vec![titled("main", "editor"), titled("torn-x", "console")];
        let new = vec![titled("main", "editor")];
        assert_eq!(window_title_changes(&old, &new), Vec::new());
    }

    #[test]
    fn a_retitled_window_that_also_moved_is_reported_by_both_passes() {
        // The two passes are ORTHOGONAL — a floating pane that is dragged AND
        // renamed in the same reconcile must get both applies (a shared "the spec
        // changed" pass would have to pick one).
        use super::window_placement_moves;
        use crate::WindowPlacement;
        let old = vec![titled("torn-x", "console").with_position(10, 10)];
        let new = vec![titled("torn-x", "vim README").with_position(40, 50)];
        assert_eq!(
            window_title_changes(&old, &new),
            vec![("torn-x".to_owned(), "vim README")]
        );
        assert_eq!(
            window_placement_moves(&old, &new),
            vec![(
                "torn-x".to_owned(),
                WindowPlacement {
                    display: None,
                    offset: (40, 50)
                }
            )]
        );
    }

    #[test]
    fn multiple_simultaneous_retitles_all_reported_in_new_order() {
        let old = vec![titled("a", "one"), titled("b", "two"), titled("c", "three")];
        let new = vec![
            titled("a", "ONE"),
            titled("b", "two"), // unchanged
            titled("c", "THREE"),
        ];
        assert_eq!(
            window_title_changes(&old, &new),
            vec![("a".to_owned(), "ONE"), ("c".to_owned(), "THREE")]
        );
    }

    #[test]
    fn a_user_move_writeback_does_not_swallow_a_pending_retitle() {
        // ★R1320 — the clobber. `note_window_moved` acknowledges a user drag in the
        // reconcile cache; pre-R1320 it overwrote the WHOLE cache with the signal's
        // current value, so a title written but not yet reconciled was marked "already
        // applied" and never reached the OS window.
        //
        // Sequence: the binding retitles a floating pane (signal write, reconcile not
        // drained yet) → the user drags that window (WM move → write-back). The cache
        // must acknowledge ONLY the position, leaving the title still diffing.
        use super::{cache_moved_position, window_placement_moves};
        let cached_before = vec![titled("torn-x", "console").with_position(10, 10)];
        // The signal now carries BOTH the new title and (after the drag) the new
        // position; `last_known_specs` is still the pre-retitle snapshot.
        let mut cache = cached_before.clone();
        cache_moved_position(&mut cache, "torn-x", (40, 50));
        let signal_now = vec![titled("torn-x", "vim README").with_position(40, 50)];
        assert_eq!(
            window_placement_moves(&cache, &signal_now),
            Vec::new(),
            "the user's own drag is acknowledged — no redundant set_outer_position",
        );
        assert_eq!(
            window_title_changes(&cache, &signal_now),
            vec![("torn-x".to_owned(), "vim README")],
            "★…but the pending retitle SURVIVES the acknowledgement and still applies",
        );
    }

    #[test]
    fn decoration_changes_mirror_title_changes() {
        // R1320 — `decorations` is an APPLY axis now (winit HAS `set_decorations`; the
        // R1118 warn cited a limit that does not exist). Same diff shape as the title.
        use super::window_decoration_changes;
        let old = vec![titled("main", "editor")];
        let new = vec![titled("main", "editor").with_decorations(false)];
        assert_eq!(
            window_decoration_changes(&old, &new),
            vec![("main".to_owned(), false)],
        );
        assert_eq!(window_decoration_changes(&old, &old), Vec::new());
        // An ADD is not a change — `resume_spec`'s `with_decorations` applies at create.
        let added = vec![
            titled("main", "editor"),
            titled("torn-x", "pane").with_decorations(false),
        ];
        assert_eq!(window_decoration_changes(&old, &added), Vec::new());
    }

    #[test]
    fn r1610_level_changes_mirror_the_other_live_axes() {
        // The third member of the live-axis family, and the reason it exists as
        // a family: all three are now `window_axis_changes` with a different
        // projection, so the ADD / DROP / unchanged rules cannot be right in one
        // and wrong in another.
        use super::window_level_changes;
        use pinion_core::window_level::WindowLevel;
        let old = vec![titled("main", "editor")];
        let new = vec![titled("main", "editor").with_level(WindowLevel::AlwaysOnTop)];
        assert_eq!(
            window_level_changes(&old, &new),
            vec![("main".to_owned(), WindowLevel::AlwaysOnTop)],
        );
        assert_eq!(window_level_changes(&old, &old), Vec::new());
        // An ADD is not a change — `resume_spec`'s `with_window_level` applies
        // it at create.
        let added = vec![
            titled("main", "editor"),
            titled("torn-x", "pane").with_level(WindowLevel::AlwaysOnTop),
        ];
        assert_eq!(window_level_changes(&old, &added), Vec::new());
        // A DROP is not a change either.
        assert_eq!(window_level_changes(&added, &old), Vec::new());
        // And going BACK to Normal is a change like any other — a user
        // un-pinning a panel must reach the window manager.
        let pinned = vec![titled("main", "editor").with_level(WindowLevel::AlwaysOnTop)];
        assert_eq!(
            window_level_changes(&pinned, &old),
            vec![("main".to_owned(), WindowLevel::Normal)],
        );
    }

    #[test]
    fn r1610_the_three_live_axes_are_orthogonal() {
        // A window pinned on top, renamed and undecorated in ONE reconcile must
        // get all three applies. This is the property that makes them separate
        // passes rather than one "the spec changed" pass — and now that they
        // share a derivation, the property is what says the sharing did not
        // merge them.
        use super::{window_decoration_changes, window_level_changes};
        use pinion_core::window_level::WindowLevel;
        let old = vec![titled("panel", "KPI")];
        let new = vec![
            titled("panel", "KPI (pinned)")
                .with_decorations(false)
                .with_level(WindowLevel::AlwaysOnTop),
        ];
        assert_eq!(
            window_title_changes(&old, &new),
            vec![("panel".to_owned(), "KPI (pinned)")],
        );
        assert_eq!(
            window_decoration_changes(&old, &new),
            vec![("panel".to_owned(), false)],
        );
        assert_eq!(
            window_level_changes(&old, &new),
            vec![("panel".to_owned(), WindowLevel::AlwaysOnTop)],
        );
    }

    #[test]
    fn r1610_a_change_on_one_axis_is_not_a_change_on_another() {
        // The mirror image of the test above, and the one that would catch a
        // lift that dropped its projection argument: retitling a window must
        // leave the level pass with nothing to do.
        use super::{window_decoration_changes, window_level_changes};
        let old = vec![titled("panel", "KPI")];
        let new = vec![titled("panel", "KPI 2")];
        assert!(!window_title_changes(&old, &new).is_empty());
        assert_eq!(window_level_changes(&old, &new), Vec::new());
        assert_eq!(window_decoration_changes(&old, &new), Vec::new());
    }

    #[test]
    fn r1610_every_pinion_level_maps_to_its_winit_twin() {
        // The one place the two vocabularies meet. A wrong arm here would pin a
        // window to the BOTTOM, which no unit test above could see because
        // every one of them stops at the pinion enum.
        use super::{WinitWindowLevel, winit_window_level};
        use pinion_core::window_level::WindowLevel;
        assert_eq!(
            winit_window_level(WindowLevel::AlwaysOnBottom),
            WinitWindowLevel::AlwaysOnBottom,
        );
        assert_eq!(
            winit_window_level(WindowLevel::Normal),
            WinitWindowLevel::Normal,
        );
        assert_eq!(
            winit_window_level(WindowLevel::AlwaysOnTop),
            WinitWindowLevel::AlwaysOnTop,
        );
        // The map is injective — collapsing two levels onto one winit arm would
        // make "on top" and "on bottom" the same window.
        let mapped: Vec<WinitWindowLevel> = WindowLevel::ALL
            .into_iter()
            .map(winit_window_level)
            .collect();
        for (i, a) in mapped.iter().enumerate() {
            for b in &mapped[i + 1..] {
                assert_ne!(a, b, "two pinion levels must not share a winit level");
            }
        }
    }

    #[test]
    fn an_emptied_title_is_still_a_change() {
        // Unlike `position` (whose `None` means "leave it to the WM" and is NOT a
        // move — `set_outer_position` cannot un-place a window), a title has no
        // "unset": an empty string is a legal title and the OS must be told.
        let old = vec![titled("main", "editor")];
        let new = vec![titled("main", "")];
        assert_eq!(
            window_title_changes(&old, &new),
            vec![("main".to_owned(), "")]
        );
    }
}

#[cfg(test)]
mod r1088_moved_echo_tests {
    //! R1088 §5.16 §5.41 §2 #7 PR-31 — the pure echo-vs-user-move
    //! classification (`moved_is_command_echo`) + the write-back core
    //! (`user_move_writeback`, R1091) that together gate
    //! `note_window_moved`. The live `Moved` delivery is HW-gated; these are
    //! the testable decision cores.
    use pinion_core::display::{DisplayId, DisplayInfo, DisplayRect, DisplayTopology};

    use super::{WindowSpec, moved_is_command_echo, user_move_writeback};
    use crate::SizeStrategy;

    /// A desk with no monitors — what an absolute-placement spec is resolved
    /// against, because it never consults the topology at all.
    fn nodesk() -> DisplayTopology {
        DisplayTopology::empty()
    }

    /// Two 1000x1000 panels side by side, the left one primary.
    fn two_panels() -> DisplayTopology {
        DisplayTopology::new(vec![
            DisplayInfo::new("left", DisplayRect::new(0, 0, 1000, 1000)).as_primary(),
            DisplayInfo::new("right", DisplayRect::new(1000, 0, 1000, 1000)),
        ])
    }

    fn spec(id: &'static str, pos: Option<(i32, i32)>) -> WindowSpec {
        let s = WindowSpec::new(
            id,
            id,
            SizeStrategy::Fixed {
                width: 100,
                height: 100,
            },
        );
        match pos {
            Some((x, y)) => s.with_position(x, y),
            None => s,
        }
    }

    #[test]
    fn no_command_is_a_user_move() {
        // Latch empty (a WM-placed window the shell never commanded) →
        // never an echo; every Moved is a user/WM move.
        assert!(!moved_is_command_echo(None, (100, 80)));
    }

    #[test]
    fn exact_match_is_an_echo() {
        assert!(moved_is_command_echo(Some((120, 90)), (120, 90)));
    }

    #[test]
    fn one_pixel_off_is_still_an_echo() {
        // R1091: a WM may honour set_outer_position 1px off (snap / inset);
        // that is still our own command echoing, not a user drag.
        assert!(moved_is_command_echo(Some((120, 90)), (121, 89)));
        assert!(moved_is_command_echo(Some((120, 90)), (119, 91)));
    }

    #[test]
    fn two_pixels_off_is_a_user_move() {
        // Beyond the WM-imprecision tolerance → a genuine user/WM drag.
        assert!(!moved_is_command_echo(Some((120, 90)), (122, 90)));
        assert!(!moved_is_command_echo(Some((120, 90)), (120, 88)));
    }

    #[test]
    fn divergent_position_is_a_user_move() {
        assert!(!moved_is_command_echo(Some((120, 90)), (400, 300)));
    }

    // ── user_move_writeback (R1091 S2) — the conservative-scope + ──────
    //    idempotency write-back core, previously untested.

    #[test]
    fn writeback_writes_an_already_positioned_window() {
        let specs = vec![spec("main", None), spec("torn-x", Some((40, 50)))];
        let out = user_move_writeback(specs, "torn-x", (200, 130), (200, 130), &nodesk())
            .expect("a positioned window writes");
        let torn = out.iter().find(|s| s.id.as_ref() == "torn-x").unwrap();
        assert_eq!(torn.position, Some((200, 130)));
        // The other window is untouched.
        assert_eq!(
            out.iter()
                .find(|s| s.id.as_ref() == "main")
                .unwrap()
                .position,
            None
        );
    }

    #[test]
    fn writeback_skips_a_wm_placed_none_window() {
        // Conservative scope: a None (WM-placed) window must NOT be pinned by
        // one user drag → None (no emit).
        let specs = vec![spec("main", None)];
        assert!(user_move_writeback(specs, "main", (300, 220), (300, 220), &nodesk()).is_none());
    }

    #[test]
    fn writeback_skips_unchanged_position() {
        // Idempotent: already at the moved position → None (no churn).
        let specs = vec![spec("torn-x", Some((40, 50)))];
        assert!(user_move_writeback(specs, "torn-x", (40, 50), (40, 50), &nodesk()).is_none());
    }

    #[test]
    fn writeback_skips_missing_id() {
        let specs = vec![spec("torn-x", Some((40, 50)))];
        assert!(user_move_writeback(specs, "ghost", (10, 10), (10, 10), &nodesk()).is_none());
    }

    // ── R1576 — the write-back keeps a display-relative declaration ────
    //    RELATIVE. Overwriting the offset with an absolute desktop
    //    coordinate would leave the preset reading "40 in from that
    //    monitor" while pointing at the desktop's origin.

    #[test]
    fn r1576_a_display_relative_window_writes_back_an_offset_not_a_desktop_point() {
        let specs = vec![spec("torn-x", Some((40, 50))).with_display(DisplayId::new("right"))];
        // The user dragged it to (1600, 300) on the desktop — 600 in from the
        // right panel's corner.
        let out = user_move_writeback(specs, "torn-x", (1600, 300), (1600, 300), &two_panels())
            .expect("a positioned window writes");
        let torn = out.iter().find(|s| s.id.as_ref() == "torn-x").unwrap();
        assert_eq!(torn.position, Some((600, 300)), "relative to its display");
        assert_eq!(torn.display.as_ref().map(DisplayId::as_str), Some("right"));
    }

    #[test]
    fn r1576_dragging_onto_another_monitor_re_declares_the_display() {
        // ★The loop that keeps the declaration true of the world: the user
        // moved the window to the other panel, so the preset now says so.
        let specs = vec![spec("torn-x", Some((600, 300))).with_display(DisplayId::new("right"))];
        let out = user_move_writeback(specs, "torn-x", (120, 300), (120, 300), &two_panels())
            .expect("a cross-monitor drag writes");
        let torn = out.iter().find(|s| s.id.as_ref() == "torn-x").unwrap();
        assert_eq!(torn.display.as_ref().map(DisplayId::as_str), Some("left"));
        assert_eq!(torn.position, Some((120, 300)));
    }

    #[test]
    fn r1576_a_relative_offset_is_logical_so_a_hidpi_panel_halves_it() {
        let desk = DisplayTopology::new(vec![
            DisplayInfo::new("hidpi", DisplayRect::new(0, 0, 2000, 2000))
                .with_scale(2.0)
                .as_primary(),
        ]);
        let specs = vec![spec("torn-x", Some((0, 0))).with_display(DisplayId::new("hidpi"))];
        // 200 PHYSICAL pixels in on a 2x panel is 100 LOGICAL pixels in.
        let out = user_move_writeback(specs, "torn-x", (200, 200), (200, 200), &desk)
            .expect("a positioned window writes");
        let torn = out.iter().find(|s| s.id.as_ref() == "torn-x").unwrap();
        assert_eq!(torn.position, Some((100, 100)));
    }

    #[test]
    fn r1576_a_window_dragged_off_every_monitor_keeps_the_display_it_was_told() {
        // Re-pointing it at a fallback would quietly lose the monitor the user
        // chose; a negative offset is the honest reading of where it is, and it
        // is recoverable.
        let specs = vec![spec("torn-x", Some((40, 50))).with_display(DisplayId::new("right"))];
        let out = user_move_writeback(specs, "torn-x", (2500, 40), (2500, 40), &two_panels())
            .expect("a positioned window writes");
        let torn = out.iter().find(|s| s.id.as_ref() == "torn-x").unwrap();
        assert_eq!(torn.display.as_ref().map(DisplayId::as_str), Some("right"));
        assert_eq!(torn.position, Some((1500, 40)));
    }

    #[test]
    fn r1576_a_relative_writeback_is_idempotent() {
        let specs = vec![spec("torn-x", Some((600, 300))).with_display(DisplayId::new("right"))];
        assert!(
            user_move_writeback(specs, "torn-x", (1600, 300), (1600, 300), &two_panels()).is_none(),
            "already there in BOTH the display and the offset"
        );
    }
}

#[cfg(test)]
mod r56_2_a_winit_ime_mapping_tests {
    //! R56.2.a §5.13 §5.38 — `winit_ime_to_composition` mapping
    //! regression. Drives the function table (winit `Ime` × shell
    //! `was_composing` → dispatched `CompositionEvent` sequence +
    //! next state) so the bridge between winit's four-variant IME
    //! abstraction and the W3C-aligned pinion-native enum is pinned
    //! end-to-end without needing a real winit `EventLoop`.

    use super::winit_ime_to_composition;
    use pinion_core::CompositionEvent;
    use winit::event::Ime;

    #[test]
    fn r56_2_a_enabled_is_no_op() {
        let (events, next) = winit_ime_to_composition(&Ime::Enabled, false);
        assert!(events.is_empty(), "Enabled is informational, no dispatch");
        assert!(!next, "Enabled leaves was_composing unchanged");

        let (events, next) = winit_ime_to_composition(&Ime::Enabled, true);
        assert!(events.is_empty());
        assert!(next, "Enabled mid-session does not reset was_composing");
    }

    #[test]
    fn r56_2_a_first_nonempty_preedit_dispatches_start_then_update() {
        let (events, next) =
            winit_ime_to_composition(&Ime::Preedit("ha".to_owned(), Some((2, 2))), false);
        assert_eq!(
            events,
            vec![
                CompositionEvent::Start,
                CompositionEvent::Update("ha".to_owned())
            ],
        );
        assert!(next, "first non-empty preedit opens a composition session");
    }

    #[test]
    fn r56_2_a_subsequent_nonempty_preedit_dispatches_update_only() {
        let (events, next) =
            winit_ime_to_composition(&Ime::Preedit("han".to_owned(), Some((3, 3))), true);
        assert_eq!(events, vec![CompositionEvent::Update("han".to_owned())]);
        assert!(next, "subsequent preedit keeps the session open");
    }

    #[test]
    fn r56_2_a_empty_preedit_while_composing_is_update_empty_not_cancel() {
        // The synthetic-clear winit injects right before every
        // Commit. Treating it as Cancel would fire a spurious
        // Cancel+Commit pair on every pinyin / Hangul commit; the
        // substrate stays composing via `Update("")` so the
        // immediately-following Commit lands at the caret with
        // `was_composing` still true.
        let (events, next) = winit_ime_to_composition(&Ime::Preedit(String::new(), None), true);
        assert_eq!(events, vec![CompositionEvent::Update(String::new())]);
        assert!(next, "empty preedit during session keeps was_composing");
    }

    #[test]
    fn r56_2_a_empty_preedit_while_idle_is_idempotent_no_op() {
        let (events, next) = winit_ime_to_composition(&Ime::Preedit(String::new(), None), false);
        assert!(events.is_empty());
        assert!(!next);
    }

    #[test]
    fn r56_2_a_commit_during_session_dispatches_commit_and_closes_session() {
        // Pinyin / Hangul canonical sequence: …Preedit("han") →
        // Preedit("", None) [synthetic clear, dispatched as
        // Update("")] → Commit("\u{D55C}") lands here with
        // was_composing=true.
        let (events, next) = winit_ime_to_composition(&Ime::Commit("\u{D55C}".to_owned()), true);
        assert_eq!(
            events,
            vec![CompositionEvent::Commit("\u{D55C}".to_owned())]
        );
        assert!(!next, "Commit closes the session");
    }

    #[test]
    fn r56_2_a_commit_without_session_injects_synthetic_start() {
        // macOS dead-key sequences emit Commit without a prior
        // Preedit. Inject a synthetic Start so the substrate drives
        // through Focused → Editing and the `was_composing` gate
        // inside `apply_composition_commit` fires the
        // `text_committed` intent.
        let (events, next) = winit_ime_to_composition(&Ime::Commit("e\u{301}".to_owned()), false);
        assert_eq!(
            events,
            vec![
                CompositionEvent::Start,
                CompositionEvent::Commit("e\u{301}".to_owned()),
            ],
        );
        assert!(!next, "Commit always closes the session");
    }

    #[test]
    fn r56_2_a_disabled_mid_session_dispatches_cancel() {
        let (events, next) = winit_ime_to_composition(&Ime::Disabled, true);
        assert_eq!(events, vec![CompositionEvent::Cancel]);
        assert!(!next, "Disabled closes the session");
    }

    #[test]
    fn r56_2_a_disabled_while_idle_is_idempotent_no_op() {
        let (events, next) = winit_ime_to_composition(&Ime::Disabled, false);
        assert!(events.is_empty());
        assert!(!next);
    }

    #[test]
    fn r56_2_a_full_pinyin_sequence_round_trips() {
        // Canonical pinyin "啊不" commit sequence (winit docs example).
        let mut state = false;
        let mut collected = Vec::new();
        for ime in [
            Ime::Preedit("a".to_owned(), Some((1, 1))),
            Ime::Preedit("a b".to_owned(), Some((3, 3))),
            Ime::Preedit("a b".to_owned(), Some((1, 1))),
            Ime::Preedit("\u{554A}b".to_owned(), Some((3, 3))),
            Ime::Preedit(String::new(), None),
            Ime::Commit("\u{554A}\u{4E0D}".to_owned()),
        ] {
            let (events, next) = winit_ime_to_composition(&ime, state);
            collected.extend(events);
            state = next;
        }
        assert_eq!(
            collected,
            vec![
                CompositionEvent::Start,
                CompositionEvent::Update("a".to_owned()),
                CompositionEvent::Update("a b".to_owned()),
                CompositionEvent::Update("a b".to_owned()),
                CompositionEvent::Update("\u{554A}b".to_owned()),
                CompositionEvent::Update(String::new()),
                CompositionEvent::Commit("\u{554A}\u{4E0D}".to_owned()),
            ],
        );
        assert!(!state, "session closed after Commit");
    }

    #[test]
    fn r56_2_a_full_cancel_sequence_round_trips() {
        // User escapes mid-composition: IME sends Preedit("",None)
        // then Disabled (or just Preedit("",None) if the IME stays
        // active for the next character). The Update("") clears the
        // visual; the explicit Disabled then Cancel cleans up.
        let mut state = false;
        let mut collected = Vec::new();
        for ime in [
            Ime::Preedit("\u{1112}\u{1161}".to_owned(), Some((6, 6))),
            Ime::Preedit(String::new(), None),
            Ime::Disabled,
        ] {
            let (events, next) = winit_ime_to_composition(&ime, state);
            collected.extend(events);
            state = next;
        }
        assert_eq!(
            collected,
            vec![
                CompositionEvent::Start,
                CompositionEvent::Update("\u{1112}\u{1161}".to_owned()),
                CompositionEvent::Update(String::new()),
                CompositionEvent::Cancel,
            ],
        );
        assert!(!state);
    }
}

#[cfg(test)]
mod r1009_named_key_str_tests {
    //! R1009 §5.13 — `named_key_str` content-surface vocabulary regression.
    //! The winit `NamedKey` → W3C `KeyboardEvent.key` bridge is a pure table
    //! (no `EventLoop` needed, the `winit_ime_to_composition` precedent), so the
    //! editing + function keys a terminal forwards to its PTY are pinned here
    //! directly — the winit-path half the RPC `scene/key` plane bypasses.

    use super::named_key_str;
    use winit::keyboard::NamedKey;

    #[test]
    fn editing_keys_surface_their_w3c_names() {
        // R1009 — the forcing case: a content-surface widget (sprag's PTY pane)
        // must receive these; before R1009 they were dropped at the shell.
        assert_eq!(named_key_str(NamedKey::Backspace), Some("Backspace"));
        assert_eq!(named_key_str(NamedKey::Delete), Some("Delete"));
        assert_eq!(named_key_str(NamedKey::Insert), Some("Insert"));
    }

    #[test]
    fn function_row_f1_to_f12_surfaces() {
        let row = [
            (NamedKey::F1, "F1"),
            (NamedKey::F2, "F2"),
            (NamedKey::F3, "F3"),
            (NamedKey::F4, "F4"),
            (NamedKey::F5, "F5"),
            (NamedKey::F6, "F6"),
            (NamedKey::F7, "F7"),
            (NamedKey::F8, "F8"),
            (NamedKey::F9, "F9"),
            (NamedKey::F10, "F10"),
            (NamedKey::F11, "F11"),
            (NamedKey::F12, "F12"),
        ];
        for (key, name) in row {
            assert_eq!(named_key_str(key), Some(name), "{name} surfaces");
        }
    }

    #[test]
    fn navigation_and_activation_baseline_unchanged() {
        // The R51.37 baseline is byte-identical for existing widgets.
        assert_eq!(named_key_str(NamedKey::ArrowDown), Some("ArrowDown"));
        assert_eq!(named_key_str(NamedKey::Home), Some("Home"));
        assert_eq!(named_key_str(NamedKey::PageUp), Some("PageUp"));
        assert_eq!(named_key_str(NamedKey::Enter), Some("Enter"));
        assert_eq!(named_key_str(NamedKey::Space), Some("Space"));
    }

    #[test]
    fn escape_and_tab_stay_none_filtered_upstream() {
        // Escape / Tab are offered to the focused widget by the dedicated arms
        // in handle_key_press, so the CONTENT / chord bridge deliberately does
        // NOT surface them. R1073.1: the separate DISPATCH-gate vocabulary
        // (`dispatch_named_key_str`) DOES cover them — see
        // `dispatch_gate_vocabulary_adds_escape_tab` below.
        assert_eq!(named_key_str(NamedKey::Escape), None);
        assert_eq!(named_key_str(NamedKey::Tab), None);
    }

    #[test]
    fn device_control_keys_stay_none() {
        // The curation boundary: media / browser / launch keys have no widget
        // meaning, so they stay unsurfaced (the doc's premise, correct here).
        assert_eq!(named_key_str(NamedKey::MediaPlayPause), None);
        assert_eq!(named_key_str(NamedKey::BrowserBack), None);
        assert_eq!(named_key_str(NamedKey::AudioVolumeUp), None);
    }
}

#[cfg(test)]
mod r1073_dispatch_gate_vocabulary_tests {
    //! R1073.1 PR-27.4 §5.39 — `dispatch_named_key_str` is the press-owner
    //! gate's key vocabulary: a SUPERSET of the content/chord `named_key_str`
    //! that additionally covers the two shell-reserved keys `handle_key_press`
    //! dispatches (`Escape` → exit / modal-cancel, `Tab` → focus traverse), so
    //! the close-during-dispatch gate sees them — without widening the R1009
    //! content / RPC chord vocabulary. Pure table, no `EventLoop` needed.

    use super::{dispatch_named_key_str, named_key_str};
    use winit::keyboard::NamedKey;

    #[test]
    fn dispatch_gate_vocabulary_adds_escape_tab() {
        // The whole point of R1073.1: the gate vocabulary covers the reserved
        // keys the content vocabulary excludes, so a window-closing Escape gets
        // the same one-press-one-action gating as Enter.
        assert_eq!(dispatch_named_key_str(NamedKey::Escape), Some("Escape"));
        assert_eq!(dispatch_named_key_str(NamedKey::Tab), Some("Tab"));
        assert_eq!(
            named_key_str(NamedKey::Escape),
            None,
            "content vocab still excludes them"
        );
        assert_eq!(named_key_str(NamedKey::Tab), None);
    }

    #[test]
    fn dispatch_gate_vocabulary_is_a_superset_of_named_key_str() {
        // Every key the content vocabulary names, the gate vocabulary names
        // identically — the gate only ADDS, never diverges, on the shared keys.
        for key in [
            NamedKey::Enter,
            NamedKey::Space,
            NamedKey::ArrowLeft,
            NamedKey::Backspace,
            NamedKey::F5,
            NamedKey::PageDown,
        ] {
            assert_eq!(
                dispatch_named_key_str(key),
                named_key_str(key),
                "{key:?} shared"
            );
        }
    }

    #[test]
    fn non_dispatched_keys_stay_none_in_the_gate_vocabulary() {
        // Keys the shell does not dispatch are absent from the gate too, so the
        // `None` branch in `handle_keyboard_input` only ever covers keys that
        // would not dispatch anyway (gate result moot).
        assert_eq!(dispatch_named_key_str(NamedKey::MediaPlayPause), None);
        assert_eq!(dispatch_named_key_str(NamedKey::BrowserBack), None);
    }
}

/// (R1160 §5.16) Install the global `tracing` subscriber once, env-filtered by
/// `PINION_LOG` (per-target levels: `PINION_LOG=pinion::dock=debug` traces the
/// dock-drag decisions; default `warn` is quiet). Idempotent via `try_init` — a
/// second shell instance / a test that already set a subscriber is a no-op, not a
/// panic. Writes to stderr so it never corrupts the stdout JSON-RPC stream. The
/// permanent home for first-party diagnostics
/// ([[use-substrate-not-hand-rolled-equivalent]]) — no eprintln add/remove churn.
fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_env("PINION_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

/// R-PR47 §5.7 — startup configuration for [`run_with_config`], the
/// general GUI entry point that [`run`] and [`run_with_handlers`] are thin
/// wrappers over.
///
/// The two knobs are orthogonal:
///
/// - `handlers` — an optional [`HandlerRegistry`] whose presence installs
///   the async [`CommandExecutor`] (the [`run_with_handlers`] behaviour);
///   absent, no executor is installed (the [`run`] behaviour).
/// - `on_ingress` — an optional hook handed the winit-free
///   [`RpcIngress`] seam once the event loop
///   exists but before it starts blocking. This is where a consumer
///   mounts its own transport (e.g. the Unix-socket adapter in
///   `pinion-rpc-transport`) to get an always-on, execution-independent
///   RPC endpoint. The consumer owns whatever it spawns — including its
///   lifetime and its boot exposure (R-PR48), so runtime on/off is the
///   consumer toggling its own transport, with no framework-side toggle
///   mechanism. pinion never owns transport *policy*; it only exposes the
///   seam.
///
/// The built-in `stdin → stdout` transport is always installed regardless
/// of `on_ingress`, so the pre-PR47 pipe-driven workflow is unchanged.
/// R-PR47 §5.7 — the [`ShellConfig::on_rpc_ingress`] hook: a one-shot,
/// main-thread callback handed the winit-free ingress seam so a consumer
/// can mount its own transport before the loop starts.
type RpcIngressHook = Box<dyn FnOnce(Arc<dyn RpcIngress>)>;

#[derive(Default)]
pub struct ShellConfig {
    handlers: Option<HandlerRegistry>,
    on_ingress: Option<RpcIngressHook>,
    /// R1448 §5.36 — faces the application ships, in declaration order.
    app_fonts: Vec<Vec<u8>>,
    /// R1472 §5.36 — the family unset text resolves to; see
    /// [`ShellConfig::with_default_font_family`].
    default_font_family: Option<pinion_core::style::FontFamily>,
}

impl ShellConfig {
    /// A config with no async handlers and no injected transport —
    /// equivalent to bare [`run`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install an async [`CommandExecutor`] backed by `registry` at boot
    /// (the [`run_with_handlers`] behaviour).
    #[must_use]
    pub fn with_handlers(mut self, registry: HandlerRegistry) -> Self {
        self.handlers = Some(registry);
        self
    }

    /// R1448 §5.36 — declare a font the application ships, from memory.
    ///
    /// The toolkit's `addApplicationFont` / `…FromData`, called from `main()` before any widget exists. Here
    /// the "before any widget" part is structural rather than a rule to
    /// remember: the shell registers these into its render cache while it is
    /// being built, so a family declared this way is selectable by name — as
    /// [`TextStyle::with_font_family`](pinion_core::style::TextStyle::with_font_family) — from the
    /// binding's very first `view`.
    ///
    /// Call it once per face; declarations accumulate. The resulting families
    /// and the platform-scan verdict are published to the binding as
    /// [`FontSourceReport`](pinion_core::reactive::FontSourceReport), readable from a view fn
    /// via [`font_sources()`](pinion_core::reactive::font_sources()) — so an application
    /// can *render* its font state, and an agent can read it off `scene/snapshot`, which is
    /// what the toolkit's stderr `qWarning` cannot offer.
    ///
    /// This is the only way an application supplies a face, and that is
    /// deliberate: fonts declared before boot cannot make the published report
    /// stale, so the report is a snapshot rather than a signal.
    #[must_use]
    pub fn with_application_font(mut self, data: Vec<u8>) -> Self {
        self.app_fonts.push(data);
        self
    }

    /// R1472 §5.36 — the family text that names none of its own resolves to.
    ///
    /// The toolkit's `setFont`, and the other half of [`Self::with_application_font`]. Declaring a face makes
    /// it selectable *by name*; this makes it what the binding gets without
    /// naming it, so a view fn does not have to spell the family on every
    /// [`TextStyle`](pinion_core::style::TextStyle) it emits — which a toolkit
    /// application does either, and which no binding reliably remembers.
    ///
    /// The two are separate calls for the reason the toolkit keeps them
    /// separate: an application may ship several faces and default to one of
    /// them, or ship a face used only where it is named (an icon or code face)
    /// and leave the default alone. Folding the choice into the declaration
    /// would make the common case shorter and the other two unreachable.
    ///
    /// Without this, an application whose text is in a script the host has no
    /// face for renders nothing while holding the glyphs in memory — the state
    /// R1471 measured, where a Hangul view passed on a developer box and drew
    /// tofu on CI.
    ///
    /// Name a family that was declared here or that the host installs; a name
    /// nothing resolves to falls back exactly as a named
    /// [`TextStyle::font_family`](pinion_core::style::TextStyle) would, and
    /// the resolved value is published on
    /// [`FontSourceReport::default_family`](pinion_core::reactive::FontSourceReport)
    /// so the binding can render what it actually got.
    ///
    /// Takes the typed [`FontFamily`](pinion_core::style::FontFamily) rather
    /// than a string: whether a token is a family name or a CSS generic class
    /// is a decision the type carries, made once at construction, and a
    /// `&str` overload here would re-open it at every call site.
    #[must_use]
    pub fn with_default_font_family(mut self, family: pinion_core::style::FontFamily) -> Self {
        self.default_font_family = Some(family);
        self
    }

    /// Register a hook invoked with the winit-free
    /// [`RpcIngress`] seam after the event loop is
    /// built but before it starts. Mount an injected transport here. The
    /// hook runs on the main thread; the transport it spawns owns its own
    /// threads and lifetime.
    ///
    /// ```
    /// use std::sync::Arc;
    /// use pinion_shell::{RpcIngress, ShellConfig};
    ///
    /// let _config = ShellConfig::new().on_rpc_ingress(|ingress: Arc<dyn RpcIngress>| {
    ///     // Mount any transport here and drive `ingress`. For an
    ///     // always-on Unix socket:
    ///     //   let control = pinion_rpc_transport::UnixSocketTransport
    ///     //       ::serve_with_exposure("/run/user/1000/app.sock", ingress,
    ///     //                             boot_exposure)?;
    ///     // R-PR48 — the boot exposure belongs to the bind, so a
    ///     // "bound but withdrawn" policy has no serving window. Keep
    ///     // `control` alive for the endpoint's lifetime; toggle
    ///     // `control.set_exposure(..)` for runtime on/off; drop it to stop.
    ///     let _ = ingress;
    /// });
    /// ```
    #[must_use]
    pub fn on_rpc_ingress(mut self, hook: impl FnOnce(Arc<dyn RpcIngress>) + 'static) -> Self {
        self.on_ingress = Some(Box::new(hook));
        self
    }
}

/// Run the visual binary end-to-end: build the winit event loop with the
/// [`AppEvent`] user-event slot, install the built-in stdin RPC reader
/// (and any [`ShellConfig::on_rpc_ingress`] transport), optionally install
/// the async [`CommandExecutor`], run the [`AppShell<V>`] until quit.
///
/// R-PR47 §5.7 — the single general entry point. [`run`] and
/// [`run_with_handlers`] delegate here; a consumer that wants an injected
/// transport (e.g. an always-on RPC socket) calls this directly with a
/// [`ShellConfig::on_rpc_ingress`] hook.
///
/// # Panics
/// Panics if `winit::event_loop::EventLoop::with_user_event().build()`
/// fails (only on platforms that cannot supply a user-event loop — none
/// of the desktop targets pinion supports), or, when `config.handlers` is
/// set, if the tokio runtime cannot spin up its worker thread (the
/// OS-level thread-spawn failure that
/// [`TokioExecutor::new`](crate::TokioExecutor) wraps).
pub fn run_with_config<V: WidgetView>(config: ShellConfig) {
    init_tracing();
    // R637 §5.16 §5.7 — `PINION_SCREENSHOT=<path>` env hook. When set, the
    // binary bypasses winit entirely: build the initial paint scene
    // through the same `ShellCore` substrate the live path uses, render it
    // through `HeadlessScreenshot`, write the PNG, exit. No event loop, so
    // neither the stdin reader nor an injected transport is installed —
    // there is no live loop to feed. See [`crate::headless_screenshot`].
    if try_headless_screenshot::<V>() {
        drop(config);
        return;
    }
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("winit EventLoop::with_user_event failed");
    event_loop.set_control_flow(ControlFlow::Wait);

    // R-PR47 §5.7 — build the winit-free ingress seam once, then feed it
    // to every producer: the built-in stdin reader AND the consumer's
    // optional injected transport share the identical dispatch path.
    // R1448 §5.36 — take the declared faces out of the config before the hook
    // consumes the rest of it; they are handed to the shell constructor, which
    // registers them into the render cache and publishes the resulting report.
    let app_fonts = config.app_fonts;
    let default_font_family = config.default_font_family;
    let ingress: Arc<dyn RpcIngress> = Arc::new(ProxyRpcIngress::new(event_loop.create_proxy()));
    spawn_stdin_rpc_reader(Arc::clone(&ingress));
    if let Some(hook) = config.on_ingress {
        hook(Arc::clone(&ingress));
    }

    let mut app =
        AppShell::<V>::new_with_fonts(event_loop.create_proxy(), app_fonts, default_font_family);

    // R51.159 §5.23 — when handlers are supplied, assemble the
    // CommandExecutor and inject it before the loop starts so the first
    // dispatch tail can already drain pending commands. Absent handlers,
    // pending Command queues stay parked (the bare `run` behaviour).
    if let Some(registry) = config.handlers {
        let (executor, sink) =
            build_executor_and_sink(event_loop.create_proxy()).expect("tokio runtime build failed");
        let cmd_exec = Arc::new(CommandExecutor::new(registry, executor, sink));
        let _prior = app.core.set_command_executor(cmd_exec);
    }

    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("shell: event loop error: {e}");
    }
}

/// Run the visual binary end-to-end. The single line every shell consumer
/// needs in `fn main()`.
///
/// R51.159 §5.23 — no [`CommandExecutor`] is installed by this entry
/// point; pending [`pinion_core::Command`] queues stay parked on the
/// owner side and never fire. Use [`run_with_handlers`] to register async
/// [`Handler`](pinion_runtime::Handler)s, or [`run_with_config`] for full
/// control including an injected RPC transport.
///
/// # Panics
/// Panics if the winit event loop cannot be built (see
/// [`run_with_config`]).
pub fn run<V: WidgetView>() {
    run_with_config::<V>(ShellConfig::new());
}

/// R51.159 §5.23 — variant of [`run`] that installs a [`CommandExecutor`]
/// at boot so pending [`pinion_core::Command`]s queued by reducer fallout
/// or SCXML / Update steps reach their registered
/// [`Handler`](pinion_runtime::Handler)s asynchronously.
///
/// Composes a tokio multi-thread [`TokioExecutor`](crate::TokioExecutor)
/// (1 worker, `enable_all`), a [`ProxyIntentSink`](crate::ProxyIntentSink)
/// wrapping the winit [`EventLoopProxy`] so resolved
/// [`pinion_core::Intent`]s arrive through [`AppEvent::IntentArrived`],
/// and the supplied `registry` keyed by
/// [`pinion_core::Command::kind_str`]. Equivalent to
/// `run_with_config(ShellConfig::new().with_handlers(registry))`.
///
/// # Panics
/// Panics if the winit event loop cannot be built or if the tokio runtime
/// cannot spin up its worker thread (see [`run_with_config`]).
pub fn run_with_handlers<V: WidgetView>(registry: HandlerRegistry) {
    run_with_config::<V>(ShellConfig::new().with_handlers(registry));
}

/// R835 §5.16 — `true` when `PINION_HIDDEN_WINDOW` requests the
/// offscreen (unmapped) window mode for headless local verification (any
/// value except empty / `0`). The window still renders to its GPU
/// surface; only the OS map is suppressed, so no window flashes on the
/// developer's display while RPC demos drive the binary.
fn hidden_window_requested() -> bool {
    std::env::var("PINION_HIDDEN_WINDOW")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// R637 §5.16 §5.7 — env-hook plumbing for [`run`] / [`run_with_handlers`].
///
/// Reads `PINION_SCREENSHOT`. When unset, returns `false` so the
/// caller continues into the winit event loop. When set, builds the
/// initial paint scene through [`ShellCore::compute_paint_scene`]
/// (the same path the live render loop drives every redraw — closes
/// the [[ai-first-rpc-introspection-obligation]] gap that
/// design-parity verification cannot use the live binary in
/// headless / CI environments), renders it through
/// [`crate::headless_screenshot::HeadlessScreenshot`] (wgpu + vello,
/// no winit surface), writes a PNG, and returns `true`. Any error
/// surfaces as `eprintln!` + `std::process::exit(1)` so CI / shell
/// pipelines see a non-zero exit code on capture failure rather
/// than a silently-empty PNG.
fn try_headless_screenshot<V: WidgetView>() -> bool {
    let Ok(path) = std::env::var("PINION_SCREENSHOT") else {
        return false;
    };
    let mut core = ShellCore::<V>::new();
    // R668 §5.16 — `IntrinsicAfterFirstPaint` headless path mirrors
    // the live shell: render once at the upper bound (so the layout
    // pass sees enough viewport to lay out the content), walk for
    // the intrinsic bbox, clamp to `[min, max]`, then re-paint at
    // the clamped size so the PNG dimensions match the final winit
    // window size. `Fixed` skips the two-pass and paints directly.
    let strategy = V::initial_size_strategy();
    let (w, h, paint_scene) = match strategy {
        crate::SizeStrategy::Fixed { width, height } => {
            let scene = core.compute_paint_scene(width, height);
            (width, height, scene)
        }
        // R1059 — `OpenResizable` paints once at its open `size`, the
        // same single-pass path as `Fixed`; the OS-resize floor
        // (`min`) is a live-winit concern with no headless effect.
        crate::SizeStrategy::OpenResizable { size, .. } => {
            let scene = core.compute_paint_scene(size.0, size.1);
            (size.0, size.1, scene)
        }
        crate::SizeStrategy::IntrinsicAfterFirstPaint { min, max } => {
            let measure = core.compute_paint_scene(max.0.max(1), max.1.max(1));
            let (cw, ch) = measure.intrinsic_content_size();
            let target_w = cw.clamp(min.0, max.0).max(1);
            let target_h = ch.clamp(min.1, max.1).max(1);
            let scene = core.compute_paint_scene(target_w, target_h);
            (target_w, target_h, scene)
        }
    };
    let base = paint_adapter::root_background(&paint_scene);
    let mut vello_scene = VelloScene::new();
    // R706 §5.16 — rasterize through `to_vello_cached`, the SAME path the
    // live winit render loop drives (`AppShell::render_window`), so the
    // headless screenshot the AI introspects is pixel-faithful to the
    // live window. The previous `to_vello` (uncached) call rasterized
    // through a different code path than the live render, so a
    // cache-path-only rasterization defect (R706: the focus-ring overlay
    // drawing one grid column off through `to_vello_cached`) was visible
    // on screen yet ABSENT from the headless screenshot — defeating the
    // introspection-parity this hook exists to provide
    // ([[introspection-from-paint-not-screen]]). A fresh per-capture
    // `FragmentCache` makes every subtree a first-paint miss; the output
    // matches `to_vello` when both are correct and tracks `to_vello_cached`
    // when they would diverge.
    let mut fragment_cache = paint_adapter::FragmentCache::new();
    // R1404 §5.16 — resolve the producer store `ShellCore::new` seeded at root
    // (a `use_image_store` registration in `V::create_external` above already
    // landed there), so a `memory://<key>` source paints in the headless PNG
    // exactly as in the live window — the north-star "headless render valid".
    let mut image_cache =
        image_cache::ImageCache::with_store(image_cache::resolve_image_store(core.root_owner()));
    // R1072 §5.37 — same engine-aware cached paint as the live winit path, so a
    // headless `PINION_SCREENSHOT` is pixel-faithful to the window painted via §5.37.
    let (text_cache, text_engine) = core.text_cache_and_engine();
    paint_adapter::to_vello_cached_with_text_engine(
        &paint_scene,
        &|_b: &BoxNode| None,
        text_cache,
        &mut image_cache,
        &mut fragment_cache,
        text_engine,
        &mut vello_scene,
        // R1426 §5.41 — a headless PNG renders the cursor STEADY (blink phase
        // ON): the render-time phase is wall-clock-driven, so forcing ON keeps a
        // golden screenshot deterministic (never captures a mid-blink off-phase).
        true,
        // R1427 §5.41 §5.39 — and FOCUSED (filled, not the unfocused hollow box):
        // a headless capture has no live OS-focus fact, so the deterministic
        // golden shows the filled cursor.
        true,
    );
    let mut shot = match crate::HeadlessScreenshot::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("shell: PINION_SCREENSHOT: {e}");
            std::process::exit(1);
        }
    };
    let file = match std::fs::File::create(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("shell: PINION_SCREENSHOT create {path}: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = shot.render_to_png(&vello_scene, w, h, base, std::io::BufWriter::new(file)) {
        eprintln!("shell: PINION_SCREENSHOT render_to_png: {e}");
        std::process::exit(1);
    }
    eprintln!("shell: PINION_SCREENSHOT wrote {w}x{h} RGBA8 → {path}");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;

    // ─────────────────────────────────────────────────────────────
    // R51.192 §5.45 R55.C.2 — winit ↔ W3C wheel sign convention.
    // winit's `MouseScrollDelta` positive = content moves toward
    // origin (reveal above/left); W3C `WheelEvent.deltaY` positive
    // = scroll toward content end (reveal below/right). Boundary
    // flips both axes so the substrate's `ScrollState::scroll_by`
    // receives W3C-signed deltas — matching the TUI sibling
    // (`MouseEventKind::ScrollDown` already emits `dy = +1.0`).
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r51_192_line_delta_y_flips_sign() {
        // User scrolls wheel forward (away from them) — winit
        // reports y > 0. W3C / substrate convention: forward wheel
        // moves toward content origin (deltaY < 0).
        let pinion = winit_wheel_to_pinion(MouseScrollDelta::LineDelta(0.0, 1.0), 1.0);
        match pinion {
            WheelDelta::Lines { dx, dy } => {
                assert!((dx - 0.0).abs() < f32::EPSILON);
                assert!(
                    (dy - (-1.0)).abs() < f32::EPSILON,
                    "forward wheel must emit W3C deltaY < 0, got {dy}",
                );
            }
            other => panic!("expected Lines, got {other:?}"),
        }
    }

    #[test]
    fn r51_192_line_delta_x_flips_sign() {
        // Horizontal tilt right — winit x > 0. W3C: deltaX < 0
        // (reveals content to the left).
        let pinion = winit_wheel_to_pinion(MouseScrollDelta::LineDelta(1.0, 0.0), 1.0);
        match pinion {
            WheelDelta::Lines { dx, dy } => {
                assert!(
                    (dx - (-1.0)).abs() < f32::EPSILON,
                    "tilt right must emit W3C deltaX < 0, got {dx}",
                );
                assert!((dy - 0.0).abs() < f32::EPSILON);
            }
            other => panic!("expected Lines, got {other:?}"),
        }
    }

    #[test]
    fn r51_192_pixel_delta_both_axes_flip() {
        // Trackpad inertia — both axes flip. The conversion narrows
        // f64 → f32 at the same boundary.
        let pinion = winit_wheel_to_pinion(
            MouseScrollDelta::PixelDelta(PhysicalPosition { x: 12.5, y: 24.0 }),
            1.0,
        );
        match pinion {
            WheelDelta::Pixels { dx, dy } => {
                assert!((dx - (-12.5)).abs() < f32::EPSILON);
                assert!((dy - (-24.0)).abs() < f32::EPSILON);
            }
            other => panic!("expected Pixels, got {other:?}"),
        }
    }

    #[test]
    fn r1027_pixel_delta_scaled_to_logical() {
        // R1027 §5.16 §5.45 — a `PixelDelta` is physical device pixels; on
        // a 2x display it must be halved to logical so the content scrolls
        // the intended distance (not 2x). The sign flip still applies.
        let pinion = winit_wheel_to_pinion(
            MouseScrollDelta::PixelDelta(PhysicalPosition { x: 12.5, y: 24.0 }),
            2.0,
        );
        match pinion {
            WheelDelta::Pixels { dx, dy } => {
                assert!(
                    (dx - (-6.25)).abs() < f32::EPSILON,
                    "12.5 physical / 2 = 6.25 logical"
                );
                assert!(
                    (dy - (-12.0)).abs() < f32::EPSILON,
                    "24 physical / 2 = 12 logical"
                );
            }
            other => panic!("expected Pixels, got {other:?}"),
        }
        // LineDelta (notch counts) is unitless — scale must NOT change it.
        let lines = winit_wheel_to_pinion(MouseScrollDelta::LineDelta(0.0, 1.0), 2.0);
        match lines {
            WheelDelta::Lines { dy, .. } => {
                assert!(
                    (dy - (-1.0)).abs() < f32::EPSILON,
                    "line notches are scale-independent"
                );
            }
            other => panic!("expected Lines, got {other:?}"),
        }
    }

    #[test]
    fn r51_192_winit_tui_sibling_direction_agreement() {
        // Sanity guard against the conversion drifting back to a
        // pass-through: winit's forward wheel (y = +1.0) must
        // produce the same dy sign the TUI `ScrollUp` arm sends
        // (`WheelDelta::Lines { dx: 0.0, dy: -1.0 }`). If this
        // trips, the two backends disagree on direction and
        // `ScrollState::scroll_by` will move the offset opposite
        // ways per backend — §2 #6 GUI/TUI dual invariant break.
        let from_winit_forward = winit_wheel_to_pinion(MouseScrollDelta::LineDelta(0.0, 1.0), 1.0);
        let from_tui_scroll_up = WheelDelta::Lines { dx: 0.0, dy: -1.0 };
        match (from_winit_forward, from_tui_scroll_up) {
            (WheelDelta::Lines { dy: w_dy, .. }, WheelDelta::Lines { dy: t_dy, .. }) => {
                assert!(
                    w_dy.signum() == t_dy.signum(),
                    "winit forward must match TUI ScrollUp sign (both negative dy); \
                     got winit={w_dy} vs tui={t_dy}",
                );
            }
            _ => panic!("both branches must be Lines variants"),
        }
    }

    // ─────────────────────────────────────────────────────────────
    // R1027 §5.16 §5.35 — `HiDPI` scale_factor coordinate policy. The
    // whole pinion scene / layout / pointer world is logical; physical
    // pixels appear only at the GPU surface, the vello raster scale,
    // and these winit input + layout-dim boundaries. These pin the pure
    // conversions (the shell wires them into render_window + the
    // CursorMoved / Touch arms).
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r1027_logical_layout_size_divides_physical_by_scale() {
        // 1920x1200 physical on a 2x display = 960x600 logical — the
        // size the paint scene is built in (app dims rastered at 2x).
        assert_eq!(
            logical_layout_size(PhysicalSize::new(1920, 1200), 2.0),
            (960, 600)
        );
        // Identity: logical == physical (non-`HiDPI`, unchanged path).
        assert_eq!(
            logical_layout_size(PhysicalSize::new(800, 600), 1.0),
            (800, 600)
        );
        // Fractional scale rounds to nearest: 1440/1.5=960, 901/1.5=600.67->601.
        assert_eq!(
            logical_layout_size(PhysicalSize::new(1440, 901), 1.5),
            (960, 601)
        );
        // Degenerate: a sub-logical-pixel physical dimension (1px / 4 =
        // 0.25 -> round -> 0) returns 0 — NOT clamped to 1. render_window's
        // `NonZeroU32` guard then early-returns (no paint), matching the
        // pre-R1027 0-size skip rather than painting a wasted 1px frame.
        assert_eq!(logical_layout_size(PhysicalSize::new(1, 1), 4.0), (0, 0));
    }

    #[test]
    fn r1027_pointer_physical_to_logical() {
        // The splitter case (PR-15): a physical cursor at x=960 on a 2x
        // display is logical 480 — exactly the 4-logical-px handle the
        // router must resolve. Pre-R1027 it saw 960 and missed the handle.
        assert_eq!(
            winit_pointer_to_logical(PhysicalPosition::new(960.0, 600.0), 2.0),
            (480.0, 300.0)
        );
        // Identity is a pass-through (byte-identical pre-R1027 routing).
        assert_eq!(
            winit_pointer_to_logical(PhysicalPosition::new(12.0, 34.0), 1.0),
            (12.0, 34.0)
        );
        // Fractional scale: 150/1.5 = 100, 75/1.5 = 50.
        let (x, y) = winit_pointer_to_logical(PhysicalPosition::new(150.0, 75.0), 1.5);
        assert!((x - 100.0).abs() < f64::EPSILON);
        assert!((y - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn r1027_scale_is_non_identity_gate() {
        // Drives the byte-identical fast path (scaled append + AccessKit
        // root transform are both skipped at identity).
        assert!(!scale_is_non_identity(1.0));
        assert!(scale_is_non_identity(2.0));
        assert!(scale_is_non_identity(1.5));
        // A downscaled (< 1.0) display is also non-identity.
        assert!(scale_is_non_identity(0.75));
    }
}

#[cfg(test)]
mod r1121_chrome_action_tests {
    //! R1121 §5.16 §5.39 — the window-chrome tag -> control mapping that
    //! `try_chrome_press` routes to winit. Pure, so the full vocabulary is
    //! covered without a live window.
    use super::{ChromeAction, chrome_action_for_tag};
    use winit::window::ResizeDirection;

    #[test]
    fn maps_every_chrome_control_tag() {
        // R1188 — the three discrete buttons resolve through the shared
        // `window_control_for_tag` SSOT (the same mapping the RPC click drain
        // uses), wrapped as `ChromeAction::Control`.
        assert!(matches!(
            chrome_action_for_tag(pinion_overlay::WINDOW_CHROME_CLOSE_TAG),
            Some(ChromeAction::Control(pinion_overlay::WindowControl::Close))
        ));
        assert!(matches!(
            chrome_action_for_tag(pinion_overlay::WINDOW_CHROME_MINIMIZE_TAG),
            Some(ChromeAction::Control(
                pinion_overlay::WindowControl::Minimize
            ))
        ));
        assert!(matches!(
            chrome_action_for_tag(pinion_overlay::WINDOW_CHROME_MAXIMIZE_TAG),
            Some(ChromeAction::Control(
                pinion_overlay::WindowControl::Maximize
            ))
        ));
        assert!(matches!(
            chrome_action_for_tag(pinion_overlay::WINDOW_CHROME_GRIP_TAG),
            Some(ChromeAction::Move)
        ));
    }

    #[test]
    fn maps_every_resize_region_tag_to_its_direction() {
        // R1122 — the eight resize edges / corners map to the matching winit
        // `ResizeDirection` that `try_chrome_press` feeds `drag_resize_window`.
        let cases = [
            (
                pinion_overlay::WINDOW_RESIZE_NORTH_TAG,
                ResizeDirection::North,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_SOUTH_TAG,
                ResizeDirection::South,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_WEST_TAG,
                ResizeDirection::West,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_EAST_TAG,
                ResizeDirection::East,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_NORTH_WEST_TAG,
                ResizeDirection::NorthWest,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_NORTH_EAST_TAG,
                ResizeDirection::NorthEast,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_SOUTH_WEST_TAG,
                ResizeDirection::SouthWest,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_SOUTH_EAST_TAG,
                ResizeDirection::SouthEast,
            ),
        ];
        for (tag, dir) in cases {
            assert!(
                matches!(chrome_action_for_tag(tag), Some(ChromeAction::Resize(d)) if d == dir),
                "{tag} maps to ResizeDirection::{dir:?}",
            );
        }
    }

    #[test]
    fn non_chrome_tags_are_not_controls() {
        assert!(chrome_action_for_tag("some-widget").is_none());
        assert!(chrome_action_for_tag("ai-overlay/focus-ring").is_none());
        // The strip CONTAINER tag is not itself a control (its children are).
        assert!(chrome_action_for_tag(pinion_overlay::WINDOW_CHROME_TAG).is_none());
        // The resize family PREFIX is not itself a control (the suffixed
        // edge / corner tags are).
        assert!(chrome_action_for_tag(pinion_overlay::WINDOW_RESIZE_TAG_PREFIX).is_none());
    }

    #[test]
    fn r1189_resize_hover_maps_each_region_to_its_cursor() {
        // R1189 §5.16 §5.39 — the hover→cursor mapping `handle_cursor_moved`
        // drives (tag → chrome_action_for_tag → resize_cursor_for_action). The
        // four CSS-standard resize axes: N/S = NsResize, W/E = EwResize, the two
        // main-diagonal corners = NwseResize, the two anti-diagonal = NeswResize.
        use super::resize_cursor_for_action;
        use winit::window::CursorIcon;
        let cursor_for = |tag: &str| chrome_action_for_tag(tag).and_then(resize_cursor_for_action);
        let cases = [
            (
                pinion_overlay::WINDOW_RESIZE_NORTH_TAG,
                CursorIcon::NsResize,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_SOUTH_TAG,
                CursorIcon::NsResize,
            ),
            (pinion_overlay::WINDOW_RESIZE_WEST_TAG, CursorIcon::EwResize),
            (pinion_overlay::WINDOW_RESIZE_EAST_TAG, CursorIcon::EwResize),
            (
                pinion_overlay::WINDOW_RESIZE_NORTH_WEST_TAG,
                CursorIcon::NwseResize,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_SOUTH_EAST_TAG,
                CursorIcon::NwseResize,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_NORTH_EAST_TAG,
                CursorIcon::NeswResize,
            ),
            (
                pinion_overlay::WINDOW_RESIZE_SOUTH_WEST_TAG,
                CursorIcon::NeswResize,
            ),
        ];
        for (tag, want) in cases {
            assert_eq!(cursor_for(tag), Some(want), "{tag} hover cursor");
        }
        // Non-resize chrome + ordinary tags command NO special cursor (the grip
        // keeps the default arrow — a title bar is dragged, not shown a move
        // cursor; a control button is a normal click target).
        assert_eq!(cursor_for(pinion_overlay::WINDOW_CHROME_GRIP_TAG), None);
        assert_eq!(cursor_for(pinion_overlay::WINDOW_CHROME_CLOSE_TAG), None);
        assert_eq!(cursor_for(pinion_overlay::WINDOW_CHROME_MINIMIZE_TAG), None);
        assert_eq!(cursor_for("some-widget"), None);
    }

    #[test]
    fn r1196_cursor_icon_for_hint_maps_resize_hints() {
        use super::cursor_icon_for_hint;
        use pinion_core::style::CursorHint;
        use winit::window::CursorIcon;
        // The generic node-hint → winit cursor map (the splitter divider path):
        // a left-right col-resize is EwResize, an up-down row-resize NsResize —
        // the same icons a W/E and N/S chrome edge command, so a divider and a
        // window edge read identically.
        assert_eq!(
            cursor_icon_for_hint(CursorHint::ColResize),
            CursorIcon::EwResize
        );
        assert_eq!(
            cursor_icon_for_hint(CursorHint::RowResize),
            CursorIcon::NsResize
        );
        // R1609 — the two diagonals a corner handle asks for. Asserted against
        // the SAME icons `resize_cursor_for_action` commands for a window's
        // corner, which is the point of the round's finding: the icons were
        // reachable from the chrome path since R1189 and the node vocabulary
        // could not name them, so a card's corner grip and a window's corner now
        // read identically rather than by coincidence.
        assert_eq!(
            cursor_icon_for_hint(CursorHint::NwseResize),
            CursorIcon::NwseResize
        );
        assert_eq!(
            cursor_icon_for_hint(CursorHint::NeswResize),
            CursorIcon::NeswResize
        );
        assert_eq!(
            cursor_icon_for_hint(CursorHint::NwseResize),
            super::resize_cursor_for_action(ChromeAction::Resize(ResizeDirection::SouthEast))
                .expect("a corner chrome edge commands a cursor"),
            "a card's ⤡ grip and a window's south-east corner are one icon"
        );
    }

    #[test]
    fn r1189_cursor_latch_only_commands_on_change() {
        // R1189 §5.16 §5.39 — the min-change latch decision `command_resize_cursor`
        // drives. `Some(icon)` = call winit set_cursor; `None` = suppress.
        use super::next_cursor_command;
        use winit::window::CursorIcon;
        // Enter a region from the default arrow → command the resize icon.
        assert_eq!(
            next_cursor_command(None, Some(CursorIcon::NsResize)),
            Some(CursorIcon::NsResize),
        );
        // Same region on the next move → suppress (the hot-path no-op).
        assert_eq!(
            next_cursor_command(Some(CursorIcon::NsResize), Some(CursorIcon::NsResize)),
            None,
        );
        // Region → different region → command the new icon.
        assert_eq!(
            next_cursor_command(Some(CursorIcon::NsResize), Some(CursorIcon::EwResize)),
            Some(CursorIcon::EwResize),
        );
        // Region → content / leave (desired None) → command the default arrow ONCE.
        assert_eq!(
            next_cursor_command(Some(CursorIcon::NsResize), None),
            Some(CursorIcon::Default),
        );
        // Content → content (never in a region) → suppress: a window that never
        // enters a resize region is never commanded a cursor at all.
        assert_eq!(next_cursor_command(None, None), None);
    }
}

#[cfg(test)]
mod r1364_source_scan {
    //! R1364 — the production-source walk this file's source-text tests share.
    //!
    //! Extracted rather than copied: the subtle part is not the property each
    //! test checks, it is knowing that `app.rs` interleaves EIGHT top-level
    //! `#[cfg(test)]` modules with production code. `pinion_core::widgets::commit`'s
    //! precedent stops at the FIRST `#[cfg(test)]`, which is exact for a file
    //! whose tests all sit at the end and silently wrong here —
    //! `try_headless_screenshot`, which owns three termination sites, follows
    //! six of them. A second hand-rolled copy of this walk is where that
    //! mistake would come back, quietly, as a green test over a short file.

    /// Every line of `app.rs` OUTSIDE a top-level `#[cfg(test)] mod`, as
    /// `(1-based line number, line)`.
    ///
    /// Comments are KEPT: one caller must read rustdoc (the termination map is
    /// made of `///` lines), the others skip comments themselves. Every test
    /// module here is top-level, so its closing `}` is the next one at column 0;
    /// anything that breaks that shape fails loudly rather than mis-scoping.
    pub(super) fn production_lines() -> Vec<(usize, &'static str)> {
        let src = include_str!("app.rs");
        let mut out = Vec::new();
        let mut in_test_mod = false;
        let mut expect_test_mod = false;
        for (n, line) in src.lines().enumerate() {
            let lineno = n + 1;
            if in_test_mod {
                if line == "}" {
                    in_test_mod = false;
                }
                continue;
            }
            if expect_test_mod {
                assert!(
                    line.starts_with("mod ") && line.ends_with('{'),
                    "app.rs:{lineno}: a column-0 `#[cfg(test)]` introduces \
                     {line:?}, not a top-level `mod ... {{`. This scan separates \
                     production from test code by skipping whole top-level test \
                     modules; teach it the new shape rather than let it mis-scope."
                );
                in_test_mod = true;
                expect_test_mod = false;
                continue;
            }
            if line == "#[cfg(test)]" {
                expect_test_mod = true;
                continue;
            }
            assert!(
                !line.trim_start().starts_with("#[cfg(test)]"),
                "app.rs:{lineno}: an INDENTED `#[cfg(test)]`. This scan cannot \
                 see an inner one and would count its body as production."
            );
            out.push((lineno, line));
        }
        assert!(!in_test_mod, "app.rs: unterminated top-level test module");
        assert!(!out.is_empty(), "app.rs: the production walk found nothing");
        out
    }
}

#[cfg(test)]
mod r1364_control_producer_tests {
    //! R1364 §5.16 §5.55 — [`ControlProducer`]'s roster IS the arm's call sites.
    //!
    //! The enum's rustdoc claims "the variants ARE the count, every call site
    //! names itself at the call". That is a claim, and this round exists because
    //! nine unchecked claims about this exact count drifted until three were
    //! wrong. So the claim gets a test.
    //!
    //! Source-text, for the reason the termination map's is: `apply_window_control`
    //! takes an `&ActiveEventLoop`, which `tests/dispatch_core.rs` records a
    //! `#[test]` cannot synthesise — not one of these call sites is reachable
    //! from the suite, so only the text can be asked.
    //!
    //! What this adds over the compiler, precisely: `dead_code` (denied here)
    //! already fails a variant no call site names, so the `unused` arm below is
    //! defence in depth, not the point — it earns its keep only if a TEST
    //! constructs the variant and silences that lint. The arms the compiler
    //! cannot make are the other two: two call sites SHARING a producer, and a
    //! call-site count that has drifted from the variant count. Verified by
    //! attempting both: swapping a variant fails to COMPILE (so the test never
    //! runs), while a fifth call site reusing `ChromePress` fails HERE.

    /// Parse the variant names out of `enum ControlProducer { .. }`.
    fn variants() -> Vec<String> {
        let mut out = Vec::new();
        let mut inside = false;
        for (_, line) in super::r1364_source_scan::production_lines() {
            if line.starts_with("enum ControlProducer {") {
                inside = true;
                continue;
            }
            if !inside {
                continue;
            }
            if line == "}" {
                break;
            }
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            if let Some(name) = t.strip_suffix(',')
                && name.chars().all(|c| c.is_ascii_alphanumeric())
            {
                out.push(name.to_owned());
            }
        }
        assert!(
            !out.is_empty(),
            "`enum ControlProducer` parsed to no variants"
        );
        out
    }

    #[test]
    fn r1364_every_producer_names_exactly_one_call_site() {
        let variants = variants();
        let mut calls = 0usize;
        let mut used: Vec<(String, usize)> = variants.iter().map(|v| (v.clone(), 0)).collect();
        for (_, line) in super::r1364_source_scan::production_lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            calls += line.matches("self.apply_window_control(").count();
            for (v, n) in &mut used {
                *n += line.matches(&format!("ControlProducer::{v}")).count();
            }
        }
        let unused: Vec<&str> = used
            .iter()
            .filter(|(_, n)| *n == 0)
            .map(|(v, _)| v.as_str())
            .collect();
        assert!(
            unused.is_empty(),
            "`ControlProducer` variants no call site names: {unused:?}. A \
             producer that exists only in the enum is the count drifting again, \
             in the other direction."
        );
        let over: Vec<&(String, usize)> = used.iter().filter(|(_, n)| *n > 1).collect();
        assert!(
            over.is_empty(),
            "`ControlProducer` variants named more than once: {over:?}. Two call \
             sites sharing a producer means the roster no longer distinguishes \
             them, so the warn's `producer` field points at the wrong author."
        );
        assert_eq!(
            calls,
            variants.len(),
            "`apply_window_control` has {calls} call sites but `ControlProducer` \
             has {} variants. The arm is ONE and its producers are the enum: a \
             new call site must add the variant that names it, not borrow one.",
            variants.len(),
        );
    }
}

#[cfg(test)]
mod r1364_termination_map_tests {
    //! R1364 §5.55 — the termination map on [`AppShell::request_quit`] is
    //! machine-checked against this file's own source text.
    //!
    //! A SOURCE-TEXT check, unusual and deliberate, for the same reason
    //! `pinion_core::widgets::commit`'s is (R1349.1): the property is "the
    //! documented map is the WHOLE map", and no runtime assertion can reach it.
    //! `tests/dispatch_core.rs` records that a `#[test]` cannot synthesise an
    //! `EventLoop`, so every `event_loop.exit()` below is invisible to the suite
    //! by construction — a stale row costs nothing at runtime and stays green
    //! forever.
    //!
    //! It is not hypothetical. R1362 published the map as prose; R1363 rewired
    //! it one round later — `Close` and `handle_key_press` stopped exiting
    //! inline, `request_quit` became THE arm — and all four gates stayed green
    //! over a map that now described a world which no longer existed. Three of
    //! its six rows were wrong, and the arm that R1363's whole round existed to
    //! create was missing from the census of exits.

    use std::collections::BTreeMap;

    /// The two termination families, spelled as the call sites spell them.
    /// Matched as substrings: the map documents `std::process::exit(1)`, the
    /// sites spell that same prefix.
    const FAMILIES: [&str; 2] = ["event_loop.exit()", "std::process::exit"];

    /// The row that opens the map. Scoping to it matters: this file carries a
    /// second rustdoc table (the winit IME mapping), so an unscoped `/// |`
    /// scan would swallow it and demand call sites for `Ime::Preedit`.
    const TABLE_HEADER: &str = "/// | Termination | Trigger | Binding-drivable? |";

    /// A termination site: which function ends the process, and by which family.
    type Site = (String, &'static str);

    /// `fn foo(` / `pub fn foo(` -> `foo`. Callers pass a trimmed, non-comment
    /// line.
    fn fn_name(trimmed: &str) -> Option<String> {
        let rest = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
        let rest = rest.strip_prefix("fn ")?;
        let end = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_')?;
        Some(rest[..end].to_owned())
    }

    /// Split a leading `` `...` `` off `s`, returning (contents, remainder).
    fn backticked(s: &str) -> Option<(&str, &str)> {
        let rest = s.strip_prefix('`')?;
        let end = rest.find('`')?;
        Some((&rest[..end], &rest[end + 1..]))
    }

    /// Parse a map row's first cell: `` `path` xN -> `family` ``.
    fn parse_row(cell: &str) -> Option<(String, &'static str, usize)> {
        let (path, rest) = backticked(cell)?;
        let (count, rest) = rest.trim_start().strip_prefix('×')?.split_once(' ')?;
        let rest = rest.trim_start().strip_prefix('→')?;
        let (fam_doc, _) = backticked(rest.trim_start())?;
        let fam = FAMILIES.iter().copied().find(|f| fam_doc.starts_with(f))?;
        let name = path.rsplit("::").next()?.to_owned();
        Some((name, fam, count.parse().ok()?))
    }

    /// Every termination call site in this file's PRODUCTION source, keyed by
    /// enclosing function, with multiplicity.
    fn source_sites() -> BTreeMap<Site, usize> {
        let mut sites: BTreeMap<Site, usize> = BTreeMap::new();
        let mut current_fn: Option<String> = None;
        for (lineno, line) in super::r1364_source_scan::production_lines() {
            let trimmed = line.trim_start();
            // Docs and comments name these sites legitimately — the map itself
            // is made of them, and so is this module.
            if trimmed.starts_with("//") {
                continue;
            }
            if let Some(name) = fn_name(trimmed) {
                let indent = line.len() - trimmed.len();
                assert!(
                    indent == 0 || indent == 4,
                    "app.rs:{lineno}: `fn {name}` at indent {indent}. Only free \
                     functions (0) and methods (4) exist here; a nested `fn` \
                     would make \"the last `fn` seen\" the wrong enclosing \
                     function and misattribute a row."
                );
                current_fn = Some(name);
            }
            for fam in FAMILIES {
                let hits = line.matches(fam).count();
                if hits == 0 {
                    continue;
                }
                let Some(f) = current_fn.clone() else {
                    panic!("app.rs:{lineno}: `{fam}` outside any function")
                };
                *sites.entry((f, fam)).or_insert(0) += hits;
            }
        }
        sites
    }

    /// The map's rows, parsed out of the rustdoc.
    fn documented_sites() -> BTreeMap<Site, usize> {
        let mut out: BTreeMap<Site, usize> = BTreeMap::new();
        let mut in_table = false;
        let mut rows = 0usize;
        for (_, line) in super::r1364_source_scan::production_lines() {
            let t = line.trim_start();
            if t == TABLE_HEADER {
                in_table = true;
                continue;
            }
            if !in_table {
                continue;
            }
            let Some(row) = t.strip_prefix("/// |") else {
                break; // the first non-row line closes the table
            };
            if row.starts_with("---") {
                continue;
            }
            let cell = row.split('|').next().unwrap_or_default().trim();
            let Some((name, fam, count)) = parse_row(cell) else {
                panic!(
                    "termination map row {cell:?} does not parse. Every row must \
                     read ``fn` xN -> `family`` so the map stays checkable — a \
                     prose row cannot be enforced, which is the whole point."
                )
            };
            *out.entry((name, fam)).or_insert(0) += count;
            rows += 1;
        }
        assert!(
            in_table,
            "the termination map's header vanished from app.rs"
        );
        assert!(rows > 0, "the termination map has no rows");
        out
    }

    /// The map opens with "Two families, and BOTH live in this file" — a claim
    /// about the whole crate, made in prose, and therefore exactly the kind of
    /// sentence this module exists to stop trusting. For `event_loop.exit()` the
    /// rustdoc offers an argument from types; for `std::process::exit` it admits
    /// it is "enumerated on its own evidence, by grep" — so here is the grep.
    #[test]
    fn r1364_both_termination_families_live_only_in_app_rs() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut offenders: Vec<String> = Vec::new();
        let mut checked = 0usize;
        for entry in std::fs::read_dir(dir).expect("src dir readable") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|e| e != "rs")
                || path.file_name().is_some_and(|f| f == "app.rs")
            {
                continue;
            }
            checked += 1;
            let src = std::fs::read_to_string(&path).expect("source readable");
            for (n, line) in src.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue; // prose may name them; only code may not
                }
                for fam in FAMILIES {
                    if line.contains(fam) {
                        offenders.push(format!(
                            "{}:{} spells `{fam}`",
                            path.file_name().unwrap_or_default().to_string_lossy(),
                            n + 1,
                        ));
                    }
                }
            }
        }
        assert!(
            checked > 0,
            "no sibling sources scanned — the walk is broken"
        );
        assert!(
            offenders.is_empty(),
            "`AppShell::request_quit`'s map says both termination families live \
             in app.rs, and it is the census every reader trusts. These sites \
             are outside it, so they are ways this app ends that the map does \
             not know about: {offenders:#?}"
        );
    }

    #[test]
    fn r1364_the_termination_map_is_the_whole_map() {
        let documented = documented_sites();
        let actual = source_sites();
        assert_eq!(
            documented, actual,
            "\nThe termination map on `AppShell::request_quit` disagrees with \
             this file's source.\n  documented: {documented:#?}\n  actual:     \
             {actual:#?}\nEvery `event_loop.exit()` / `std::process::exit` in \
             production code is a way this app ends, and the map is the only \
             place they are collected. R1363 proved a stale one is silent."
        );
    }
}
