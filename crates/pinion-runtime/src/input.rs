//! R48 §5.35 input dispatch primitive — cursor/key → widget routing.
//!
//! [`InputRouter`] owns the framework-side input retention and dispatch
//! that R47 (hello-button hit-test fix) had to implement at the
//! application level. By moving it into pinion-runtime, every example
//! and every future widget catalog entry (R47+ Slider / Toggle /
//! `TextField`) shares the same routing — the R47-class bug (cursor on
//! background still drives the widget SCXML) cannot reappear in
//! application code because application code no longer owns the
//! routing.
//!
//! ## Lifecycle
//!
//! ```text
//!   ┌─ winit CursorMoved ──────┐
//!   │                          ▼
//!   │   router.cursor_moved(id, x, y, &mut state_scene)
//!   │       │  re-resolve hover_targets[id] from last paint scene
//!   │       │  PointerEnter/Leave dispatch on tag transition
//!   │       ▼
//!   ┌─ winit MouseInput Press ─┐
//!   │   router.pointer_down(id, &mut state_scene)
//!   │       │  PointerDown to hover_targets[id] (no-op when none)
//!   │       ▼
//!   ┌─ winit MouseInput Release┐
//!   │   router.pointer_up(id, &mut state_scene)
//!   │       │  PointerUp to hover_targets[id] (no-op when none)
//!   │       ▼
//!   ┌─ winit CursorLeft ───────┐
//!   │   router.cursor_left(id, &mut state_scene)
//!   │       │  drop cursor for id, rollback in-flight Hover
//!   │       ▼
//!   ┌─ post-render ────────────┐
//!   │   router.update_paint_scene(paint_scene, &mut state_scene)
//!   │       │  retain paint scene, refresh hover_targets for
//!   │       │  every active pointer (handles window resize moving
//!   │       │  a widget under a stationary cursor)
//!   └──────────────────────────┘
//! ```
//!
//! ## Tag matching
//!
//! The hit-test walks the *paint* scene's tagged Container / Box /
//! Path / Image / Text nodes (§5.20 [`Scene::tag`]) — these carry the
//! visual layout for the cursor to land on. The dispatch target is the
//! corresponding *state* scene's [`ExternalNode`] with the same tag —
//! that node carries the live SCXML statechart (or any other §5.15
//! introspectable handle). Application code only needs to keep the
//! two scenes' tags in sync: the same `"main_btn"` literal on the
//! paint Container and the state [`ExternalNode`].
//!
//! ## Multi-pointer (R51.38 §5.35)
//!
//! Every input method takes a [`PointerId`] identifying the source
//! pointer. Mouse-driven shells pass [`PointerId::MOUSE`]; touch /
//! pen / future input sources mint distinct ids via
//! [`PointerId::touch`]. Per-pointer state (`cursor`, `hover_target`,
//! `captured_target`) lives in `HashMap<PointerId, _>` so two
//! simultaneous touches can drag two different widgets without
//! aliasing the capture lock. Single-pointer mouse shells observe no
//! behavioural change — the maps degenerate to a single entry under
//! `PointerId::MOUSE`. This is the first-design ratify for the
//! mobile / multi-touch axis; designing in capture-aliasing-by-default
//! and refactoring later was the carry-forward path the R51.38
//! substrate-first decision rejected.
//!
//! ## Out of scope (R48+ carry-forward)
//!
//! - Multi-target dispatch (capture / bubble). The current router
//!   picks the deepest tagged ancestor and dispatches once.
//! - Focus tab order + keyboard dispatch. v0 routes pointer events
//!   only; key events stay with the application until the focus model
//!   lands (carry).
//! - Touch event wiring at the shell layer. The router's API accepts
//!   touch pointers via [`PointerId::touch`], but no `pinion-shell`
//!   call site sources them yet — winit `Touch` event integration is
//!   a separate carry.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use pinion_core::composite_tag::{compose_send_payload, split_subindex};
use pinion_core::event::WheelDelta;
use pinion_core::external::{
    CaptureNormalize, DOCK_PANEL_DRAG_KIND, DragPayload, DragUpdate, DropPoint, IntrospectValue,
    OUTER_DOCK_MARGIN, OUTER_DOCK_ZONE_TAG,
};
use pinion_core::input::PointerWireEvent;
use pinion_core::scene::{ExternalNode, Rect, Scene};
use pinion_core::widgets::scroll::ScrollState;

/// R664 §5.49 — W3C UI Events `dblclick` time threshold (milliseconds).
/// Two consecutive `pointer_down` calls within this window on the same
/// target with a position delta under [`DOUBLE_CLICK_DIST_PX`] dispatch
/// a synthetic `DoubleClick` named event in addition to the second
/// `PointerDown`. 300 ms is the W3C-canonical default
/// (Web `UIEvent.detail` definition, Windows `GetDoubleClickTime`'s
/// system-tunable default, macOS `NSEvent.doubleClickInterval` default).
const DOUBLE_CLICK_TIME_MS: u128 = 300;

/// R664 §5.49 — W3C UI Events `dblclick` position tolerance (logical
/// pixels). Two consecutive presses within [`DOUBLE_CLICK_TIME_MS`] on
/// the same target must land within this Manhattan-distance window per
/// axis to qualify as a double-click; a small drag between the two
/// presses disqualifies (mirrors the Material 3 "intentional gesture"
/// + Cocoa `NSEvent.mouseLocation` tolerance). 5 logical px is the
///   `Material 3` + Cocoa convention.
const DOUBLE_CLICK_DIST_PX: f64 = 5.0;

/// R794 §5.51 — drag-vs-click distance (logical pixels). A pressed drag
/// source whose cursor moves more than this from the press point before
/// release is a **drag**, not a click: the router commits the drop via
/// [`External::drag_release`](pinion_core::external::External::drag_release)
/// and does *not* synthesize the trailing `PointerUp` (a drag and a click
/// are mutually exclusive — Qt `startDragDistance`, the DOM "no `click`
/// after a drag" rule). A press-release under this threshold is a click:
/// the drop resolves to the source (a no-op) and the `PointerUp` fires so
/// press-to-activate stays reachable. Owning this once makes click-vs-drag
/// a framework SSOT, so no click-activatable drag surface (file tree,
/// asset browser, kanban) re-derives it per binding. R879 relocated the
/// constant itself to `pinion-core::input` (the contract crate): a
/// capture-path External judging its own click-vs-drag (the node graph)
/// measures against the same value ([[helper-crate-home-ssot-axis]]).
use pinion_core::DragLatch;

/// R51.38 §5.35 — pointer identity used by every [`InputRouter`]
/// input method to route per-pointer cursor / hover / capture state.
/// Mouse events on every desktop platform pinion supports come from a
/// single (logical) source, so [`PointerId::MOUSE`] is a fixed `const`
/// and mouse-driven shells never allocate. Touch finger IDs route
/// through [`PointerId::touch`] which offsets by one so `PointerId(0)`
/// stays reserved for the mouse.
///
/// `Hash` + `Eq` + `Copy` so the routing tables can key on it without
/// allocation; `Debug` for diagnostic logging. The internal `u64`
/// width matches winit's `FingerId` to avoid lossy narrowing when
/// shells eventually wire touch events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PointerId(u64);

impl PointerId {
    /// The primary mouse pointer. Mouse events on every desktop
    /// platform are single-source, so this constant suffices —
    /// shells pass it unconditionally for every winit `CursorMoved`
    /// / `MouseInput` event. The reserved id `0` cannot collide with
    /// any [`PointerId::touch`] result because that factory offsets
    /// by one.
    pub const MOUSE: PointerId = PointerId(0);

    /// Touch-finger pointer id. The factory offsets by one so a
    /// `winit::event::Touch::id` of `0` maps to `PointerId(1)`,
    /// keeping `PointerId(0)` reserved for [`MOUSE`]. Wrapping
    /// addition handles the (theoretical) `u64::MAX` finger id edge
    /// without panic — wrap-around lands at `PointerId(0)` which
    /// then aliases the mouse, but in practice no platform mints
    /// finger ids anywhere near that magnitude.
    #[must_use]
    pub fn touch(finger_id: u64) -> Self {
        PointerId(finger_id.wrapping_add(1))
    }

    /// Raw underlying value. Exposed for diagnostic logging and for
    /// shells that mint custom synthetic pointer IDs (e.g. pen input
    /// on platforms pinion adds later). Application code that just
    /// routes mouse + touch should prefer the [`MOUSE`] constant and
    /// the [`touch`](Self::touch) factory.
    #[must_use]
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// R51.108 §5.41 — abstract pointer touch phase, mirroring winit's
/// `TouchPhase` semantics without leaking the winit dependency into
/// the substrate. The shell-side `app.rs` (winit-coupled) converts
/// `winit::event::TouchPhase` to this enum at the window-system
/// boundary; the substrate (`ShellCore`) and runtime stay
/// backend-agnostic so future TUI (§5.41) / mobile / RPC-driven input
/// paths reuse the same vocabulary without an extra translation layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TouchPhase {
    /// Finger / pointer first contact (W3C `pointerdown` equivalent).
    Started,
    /// Finger / pointer position moved while contact maintained.
    Moved,
    /// Finger / pointer released cleanly (W3C `pointerup` equivalent).
    Ended,
    /// OS revoked the gesture (R51.93 §5.13 sibling of `pointer_up`) —
    /// system gesture, app switcher, notification pull-down,
    /// edge-swipe back, phone-call interrupt. Distinct from `Ended`:
    /// the substrate routes through `pointer_cancel` not `pointer_up`
    /// so the widget statechart sees `PointerCancel` and skips the
    /// click / toggle / value-committed intent the user did not
    /// authorise.
    Cancelled,
}

/// R51.108 §5.41 — pointer touch event, the abstract counterpart of
/// `winit::event::Touch`. Carries the OS-assigned finger id (raw,
/// pre-`PointerId::touch` offset), contact location in logical
/// (DPI-aware) pixels per §5.13 coord, and the phase. The shell-side
/// `app.rs` maps `winit::event::Touch::id` to this struct's `id`
/// field directly; the substrate immediately wraps via
/// `PointerId::touch(id)` so two simultaneous fingers route to
/// distinct routers without aliasing the capture lock (R51.38 §5.35).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touch {
    /// OS-assigned finger id (raw, pre-`PointerId::touch` offset).
    pub id: u64,
    /// Contact x position in logical (DPI-aware) pixels.
    pub x: f64,
    /// Contact y position in logical (DPI-aware) pixels.
    pub y: f64,
    /// Phase of the touch lifecycle.
    pub phase: TouchPhase,
}

/// R51.108 §5.41 — abstract modifier-key state. Defined in
/// `pinion-core` since R56.1.f.0 so the widget catalog's
/// `WidgetCore::apply_key` signature can carry the four-bit modifier
/// state without inverting the crate graph; re-exported here for
/// downstream call-sites (`pinion-shell` / `pinion-tui`) that have
/// already aged on `pinion_runtime::Modifiers`. See
/// [`pinion_core::input::Modifiers`] for the canonical definition,
/// W3C `KeyboardEvent` surface mirror, and the four accessor methods
/// (`shift_key` / `control_key` / `alt_key` / `meta_key`).
pub use pinion_core::input::Modifiers;

/// Framework-side input dispatch primitive. Owns retained paint scene,
/// cursor state, and hover target; dispatches winit-side input events
/// to the state scene's matching [`ExternalNode`] via the
/// `introspect_mut().invoke("send", Text(<event name>))` channel
/// (§5.15 item 5 input forwarding).
///
/// Application code calls into the router on every winit input event
/// and once per frame to refresh the retained paint scene. The router
/// does the hit-test, decides which widget should receive each event,
/// and dispatches through the same channel that `pinion_rpc::dispatch`
/// uses for AI-driven `scene/invoke` calls — the §2 invariant #2
/// ("RPC headless as AI primary path") stays literal: a human cursor
/// and an AI agent both reach the SCXML through the same
/// `invoke("send", ...)` path.
///
/// Widget catalog R47+ (`Slider` / `Toggle` / `TextField`) plugs in by
/// attaching a tag on its paint Container and a matching tag on its
/// state [`ExternalNode`]. No application-level hit-test code is
/// needed — adding a new widget cannot reintroduce the R47-class bug
/// because the routing primitive is framework-owned.
#[derive(Debug, Default)]
pub struct InputRouter {
    /// Last-rendered paint scene (post-layout). `None` until the
    /// first [`update_paint_scene`](Self::update_paint_scene) call.
    /// The router holds it across input events so hit-tests don't
    /// need a fresh `view()` rebuild per cursor move.
    last_paint_scene: Option<Scene>,
    /// R51.38 §5.35 — per-pointer cursor position in window physical
    /// pixels. Absence means the pointer is outside the window or
    /// has never entered. Mouse-driven shells observe a single
    /// `PointerId::MOUSE` entry; touch / pen shells route each
    /// finger / stylus through its own [`PointerId`].
    cursors: HashMap<PointerId, (f64, f64)>,
    /// R51.38 §5.35 — per-pointer hover target tag. Empty when no
    /// pointer is over a tagged region. Drives `PointerEnter` /
    /// `PointerLeave` dispatch and gates `PointerDown` /
    /// `PointerUp` per pointer, so two simultaneous touches can sit
    /// on two different widgets without aliasing.
    hover_targets: HashMap<PointerId, String>,
    /// R51.34 §5.35 + R51.38 §5.35 — per-pointer capture-lock map:
    /// tag of the widget each pointer claimed on its most recent
    /// `pointer_down` via [`External::wants_pointer_capture`]. While
    /// an entry is present, every
    /// [`cursor_moved`](Self::cursor_moved) for that pointer skips
    /// [`refresh_hover`](Self::refresh_hover) and forwards the
    /// cursor position to the widget's
    /// [`External::pointer_move`](pinion_core::external::External::pointer_move).
    /// Cleared on [`pointer_up`](Self::pointer_up); the subsequent
    /// `refresh_hover` fires the deferred `PointerLeave` if the
    /// cursor strayed off the widget during the drag. `cursor_left`
    /// is suppressed for that pointer while capture is in flight so
    /// the drag survives the window-leave / re-enter cycle. Multi-
    /// touch drags (two fingers, two widgets) each get an
    /// independent entry — the R51.38 first-design ratify avoids
    /// the aliasing-by-default refactor cost of single-target
    /// capture.
    captured_targets: HashMap<PointerId, String>,
    /// R51.40 §5.35 — per-pointer cached
    /// [`External::wants_pointer_capture`] flag for the widget under
    /// the corresponding `hover_targets` entry. Refreshed in the
    /// same hover walk as [`refresh_hover`], so [`pointer_down`]
    /// reads a bit instead of re-walking the scene tree. The cache
    /// stays consistent with the hover lifecycle: dropped when a
    /// pointer's hover clears, replaced when it moves between
    /// tagged widgets, never read while capture is in flight (the
    /// `captured_targets` map already pins the answer for that
    /// pointer). Relies on [`External::wants_pointer_capture`]
    /// being effectively constant per widget instance — the
    /// documented industry precedent (Button=false, Slider=true)
    /// and pinion's own widget catalog all return static bools.
    hover_wants_capture: HashMap<PointerId, bool>,
    /// R664 §5.49 — per-pointer "last `pointer_down` we dispatched"
    /// snapshot used by [`pointer_down`](Self::pointer_down) to detect
    /// the W3C `UIEvent.detail == 2` double-click pattern: the next
    /// press lands within [`DOUBLE_CLICK_TIME_MS`] on the same target
    /// with a position delta below [`DOUBLE_CLICK_DIST_PX`] per axis.
    /// `(instant, x, y, target_tag)` tuple — `instant` is the press
    /// timestamp, `(x, y)` is the cursor at press time (logical pixels),
    /// `target_tag` is the resolved hit-test target so a 2nd press on a
    /// *different* widget never triggers a stale double-click. Cleared
    /// after a double-click fires so the next 2-click cycle starts
    /// fresh (a triple-click is *not* the same as detail=2 + detail=3
    /// in the W3C spec — pinion sticks to the binary single/double
    /// distinction until a 2nd consumer requests triple, per
    /// `[[abstraction-needs-second-consumer]]`).
    last_press: HashMap<PointerId, (Instant, f64, f64, String)>,
    /// R742 §5.51 — per-pointer in-flight drag session. Present between a
    /// [`pointer_down`](Self::pointer_down) whose target returned `Some`
    /// from [`External::begin_drag`](pinion_core::external::External::begin_drag)
    /// and the matching [`pointer_up`](Self::pointer_up). While present,
    /// [`cursor_moved`](Self::cursor_moved) resolves the drop location
    /// under the *absolute* cursor and forwards it to the **source**
    /// widget via
    /// [`External::drag_to`](pinion_core::external::External::drag_to),
    /// suppressing hover re-resolution so the source statechart sees no
    /// spurious `PointerLeave` mid-drag (the capture-lock guarantee, for
    /// the drag path). [`pointer_up`](Self::pointer_up) commits via
    /// [`External::drag_release`](pinion_core::external::External::drag_release)
    /// and clears the entry. Keyed by [`PointerId`] so two simultaneous
    /// touch-drags stay independent, mirroring `captured_targets`. The
    /// map is disjoint from `captured_targets` for every current consumer
    /// (a reorder source does not opt into `wants_pointer_capture`), so
    /// the two mechanisms never contend for one pointer.
    drag_sessions: HashMap<PointerId, DragSession>,
    /// R876 §5.49 §5.51 — per-pointer press-to-drag tracker: the single
    /// router answer to "has this held press strayed far enough to be a
    /// *drag* rather than a *click*?" Opened on [`pointer_down`](Self::pointer_down)
    /// (origin = the press cursor), advanced by
    /// [`cursor_moved`](Self::cursor_moved) (the R880 [`DragLatch`]
    /// contract predicate — Euclidean over
    /// [`DRAG_CLICK_THRESHOLD_PX`](pinion_core::DRAG_CLICK_THRESHOLD_PX)),
    /// cleared on
    /// [`pointer_up`](Self::pointer_up). It unifies the two click-vs-drag
    /// consumers behind one metric + one threshold: the R794 trailing-click
    /// suppression (a moved drag must not also activate its source) and the
    /// R875 double-click invalidation (a press that became a drag must not
    /// seed a `DoubleClick`). Covers every press flavour — capture
    /// (slider / scrub), `begin_drag` `DnD`, and plain free-mode — so no
    /// gesture path re-derives the determination ([[drag-release-trailing-pointerup-suppress]]).
    press_gestures: HashMap<PointerId, DragLatch>,
    /// R881 §5.35 §5.49 — per-pointer in-flight pan-class gesture.
    /// Opened by [`middle_down`](Self::middle_down) (the middle-button
    /// chord) or [`left_pan_down`](Self::left_pan_down) (R882 — the
    /// shell's Space-hold chord routing a left press into the pan
    /// channel), advanced by [`cursor_moved`](Self::cursor_moved) (the
    /// pan arm), consumed by the matching-button release
    /// ([`middle_up`](Self::middle_up) / [`left_pan_up`](Self::left_pan_up)),
    /// revoked by [`pointer_cancel`](Self::pointer_cancel). A pan press
    /// is a *gesture chord*, not a routed widget event: a latched move
    /// is drag-to-pan (Blender / Unreal / Figma hand-tool family); a
    /// release-in-place is button policy — the middle chord's X11
    /// PRIMARY paste (deferred to release — xterm / Qt convention), the
    /// left chord's no-op (Figma: Space+click is inert). One map for
    /// every opening button so gesture exclusivity (one pan-class
    /// gesture per pointer, first press wins) needs no cross-map
    /// bookkeeping. See [`PanGesture`].
    pan_gestures: HashMap<PointerId, PanGesture>,
    /// R881.1 §5.35 — per-pointer wheel-side sub-pixel remainder (the
    /// stage-2 carry of [`dispatch_wheel_two_stage`]). Keyed to the
    /// scroll container it accumulated against via a [`Weak`] handle:
    /// the carry resets when the pointer's resolved scroll target
    /// changes (a remainder must never leak across containers — Qt's
    /// accumulator discipline) and drops with the cursor on
    /// [`cursor_left`](Self::cursor_left). The middle pan keeps its
    /// remainder in its own gesture state instead — one carry per
    /// contiguous delta stream, whichever producer owns the stream.
    wheel_remainders: HashMap<PointerId, WheelRemainder>,
    /// (R1107 §5.16 §5.41 §5.51) This router's own window spec id — the
    /// window it dispatches for. `None` until the shell stamps it via
    /// [`ensure_window`](Self::ensure_window) at the per-window dispatch
    /// choke. Filled into [`DragUpdate::source_window`] so a drag source can
    /// convert its window-logical cursor to a desktop position via the
    /// CORRECT window's outer origin (a re-dragged floating header reports a
    /// cursor in its own frame, not the main window's). The router otherwise
    /// stays window-id-blind for cross-window resolution (that rides the
    /// shell-composed `over_window`); knowing its OWN id is a different, local
    /// fact.
    window_id: Option<String>,
    /// (R1203 §5.51 §5.39) Logical-pixel height the DOCK AREA is inset from the
    /// window's top edge — the client-side chrome strip height, or `0` for an
    /// OS-decorated / naked window. The shell stamps it per window
    /// ([`crate::CoreShell::set_dock_area_top_inset_for_window`]) from its
    /// `chrome_inset_height`. [`Self::resolve_own_outer_dock`] measures the
    /// same-window OUTER band against `paint.rect()` shrunk by this, so the top
    /// full-span band sits at the DOCK's top edge (below the min / max / close
    /// controls) — not up in the chrome strip. The R1202 peer for the same-window
    /// band: R1202 fixed the cross-window preview rect (shell-side), this fixes
    /// the same-window band membership (router-side) so the band and the preview
    /// agree on where the dock area is.
    dock_area_top_inset: u32,
}

/// R881.1 §5.35 — one pointer's wheel remainder: the scroll container
/// the fraction accumulated against (identity-compared on the next
/// event; a dead `Weak` or a different container resets the carry) and
/// the banked sub-pixel fraction per axis.
#[derive(Debug)]
struct WheelRemainder {
    target: std::rc::Weak<ScrollState>,
    frac: (f32, f32),
}

/// R882 §5.35 — which physical button opened a pan-class gesture.
/// Internal discriminator only (NOT a wire vocabulary — the RPC
/// `scene/drag` button rides `pinion-rpc`'s `DragButton` and reaches
/// this router through the per-button shell methods): a release closes
/// a gesture only when its button matches, so a left release can never
/// consume a live middle pan or vice versa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanButton {
    /// The primary button routed into the pan channel by the shell's
    /// R882 Space-hold chord (the Figma / Photoshop hand tool).
    Left,
    /// The middle (W3C auxiliary) button — the chord-free R881 pan.
    Middle,
}

/// R881 §5.35 §5.49 — one held pan-channel press. `pan` is `None`
/// only when the press arrived before any `cursor_moved` seeded a
/// cursor for the pointer (then the press can never pan — there is no
/// origin to latch against — and release degrades to the click
/// path, the pre-R881 behaviour).
#[derive(Debug)]
struct PanGesture {
    /// The button that opened this gesture — release / in-flight
    /// queries match on it (R882).
    button: PanButton,
    /// R882.1 §5.35 — count of *swallowed* same-button presses that
    /// arrived while this gesture owned the pointer (an RPC injection
    /// racing a native hold — one physical button cannot double-press).
    /// Each such press is refused (first press wins), so its matching
    /// release must NOT consume the gesture either: `pan_up` drains
    /// this counter first and reports [`PanRelease::NoPress`] for the
    /// stray pair. Without it, an injected same-button click mid-pan
    /// would end the user's gesture early and the user's real release
    /// would fall through as an orphan free-mode `PointerUp` — the
    /// exact phantom-activation hazard the R881.1 exclusivity arms
    /// exist to prevent (its cross-button half; this is the
    /// same-button half).
    swallowed_presses: u32,
    pan: Option<PanState>,
}

/// R881 §5.35 §5.49 — the drag-to-pan state (any opening button).
/// Targets are **pinned at press** (gesture-capture semantics): a pan
/// must keep driving the scrollable it started on even when the moving
/// content slides a different container under the cursor mid-gesture —
/// per-move re-resolution would hop containers, which no native pan
/// implementation does.
#[derive(Debug)]
struct PanState {
    /// The click-vs-pan dead zone — the R880 [`DragLatch`] contract
    /// predicate, its 2nd direct capture-path consumer. Until it
    /// latches, the press is still a paste-click candidate; once
    /// latched the gesture is a pan for its lifetime (release never
    /// pastes).
    latch: DragLatch,
    /// Cursor at the previous pan dispatch — each move dispatches the
    /// `last - current` delta (content follows the cursor, the grab
    /// convention every canvas pan implements).
    last: (f64, f64),
    /// Sub-pixel remainder carried between moves on the integer
    /// [`ScrollState::scroll_by`] branch (the Qt wheel-remainder
    /// accumulator) so a slow high-DPI pan whose per-event delta
    /// rounds to zero still accumulates motion.
    frac: (f32, f32),
    /// Hover target tag at press (full routed, possibly composite,
    /// form) — the pinned stage-1 wheel-offer recipient, mirroring
    /// [`InputRouter::wheel_with_modifiers`]'s two-stage routing.
    tag: Option<String>,
    /// Deepest attached [`ScrollState`] under the press point — the
    /// pinned stage-2 fallback recipient.
    scroll: Option<Rc<ScrollState>>,
}

/// R881 §5.35 — what a pan-channel release resolved to (renamed from
/// `MiddleRelease` in R882, when the left button gained the Space-hold
/// chord entry into the same channel). The router owns the
/// click-vs-pan determination (the [`DragLatch`] SSOT); the *action*
/// on `Click` is per-button shell policy — the middle chord pastes
/// (`ShellCore::middle_click`, the X11 PRIMARY funnel), the left
/// Space-chord is inert (Figma: Space+click does nothing) — substrate
/// decides the gesture, backend decides the action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanRelease {
    /// The press latched into a drag-to-pan; the pan deltas were
    /// already dispatched move-by-move. Release performs no action.
    Pan,
    /// Press-release in place (never strayed past the dead zone) —
    /// the canonical click verdict. The shell applies its per-button
    /// policy (middle: paste funnel; left Space-chord: no-op).
    Click,
    /// No gesture was in flight for this pointer (or none opened by
    /// the releasing button): the press was never seen, a different
    /// button owns the gesture, or
    /// [`InputRouter::pointer_cancel`] revoked it (a cancelled
    /// gesture is "never happened" — it must not paste).
    NoPress,
}

/// R742 §5.51 — in-flight drag state owned by the [`InputRouter`].
/// Constructed on `pointer_down` when the pressed widget arms a drag,
/// consumed on `pointer_up`.
#[derive(Debug)]
struct DragSession {
    /// Full paint tag of the widget the drag started on — the source
    /// coordinator that receives `drag_to` / `drag_release`. Its primary
    /// half (`split_subindex`) resolves the `ExternalNode` to forward to.
    source_tag: String,
    /// The payload [`External::begin_drag`](pinion_core::external::External::begin_drag)
    /// produced, carried back to the source on every update so a future
    /// cross-widget target can match on it without the router re-deriving
    /// it.
    payload: DragPayload,
    /// R1102 §5.51 PR-33 — the shell's cross-window drop resolution for the
    /// CURRENT cursor (another window's dock zone the abs cursor maps onto,
    /// in THAT window's local frame), or `None` when the cursor is over no
    /// other window's drop target. The per-window router is cross-window-blind
    /// (it sees only its own scene), so the shell — the sole holder of every
    /// window's geometry — resolves this each move via
    /// [`set_drag_cross_window`](InputRouter::set_drag_cross_window) and the
    /// drag dispatch reads it to fill [`DragUpdate::over_window`] when the
    /// cursor escaped this window's own drop targets (own-window resolve
    /// first). Lifecycle-scoped to the session, so it clears with the drag.
    cross_window: Option<CrossWindowDrop>,
    /// (R1117 §5.15 §5.51) The window-logical cursor when this session opened
    /// (the PRESS point). Captured once at `begin_drag` from the router's held
    /// cursor and forwarded verbatim as [`DragUpdate::press_cursor`] on every
    /// update, so a grab-offset drag (window move by a title bar) anchors at the
    /// exact press, not the first move sample.
    ///
    /// **Deliberately duplicates `press_gestures[id]`'s `DragLatch` origin** (the
    /// click-vs-drag axis holds the same press point). It is a separate field, not
    /// a read of that latch, because `press_gestures` is removed in `pointer_up`
    /// BEFORE this session's release `DragUpdate` is built — so the session keeps
    /// its own copy to survive into the release-path forward. The two are seeded
    /// from the same `self.cursors[id]` at the same pointer-down, so they cannot
    /// diverge.
    press_cursor: (f64, f64),
}

impl InputRouter {
    /// Construct an empty router. No retained paint scene, no
    /// cursors, no hover targets, no capture locks.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// (R1107 §5.16 §5.41 §5.51) Stamp this router's own window spec id the
    /// first time the shell dispatches for it (idempotent — a router's window
    /// never changes, so the first stamp wins and later calls are no-ops).
    /// The shell calls this at the per-window drag-dispatch choke (it holds
    /// the `window_id` map key), so [`DragUpdate::source_window`] is set
    /// before any drag builds an update. A router that never saw a drag
    /// dispatch keeps `None` (harmless — `source_window` then falls back).
    pub fn ensure_window(&mut self, window_id: &str) {
        if self.window_id.is_none() {
            self.window_id = Some(window_id.to_owned());
        }
    }

    /// Current hover target tag for `id`, when any. Mainly for tests
    /// and diagnostic logging; application dispatch should not need
    /// to inspect this directly.
    #[must_use]
    pub fn hover_target(&self, id: PointerId) -> Option<&str> {
        self.hover_targets.get(&id).map(String::as_str)
    }

    /// R762 §5.36 §5.38 — last known cursor position (window-local
    /// logical pixels) for `id`, when the pointer has reported a move.
    /// The press handlers read this to feed text hit-test
    /// (click-to-position-caret) — `cursor_moved` runs before
    /// `pointer_down` in every press path (native winit `MouseInput`
    /// and the `scene/click` deferred-input drain), so the stored
    /// position is the press location.
    #[must_use]
    pub fn cursor_position(&self, id: PointerId) -> Option<(f64, f64)> {
        self.cursors.get(&id).copied()
    }

    /// (R1196 §5.16 §5.39) The hover [`CursorHint`] the deepest hinted node
    /// under pointer `id` declares, resolved against the last painted scene
    /// ([`Scene::cursor_hint_at`]). `None` when the pointer is over no hinted
    /// region, no move has been reported, or nothing has been painted. The
    /// cursor-axis sibling of [`Self::hover_target`]: the shell reads it every
    /// `cursor_moved` and maps the hint to a backend `CursorIcon`.
    #[must_use]
    pub fn cursor_hint(&self, id: PointerId) -> Option<pinion_core::style::CursorHint> {
        let (x, y) = self.cursor_position(id)?;
        self.last_paint_scene()?
            .cursor_hint_at(floor_clamp_u32(x), floor_clamp_u32(y))
    }

    /// R1102 §5.51 PR-33 — whether a drag session this router owns is in flight
    /// for `id`. The shell reads this to gate the (otherwise per-move) cross-
    /// window resolution: only an active drag needs a cross-window drop
    /// computed, so an idle hover pays nothing.
    #[must_use]
    pub fn drag_session_active(&self, id: PointerId) -> bool {
        self.drag_sessions.contains_key(&id)
    }

    /// (R1113 §5.51 §5.33 §2 #7) The in-flight drag's display label + the
    /// window-logical cursor it is at — the projection the shell injects as a
    /// drag-image overlay ([`pinion_overlay::inject_drag_image`]), the way it
    /// reads focus state to inject the focus ring. `Some` only once the press
    /// became a REAL drag (the [`press_became_drag`](Self::press_became_drag)
    /// click-vs-drag SSOT, so a pending click shows no follower) AND the
    /// payload carries a non-empty text label AND a cursor is known. A
    /// capture-drag (a splitter resize — no [`begin_drag`] session) has no
    /// session, so it never shows a follower. No new state: a pure projection
    /// of the session the router already owns.
    #[must_use]
    pub fn active_drag_label(&self, id: PointerId) -> Option<(String, (f64, f64))> {
        if !self.press_became_drag(id) {
            return None;
        }
        let session = self.drag_sessions.get(&id)?;
        let IntrospectValue::Text(label) = &session.payload.value else {
            return None;
        };
        if label.is_empty() {
            return None;
        }
        let cursor = self.cursor_position(id)?;
        Some((label.clone(), cursor))
    }

    /// R1102 §5.51 PR-33 — stash the shell's cross-window drop resolution for
    /// the in-flight drag on `id` (the window whose dock zone the abs cursor
    /// currently maps onto, plus the drop point in THAT window's local frame),
    /// or clear it (`None`) when the cursor maps onto no other window's drop
    /// target. No-op when no session owns `id`. The drag dispatch
    /// ([`update_drag`](Self::update_drag) / [`pointer_up`](Self::pointer_up))
    /// reads it to fill [`DragUpdate::over_window`] once this window's own drop
    /// resolution comes up empty (own-window first). The router itself stays
    /// cross-window-blind — it only *consumes* what the shell, holding every
    /// window's geometry, resolved.
    pub fn set_drag_cross_window(&mut self, id: PointerId, drop: Option<CrossWindowDrop>) {
        if let Some(session) = self.drag_sessions.get_mut(&id) {
            session.cross_window = drop;
        }
    }

    /// (R1125 §5.51 PR-33) The cross-window drop the shell last stashed for the
    /// in-flight drag on `id`, if any — the symmetric READ of
    /// [`set_drag_cross_window`](Self::set_drag_cross_window). The shell scans every
    /// window's router with this to find a drag whose drop targets a GIVEN window
    /// ([`CoreShell::cross_window_drag_into`](crate::CoreShell::cross_window_drag_into)),
    /// so it can paint that window's incoming drop-zone preview. `None` when no
    /// session owns `id` or the cursor is over no other window.
    #[must_use]
    pub fn drag_cross_window(&self, id: PointerId) -> Option<&CrossWindowDrop> {
        self.drag_sessions.get(&id)?.cross_window.as_ref()
    }

    /// R51.34 §5.35 — current capture-lock target tag for `id`, when
    /// that pointer claimed a widget via
    /// [`External::wants_pointer_capture`] on its most recent
    /// [`pointer_down`](Self::pointer_down). `None` when no drag is
    /// in flight for that pointer. Diagnostic / test surface only —
    /// application code never needs to inspect this directly.
    #[must_use]
    pub fn captured_target(&self, id: PointerId) -> Option<&str> {
        self.captured_targets.get(&id).map(String::as_str)
    }

    /// (R684 §5.35 §5.41 §5.16) Read-only predicate — whether the
    /// router has ever received a paint scene via
    /// [`Self::update_paint_scene`]. The substrate-level signal that
    /// the per-window `InputRouter` is "primed" for hit-testing /
    /// drag dispatch / hover resolution. Pre-R684 callers had no
    /// substrate-visible way to distinguish a never-painted router
    /// from one whose `last_paint_scene` carried an empty
    /// [`Scene::Container`]; R684 atomic 3's headless-RPC finalize
    /// uses this predicate to skip the post-dispatch finalize when
    /// the live winit paint loop already populated the router.
    #[must_use]
    pub fn has_last_paint_scene(&self) -> bool {
        self.last_paint_scene.is_some()
    }

    /// (R705 §5.12 §2 #7) Read-only borrow of the most recently
    /// painted scene — the exact tree that produced the pixels on
    /// screen (the winit paint loop stores it via
    /// [`Self::update_paint_scene`] at the end of every frame; the
    /// headless-RPC finalize stores it via [`Self::set_paint_scene`]).
    ///
    /// `scene/snapshot from: paint` serializes THIS borrow instead of
    /// re-running `V::view` at query time, so introspection equals the
    /// displayed frame *by construction* rather than by the two
    /// renderers happening to agree. Re-rendering at query time was the
    /// §2 #7 violation R705 closes: a state mutation that had not yet
    /// repainted left the screen showing one frame while a query-time
    /// re-render produced another ([[introspection-from-paint-not-screen]]).
    ///
    /// `None` until the router has received its first paint scene
    /// (never-painted window); the snapshot handler falls back to the
    /// paint producer in that bootstrap window.
    #[must_use]
    pub fn last_paint_scene(&self) -> Option<&Scene> {
        self.last_paint_scene.as_ref()
    }

    /// Update the retained paint scene after each render. Re-resolves
    /// `hover_targets` for every active pointer against the new
    /// layout — a window resize may move the button rect under a
    /// stationary cursor, and the resulting `PointerEnter` /
    /// `PointerLeave` transitions fire here so the SCXML matches the
    /// new visual state on the next frame. Pointers under capture
    /// lock keep their hover pinned (the drag invariant).
    ///
    /// (R685 §5.16 §5.35) Composition of [`Self::set_paint_scene`] (pure
    /// storage write) + [`Self::refresh_hover_for_all_active_pointers`]
    /// (synthetic hover-arc dispatch). Pre-R685 the two responsibilities
    /// were inlined here; the split lets RPC paths refresh hit-test
    /// geometry without firing synthetic hover arcs — the R660 `RadioGroup`
    /// regression bisect end-to-end (R684 atomic 3 worked around via a
    /// first-paint-only gate; R685 lands the proper substrate split so
    /// every-RPC refresh is safe).
    pub fn update_paint_scene(&mut self, scene: Scene, state_scene: &mut Scene) {
        self.set_paint_scene(scene);
        self.refresh_hover_for_all_active_pointers(state_scene);
    }

    /// (R685 §5.16 §5.35) Pure-storage paint-scene write — no
    /// `refresh_hover` side effect.
    ///
    /// Splits the R51.39 [`Self::update_paint_scene`] composition into
    /// (a) storage + (b) side-effect refresh so RPC paths that need
    /// fresh hit-test geometry **without** firing synthetic
    /// `PointerEnter` / `PointerLeave` arcs have a safe primitive.
    /// Pre-R685 the only path was the composed
    /// [`Self::update_paint_scene`], which made every per-RPC refresh
    /// double-fire hover transitions and mutate widget state in ways
    /// the application did not request (R660 `RadioGroup` End-key
    /// regression caught the issue end-to-end; R684 atomic 3 worked
    /// around it via a first-paint-only gate; R685 lands the textbook
    /// split).
    ///
    /// Use cases for the storage-only path:
    ///
    /// * RPC `scene/drag` / `scene/click` after a state change moved
    ///   the hit-test geometry — the AI client wants the next hit-test
    ///   to see fresh rects, but the synthetic hover arcs from a real
    ///   user "didn't move" the cursor (the cursor stayed put; only
    ///   the layout shifted under it).
    /// * Headless / scripted scenarios where firing input arcs would
    ///   pollute the SCXML transition log captured for assertions.
    ///
    /// The composed [`Self::update_paint_scene`] is still the right
    /// choice for live winit paint cycles (where a real
    /// `RedrawRequested` reflects either a state change with implicit
    /// pointer re-targeting, or a window resize moving widgets under a
    /// stationary cursor — both want the hover arcs).
    pub fn set_paint_scene(&mut self, scene: Scene) {
        self.last_paint_scene = Some(scene);
    }

    /// (R1203 §5.51 §5.39) Stamp the DOCK-AREA top inset (the client-side chrome
    /// strip height, `0` for OS-decorated) the shell resolves for this window, so
    /// [`Self::resolve_own_outer_dock`] measures the same-window OUTER band
    /// against the dock area — below the chrome — not the whole window. See
    /// [`Self::dock_area_top_inset`].
    pub fn set_dock_area_top_inset(&mut self, inset: u32) {
        self.dock_area_top_inset = inset;
    }

    /// (R1203 §5.51 §5.39) The stamped DOCK-AREA top inset. `0` until the shell
    /// stamps a chrome height (OS-decorated windows stay `0`).
    #[must_use]
    pub fn dock_area_top_inset(&self) -> u32 {
        self.dock_area_top_inset
    }

    /// (R685 §5.16 §5.35) Refresh-hover side-effect half of the R51.39
    /// `update_paint_scene` split. Walks every non-captured pointer +
    /// re-evaluates its hover target against the current
    /// `last_paint_scene`, dispatching synthetic `PointerEnter` /
    /// `PointerLeave` if the deepest-tagged hit changed.
    ///
    /// Lives next to [`Self::set_paint_scene`] so the
    /// [`Self::update_paint_scene`] composition is mechanically
    /// derivable: write the scene, refresh hover. Pure side effect —
    /// the function takes `&mut Scene` because `refresh_hover` mutates
    /// the state scene through `dispatch_send`.
    pub fn refresh_hover_for_all_active_pointers(&mut self, state_scene: &mut Scene) {
        // Snapshot pointer ids before iterating — refresh_hover
        // takes &mut self and mutates `hover_targets`. Cloning the
        // key set keeps the multi-pointer iteration self-contained
        // (single-pointer shells: 1 entry, negligible cost).
        let ids: Vec<PointerId> = self.cursors.keys().copied().collect();
        for id in ids {
            // R881.1 — ONE predicate for "a gesture owns this pointer's
            // hover" across all three refresh producers (cursor_moved,
            // cursor_left, this per-repaint walk). Pre-R881.1 this arm
            // skipped only capture, so a DnD drag or a live middle pan
            // saw Enter/Leave churn fire on every repaint as content
            // slid under the cursor — the exact churn the gesture
            // suppression exists to prevent (one-gate discipline).
            if self.gesture_pins_hover(id) {
                continue;
            }
            self.refresh_hover(id, state_scene);
        }
    }

    /// R881.1 §5.35 — whether an in-flight gesture owns `id`'s hover:
    /// capture lock (R51.34), a `DnD` drag session (R742), or a live
    /// middle pan (R881). While true, every hover-refresh producer
    /// must leave the pinned hover untouched — the three gesture
    /// classes share ONE predicate so no producer can drift to a
    /// subset (the R873 one-gate discipline applied to hover).
    fn gesture_pins_hover(&self, id: PointerId) -> bool {
        self.captured_targets.contains_key(&id)
            || self.drag_sessions.contains_key(&id)
            || self.pan_live(id)
    }

    /// R876 §5.49 §5.51 — advance the press-to-drag tracker for `id` against
    /// its current cursor (the R880 [`DragLatch`] contract predicate over
    /// [`DRAG_CLICK_THRESHOLD_PX`](pinion_core::DRAG_CLICK_THRESHOLD_PX)).
    /// No-op when no press is in flight for
    /// `id`. The single producer of the click-vs-drag determination
    /// [`pointer_up`](Self::pointer_up) and the double-click detector both
    /// consume — see [`press_became_drag`](Self::press_became_drag).
    fn track_press_drag(&mut self, id: PointerId, x: f64, y: f64) {
        if let Some(gesture) = self.press_gestures.get_mut(&id) {
            gesture.advance((x, y));
        }
    }

    /// R876 §5.49 §5.51 — whether the in-flight press for `id` has strayed
    /// into a drag. `false` when no press is tracked (already released, or a
    /// press that never reached a tagged target). The click-vs-drag SSOT
    /// query: a moved drag must neither activate its source on release (R794)
    /// nor seed a `DoubleClick` (R875).
    fn press_became_drag(&self, id: PointerId) -> bool {
        self.press_gestures.get(&id).is_some_and(DragLatch::live)
    }

    /// winit `CursorMoved` handler. Stores the new cursor position
    /// under `id` then either:
    ///
    /// * **Capture mode** (R51.34 §5.35): when a drag-aware widget
    ///   holds the lock for this pointer, forward the cursor
    ///   position to its
    ///   [`External::pointer_move`](pinion_core::external::External::pointer_move)
    ///   as widget-relative normalised `(x_rel, y_rel)`. The hover
    ///   target stays pinned so the SCXML does not see spurious
    ///   `PointerLeave` events when the cursor strays off the
    ///   widget rect mid-drag.
    /// * **Free mode** (pre-R51.34 default): re-resolve this
    ///   pointer's hover target and dispatch `PointerEnter` /
    ///   `PointerLeave` on transitions — the canonical button-like
    ///   cancel-by-leave UX.
    ///
    /// Zero-modifier wrapper around
    /// [`cursor_moved_with_modifiers`](Self::cursor_moved_with_modifiers)
    /// (tests; backends reach the modifier-threading variant through
    /// `CoreShell`'s pair) — mirrors the
    /// [`pointer_up`](Self::pointer_up) /
    /// [`pointer_up_with_modifiers`](Self::pointer_up_with_modifiers)
    /// pair.
    pub fn cursor_moved(&mut self, id: PointerId, x: f64, y: f64, state_scene: &mut Scene) -> bool {
        self.cursor_moved_with_modifiers(id, x, y, Modifiers::empty(), state_scene)
    }

    /// R881 §5.35 §5.49 — [`cursor_moved`](Self::cursor_moved) carrying
    /// the held keyboard `modifiers` (the shell threads its out-of-band
    /// `ModifiersChanged` cache here, the R781 / R877 pattern). The
    /// modifiers feed the middle-button pan arm's wheel-vocabulary
    /// dispatch so a `Ctrl`+middle-drag reaches a canvas's `Ctrl`-zoom
    /// wheel arm exactly as a held `Ctrl`+wheel would (the Blender
    /// chord set, for free, because pan rides the wheel vocabulary).
    ///
    /// Returns `true` when an in-flight middle pan dispatched a delta
    /// this move (an `External` consumed it or a pinned [`ScrollState`]
    /// scrolled) — the backend's repaint cue, mirroring
    /// [`wheel_with_modifiers`](Self::wheel_with_modifiers)'s return.
    pub fn cursor_moved_with_modifiers(
        &mut self,
        id: PointerId,
        x: f64,
        y: f64,
        modifiers: Modifiers,
        state_scene: &mut Scene,
    ) -> bool {
        self.cursors.insert(id, (x, y));
        // R881 §5.35 §5.49 — advance the pan channel first (middle drag
        // or the R882 Space-chord left drag): while a pan is live the
        // pointer is captured by the gesture, so free-mode hover
        // Enter/Leave churn is suppressed below (the same pinning
        // capture and DnD already get).
        let (pan_live, pan_dispatched) = self.advance_pan(id, x, y, modifiers, state_scene);
        // R876 §5.49 §5.51 — advance the click-vs-drag SSOT, then let its two
        // consumers read it. Once this press has strayed into a drag it is no
        // longer a click candidate, so drop the `last_press` snapshot the
        // matching `pointer_down` recorded — a press-drag-press sequence (e.g.
        // two numeric scrubs back and forth over one property row) must not
        // read as a `DoubleClick` (W3C: only a press-release in place is a
        // `click`; native: a drag cancels the double-click cycle). The drag
        // determination is `track_press_drag`'s alone — same metric +
        // threshold the R794 trailing-click suppression reads — so no gesture
        // path re-derives it. A tracked press only exists between
        // `pointer_down` and `pointer_up`, so free-mode hover *between* two
        // genuine clicks (no press held) never clears the candidate.
        self.track_press_drag(id, x, y);
        if self.press_became_drag(id) {
            self.last_press.remove(&id);
        }
        if self.drag_sessions.contains_key(&id) {
            // R742 §5.51 — a drag started on this pointer: resolve the
            // drop location under the absolute cursor and forward it to
            // the source. Takes precedence over capture/free so the
            // source's hover stays pinned (no spurious mid-drag leave).
            self.update_drag(id, x, y, state_scene);
        } else if let Some(tag) = self.captured_targets.get(&id).cloned() {
            self.forward_pointer_move(state_scene, &tag, x, y);
        } else if !pan_live {
            self.refresh_hover(id, state_scene);
        }
        pan_dispatched
    }

    /// R881 §5.35 §5.49 — open a middle-button gesture for `id` (winit
    /// `MouseInput { Middle, Pressed }`). Dispatches **nothing**: the
    /// press is ambiguous between a paste-click and a drag-to-pan until
    /// the [`DragLatch`] resolves it, so the X11 PRIMARY paste that
    /// pre-R881 fired here is deferred to a release-in-place
    /// ([`middle_up`](Self::middle_up) → [`PanRelease::Click`]) —
    /// the xterm / Qt release-paste convention.
    ///
    /// Pan targets are pinned now, against the press point: the hover
    /// target tag (the stage-1 wheel-offer recipient) and the deepest
    /// attached [`ScrollState`] under the cursor (the stage-2
    /// fallback) — the same two-stage routing
    /// [`wheel_with_modifiers`](Self::wheel_with_modifiers) resolves
    /// per-event, frozen per-gesture here so a pan can never hop
    /// containers mid-drag.
    pub fn middle_down(&mut self, id: PointerId) {
        self.pan_down(id, PanButton::Middle);
    }

    /// R882 §5.35 §5.39 — open the pan channel for a **left** press: the
    /// shell routes a `MouseInput { Left, Pressed }` here instead of
    /// [`pointer_down`](Self::pointer_down) while its Space chord is held
    /// (the Figma / Photoshop / Krita hand tool). The press dispatches
    /// nothing to widgets — no `PointerDown`, no focus steal, no caret
    /// move — and pan targets pin exactly as a middle press would. The
    /// chord policy (which key arms the channel) is the shell's; the
    /// router only knows a left press entered the pan channel.
    pub fn left_pan_down(&mut self, id: PointerId) {
        self.pan_down(id, PanButton::Left);
    }

    /// R881 / R882 §5.35 — the shared pan-channel press arm.
    fn pan_down(&mut self, id: PointerId, button: PanButton) {
        // R881.1 §5.35 — gesture exclusivity: a pointer already owned
        // by a routed gesture (capture lock, DnD session, or a tracked
        // press) does not open a pan gesture — panning a container
        // *while* a slider drag or a DnD ride the same cursor would
        // feed each gesture the other's motion. The trailing release
        // then resolves `NoPress` (no paste / no action mid-drag). And
        // a pan gesture already open for `id` is never overwritten
        // regardless of button (one pan-class gesture per pointer,
        // first press wins — an RPC injection racing a native hold
        // cannot reset a live pan's latch back into the dead zone and
        // turn the user's pan into a paste-click).
        //
        // R882.1 — a refused SAME-button press is additionally counted
        // on the owning gesture so its matching release pairs with the
        // refusal (`NoPress`) instead of consuming the live gesture —
        // see [`PanGesture::swallowed_presses`].
        if let Some(gesture) = self.pan_gestures.get_mut(&id) {
            if gesture.button == button {
                gesture.swallowed_presses += 1;
            }
            return;
        }
        if self.captured_targets.contains_key(&id)
            || self.drag_sessions.contains_key(&id)
            || self.press_gestures.contains_key(&id)
        {
            return;
        }
        let pan = self
            .cursors
            .get(&id)
            .copied()
            .map(|origin| self.pin_pan_targets(id, origin));
        self.pan_gestures.insert(
            id,
            PanGesture {
                button,
                swallowed_presses: 0,
                pan,
            },
        );
    }

    /// R881.1 §5.35 — pin the pan state for a gesture whose origin is
    /// `origin`: the dead-zone latch plus the two-stage targets (hover
    /// `External` tag + deepest attached [`ScrollState`]) resolved at
    /// that point. Shared by the press-time seed (`middle_down`) and
    /// the first-move lazy seed (`advance_pan` — a press that
    /// arrived before any cursor seeds at the first position the
    /// gesture learns).
    fn pin_pan_targets(&self, id: PointerId, origin: (f64, f64)) -> PanState {
        let scroll = self.last_paint_scene.as_ref().and_then(|paint| {
            paint.scroll_state_at(floor_clamp_u32(origin.0), floor_clamp_u32(origin.1))
        });
        PanState {
            latch: DragLatch::new(origin),
            last: origin,
            frac: (0.0, 0.0),
            tag: self.hover_targets.get(&id).cloned(),
            scroll,
        }
    }

    /// R881 §5.35 §5.49 — close the middle-button gesture for `id`
    /// (winit `MouseInput { Middle, Released }`) and report what it
    /// was. The shell acts on [`PanRelease::Click`] only (the
    /// paste funnel); a pan already applied itself move-by-move, and
    /// [`PanRelease::NoPress`] covers both a spurious release and a
    /// gesture [`pointer_cancel`](Self::pointer_cancel) revoked — a
    /// cancelled press is "never happened" and must not paste.
    pub fn middle_up(&mut self, id: PointerId) -> PanRelease {
        self.pan_up(id, PanButton::Middle)
    }

    /// R882 §5.35 §5.39 — close a **left**-opened pan gesture (the
    /// release half of [`left_pan_down`](Self::left_pan_down)). The
    /// shell routes a left release here when
    /// [`left_pan_in_flight`](Self::left_pan_in_flight) reports the
    /// press entered the pan channel — release routing follows the
    /// gesture in flight, NOT the current chord state, so releasing
    /// Space mid-pan never strands the gesture (gesture-capture, the
    /// same pinning the targets get). The verdict's `Click`
    /// (release-in-place) is inert for the left chord (Figma:
    /// Space+click does nothing).
    pub fn left_pan_up(&mut self, id: PointerId) -> PanRelease {
        self.pan_up(id, PanButton::Left)
    }

    /// R882 §5.35 — whether a left-opened pan gesture (latched or still
    /// in its dead zone) is in flight for `id`. The left-release
    /// routing reads this: a press that entered the pan channel must
    /// resolve there even if the Space chord lifted mid-gesture.
    #[must_use]
    pub fn left_pan_in_flight(&self, id: PointerId) -> bool {
        self.pan_gestures
            .get(&id)
            .is_some_and(|g| g.button == PanButton::Left)
    }

    /// R882.1 §5.35 — whether ANY pan-class gesture (either button,
    /// latched or dead-zone) owns `id`. The shell-tier press front
    /// door reads this to skip its press follow-ups (click-to-focus /
    /// caret positioning / immediate-mode forward) for a press the
    /// router is about to swallow — pre-R882.1 those follow-ups ran
    /// on the pinned (stale) hover target and stole focus during a
    /// live pan.
    #[must_use]
    pub fn pan_gesture_in_flight(&self, id: PointerId) -> bool {
        self.pan_gestures.contains_key(&id)
    }

    /// R881 / R882 §5.35 — the shared pan-channel release arm. Only a
    /// matching-button release consumes the gesture: a left release
    /// while a middle pan owns the pointer (or vice versa) resolves
    /// `NoPress` and leaves the gesture in flight — cross-button
    /// releases must not steal a live pan (R881.1 exclusivity, the
    /// release half). R882.1 — a release pairing with a *swallowed*
    /// same-button press (an RPC injection racing the native hold)
    /// drains the gesture's refusal counter and resolves `NoPress`
    /// too: only the press that opened the gesture may close it.
    fn pan_up(&mut self, id: PointerId, button: PanButton) -> PanRelease {
        let Some(gesture) = self.pan_gestures.get_mut(&id) else {
            return PanRelease::NoPress;
        };
        if gesture.button != button {
            return PanRelease::NoPress;
        }
        if gesture.swallowed_presses > 0 {
            gesture.swallowed_presses -= 1;
            return PanRelease::NoPress;
        }
        match self.pan_gestures.remove(&id).map(|g| g.pan) {
            Some(Some(pan)) if pan.latch.live() => PanRelease::Pan,
            Some(_) => PanRelease::Click,
            None => PanRelease::NoPress,
        }
    }

    /// R881 §5.35 — whether a *latched* pan (any opening button) is in
    /// flight for `id` (a non-latched pan press is still a click
    /// candidate and does not pin the hover).
    fn pan_live(&self, id: PointerId) -> bool {
        self.pan_gestures
            .get(&id)
            .and_then(|g| g.pan.as_ref())
            .is_some_and(|pan| pan.latch.live())
    }

    /// R881 §5.35 §5.49 — the pan-channel `cursor_moved` arm (any
    /// opening button). Advances the gesture's [`DragLatch`]; once
    /// live, dispatches the `last - current` cursor delta (content
    /// follows the cursor — the grab convention) through the pinned
    /// two-stage wheel routing: offer the press-time hover `External`
    /// first (a consuming canvas pans / `Ctrl`-zooms itself), else
    /// [`ScrollState::scroll_by`] on the pinned scroll container. Pan
    /// deltas ARE wheel-vocabulary pixel deltas — winit itself reports
    /// touchpad pan gestures as `WheelDelta::PixelDelta`, so a widget's
    /// wheel arm already speaks this dialect; the middle drag (R881)
    /// and the Space-chord left drag (R882) are just further producers.
    ///
    /// Returns `(pan_live, dispatched_this_move)`.
    fn advance_pan(
        &mut self,
        id: PointerId,
        x: f64,
        y: f64,
        modifiers: Modifiers,
        state_scene: &mut Scene,
    ) -> (bool, bool) {
        // R881.1 — lazy seed: a press that arrived before any cursor
        // for this pointer had no origin to latch against; the first
        // move the gesture learns about IS its origin (so motion still
        // disambiguates pan from click — the degraded press is not
        // click-forever).
        if self.pan_gestures.get(&id).is_some_and(|g| g.pan.is_none()) {
            let pan = self.pin_pan_targets(id, (x, y));
            if let Some(gesture) = self.pan_gestures.get_mut(&id) {
                gesture.pan = Some(pan);
            }
            return (false, false);
        }
        // Stage 0: advance the latch + compute the delta, then release
        // the gesture borrow before touching the paint scene.
        let (dx, dy, tag, scroll, frac) = {
            let Some(pan) = self.pan_gestures.get_mut(&id).and_then(|g| g.pan.as_mut()) else {
                return (false, false);
            };
            if !pan.latch.advance((x, y)) {
                return (false, false);
            }
            let (last_x, last_y) = pan.last;
            pan.last = (x, y);
            #[allow(
                clippy::cast_possible_truncation,
                reason = "per-move cursor deltas are a few logical px; f32 is the wheel \
                          vocabulary's precision"
            )]
            let (dx, dy) = ((last_x - x) as f32, (last_y - y) as f32);
            (dx, dy, pan.tag.clone(), pan.scroll.clone(), pan.frac)
        };
        // R881.1 — mask Shift out of the pan's wheel-dialect dispatch.
        // The Shift+wheel convention ("vertical notches drive x") is an
        // axis REMAP for one-dimensional notch devices; a pan delta is
        // already two-dimensional, so remapping it scrambles the grab
        // semantics (vertical drag panning horizontally). Masking makes
        // Shift+middle-drag a plain pan — exactly Blender's chord set.
        // Ctrl / Cmd (zoom-class chords) pass through untouched.
        let modifiers = Modifiers {
            shift: false,
            ..modifiers
        };
        let Some(paint) = self.last_paint_scene.as_ref() else {
            return (true, false);
        };
        let (dispatched, new_frac) = dispatch_wheel_two_stage(
            paint,
            state_scene,
            WheelDispatchArgs {
                target_tag: tag.as_deref(),
                scroll: scroll.as_ref(),
                cursor: (x, y),
                delta: (dx, dy),
                modifiers,
                frac,
            },
        );
        if let Some(pan) = self.pan_gestures.get_mut(&id).and_then(|g| g.pan.as_mut()) {
            pan.frac = new_frac;
        }
        (true, dispatched)
    }

    /// winit `CursorLeft` handler. Drops the cursor for `id` and
    /// dispatches a `PointerLeave` if a hover was in flight for that
    /// pointer — *unless* a drag is in flight (R51.34 §5.35 capture
    /// lock), in which case the hover stays pinned so the drag
    /// survives the window-leave / re-enter cycle that a real
    /// drag-out gesture produces. The deferred `PointerLeave` (if
    /// the cursor never returns) fires on the matching
    /// [`pointer_up`](Self::pointer_up).
    pub fn cursor_left(&mut self, id: PointerId, state_scene: &mut Scene) {
        self.cursors.remove(&id);
        // R881.1 — the wheel remainder accumulates one contiguous delta
        // stream; the cursor leaving breaks it, so the carry drops with
        // the cursor regardless of gesture state.
        self.wheel_remainders.remove(&id);
        // R742 §5.51 / R881 — an in-flight gesture (capture, DnD drag,
        // live middle pan) suppresses the leave: the gesture survives
        // the cursor straying outside the window and re-entering (the
        // OS implicit grab keeps streaming motion while a button is
        // held). R881.1 — same shared predicate as every other hover
        // producer.
        if self.gesture_pins_hover(id) {
            return;
        }
        if let Some(tag) = self.hover_targets.remove(&id) {
            dispatch_send(state_scene, &tag, PointerWireEvent::Leave.as_wire_name());
        }
    }

    /// winit `MouseInput` (or touch-down) press handler for `id`.
    /// Dispatches `PointerDown` to the pointer's current hover
    /// target. No-op when that pointer is over no tagged region —
    /// clicks on the background don't drive the SCXML (this is the
    /// R47 fix internalized).
    ///
    /// R51.34 §5.35: after dispatch, if the target widget opts in to
    /// pointer capture via
    /// [`External::wants_pointer_capture`](pinion_core::external::External::wants_pointer_capture),
    /// the router pins this pointer's `captured_targets` entry to
    /// that tag for the duration of the press. While pinned,
    /// [`cursor_moved`] forwards the cursor to the widget through
    /// [`External::pointer_move`](pinion_core::external::External::pointer_move)
    /// and suppresses hover / leave dispatch for this pointer.
    /// R741 §5.35: button-like widgets now also opt in (so a click is
    /// jitter-robust) and pair it with
    /// [`External::cancel_on_release_off_target`] so a release off the
    /// widget still cancels — see [`pointer_up`](Self::pointer_up).
    ///
    /// R664 §5.49 — W3C UI Events `dblclick` detection. After the
    /// standard `PointerDown` dispatch, the router compares this
    /// press against [`last_press`](Self::last_press) for `id`: if the
    /// previous press hit the same `target_tag` within
    /// [`DOUBLE_CLICK_TIME_MS`] and the cursor moved less than
    /// [`DOUBLE_CLICK_DIST_PX`] per axis, the router synthesises a
    /// second named event `DoubleClick` to the same target on top of
    /// the normal `PointerDown`. Widgets that distinguish single from
    /// double activation handle the `DoubleClick` arm in their
    /// `invoke("send", ...)` `match`; widgets that don't (the entire
    /// pre-R664 catalogue) silently ignore the extra event so the
    /// extension is fully additive. The
    /// [`DeferredInput::DoubleClick`](pinion_rpc::dispatch::DeferredInput::DoubleClick)
    /// RPC drain reaches this same detection because its expansion
    /// fires two consecutive `pointer_down` calls with zero cursor
    /// move in between — the threshold check trivially fires for the
    /// second press, unifying the native winit and RPC-injected paths
    /// at the framework tier per [[r47-class-incident-prevention]].
    pub fn pointer_down(&mut self, id: PointerId, state_scene: &mut Scene) {
        // R881.1 §5.35 — gesture exclusivity: while a pan-class gesture
        // (middle drag or R882 Space-chord left drag) owns the pointer,
        // a routed press is swallowed. R882.1 widened the guard from
        // latched-only (`pan_live`) to ANY in-flight pan gesture: a
        // dead-zone pan press is already a gesture candidate, and
        // letting a routed press open a capture / press tracker beside
        // it would feed both gestures the same motion once the pan
        // latches — the exact coexistence `pan_down`'s own guard
        // refuses in the mirror direction (the guards must be
        // symmetric or the exclusivity is one-way). The hover snapshot
        // a press would route by is also stale the moment content
        // slides under the cursor (Qt ignores secondary-button presses
        // during an active gesture); the matching release is swallowed
        // in `pointer_up_with_modifiers` so no widget sees an Up
        // without its Down. A swallowed press on a LEFT-owned gesture
        // is counted so its release pairs with the refusal instead of
        // consuming the gesture (see [`PanGesture::swallowed_presses`];
        // this arc IS the left-button channel — middle has its own).
        if let Some(gesture) = self.pan_gestures.get_mut(&id) {
            if gesture.button == PanButton::Left {
                gesture.swallowed_presses += 1;
            }
            return;
        }
        if let Some(tag) = self.hover_targets.get(&id).cloned() {
            dispatch_send(state_scene, &tag, PointerWireEvent::Down.as_wire_name());
            // R876 §5.49 §5.51 — open the click-vs-drag tracker for this
            // press (origin = the press cursor). Every press over a tagged
            // target is a click *candidate*; `cursor_moved` latches it to a
            // drag once it strays, and `pointer_up` closes it. One record per
            // pointer feeds both the trailing-click suppression and the
            // double-click detector.
            if let Some(&origin) = self.cursors.get(&id) {
                self.press_gestures.insert(id, DragLatch::new(origin));
            }
            // R51.40 §5.35 — read the cached wants_capture bit
            // populated by the matching `refresh_hover` instead of
            // re-walking the state-scene tree. The cache is
            // populated when the pointer enters this tag and
            // cleared on leave, so it is always consistent with the
            // current `hover_targets[id]`.
            let wants = self.hover_wants_capture.get(&id).copied().unwrap_or(false);
            if wants {
                self.captured_targets.insert(id, tag.clone());
                // R51.35 §5.35 — click-to-position: forward the
                // press-time cursor as the initial `pointer_move` so
                // a click-without-drag still seeds the widget's
                // value at the click point (Material / `SwiftUI` / Qt
                // Slider click-jumps-to-position UX). Without this
                // forward the value would not update unless the user
                // also dragged the cursor at least one pixel.
                if let Some(&(x, y)) = self.cursors.get(&id) {
                    self.forward_pointer_move(state_scene, &tag, x, y);
                }
            }

            // R742 §5.51 — drag-source arming. PointerDown already
            // reached the widget (so it recorded which sub-region was
            // pressed); ask whether it wants to start a drag. `Some`
            // opens a session the router drives via `drag_to` /
            // `drag_release` until `pointer_up`. Default `begin_drag` is
            // `None`, so non-DnD widgets never open a session.
            if let Some(payload) = widget_begin_drag(state_scene, &tag) {
                self.drag_sessions.insert(
                    id,
                    DragSession {
                        source_tag: tag.clone(),
                        payload,
                        // R1102 — no cross-window resolution yet; the shell
                        // fills it on the first move that escapes this window.
                        cross_window: None,
                        // R1117 §5.15 §5.51 — the PRESS point: the cursor the
                        // router holds at PointerDown (a CursorMoved preceded
                        // it). A grab-offset window move anchors here, not on the
                        // first move sample. Degenerate (no held cursor) → origin.
                        press_cursor: self.cursors.get(&id).copied().unwrap_or_default(),
                    },
                );
            }

            // R664 §5.49 — double-click detection. Same target +
            // within W3C `dblclick` time + space window → synthesise
            // a `DoubleClick` named event on top of `PointerDown`.
            let now = Instant::now();
            let cursor = self.cursors.get(&id).copied();
            let is_double = match (self.last_press.get(&id), cursor) {
                (Some(prev), Some((cx, cy))) => {
                    let elapsed = now.duration_since(prev.0).as_millis();
                    let dx = (prev.1 - cx).abs();
                    let dy = (prev.2 - cy).abs();
                    prev.3 == tag
                        && elapsed < DOUBLE_CLICK_TIME_MS
                        && dx < DOUBLE_CLICK_DIST_PX
                        && dy < DOUBLE_CLICK_DIST_PX
                }
                _ => false,
            };
            if is_double {
                dispatch_send(state_scene, &tag, "DoubleClick");
                // Detail=2 fired; the next press starts a fresh cycle
                // (no rolling triple-click — pinion stops at binary
                // single/double until a 2nd consumer surfaces).
                self.last_press.remove(&id);
            } else if let Some((cx, cy)) = cursor {
                self.last_press.insert(id, (now, cx, cy, tag));
            }
        }
    }

    /// winit `MouseInput` (or touch-up) release handler for `id`.
    /// Dispatches `PointerUp` to that pointer's current hover
    /// target. Release with the cursor off-button is a no-op in
    /// free mode: `cursor_moved`'s `PointerLeave` already drove the
    /// SCXML out of `Pressed` back to `Idle`.
    ///
    /// R51.34 §5.35: in capture mode the cursor may currently sit
    /// off the widget rect (the drag strayed, or a button-like press
    /// slid off). The release event then depends on the captured
    /// widget's [`External::cancel_on_release_off_target`] policy:
    ///
    /// * `false` (drag widgets, e.g. Slider) — always dispatch
    ///   `PointerUp` so the drag commits its value wherever the cursor
    ///   ended (`Dragging → Hover` → `value_committed`).
    /// * `true` (R741 button-like widgets) — dispatch `PointerUp`
    ///   (activate) only when the cursor is still over the captured
    ///   tag, else dispatch `PointerLeave` (cancel). This is the
    ///   "slide off to abort" gesture; capture made it reachable by
    ///   suppressing the mid-press stray leave.
    ///
    /// Capture for this pointer is then released and
    /// [`refresh_hover`](Self::refresh_hover) re-runs to resettle the
    /// hover state against the release position.
    pub fn pointer_up(&mut self, id: PointerId, state_scene: &mut Scene) {
        self.pointer_up_with_modifiers(id, state_scene, Modifiers::empty());
    }

    /// R781 §5.35 §5.41 — [`pointer_up`](Self::pointer_up) carrying the
    /// held keyboard `modifiers` at the release (activate) edge. The shell
    /// passes its `ShellCore::modifiers` cache here so a `Shift` / `Ctrl`
    /// click reaches the composite send wire as the third payload segment
    /// (`"<key>:PointerUp:<token>"`); a multi-select coordinator extends /
    /// toggles, every other widget ignores it. `pointer_up` is the
    /// zero-modifier wrapper (native single clicks, tests, the TUI shell).
    pub fn pointer_up_with_modifiers(
        &mut self,
        id: PointerId,
        state_scene: &mut Scene,
        modifiers: Modifiers,
    ) {
        // R881.1 §5.35 — gesture exclusivity, the release half: the
        // matching `pointer_down` was swallowed while a pan-class
        // gesture (middle drag or R882 Space-chord left drag) owned
        // the pointer, so this release must not dispatch either (a
        // free-mode `PointerUp` with no prior `Down` would reach
        // activation-edge decoders — `is_activation_event` — and
        // click a widget that was never pressed). R882.1 — same
        // widened any-gesture predicate as the press half (the two
        // guards must agree or a dead-zone-gesture press/release pair
        // dispatches an orphan `Up`). A routed gesture cannot be in
        // flight here: its press was refused, so there is no capture /
        // drag / press tracker to close. This arc IS the left-button
        // release channel, so a swallowed-press refusal on a
        // LEFT-owned gesture is drained here exactly as
        // [`left_pan_up`](Self::left_pan_up) drains it on the shell
        // front-door path — each release travels exactly one of the
        // two, so the counter can neither leak nor double-drain.
        if let Some(gesture) = self.pan_gestures.get_mut(&id) {
            if gesture.button == PanButton::Left && gesture.swallowed_presses > 0 {
                gesture.swallowed_presses -= 1;
            }
            return;
        }
        // R876 §5.49 §5.51 — read the click-vs-drag SSOT for this press, then
        // close its tracker: the press ends here on every path (DnD commit,
        // capture release, free release). `became_drag` gates the DnD
        // trailing click below; the same determination already invalidated
        // any `DoubleClick` candidate mid-drag in `cursor_moved`.
        let became_drag = self.press_became_drag(id);
        self.press_gestures.remove(&id);
        // R742 §5.51 — a drag started on this pointer commits here. The
        // final drop location is the tag under the release cursor; the
        // source coordinator applies the move (or ignores `None`). The
        // normal `PointerUp` is dispatched afterwards so a
        // press-release-in-place (no real drag) still reaches the
        // statechart as a click.
        //
        // A drag session *supersedes* capture for the same pointer: while
        // a session is in flight `cursor_moved` already routes to the drag
        // path (not `forward_pointer_move`), so any `captured_targets`
        // entry — if a widget ever opts into both `wants_pointer_capture`
        // and `begin_drag` — is vestigial. Clear it here so the lock can
        // never outlive the gesture (no current widget sets both, but the
        // release must not depend on that staying true).
        if let Some(session) = self.drag_sessions.remove(&id) {
            self.captured_targets.remove(&id);
            let cursor = self.cursors.get(&id).copied();
            // R1167 §5.51 — same-window OUTER-dock override (dock-panel drag only),
            // shared with the move path so preview (`update_drag`) == result here.
            let own_over =
                cursor.and_then(|(x, y)| self.resolve_drag_own_over(&session.payload, x, y));
            // R1124 §5.51 PR-33 — a SELF-DROP (the own hit is the dragged panel's
            // own header / content) must not mask the cross-window redock below: a
            // floater being dragged onto another window has the cursor over its OWN
            // window, and the dragged panel cannot reorganize into itself.
            let own_is_self_drop = own_over
                .as_ref()
                .is_some_and(|p| self.own_drop_is_self(p, &session.source_tag));
            // R1100/R1102 §5.51 PR-33 — the same own-window-first rule as the
            // move: an own-window drop is a same-window commit (`over_window`
            // None); otherwise the shell's last-resolved cross-window drop redocks
            // into that window. NOTE the `own_over` half is re-resolved HERE at the
            // release cursor, but the cross-window half (`session.cross_window`) is
            // NOT — it is whatever the final `cursor_moved` stashed (only
            // `cursor_moved_for_window` calls `set_drag_cross_window`, never the
            // release path). The drag harness emits a move at the release point
            // immediately before `pointer_up`, and a native release is normally
            // preceded by a `CursorMoved` at the same point, so the stash matches
            // the release cursor. A native release whose cursor differs from the
            // last move (event coalescing) would commit a STALE `over_window` —
            // own-window-first bounds the blast radius (it only applies when
            // `own_over` is None), but a future tightening could re-resolve cross-
            // window here too (it needs the shell, which the per-window router
            // lacks — a slice-3 wiring once the redock executes).
            let (over, over_window) =
                resolve_drag_targets(own_over, own_is_self_drop, session.cross_window);
            let (primary, _) = split_subindex(&session.source_tag);
            if let Some(external) = find_external_by_tag(state_scene, primary) {
                // R1093 §5.15 — forward the release cursor via the `_at`
                // sibling (default delegates to `drag_release`). On the rare
                // path where no cursor was ever recorded for this pointer,
                // fall back to the cursor-less hook.
                match cursor {
                    // `became_drag` is the router's click-vs-drag verdict the
                    // source consumes (R1101); `over_window` is the shell's
                    // cross-window resolution (R1102) when the release escaped
                    // this window into another's dock zone.
                    Some(c) => external.handle.drag_release_at(
                        &session.payload,
                        &DragUpdate {
                            over,
                            cursor: c,
                            over_window: over_window.as_deref(),
                            source_window: self.window_id.as_deref(),
                            became_drag,
                            // R1117 — the gesture's press point (Copy; unaffected
                            // by the `cross_window` partial move above).
                            press_cursor: session.press_cursor,
                        },
                    ),
                    None => external.handle.drag_release(&session.payload, over),
                }
            }
            // R794 §5.51 — a drag and a click are mutually exclusive. Only a
            // press-release *in place* (the cursor never left the press point
            // by DRAG_CLICK_THRESHOLD_PX) synthesizes the trailing `PointerUp`
            // click; a real moved drag committed via `drag_release` above and
            // must NOT also activate the source (the row a file move relocated,
            // the tab a reorder shifted). This is the framework SSOT for
            // click-vs-drag — Qt `startDragDistance`, the DOM no-`click`-after-
            // drag rule — so no drag source re-derives it per binding. R876:
            // `became_drag` is the unified press-to-drag determination
            // (`track_press_drag`), shared with the double-click detector.
            if !became_drag {
                dispatch_send_mods(
                    state_scene,
                    &session.source_tag,
                    PointerWireEvent::Up.as_wire_name(),
                    modifiers,
                );
            }
            self.refresh_hover(id, state_scene);
            return;
        }
        if let Some(cap_tag) = self.captured_targets.get(&id).cloned() {
            let release_over = self.cursor_over_tag(id, &cap_tag);
            let event = if !release_over && widget_cancels_on_release_off(state_scene, &cap_tag) {
                PointerWireEvent::Leave
            } else {
                PointerWireEvent::Up
            };
            dispatch_send_mods(state_scene, &cap_tag, event.as_wire_name(), modifiers);
            self.captured_targets.remove(&id);
            self.refresh_hover(id, state_scene);
        } else if let Some(tag) = self.hover_targets.get(&id).cloned() {
            // Free (no-capture) release: the cursor is over the target
            // (a mid-press stray already drove the SCXML out of Pressed
            // via `cursor_moved`'s `PointerLeave`).
            dispatch_send_mods(
                state_scene,
                &tag,
                PointerWireEvent::Up.as_wire_name(),
                modifiers,
            );
        }
    }

    /// R741 §5.35 — whether the cursor for `id` currently resolves to
    /// `tag` (full-tag equality, so a composite `group#0` press that is
    /// released over `group#1` reads as off-target — the W3C "press and
    /// release on the same control" rule). `false` when the pointer has
    /// no tracked cursor or no last paint scene.
    fn cursor_over_tag(&self, id: PointerId, tag: &str) -> bool {
        match (self.cursors.get(&id), self.last_paint_scene.as_ref()) {
            (Some(&(x, y)), Some(scene)) => resolve_hover_tag(scene, x, y).as_deref() == Some(tag),
            _ => false,
        }
    }

    /// (R51.186 §5.45 R55.C.2) Mouse wheel input dispatch.
    /// Zero-modifier wrapper around
    /// [`wheel_with_modifiers`](Self::wheel_with_modifiers) (native
    /// notched wheels without held keys, the TUI shell, tests) —
    /// mirrors the [`pointer_up`](Self::pointer_up) /
    /// [`pointer_up_with_modifiers`](Self::pointer_up_with_modifiers)
    /// pair.
    pub fn wheel(&mut self, id: PointerId, delta: WheelDelta, state_scene: &mut Scene) -> bool {
        self.wheel_with_modifiers(id, delta, Modifiers::empty(), state_scene)
    }

    /// R877 §5.15 §5.49 — wheel dispatch carrying the held keyboard
    /// `modifiers`. Two-stage routing, listener-before-default (the
    /// W3C model where any wheel listener on the event path may
    /// `preventDefault` ahead of the scroll default action — the
    /// offered External can be an ANCESTOR of a deeper `Scroll`, the
    /// canvas-hijack pattern; declining preserves the inner chain):
    ///
    /// 1. **`External` offer** — resolve the hover target tag under
    ///    the pointer's last-known cursor, find its primary
    ///    [`External`](pinion_core::external::External), normalise the
    ///    cursor against the widget's
    ///    [`capture_normalize`](pinion_core::external::External::capture_normalize)
    ///    basis (the SAME rect its `pointer_move` drag math uses) and
    ///    call [`External::wheel`](pinion_core::external::External::wheel)
    ///    with the pixel delta + `modifiers`. A `true` return consumes
    ///    the event — no scroll dispatch (the node-editor canvas pans /
    ///    `Ctrl`-zooms here). The default impl returns `false`, so
    ///    every pre-R877 widget falls straight through.
    /// 2. **Scroll fallback** — the pre-R877 path, byte-identical:
    ///    forward to the deepest
    ///    [`ScrollNode`](pinion_core::scene::ScrollNode) covering the
    ///    cursor whose `state: Option<Rc<ScrollState>>` link is wired.
    ///    `Pixels` route through verbatim; `Lines` multiply by
    ///    [`LINE_HEIGHT_PX`] (16, the W3C / browser default). The
    ///    translated `(dx, dy)` pair feeds
    ///    [`ScrollState::scroll_by`](pinion_core::widgets::scroll::ScrollState::scroll_by),
    ///    which clamps against the declared bounds and fires the
    ///    reactive `Signal::set`.
    ///
    /// W3C sign convention: positive `dy` scrolls *downward*
    /// (content shifts up visually); positive `dx` scrolls
    /// *rightward*. The convention matches
    /// [`WheelDelta`](pinion_core::event::WheelDelta) and the
    /// `ScrollState` offset semantics directly — no per-axis
    /// inversion at this boundary.
    ///
    /// No-op (returns `false`) when any of the following hold:
    ///
    /// - The pointer `id` has no stored cursor (cursor never
    ///   entered the window for this pointer, or `cursor_left`
    ///   already dropped it). winit / web / iOS all emit wheel
    ///   events without their own position field — they reuse
    ///   the surface's tracked cursor, so the router does the
    ///   same.
    /// - The retained paint scene is unset (wheel fired before
    ///   the first frame).
    /// - No hovered `External` consumed it AND no `Scene::Scroll`
    ///   covers the cursor point.
    /// - The covering `ScrollNode` has no `state` attached (a
    ///   declarative-only scroll node the application built
    ///   without `with_state(...)` — the router silently drops
    ///   the wheel rather than panicking).
    ///
    /// Returns `true` when the wheel was consumed by an `External`
    /// or dispatched against an attached `ScrollState`. Backends
    /// (Vello: `ShellCore::wheel`; TUI: `ShellCoreTui::wheel`) use
    /// the return to decide whether to request a repaint — silent
    /// drops never bump the redraw flag.
    pub fn wheel_with_modifiers(
        &mut self,
        id: PointerId,
        delta: WheelDelta,
        modifiers: Modifiers,
        state_scene: &mut Scene,
    ) -> bool {
        let Some(&(x, y)) = self.cursors.get(&id) else {
            return false;
        };
        let Some(paint) = self.last_paint_scene.as_ref() else {
            return false;
        };
        // One unit conversion for both stages: the External offer reads
        // the fractional pair; the Scroll fallback rounds it.
        let (dx, dy) = wheel_delta_to_pixels_f32(delta);
        // R877 / R881.1 — per-event target resolution (hover tag +
        // deepest scroll under the cursor); the precedence + the
        // application math live ONCE in `dispatch_wheel_two_stage`,
        // shared with the middle-pan producer (one dialect, two
        // producers — divergence there would be a routing bug).
        let target_tag = self.hover_targets.get(&id).cloned();
        let scroll = paint.scroll_state_at(floor_clamp_u32(x), floor_clamp_u32(y));
        // R881.1 — the wheel-side sub-pixel remainder (the same carry
        // the pan gesture holds in its state): a slow high-DPI
        // `PixelDelta` stream (0.4 px/event) must accumulate instead of
        // rounding to zero forever. Per Qt's accumulator discipline the
        // carry resets when the resolved scroll target changes — a
        // remainder must never leak across containers.
        let frac = match (self.wheel_remainders.get(&id), scroll.as_ref()) {
            (Some(rem), Some(s)) if rem.target.upgrade().is_some_and(|t| Rc::ptr_eq(&t, s)) => {
                rem.frac
            }
            _ => (0.0, 0.0),
        };
        let (dispatched, new_frac) = dispatch_wheel_two_stage(
            paint,
            state_scene,
            WheelDispatchArgs {
                target_tag: target_tag.as_deref(),
                scroll: scroll.as_ref(),
                cursor: (x, y),
                delta: (dx, dy),
                modifiers,
                frac,
            },
        );
        match scroll {
            Some(s) => {
                self.wheel_remainders.insert(
                    id,
                    WheelRemainder {
                        target: Rc::downgrade(&s),
                        frac: new_frac,
                    },
                );
            }
            None => {
                self.wheel_remainders.remove(&id);
            }
        }
        dispatched
    }

    /// (R51.187 §5.45 R55.C.3) Keyboard scroll input dispatch.
    ///
    /// Routes a W3C `KeyboardEvent.key` string into the deepest
    /// [`ScrollNode`](pinion_core::scene::ScrollNode) covering the
    /// pointer's last cursor position whose `state` link is wired.
    /// Eight key names are recognised; any other string is a no-op
    /// (returns `false`) so the caller's regular `apply_key`
    /// dispatch arm can stay the primary path for widget-bound keys
    /// and the scroll router only acts on unhandled scrolling
    /// shortcuts.
    ///
    /// | Key           | Effect                                       |
    /// |---------------|----------------------------------------------|
    /// | `ArrowDown`   | `scroll_by(0, +LINE_HEIGHT_PX)`              |
    /// | `ArrowUp`     | `scroll_by(0, -LINE_HEIGHT_PX)`              |
    /// | `ArrowRight`  | `scroll_by(+LINE_HEIGHT_PX, 0)`              |
    /// | `ArrowLeft`   | `scroll_by(-LINE_HEIGHT_PX, 0)`              |
    /// | `PageDown`    | `scroll_by(0, +viewport.h)` (1-page step)    |
    /// | `PageUp`      | `scroll_by(0, -viewport.h)`                  |
    /// | `Home`        | `scroll_to(offset_x, 0)` (y-axis to top)     |
    /// | `End`         | `scroll_to(offset_x, max_y)` (y-axis bottom) |
    ///
    /// The arrow / page deltas honour the W3C `WheelEvent` sign
    /// convention: positive `dy` scrolls downward (content shifts
    /// up visually). `Home` / `End` preserve the horizontal offset
    /// — they match the W3C "vertical extreme" semantics every
    /// desktop scroll container uses; a future round adds
    /// `Ctrl+Home` / `Ctrl+End` for the (0, 0) / (max, max)
    /// corner cases (carry).
    ///
    /// No-op (returns `false`) when any of the same router-state
    /// conditions [`Self::wheel`] checks hold: no stored cursor for
    /// `id`, no retained paint scene, no `Scene::Scroll` covers the
    /// cursor, or the covering node has no `state` link.
    /// Application-level key routing reads the `false` and lets
    /// the regular [`pinion_core::WidgetCore::apply_key`] dispatch
    /// stay the primary path. Returns `true` when the key was
    /// recognised AND a scroll dispatched.
    pub fn scroll_key(&mut self, id: PointerId, key: &str) -> bool {
        let Some(&(x, y)) = self.cursors.get(&id) else {
            return false;
        };
        let Some(paint) = self.last_paint_scene.as_ref() else {
            return false;
        };
        let xu = floor_clamp_u32(x);
        let yu = floor_clamp_u32(y);
        let Some(scroll_node) = paint.scroll_target_at(xu, yu) else {
            return false;
        };
        let Some(state) = scroll_node.state.as_ref() else {
            return false;
        };
        let line: i32 = LINE_HEIGHT_PX_I32;
        // `viewport.h` / `viewport.w` are `u32` from `Rect`; clamp
        // into `i32` for the page step. Real-world viewports never
        // exceed `i32::MAX` (which would imply a 2-billion-pixel
        // window); the `try_from` fallback keeps the math defined
        // for the adversarial extreme.
        let page_y: i32 = i32::try_from(scroll_node.viewport.h).unwrap_or(i32::MAX);
        let page_x: i32 = i32::try_from(scroll_node.viewport.w).unwrap_or(i32::MAX);
        match key {
            "ArrowDown" => state.scroll_by(0, line),
            "ArrowUp" => state.scroll_by(0, -line),
            "ArrowRight" => state.scroll_by(line, 0),
            "ArrowLeft" => state.scroll_by(-line, 0),
            "PageDown" => state.scroll_by(0, page_y),
            "PageUp" => state.scroll_by(0, -page_y),
            "Home" => {
                let (ox, _) = state.offset();
                state.scroll_to(ox, 0);
            }
            "End" => {
                let (ox, _) = state.offset();
                let (_, my) = state.max();
                state.scroll_to(ox, my);
            }
            // Horizontal Home/End / Ctrl-modifier extensions are
            // future R55.C.4 carry. Silence other keys so the
            // caller's regular `apply_key` arm stays primary.
            _ => {
                // Suppress page_x to avoid unused-binding warnings
                // until a horizontal Page sub-axis lands.
                let _ = page_x;
                return false;
            }
        }
        true
    }

    /// R51.93 §5.35 — pointer cancellation handler. The OS-side
    /// counterpart to [`pointer_up`](Self::pointer_up): the user did
    /// **not** release the pointer of their own accord, the system
    /// revoked the gesture. winit emits `TouchPhase::Cancelled` for
    /// every such revoke path (4-finger system gesture, phone-call
    /// interrupt, notification banner pull-down, app-switcher
    /// invocation, edge-swipe back nav, Android `MotionEvent.ACTION_CANCEL`,
    /// iOS `UITouch` cancellation, etc.).
    ///
    /// Dispatches `PointerCancel` to the pointer's current hover or
    /// captured target. Widget statecharts route `Pressed → Idle` on
    /// this event **without raising the activate event**, so the
    /// `click` / `toggle` / `selected` / `value_committed` intent
    /// never fires for a cancelled gesture. Capture release + hover
    /// refresh mirror [`Self::pointer_up`]'s post-dispatch
    /// bookkeeping so the substrate's trailing `cursor_left` lands
    /// cleanly.
    ///
    /// Free-mode pre-R51.93 (touch cancel routed via `pointer_up`)
    /// silently committed a click the user did not authorise — this
    /// method is the textbook fix.
    pub fn pointer_cancel(&mut self, id: PointerId, state_scene: &mut Scene) {
        let target = self
            .hover_targets
            .get(&id)
            .cloned()
            .or_else(|| self.captured_targets.get(&id).cloned());
        if let Some(tag) = target {
            dispatch_send(state_scene, &tag, PointerWireEvent::Cancel.as_wire_name());
        }
        // R937.1 §5.51 — a cancelled gesture revokes an in-flight drag this
        // pointer started: remove the session (so the next `cursor_moved` can
        // never route to a dead `update_drag` — `cursor_moved` checks
        // `drag_sessions` first) and tell the source to DISCARD it via
        // `drag_cancel` (clear its preview / arm WITHOUT applying the move — a
        // cancel is "the drag never happened", unlike the `pointer_up` drop which
        // commits). The session-review caught this: pre-R937.1 the session +
        // the source's reactive drop-preview both leaked, leaving a ghost
        // insertion line and a stale arm after an OS gesture revoke.
        if let Some(session) = self.drag_sessions.remove(&id) {
            let (primary, _) = split_subindex(&session.source_tag);
            if let Some(external) = find_external_by_tag(state_scene, primary) {
                external.handle.drag_cancel(&session.payload);
            }
        }
        // R881 §5.35 — revoke any in-flight middle gesture: a cancelled
        // press is "never happened", so the trailing OS `Released` (if
        // one still arrives) resolves to `PanRelease::NoPress` and
        // neither pastes nor pans (the R880.1 mandatory-cancel-arm
        // discipline). Pan deltas already applied stay applied — a pan
        // is incremental scrolling, not a journaled transaction.
        self.pan_gestures.remove(&id);
        if self.captured_targets.remove(&id).is_some() {
            self.refresh_hover(id, state_scene);
        }
    }

    /// R51.34 §5.35 — capture-mode cursor forward. Look up the
    /// post-layout rect of the captured widget in the retained paint
    /// scene, normalise the cursor `(x, y)` into widget-relative
    /// `[0.0, 1.0]` coordinates (may exceed when the cursor strays),
    /// and hand them to the captured `External` via
    /// [`External::pointer_move`](pinion_core::external::External::pointer_move).
    /// Silent no-op when the paint scene is unset (cursor moved
    /// before the first frame) or the tag is unmappable to a rect.
    ///
    /// R51.42 §5.35 — composite hit-target paint tags (`"group#i"`)
    /// resolve the rect on the raw paint tag (the sub-region under
    /// the pointer) and the state-scene `External` on the primary
    /// half (the single composite handle). Capture-aware composite
    /// widgets are out of scope for the R51.41 RFC — `RadioGroup`
    /// returns `wants_pointer_capture = false` — but the wiring is
    /// kept symmetric so any future drag-aware composite slots in
    /// without revisiting the input router.
    fn forward_pointer_move(
        &self,
        state_scene: &mut Scene,
        target_tag: &str,
        cursor_x: f64,
        cursor_y: f64,
    ) {
        let Some(paint) = self.last_paint_scene.as_ref() else {
            return;
        };
        let (primary, _) = split_subindex(target_tag);
        let Some(external) = find_external_by_tag(state_scene, primary) else {
            return;
        };
        let Some((x_rel, y_rel)) =
            capture_rel_coords(paint, external, primary, target_tag, cursor_x, cursor_y)
        else {
            return;
        };
        external.handle.pointer_move(x_rel, y_rel);
    }

    /// R742 §5.51 — drive an in-flight drag for `id`: resolve the drop
    /// location under the absolute cursor and forward it to the source
    /// coordinator via
    /// [`External::drag_to`](pinion_core::external::External::drag_to).
    /// Hover stays pinned (no `refresh_hover`) so the source statechart
    /// sees no spurious mid-drag `PointerLeave`. No-op if the session
    /// vanished between the `contains_key` gate and here (it cannot, but
    /// the `get` keeps the borrow honest) or the source external is gone.
    fn update_drag(&mut self, id: PointerId, x: f64, y: f64, state_scene: &mut Scene) {
        // R876 — the drag-vs-click latch moved to `track_press_drag` (the
        // shared click-vs-drag SSOT `cursor_moved` advances before this), so
        // a DnD drag and a capture drag are judged by one metric + threshold.
        // `pointer_up` reads `press_became_drag` to gate the trailing click.
        let Some(session) = self.drag_sessions.get(&id) else {
            return;
        };
        let source = session.source_tag.clone();
        let payload = session.payload.clone();
        // R1102 §5.51 PR-33 — clone the shell's cross-window resolution so the
        // `session` borrow drops before the `state_scene` borrow below.
        let cross_window = session.cross_window.clone();
        // R1117 §5.15 §5.51 — the gesture's press point (Copy), forwarded so a
        // grab-offset window move anchors at the press, not this move sample.
        let press_cursor = session.press_cursor;
        // R1101 §5.51 — the router's click-vs-drag verdict (read here, while
        // `&self` is free, before the `state_scene` borrow). The source
        // consumes this instead of re-deriving it from its own distance
        // tracking (the F1 clearance — see [`DragUpdate::became_drag`]).
        let became_drag = self.press_became_drag(id);
        // R1100/R1102 §5.51 PR-33 — own-window drop resolution FIRST: a hit on
        // this window's own drop target is a same-window reorganize
        // (`over_window: None`). Only when the cursor has escaped every own-window
        // target does the shell's cross-window resolution apply — it names ANOTHER
        // window's zone, in that window's local frame, so `over_window: Some`.
        // R1124 §5.51 PR-33 — a self-drop (own hit on the dragged panel's own
        // node / subtree) yields to the cross-window redock here too, so a floater
        // dragged over another window resolves that window mid-drag, not its own
        // content. Same-window reorganize + plain own hits are unaffected.
        // R1167 §5.51 — own-window resolution applies the same-window OUTER-dock
        // override for a dock-panel drag (a cursor in this window's outer band →
        // full-span outer dock), so a docked panel reaches the window-edge full-span
        // dock without leaving its window. Non-dock drags keep the plain hit-test.
        let own_over = self.resolve_drag_own_over(&payload, x, y);
        let own_is_self_drop = own_over
            .as_ref()
            .is_some_and(|p| self.own_drop_is_self(p, &source));
        let (over, over_window) = resolve_drag_targets(own_over, own_is_self_drop, cross_window);
        let (primary, _) = split_subindex(&source);
        if let Some(external) = find_external_by_tag(state_scene, primary) {
            // R1093 §5.15 — forward the full [`DragUpdate`] context (the `_at`
            // default delegates to `drag_to` with just `over`, so pre-R1093
            // sources are unaffected). A follow-the-cursor coordinator reads the
            // cursor; the rect-relative `over` is `None` once the cursor escapes
            // every tag, so the cursor is the only live pointer signal then.
            external.handle.drag_to_at(
                &payload,
                &DragUpdate {
                    over,
                    cursor: (x, y),
                    over_window: over_window.as_deref(),
                    source_window: self.window_id.as_deref(),
                    became_drag,
                    press_cursor,
                },
            );
        }
    }

    /// R742 §5.51 — hit-test the retained paint scene at the absolute
    /// cursor `(x, y)` and build the [`DropPoint`] the source coordinator
    /// classifies: the full tag under the cursor (composite `widget#sub`
    /// when over a sub-element) plus the cursor normalised over that
    /// tag's post-layout rect. `None` when the cursor is over no tagged
    /// region or no paint scene has been recorded yet. This is the
    /// pointer-driven equivalent of the dock resolver reading
    /// `scene/layout` — the router already holds the painted tree, so the
    /// hit-test needs no `view()` rebuild.
    fn resolve_drop_point(&self, x: f64, y: f64) -> Option<DropPoint> {
        let paint = self.last_paint_scene.as_ref()?;
        // R1152 §5.51 — a cursor OUTSIDE this window (negative window-local coord)
        // has NO own-window drop target. Guard before the hit-test, whose
        // `floor_clamp_u32` would otherwise clamp a negative coord to 0 and resolve
        // a SPURIOUS top-left hit (the R1099 clamp). This is the bug behind
        // "dropped on the preview, didn't dock": a FLOATER's pointer grab keeps
        // delivering cursors after they have LEFT the floater (negative
        // floater-local) while over the dock host, and the spurious own hit made
        // `own_over_is_self_drop` false → `resolve_drag_targets` took the
        // own-window-first branch (`over_window: None`), MASKING the already
        // resolved cross-window redock, so the drop free-moved instead of docking.
        // Beyond-right/bottom self-misses the hit-test, so only the negative side
        // needs the guard; the cross-window resolver already guards negatives.
        if x < 0.0 || y < 0.0 {
            return None;
        }
        // R1080 §5.51 — prefer the nearest opted-in drop target (a dock
        // panel, a tab strip — `LayoutStyle::drop_target`) so the
        // coordinator receives the semantic drop region with the cursor
        // normalised over THAT region's rect. Falls back to the deepest
        // tagged hit when no node in the path opted in (the reorder-row
        // case, where the drop target is itself the deepest tag), so every
        // pre-R1080 R742 consumer is bit-identical.
        let tag =
            resolve_drop_target_tag(paint, x, y).or_else(|| resolve_hover_tag(paint, x, y))?;
        let rect = rect_for_tag(paint, &tag)?;
        let (x_rel, y_rel) = normalize_cursor(rect, x, y);
        Some(DropPoint { tag, x_rel, y_rel })
    }

    /// (R1167 §5.51) The SAME-window analog of [`resolve_outer_dock_zone`]: a
    /// window-local cursor `(x, y)` INSIDE this window within [`OUTER_DOCK_MARGIN`]
    /// of an edge is a FULL-SPAN outer dock at the nearest edge (a row / column
    /// across every pane), tagged [`OUTER_DOCK_ZONE_TAG`] with the cursor normalised
    /// over the WHOLE window — exactly what the cross-window perimeter pass produces
    /// for a floater, but for a drag that never left its own window (the user's
    /// "console 하단 full-width / properties 우측 컬럼"). `None` in the interior (the
    /// inner panel hit-test applies) — that asymmetry is what made same-window outer
    /// docking unreachable before R1167 (`resolve_drop` handled `OuterDock` but no
    /// same-window INPUT produced the sentinel).
    ///
    /// The band is INSIDE-only (the cursor must be within the window bounds), unlike
    /// the cross-window STRADDLE band: a same-window drag drives toward the edge from
    /// inside, and crossing the edge OUTWARD is an ESCAPE (the panel floats), so the
    /// outer band must not extend outside or it would swallow the drag-out-to-float
    /// gesture. The caller gates this on the dock-panel kind
    /// ([`Self::resolve_drag_own_over`]) so a non-dock drag (the outliner tree
    /// reparent) near the window edge keeps the plain `resolve_drop_point` hit-test.
    fn resolve_own_outer_dock(&self, x: f64, y: f64) -> Option<DropPoint> {
        let paint = self.last_paint_scene.as_ref()?;
        // (R1203) Measure the band against the DOCK AREA — the window shrunk from
        // the top by the chrome strip height — so the top full-span band sits at
        // the dock's top edge (below the client-side min/max/close controls), not
        // up in the chrome. The same-window peer of R1202's cross-window preview
        // inset; both now agree on where the dock area is.
        let root = dock_area_rect(paint.rect(), self.dock_area_top_inset);
        let (rx, ry) = (f64::from(root.x), f64::from(root.y));
        let (rw, rh) = (f64::from(root.w), f64::from(root.h));
        if rw <= 0.0 || rh <= 0.0 {
            return None;
        }
        // INSIDE-only: a cursor that has crossed the dock-area edge is escaping (→
        // float via the empty `resolve_drop_point`), not docking outer. Crossing
        // UP into the chrome strip leaves the dock, consistent with the other edges.
        if x < rx || x > rx + rw || y < ry || y > ry + rh {
            return None;
        }
        // (R1203) The band is PROPORTIONAL (capped at OUTER_DOCK_MARGIN): a fixed
        // 32px was an oversized fraction of a small window and a sliver of a large
        // one. See `outer_dock_margin`.
        if outer_edge_distance(root, x, y) > outer_dock_margin(root) {
            return None;
        }
        let (x_rel, y_rel) = normalize_cursor(root, x, y);
        Some(DropPoint {
            tag: OUTER_DOCK_ZONE_TAG.to_string(),
            x_rel,
            y_rel,
        })
    }

    /// (R1167 §5.51) Resolve the own-window drop point for an in-flight drag,
    /// applying the same-window OUTER-perimeter override for a DOCK-PANEL drag (the
    /// [`DOCK_PANEL_DRAG_KIND`] `payload.kind`): a cursor in this window's outer band
    /// is a full-span outer dock ([`Self::resolve_own_outer_dock`]), else the plain
    /// hit-test ([`Self::resolve_drop_point`]). The override is gated on the dock
    /// kind so a non-dock drag (the outliner tree reparent) near the window edge does
    /// NOT pick up the dock sentinel. The single home for the override so the move
    /// ([`Self::update_drag`]) and the release (the `pointer_up` drag branch) cannot
    /// diverge on it — the [[verify-seed-claims-audit-first]] / debt-D lesson: a
    /// resolver that handles a case the input cannot produce is an SSOT hole.
    fn resolve_drag_own_over(&self, payload: &DragPayload, x: f64, y: f64) -> Option<DropPoint> {
        if payload.kind == DOCK_PANEL_DRAG_KIND {
            if let Some(outer) = self.resolve_own_outer_dock(x, y) {
                return Some(outer);
            }
        }
        self.resolve_drop_point(x, y)
    }

    /// (R1124 §5.51 PR-33) Whether the own-window drop `own` is a SELF-DROP for a
    /// drag started on `source_tag` — a hit on the drag source's own node or
    /// subtree. The dragged panel cannot reorganize into itself, so a self-hit is
    /// not a same-window reorganize target; [`resolve_drag_targets`] lets such a
    /// hit yield to a cross-window redock (but only when one is available, so a
    /// plain same-window self-release still snaps back). This lifts
    /// `resolve_preview`'s `target == panel_id` self-drop rejection to the whole
    /// source subtree, so a floating single-panel window's own header / content
    /// drop targets (a property-grid row's intra-panel drag) do not block dragging
    /// the floater onto another window's dock zone. A genuine same-window
    /// reorganize (a hit on a sibling target — the reorder-row `dnd#0` → `dnd#1`
    /// case) is NOT in the source subtree, so it is unaffected.
    fn own_drop_is_self(&self, own: &DropPoint, source_tag: &str) -> bool {
        self.last_paint_scene
            .as_ref()
            .is_some_and(|paint| own_over_is_self_drop(paint, source_tag, &own.tag))
    }

    /// Recompute `hover_targets[id]` from `id`'s current cursor and
    /// the retained paint scene. Dispatches `PointerLeave` for the
    /// pointer's old target (if any) then `PointerEnter` for its new
    /// target (if any) so consumers always see the leave-before-
    /// enter ordering even when the cursor crosses directly from one
    /// tagged widget to another. Per-pointer ordering — two
    /// pointers crossing different widgets see two independent
    /// enter / leave streams.
    ///
    /// R51.40 §5.35: the new target's
    /// [`External::wants_pointer_capture`] is queried in the same
    /// pass and cached in `hover_wants_capture` so the next
    /// [`pointer_down`] reads a bit instead of re-walking the
    /// state-scene tree.
    fn refresh_hover(&mut self, id: PointerId, state_scene: &mut Scene) {
        let now = match (self.cursors.get(&id), &self.last_paint_scene) {
            (Some(&(x, y)), Some(scene)) => resolve_hover_tag(scene, x, y),
            _ => None,
        };
        let prev = self.hover_targets.get(&id).cloned();
        if prev == now {
            return;
        }
        if let Some(prev_tag) = prev {
            self.hover_targets.remove(&id);
            self.hover_wants_capture.remove(&id);
            dispatch_send(
                state_scene,
                &prev_tag,
                PointerWireEvent::Leave.as_wire_name(),
            );
        }
        if let Some(target) = now {
            self.hover_targets.insert(id, target.clone());
            let wants = widget_wants_capture(state_scene, &target);
            self.hover_wants_capture.insert(id, wants);
            dispatch_send(state_scene, &target, PointerWireEvent::Enter.as_wire_name());
        }
    }
}

/// Hit-test `paint_scene` at `(x, y)` and return the deepest tagged
/// ancestor's tag. Returns `None` when no node in the hit path
/// carries a tag (the cursor is over a fully untagged region —
/// usually the background, possibly some untagged decoration).
///
/// The walk is deepest-first because the visual nesting matches the
/// expected dispatch target — a tagged label inside a tagged button
/// dispatches to the label first (if anyone tags labels), falling
/// back to the button container.
fn resolve_hover_tag(paint_scene: &Scene, x: f64, y: f64) -> Option<String> {
    let xu = floor_clamp_u32(x);
    let yu = floor_clamp_u32(y);
    let hit = paint_scene.hit_test(xu, yu)?;
    // Walk segments deepest-first: the longer the prefix, the deeper
    // the ancestor. The root (empty prefix) is the last fallback.
    for k in (0..=hit.segments.len()).rev() {
        let Some(scene) = paint_scene.lookup_path_ref(&hit.segments[..k]) else {
            continue;
        };
        if let Some(tag) = scene.tag() {
            return Some(tag.to_string());
        }
    }
    None
}

/// R1080 §5.51 — hit-test `paint_scene` at `(x, y)` and return the nearest
/// ancestor that opted in as a drop target
/// ([`Scene::is_drop_target`](pinion_core::Scene::is_drop_target)) AND carries
/// a tag. `None` when no node in the hit path is a drop target — then
/// [`InputRouter::resolve_drop_point`] falls back to [`resolve_hover_tag`]'s
/// deepest tagged hit (the reorder-row case, where the drop target is itself
/// the deepest tag, so no marking is needed).
///
/// This is the semantic drop region a drag coordinator classifies: a dock
/// panel whose content is a deeper tagged child resolves to the PANEL, not the
/// content leaf, with the cursor normalised over the panel's rect. The walk is
/// deepest-first so the *innermost* opted-in target wins when drop targets nest
/// (a panel inside a panel).
fn resolve_drop_target_tag(paint_scene: &Scene, x: f64, y: f64) -> Option<String> {
    let xu = floor_clamp_u32(x);
    let yu = floor_clamp_u32(y);
    let hit = paint_scene.hit_test(xu, yu)?;
    for k in (0..=hit.segments.len()).rev() {
        let Some(scene) = paint_scene.lookup_path_ref(&hit.segments[..k]) else {
            continue;
        };
        if scene.is_drop_target() {
            if let Some(tag) = scene.tag() {
                return Some(tag.to_string());
            }
        }
    }
    None
}

/// R742 §5.51 — ask the widget at `target_tag` whether a `pointer_down`
/// on it should start a drag. Resolves the state-scene `ExternalNode`
/// from the primary half of a (possibly composite) paint tag and returns
/// its [`External::begin_drag`](pinion_core::external::External::begin_drag)
/// — `None` (no session) for a non-DnD widget or an out-of-sync tag.
/// Called right after the matching `PointerDown` dispatch, so the widget
/// has already recorded which sub-region was pressed.
fn widget_begin_drag(state_scene: &mut Scene, target_tag: &str) -> Option<DragPayload> {
    let (primary, _) = split_subindex(target_tag);
    find_external_by_tag(state_scene, primary)?
        .handle
        .begin_drag()
}

/// Dispatch a synthetic input event to the state scene's matching
/// `ExternalNode`. Walks the state scene depth-first; calls
/// `introspect_mut().invoke("send", Text(event_name))` on the first
/// node whose `tag` equals `target_tag`. Silent no-op when no
/// matching node is found — application's view-scene tag and
/// state-scene tag are out of sync, but routing keeps running rather
/// than panic.
///
/// R51.42 §5.35 — when `target_tag` carries a `'#'` sub-index suffix
/// (paint `"group#2"` for the composite hit-target convention), the
/// state-scene lookup uses the primary half (`"group"`) and the wire
/// payload is rewritten to the `"<idx>:<EventName>"` format
/// (`"2:PointerEnter"`) that composite widgets like `RadioGroup`
/// parse in their own `invoke("send", ...)` handler.
fn dispatch_send(state_scene: &mut Scene, target_tag: &str, event_name: &str) {
    dispatch_send_mods(state_scene, target_tag, event_name, Modifiers::empty());
}

/// R781 §5.35 §5.41 — [`dispatch_send`] carrying the held keyboard
/// `modifiers`. On a composite target (`'#'` sub-index present) and a
/// non-empty modifier state, the wire payload gains a third segment
/// (`"<idx>:<EventName>:<token>"`, e.g. `"4:PointerUp:sc"`) via
/// [`Modifiers::as_wire_token`](pinion_core::input::Modifiers::as_wire_token);
/// an empty modifier state emits the exact two-segment back-compat wire so
/// every pre-R781 composite consumer is unaffected.
///
/// R880 §5.35 §5.49 — a non-composite (background) target has no `<key>` to
/// anchor modifiers to, and its bare payload doubles as the SCXML event name
/// for the statechart-driven catalogue, so the modifier segment is gated on
/// the target's
/// [`External::wants_bare_send_modifiers`](pinion_core::external::External::wants_bare_send_modifiers)
/// opt-in: when granted (and modifiers are held) the payload is the
/// empty-key three-segment wire `":<EventName>:<token>"` — the same
/// `split_send_payload` grammar, `""` as the key. Every non-opted target
/// keeps the exact bare event name, held modifiers or not.
fn dispatch_send_mods(
    state_scene: &mut Scene,
    target_tag: &str,
    event_name: &str,
    modifiers: Modifiers,
) {
    let (primary, sub_index) = split_subindex(target_tag);
    let Some(external) = find_external_by_tag(state_scene, primary) else {
        return;
    };
    // The bare wire doubles as the SCXML event name, so it only carries the
    // token under the target's opt-in; composite consumers all decode via
    // the `split_send_payload` SSOT, so they take it unconditionally.
    let wire_mods = if sub_index.is_some() || external.handle.wants_bare_send_modifiers() {
        modifiers
    } else {
        Modifiers::empty()
    };
    let Some(intro) = external.handle.introspect_mut() else {
        return;
    };
    let payload = compose_send_payload(sub_index, event_name, wire_mods);
    let _ = intro.invoke("send", IntrospectValue::Text(payload));
}

/// Depth-first search for an [`ExternalNode`] whose tag matches
/// `target_tag`. Returns the first match in declaration order
/// (matches [`walk_scene_and_drain`](crate::walk_scene_and_drain)'s
/// traversal direction). Containers recurse; non-container variants
/// compare their own tag (when applicable) and stop.
fn find_external_by_tag<'a>(
    scene: &'a mut Scene,
    target_tag: &str,
) -> Option<&'a mut ExternalNode> {
    match scene {
        Scene::External(node) => {
            if tag_matches(node.tag.as_deref(), target_tag) {
                Some(node)
            } else {
                None
            }
        }
        Scene::Container(c) => {
            for child in &mut c.children {
                if let Some(found) = find_external_by_tag(child, target_tag) {
                    return Some(found);
                }
            }
            None
        }
        // Box / Text / Path / Image / Effect cannot carry an
        // `External` handle, so they never produce a dispatch target.
        _ => None,
    }
}

/// Tag comparison helper. `ExternalNode.tag` is `Option<Cow<...>>`;
/// resolve the borrow then string-compare.
fn tag_matches(node_tag: Option<&str>, target: &str) -> bool {
    matches!(node_tag, Some(t) if t == target)
}

/// R51.34 §5.35 — the **window-absolute** post-layout rect of the tagged
/// primitive named by `target_tag`. `None` when no node carries the tag
/// or it is scrolled fully out of view.
///
/// R1098 §5.51 PR-33 — a drop resolved **across windows**: which window owns
/// the drop target the absolute desktop cursor landed on, plus the
/// [`DropPoint`] in that window's own local logical frame.
///
/// The per-window [`InputRouter::resolve_drop_point`] sees only its own
/// `last_paint_scene`, so a drag captured by one window (a settled floating
/// panel) can never resolve a dock zone in another window (the main dock) —
/// the gap PR-33 closes. This is the cross-window peer: the shell, which holds
/// every window's scene + outer position, resolves the abs cursor against all
/// of them and carries the winning window id so the redock intent targets the
/// right surface.
#[derive(Debug, Clone, PartialEq)]
pub struct CrossWindowDrop {
    /// Spec id of the window whose drop target the abs cursor landed on.
    pub window: String,
    /// The drop point in THAT window's local logical frame (the same
    /// rect-normalised shape the source coordinator already classifies).
    pub point: DropPoint,
}

/// R1098 §5.51 PR-33 — resolve an absolute desktop cursor against MULTIPLE
/// windows' painted scenes, returning the window that owns the drop target +
/// the [`DropPoint`] in that window's local frame. The cross-window peer of
/// [`InputRouter::resolve_drop_point`].
///
/// Each `windows` item is `(spec_id, scene, outer_position)` in **logical**
/// pixels (the same coordinate space the router resolves in). The abs cursor
/// is transformed into each window's local frame (`abs - outer`) and the SAME
/// opted-in drop-target hit-test ([`resolve_drop_target_tag`]) runs against
/// that window's scene. Windows are tried in iteration order; the FIRST that
/// resolves a drop target wins. Ordering — including any source-window
/// exclusion or topmost-first preference a live cross-window redock wants — is
/// the **caller's** concern: this resolver imposes none and simply takes the
/// iterator as given. (The current `scene/cross_window_drop` caller passes the
/// declared window order and does not yet exclude the source window; that
/// refinement lands with the live cross-window redock wiring, not here.)
///
/// Unlike [`InputRouter::resolve_drop_point`], this takes NO hover-tag
/// fallback: a cross-window drop must land on a real opted-in drop region (a
/// dock zone), never an arbitrary tagged node in another window.
///
/// R1156 — resolution is two-pass: an OUTER-perimeter pass first
/// ([`resolve_outer_dock_zone`] — a cursor in the outermost [`OUTER_DOCK_MARGIN`]
/// band of a host window is a FULL-SPAN outer dock, tagged [`OUTER_DOCK_ZONE_TAG`]),
/// then EXACT containment (a cursor inside an inner drop target). `None` when the
/// cursor is neither at a perimeter nor inside a panel (the drop floats). (R1155's
/// near-miss edge-snap was superseded by the outer pass: the window perimeter is
/// the catchable full-span affordance an edge-flush slot actually wanted.)
#[must_use]
pub fn resolve_cross_window_drop<'a, I>(
    windows: I,
    abs_cursor: (f64, f64),
) -> Option<CrossWindowDrop>
where
    I: IntoIterator<Item = (&'a str, &'a Scene, (f64, f64))>,
{
    // Collected so the EXACT pass and the R1155 edge-snap fallback can both walk
    // the host set (the snap must first confirm NO window contained the cursor).
    let windows: Vec<(&str, &Scene, (f64, f64))> = windows.into_iter().collect();
    // Pass 0 (R1156) — OUTER perimeter: a cursor in the outermost OUTER_DOCK_MARGIN
    // band of a host window's content edge is a FULL-SPAN outer dock (a row/column
    // across EVERY pane), not an inner panel split. Checked FIRST so the perimeter
    // band wins over the inner panel at the very edge — the Qt ADS / VS outer-guide
    // model. Interior panel boundaries are untouched (they are not near the window
    // perimeter, so they keep their per-panel inner zones).
    if let Some(outer) = resolve_outer_dock_zone(&windows, abs_cursor) {
        return Some(outer);
    }
    // Pass 1 — exact containment: the cursor is INSIDE a host's drop target.
    for &(spec_id, scene, (ox, oy)) in &windows {
        let (lx, ly) = (abs_cursor.0 - ox, abs_cursor.1 - oy);
        // A cursor left of / above this window is NOT inside it. The
        // per-window `resolve_drop_target_tag` only ever sees in-window
        // cursors, so it hit-tests through `floor_clamp_u32`, which clamps a
        // negative coordinate to 0 — spuriously hitting the top-left node. The
        // cross-window caller DOES pass out-of-window cursors (that is the
        // whole point), so it must reject the negative half here; the
        // beyond-right / beyond-bottom half needs no guard (no node covers a
        // positive coordinate past the window's extent, so the hit-test misses).
        if lx < 0.0 || ly < 0.0 {
            continue;
        }
        let Some(tag) = resolve_drop_target_tag(scene, lx, ly) else {
            continue;
        };
        let Some(rect) = rect_for_tag(scene, &tag) else {
            continue;
        };
        let (x_rel, y_rel) = normalize_cursor(rect, lx, ly);
        return Some(CrossWindowDrop {
            window: spec_id.to_string(),
            point: DropPoint { tag, x_rel, y_rel },
        });
    }
    // No host contained the cursor and it is in no perimeter band → the drop
    // floats. (R1156 superseded R1155's near-miss edge-snap: the window PERIMETER
    // is now a full-span outer dock — the catchable affordance an edge-flush slot
    // actually wanted, handled by pass 0 above — and the interior is exact, pass 1.)
    None
}

/// R1156 §5.51 — the [`resolve_cross_window_drop`] OUTER-perimeter pass: when the
/// cursor sits in the outermost [`OUTER_DOCK_MARGIN`] band of a host window's
/// content rect (its [`Scene::rect`]), it is a FULL-SPAN outer dock at the nearest
/// edge — a row/column spanning every pane — not an inner panel split. Returns a
/// [`CrossWindowDrop`] tagged [`OUTER_DOCK_ZONE_TAG`] with the cursor normalised
/// over the WHOLE window (the dock consumer derives the edge from `x_rel`/`y_rel`).
/// `None` when the cursor is in no host's perimeter band (the interior, where the
/// inner passes apply, or far outside any window).
///
/// ASSUMPTION (R1156.1): the dock area == the window content rect ([`Scene::rect`]).
/// True for the current consumer (the editor's dock fills the window). A consumer
/// whose dock does NOT fill the window (chrome / a status bar / toolbars OUTSIDE
/// the dock — which the self-hosted-editor north star will have) would need the
/// reorganizer-root container's rect instead, threaded in as a parameter. Deferred
/// until such a 2nd consumer exists (YAGNI / the 2nd-consumer gate) rather than
/// generalising speculatively — a known, bounded limitation, not a latent bug for
/// today's dock-fills-window case.
fn resolve_outer_dock_zone(
    windows: &[(&str, &Scene, (f64, f64))],
    abs_cursor: (f64, f64),
) -> Option<CrossWindowDrop> {
    let mut best: Option<(f64, String, (f64, f64), Rect)> = None; // (edge dist, window, local, root)
    for &(spec_id, scene, (ox, oy)) in windows {
        let (lx, ly) = (abs_cursor.0 - ox, abs_cursor.1 - oy);
        let root = scene.rect();
        let (rx, ry) = (f64::from(root.x), f64::from(root.y));
        let (rw, rh) = (f64::from(root.w), f64::from(root.h));
        if rw <= 0.0 || rh <= 0.0 {
            continue;
        }
        // The cursor must be AT this window — inside, or within the margin just
        // outside it — not far off to the side. A cross-window floater approaches
        // the host edge from OUTSIDE, so the band STRADDLES the perimeter (unlike
        // the same-window inside-only band — see `resolve_own_outer_dock`).
        // (R1203) The band is PROPORTIONAL per window (capped at OUTER_DOCK_MARGIN).
        let margin = outer_dock_margin(root);
        if lx < rx - margin || lx > rx + rw + margin || ly < ry - margin || ly > ry + rh + margin {
            continue;
        }
        let dist = outer_edge_distance(root, lx, ly);
        if dist <= margin && best.as_ref().is_none_or(|(d, ..)| dist < *d) {
            best = Some((dist, spec_id.to_string(), (lx, ly), root));
        }
    }
    let (_, window, (lx, ly), root) = best?;
    let (x_rel, y_rel) = normalize_cursor(root, lx, ly);
    Some(CrossWindowDrop {
        window,
        point: DropPoint {
            tag: OUTER_DOCK_ZONE_TAG.to_string(),
            x_rel,
            y_rel,
        },
    })
}

/// (R1167 §5.51) Perpendicular distance from a window-local cursor `(lx, ly)` to
/// the nearest of the four edges of `root` (the window content rect) — the OUTER
/// full-span dock metric shared by the cross-window
/// ([`resolve_outer_dock_zone`]) and same-window
/// ([`InputRouter::resolve_own_outer_dock`]) perimeter passes (the one SSOT for
/// "how close to a window edge is this cursor", so the two paths classify the
/// outer band identically — the band MEMBERSHIP test differs [straddle vs inside],
/// but the distance metric does not).
fn outer_edge_distance(root: Rect, lx: f64, ly: f64) -> f64 {
    let (rx, ry) = (f64::from(root.x), f64::from(root.y));
    let (rw, rh) = (f64::from(root.w), f64::from(root.h));
    (ly - ry)
        .abs()
        .min((ly - (ry + rh)).abs())
        .min((lx - rx).abs())
        .min((lx - (rx + rw)).abs())
}

/// (R1203 §5.51 §5.39) Fraction of a window's SMALLER dimension the OUTER dock
/// trigger band spans, before the [`OUTER_DOCK_MARGIN`] cap. VS Code uses a
/// proportional edge band (≈10% for a single editor); a fixed pixel band is an
/// oversized fraction of a small window and a sliver of a large one. See
/// [`outer_dock_margin`].
const OUTER_DOCK_MARGIN_FRAC: f64 = 0.1;

/// (R1203 §5.51 §5.39) The OUTER dock trigger-band width for a window of rect
/// `root`: [`OUTER_DOCK_MARGIN_FRAC`] of the smaller dimension, CAPPED at
/// [`OUTER_DOCK_MARGIN`]. So a normal / large window keeps the familiar ~32px
/// edge band while a small window shrinks proportionally (a fixed 32px was a big
/// fraction of it, swallowing inner-split gestures near the edge). Shared by the
/// same-window ([`InputRouter::resolve_own_outer_dock`]) and cross-window
/// ([`resolve_outer_dock_zone`]) band tests so both scale identically.
fn outer_dock_margin(root: Rect) -> f64 {
    let smaller = f64::from(root.w.min(root.h));
    (OUTER_DOCK_MARGIN_FRAC * smaller).min(OUTER_DOCK_MARGIN)
}

/// (R1203 §5.51 §5.39) The DOCK-AREA rect: `root` (the window content rect)
/// shrunk from the top by `top_inset` — the client-side chrome strip height (`0`
/// for OS-decorated). The router-side peer of the shell's `inset_below_chrome`;
/// clamps the inset to the height so an over-tall chrome yields an in-bounds
/// empty rect rather than underflowing. [`InputRouter::resolve_own_outer_dock`]
/// measures its band against this so the top band sits at the dock's top, not in
/// the chrome.
fn dock_area_rect(root: Rect, top_inset: u32) -> Rect {
    let top = top_inset.min(root.h);
    Rect::new(root.x, root.y + top, root.w, root.h - top)
}

/// R1102 §5.51 PR-33 — the own-window-first precedence mapping a per-window own
/// drop resolution + the shell's [`CrossWindowDrop`] into the `(over, over_window)`
/// a [`DragUpdate`] carries. A hit on THIS window's own drop target wins (a
/// same-window reorganize, `over_window` stays `None`); only when the own
/// resolution is empty — the cursor escaped every own-window target — does the
/// cross-window drop apply (another window's zone, in that window's local frame,
/// `over_window: Some`). The single home for this rule so the move
/// ([`InputRouter::update_drag`]) and the release (the `pointer_up` drag branch)
/// cannot diverge on which target a cross-window drag lands on.
fn resolve_drag_targets(
    own_over: Option<DropPoint>,
    own_is_self_drop: bool,
    cross_window: Option<CrossWindowDrop>,
) -> (Option<DropPoint>, Option<String>) {
    match own_over {
        // R1124 §5.51 PR-33 — a SELF-DROP (the own hit is the dragged source's own
        // node / subtree) is not a same-window reorganize target, so it yields to a
        // cross-window redock WHEN one is resolved. Without a cross-window (a plain
        // same-window self-release) it keeps own-window-first, so a click /
        // snap-back on the source is unchanged.
        Some(_) if own_is_self_drop && cross_window.is_some() => {
            let cw = cross_window.expect("guarded by is_some");
            (Some(cw.point), Some(cw.window))
        }
        Some(point) => (Some(point), None),
        None => match cross_window {
            Some(cw) => (Some(cw.point), Some(cw.window)),
            None => (None, None),
        },
    }
}

/// (R1124 §5.51 PR-33) True when own-window drop target `own_tag` is a SELF-DROP
/// for a drag whose source is the FULL paint tag `source_tag` — i.e. `own_tag` is
/// that source node itself or lives inside its subtree. Used by
/// [`InputRouter::resolve_own_drop_excluding_source`] so a floating panel's own
/// header / content drop targets do not mask a cross-window redock.
///
/// The discriminator is the FULL `source_tag` (e.g. `properties#header`), NOT its
/// primary: a sibling reorder target shares the primary but is a DIFFERENT node
/// (dragging row `dnd#0` onto sibling `dnd#1` is a genuine same-window reorganize,
/// not a self-drop), so rooting at the full tag keeps the reorder own-window-first
/// rule intact while a floater's own header (the press tag the drag started on)
/// resolves as a self-drop.
fn own_over_is_self_drop(paint: &Scene, source_tag: &str, own_tag: &str) -> bool {
    own_tag == source_tag
        || find_node_with_tag(paint, source_tag).is_some_and(|node| node.contains_tag(own_tag))
}

/// Depth-first search for the node carrying `tag`, returning its subtree root.
/// Walks the same branches as [`Scene::contains_tag`] (Container children, then
/// Scroll content); other variants are tag-bearing leaves with no descendants.
fn find_node_with_tag<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
    if scene.tag() == Some(tag) {
        return Some(scene);
    }
    match scene {
        Scene::Container(n) => n.children.iter().find_map(|c| find_node_with_tag(c, tag)),
        Scene::Scroll(n) => find_node_with_tag(&n.content, tag),
        _ => None,
    }
}

/// R51.62 §5.40 — `pub` so `pinion-shell` can resolve post-layout widget
/// bounds when lowering [`pinion_a11y::AccessNode`] into
/// `accesskit::TreeUpdate`; also used by the router's pointer-capture
/// move ([`InputRouter::dispatch_pointer_move_to`]).
///
/// R705.1 §5.45 §2 #7 — delegates to the single coordinate-translation
/// authority [`Scene::rect_for_tag_absolute`]. Pre-R705.1 this was a
/// scroll-BLIND walk (recursed `Container` but not `Scroll`), so a
/// widget inside a [`Scene::Scroll`] (a listbox row, a tree row)
/// returned `None` — silently denying it AccessKit bounds (AT could not
/// locate it) and breaking pointer-capture normalization. The delegate
/// now translates by the enclosing scroll offsets + clips to the
/// viewport stack, exactly like the focus-ring overlay and the RPC
/// click resolver — one walker, no scroll-blind divergence.
#[must_use]
pub fn rect_for_tag(scene: &Scene, target_tag: &str) -> Option<Rect> {
    scene.rect_for_tag_absolute(target_tag)
}

/// R877 §5.35 — resolve a widget's capture-normalization basis rect and
/// normalise the absolute cursor against it. The shared body of
/// [`InputRouter::forward_pointer_move`] (capture drags) and the
/// [`InputRouter::wheel_with_modifiers`] `External` offer, so wheel-anchor
/// math and drag math read one coordinate basis — the normalization-rect
/// *decision* ([`CaptureNormalize`]) is encoded once.
///
/// R738 §5.35 — the decision: default `Target` is the captured
/// (sub-)tag's own rect — correct for single-tag capture widgets
/// (primary == `target_tag`) and for composites whose drag value is
/// sub-region-relative (dock tear-off). A widget whose value spans the
/// whole widget (the range slider) chooses `Primary` (track) so grabbing
/// a thumb sub-tag still maps the cursor across the full track.
///
/// R786 §5.35 — `Tag(name)` names a rect that is neither the grabbed
/// tag nor its primary (the column-resize handle, whose pixel delta
/// needs the *stable* viewport rect — the grabbed cell resizes under
/// the drag). One exhaustive decision, no precedence rule.
///
/// `None` when the chosen rect is absent from the paint scene (not yet
/// laid out) — callers skip the forward.
/// R881.1 §5.35 §5.49 — the resolved inputs of one wheel-dialect
/// dispatch (see [`dispatch_wheel_two_stage`]). Callers own target
/// *resolution* (per-event for the wheel, pinned-at-press for the
/// middle pan — that difference is data); everything after resolution
/// is the shared policy this struct feeds.
#[derive(Clone, Copy)]
struct WheelDispatchArgs<'a> {
    /// Stage-1 recipient: the (possibly composite) hover / pinned tag.
    target_tag: Option<&'a str>,
    /// Stage-2 recipient: the deepest attached scroll container.
    scroll: Option<&'a Rc<ScrollState>>,
    /// Cursor in window-local logical px (the offer's normalise basis).
    cursor: (f64, f64),
    /// Pixel delta (W3C sign: positive scrolls down / right).
    delta: (f32, f32),
    /// Held keyboard modifiers, forwarded to the `External` offer.
    modifiers: Modifiers,
    /// Incoming sub-pixel remainder for the stage-2 integer rounding.
    frac: (f32, f32),
}

/// R881.1 §5.35 §5.49 — ONE home for the wheel-dialect dispatch policy:
/// offer the `External` first (a consuming canvas pans / zooms itself —
/// the W3C listener-before-default model), else apply the delta to the
/// scroll container through the sub-pixel remainder accumulator
/// (integer scroll offsets round per event; the carry keeps a slow
/// high-DPI stream moving — Qt's wheel-remainder discipline). Both
/// producers — [`InputRouter::wheel_with_modifiers`] (per-event
/// targets, remainder keyed per pointer with target-change reset) and
/// the middle-pan arm (pinned targets, remainder in the gesture state)
/// — delegate here, so the precedence and the application math cannot
/// diverge (pre-R881.1 they already had: the remainder carry existed
/// only on the pan copy, so the exact `PixelDelta` stream the docs
/// cited stalled on the wheel path).
///
/// Returns `(dispatched, remaining_frac)`. `dispatched` is the repaint
/// cue: `true` when the `External` consumed or the scroll moved by at
/// least one integer pixel; a sub-pixel step banks into the remainder
/// and reports `false` (nothing visible changed).
fn dispatch_wheel_two_stage(
    paint: &Scene,
    state_scene: &mut Scene,
    args: WheelDispatchArgs<'_>,
) -> (bool, (f32, f32)) {
    if let Some(tag) = args.target_tag {
        if offer_wheel_to_external(
            paint,
            state_scene,
            tag,
            args.cursor,
            args.delta,
            args.modifiers,
        ) {
            return (true, args.frac);
        }
    }
    let Some(state) = args.scroll else {
        return (false, args.frac);
    };
    let (tx, ty) = (args.frac.0 + args.delta.0, args.frac.1 + args.delta.1);
    let (ix, iy) = (tx.round(), ty.round());
    let frac = (tx - ix, ty - iy);
    if ix == 0.0 && iy == 0.0 {
        return (false, frac);
    }
    state.scroll_by(round_clamp_i32(ix), round_clamp_i32(iy));
    (true, frac)
}

/// R877 / R881 §5.35 §5.49 — the wheel-vocabulary `External` offer,
/// stage 1 of [`dispatch_wheel_two_stage`]. Resolves the (possibly
/// composite) `target_tag`'s primary `External` in the state scene,
/// normalises the cursor over the widget's [`CaptureNormalize`] basis,
/// and offers the pixel delta + modifiers to
/// [`External::wheel`](pinion_core::external::External::wheel).
/// `true` = consumed (no scroll fallback may run).
fn offer_wheel_to_external(
    paint: &Scene,
    state_scene: &mut Scene,
    target_tag: &str,
    cursor: (f64, f64),
    delta: (f32, f32),
    modifiers: Modifiers,
) -> bool {
    let (primary, _) = split_subindex(target_tag);
    let Some(external) = find_external_by_tag(state_scene, primary) else {
        return false;
    };
    let Some((x_rel, y_rel)) =
        capture_rel_coords(paint, external, primary, target_tag, cursor.0, cursor.1)
    else {
        return false;
    };
    external
        .handle
        .wheel(x_rel, y_rel, delta.0, delta.1, modifiers)
}

fn capture_rel_coords(
    paint: &Scene,
    external: &ExternalNode,
    primary: &str,
    target_tag: &str,
    cursor_x: f64,
    cursor_y: f64,
) -> Option<(f32, f32)> {
    let norm_tag = match external.handle.capture_normalize() {
        CaptureNormalize::Tag(tag) => tag,
        CaptureNormalize::Primary => primary,
        CaptureNormalize::Target => target_tag,
    };
    let rect = rect_for_tag(paint, norm_tag)?;
    Some(normalize_cursor(rect, cursor_x, cursor_y))
}

/// R51.34 §5.35 — normalise a winit cursor `(f64, f64)` into
/// widget-relative `(f32, f32)` over `rect`. `0.0` maps to the
/// left / top edge, `1.0` to the right / bottom edge. Coordinates
/// may exceed `[0.0, 1.0]` (or be negative) when the cursor strays
/// outside the rect under R51.34 capture lock — Slider clamps in
/// its [`pointer_move`](pinion_core::external::External::pointer_move)
/// impl, future drag widgets may not. Zero-size rect (degenerate
/// layout) collapses to `(0.0, 0.0)` so consumers never divide by
/// zero.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn normalize_cursor(rect: Rect, cursor_x: f64, cursor_y: f64) -> (f32, f32) {
    let width = f64::from(rect.w);
    let height = f64::from(rect.h);
    let x_rel = if width > 0.0 {
        ((cursor_x - f64::from(rect.x)) / width) as f32
    } else {
        0.0
    };
    let y_rel = if height > 0.0 {
        ((cursor_y - f64::from(rect.y)) / height) as f32
    } else {
        0.0
    };
    (x_rel, y_rel)
}

/// R51.34 §5.35 — ask the state-scene `ExternalNode` matching
/// `target_tag` whether it opts in to pointer capture via
/// [`External::wants_pointer_capture`](pinion_core::external::External::wants_pointer_capture).
/// `false` when no matching node is found (out-of-sync paint and
/// state tags) so the router never claims capture on a phantom
/// widget.
///
/// R51.42 §5.35 — composite hit-target paint tags (`"group#i"`)
/// route the state-scene lookup through the primary half so the
/// single composite `External` decides capture once for the whole
/// hit-region. The sub-index is discarded here because capture is
/// a property of the composite handle, not of any one sub-region.
fn widget_wants_capture(state_scene: &Scene, target_tag: &str) -> bool {
    let (primary, _) = split_subindex(target_tag);
    widget_wants_capture_walk(state_scene, primary).unwrap_or(false)
}

/// R741 §5.35 — resolve [`External::cancel_on_release_off_target`] for
/// the external registered at `target_tag`'s primary half. `false` when
/// the tag is not found or the widget keeps the drag-commit default.
fn widget_cancels_on_release_off(state_scene: &Scene, target_tag: &str) -> bool {
    let (primary, _) = split_subindex(target_tag);
    widget_cancels_on_release_off_walk(state_scene, primary).unwrap_or(false)
}

/// Recursive helper for [`widget_cancels_on_release_off`] (mirror of
/// [`widget_wants_capture_walk`]).
fn widget_cancels_on_release_off_walk(scene: &Scene, target_tag: &str) -> Option<bool> {
    match scene {
        Scene::External(node) => {
            if tag_matches(node.tag.as_deref(), target_tag) {
                Some(node.handle.cancel_on_release_off_target())
            } else {
                None
            }
        }
        Scene::Container(c) => {
            for child in &c.children {
                if let Some(found) = widget_cancels_on_release_off_walk(child, target_tag) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// Recursive helper for [`widget_wants_capture`]. Returns
/// `Some(bool)` when the tag is found (allowing the caller to
/// distinguish "found, but declined" from "not found"), `None`
/// when the walk finds no match.
fn widget_wants_capture_walk(scene: &Scene, target_tag: &str) -> Option<bool> {
    match scene {
        Scene::External(node) => {
            if tag_matches(node.tag.as_deref(), target_tag) {
                Some(node.handle.wants_pointer_capture())
            } else {
                None
            }
        }
        Scene::Container(c) => {
            for child in &c.children {
                if let Some(found) = widget_wants_capture_walk(child, target_tag) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// Saturating cast from a winit cursor coordinate (`f64`) to the
/// `u32` accepted by [`Scene::hit_test`]. Negative values clamp to
/// `0` (cursor can never hit at sub-zero coords); fractional
/// precision is dropped (hit-test resolution is whole pixels at
/// R48). The allow-list documents what the saturating clamp protects
/// against, keeping the lint silenced only at this one call site.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn floor_clamp_u32(v: f64) -> u32 {
    v.max(0.0) as u32
}

/// R877 — re-export of the contract constant from its crate home
/// ([`pinion_core::event::LINE_HEIGHT_PX`], moved there because
/// [`External::wheel`](pinion_core::external::External::wheel)
/// consumers must read the SAME factor the router scales `Lines` by);
/// the `pinion_runtime::input::LINE_HEIGHT_PX` path stays valid for
/// existing callers and tests.
pub use pinion_core::event::LINE_HEIGHT_PX;

/// (R51.187 §5.45 R55.C.3) Integer mirror of [`LINE_HEIGHT_PX`]
/// for the arrow-key step in
/// [`InputRouter::scroll_key`](crate::input::InputRouter::scroll_key).
/// Hard-coded so the cast happens at compile time rather than
/// the (unsafe-at-saturation) `f32 as i32` path on every arrow
/// keypress.
const LINE_HEIGHT_PX_I32: i32 = 16;

/// (R51.186 §5.45 R55.C.2, fractional since R877) Convert a unit-tagged
/// [`WheelDelta`](pinion_core::event::WheelDelta) into a `(dx, dy)`
/// logical-pixel pair: `Pixels` route through verbatim, `Lines`
/// multiply by [`LINE_HEIGHT_PX`]. Kept at `f32` so a trackpad's
/// sub-pixel deltas reach an
/// [`External::wheel`](pinion_core::external::External::wheel) zoom /
/// pan consumer at full precision; the integer `ScrollState` fallback
/// rounds on top via [`round_clamp_i32`] — one conversion, two
/// precisions.
fn wheel_delta_to_pixels_f32(delta: WheelDelta) -> (f32, f32) {
    match delta {
        WheelDelta::Pixels { dx, dy } => (dx, dy),
        WheelDelta::Lines { dx, dy } => (dx * LINE_HEIGHT_PX, dy * LINE_HEIGHT_PX),
        // R55.C.2 — `WheelDelta` is `#[non_exhaustive]`; an
        // unknown future variant (e.g. `Pages` for PgUp / PgDn
        // coarse scroll) degrades to a zero delta rather than
        // panicking. The substrate must stay robust against
        // a `pinion-core` bump that introduces a variant the
        // running `pinion-runtime` does not yet recognise. The
        // R55.C.* sub-axis cascade adds the explicit arms as
        // each variant gains a defined offset semantics.
        _ => (0.0, 0.0),
    }
}

/// (R51.186 §5.45 R55.C.2) Round-to-nearest `f32 → i32` with
/// `NaN`-guard. Rust's `f32 as i32` saturates at the integer
/// boundaries since 1.45 and converts `NaN` to `0`; the explicit
/// `is_nan` check documents the policy at the call site rather
/// than relying on the silent language-level fallback (matches
/// the R51.145 `clamp_frame_dt` `NaN`-guard precedent).
#[allow(clippy::cast_possible_truncation)]
fn round_clamp_i32(v: f32) -> i32 {
    if v.is_nan() {
        return 0;
    }
    v.round() as i32
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use pinion_core::external::{
        Backend, BackendFallback, BackendSupport, CaptureNormalize, ExternalIntrospect,
        InterveneError, IntrospectSchema, InvokeError, RepaintOwner, ThreadOwnership,
    };
    use pinion_core::scene::{ContainerNode, Rect};
    use pinion_core::style::{BoxStyle, Color};

    /// Shared-state stub External — every `invoke("send", Text(name))`
    /// pushes `name` onto the held `Vec`. Constructed with
    /// [`CaptureExternal::new`] which returns the External *and* a
    /// matching `Arc<Mutex<...>>` handle the test holds for
    /// assertion. The router moves the External into an
    /// `ExternalNode`; the test keeps the Arc clone to read what
    /// arrived without re-extracting from the scene tree.
    struct CaptureExternal {
        captures: Arc<Mutex<Vec<String>>>,
    }

    impl CaptureExternal {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            let captures = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    captures: Arc::clone(&captures),
                },
                captures,
            )
        }
    }

    impl std::fmt::Debug for CaptureExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("CaptureExternal").finish()
        }
    }

    impl pinion_core::external::External for CaptureExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
            Some(self)
        }
    }

    impl ExternalIntrospect for CaptureExternal {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(&[])
        }
        fn query(&self, _path: &str) -> Option<IntrospectValue> {
            None
        }
        fn intervene(
            &mut self,
            _path: &str,
            _value: IntrospectValue,
        ) -> Result<(), InterveneError> {
            Err(InterveneError::UnknownPath)
        }
        fn invoke(
            &mut self,
            method: &str,
            args: IntrospectValue,
        ) -> Result<IntrospectValue, InvokeError> {
            if method == "send" {
                if let IntrospectValue::Text(name) = args {
                    self.captures.lock().expect("mutex poisoned").push(name);
                }
            }
            Ok(IntrospectValue::Null)
        }
    }

    fn read(captures: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        captures.lock().expect("mutex poisoned").clone()
    }

    /// Build a paint scene with one tagged button container of fixed
    /// size, centered in a wider background container. Matches the
    /// hello-button shape so tests use realistic coordinates.
    fn paint_with_button(viewport_w: u32, viewport_h: u32, btn_rect: Rect) -> Scene {
        let button = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag("main_btn")
                .with_style(BoxStyle::filled(Color::default())),
        );
        // Manually set button rect (skip taffy layout for unit-test
        // determinism; this is the post-layout artifact the router
        // would normally receive from `compute_layout`).
        let mut button_with_rect = button;
        if let Scene::Container(c) = &mut button_with_rect {
            c.rect = btn_rect;
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![button_with_rect])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    /// Build a state scene with one [`ExternalNode`] tagged `main_btn`
    /// (`CaptureExternal` inside) — the dispatch target for the paint
    /// scene above. Returns the `Arc<Mutex>` handle so tests inspect
    /// the captures without re-walking the scene tree.
    fn state_with_button() -> (Scene, Arc<Mutex<Vec<String>>>) {
        let (capture, captures) = CaptureExternal::new();
        let scene = Scene::External(ExternalNode::new(Box::new(capture)).with_tag("main_btn"));
        (scene, captures)
    }

    #[test]
    fn cursor_off_button_does_not_dispatch() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Cursor at (10, 10) — far from the button rect (80..120 x 80..120).
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(read(&captures).is_empty());
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn r741_captured_button_release_over_activates_release_off_cancels() {
        use pinion_core::external::IntrospectValue;
        use pinion_core::widgets::toggle::ToggleExternal;

        fn toggle_value(scene: &Scene) -> bool {
            let Scene::External(node) = scene else {
                panic!("external root")
            };
            matches!(
                node.handle.introspect().unwrap().query("value"),
                Some(IntrospectValue::Bool(true))
            )
        }
        fn fresh() -> (InputRouter, Scene) {
            let mut router = InputRouter::new();
            // The toggle reuses the `main_btn` tag so `paint_with_button`
            // supplies its post-layout rect (80..120 x 80..120).
            let mut state = Scene::External(
                ExternalNode::new(Box::new(ToggleExternal::new())).with_tag("main_btn"),
            );
            router.update_paint_scene(
                paint_with_button(200, 200, Rect::new(80, 80, 40, 40)),
                &mut state,
            );
            (router, state)
        }

        // ACTIVATE — press + release both over the captured toggle flips it.
        let (mut router, mut state) = fresh();
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(
            router.captured_target(PointerId::MOUSE),
            Some("main_btn"),
            "button captures the pointer on press (R741)"
        );
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(
            toggle_value(&state),
            "release over the captured button activates"
        );

        // JITTER — a stray *back onto* the widget before release still
        // activates (capture suppressed the mid-press PointerLeave).
        let (mut router, mut state) = fresh();
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 121.0, 100.0, &mut state); // 1px off
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // back on
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(
            toggle_value(&state),
            "sub-pixel jitter during press does not cancel"
        );

        // CANCEL — a deliberate slide off the widget then release cancels.
        let (mut router, mut state) = fresh();
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state); // slid off
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(
            !toggle_value(&state),
            "release off the captured button cancels"
        );
    }

    #[test]
    fn cursor_on_button_dispatches_enter_then_down_up() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Cursor on the button rect center.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&captures),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into()
            ],
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
    }

    #[test]
    fn cursor_crossing_off_button_fires_leave() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // on
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state); // off
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn cursor_left_rolls_back_hover() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // on
        router.cursor_left(PointerId::MOUSE, &mut state); // window-leave
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn r1196_cursor_hint_resolves_the_hinted_node_under_the_pointer() {
        use pinion_core::scene::{BoxNode, ContainerNode, Rect};
        use pinion_core::style::{Color, CursorHint, LayoutStyle};
        let mut router = InputRouter::new();
        // Paint scene: a hinted 4-px divider (x 98..102) in a 200x100 container,
        // the splitter shape — the handle carries the cursor, the panels do not.
        let handle = Scene::Box({
            let mut b = BoxNode::filled(Rect::new(98, 0, 4, 100), Color::default());
            b.layout = LayoutStyle::new().with_cursor(CursorHint::ColResize);
            b
        });
        let mut root = ContainerNode::new(vec![handle]);
        root.rect = Rect::new(0, 0, 200, 100);
        let mut state = Scene::Container(ContainerNode::new(vec![]));
        router.update_paint_scene(Scene::Container(root), &mut state);
        // No move reported yet → no cursor known → no hint.
        assert_eq!(router.cursor_hint(PointerId::MOUSE), None);
        // Cursor on the divider → col-resize (end-to-end through the router:
        // cursor_position + last_paint_scene + cursor_hint_at).
        router.cursor_moved(PointerId::MOUSE, 100.0, 50.0, &mut state);
        assert_eq!(
            router.cursor_hint(PointerId::MOUSE),
            Some(CursorHint::ColResize),
        );
        // Cursor off the divider (over a bare panel region) → no hint.
        router.cursor_moved(PointerId::MOUSE, 40.0, 50.0, &mut state);
        assert_eq!(router.cursor_hint(PointerId::MOUSE), None);
    }

    #[test]
    fn pointer_down_off_button_is_noop() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // No cursor_moved — cursor stays None.
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert!(read(&captures).is_empty());
    }

    #[test]
    fn pointer_down_before_first_paint_is_noop() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        // CursorMoved arrives before update_paint_scene — common at
        // startup. last_paint_scene is None, so hover_target stays
        // None, so dispatch is suppressed.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert!(read(&captures).is_empty());
    }

    #[test]
    fn resize_shifts_button_under_stationary_cursor() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        // First frame: button at (80..120) — cursor at (100, 100) hits.
        router.update_paint_scene(
            paint_with_button(200, 200, Rect::new(80, 80, 40, 40)),
            &mut state,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
        // Window resize moves the button to (10..50). Cursor stays at
        // (100, 100) — now off the button. update_paint_scene must
        // re-resolve and emit PointerLeave.
        router.update_paint_scene(
            paint_with_button(200, 200, Rect::new(10, 10, 40, 40)),
            &mut state,
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
    }

    #[test]
    fn dispatch_to_missing_state_tag_is_silent() {
        let mut router = InputRouter::new();
        // State has a different tag than the paint scene's button.
        let (capture, captures) = CaptureExternal::new();
        let mut state =
            Scene::External(ExternalNode::new(Box::new(capture)).with_tag("other_widget"));
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        // hover_target resolves to "main_btn" from paint, but state
        // has no matching ExternalNode → silent no-op.
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
        assert!(read(&captures).is_empty());
    }

    #[test]
    fn floor_clamp_u32_handles_negative_and_fractional() {
        assert_eq!(floor_clamp_u32(-1.0), 0);
        assert_eq!(floor_clamp_u32(0.0), 0);
        assert_eq!(floor_clamp_u32(1.9), 1);
        assert_eq!(floor_clamp_u32(99.5), 99);
    }

    // ─── R51.34 §5.35 capture-lock fixtures + tests ────────────

    /// Shared event log alias — symbolic input event names captured
    /// via `invoke("send", Text(<name>))` calls (the full symbolic
    /// path the router uses for `PointerEnter` / `PointerDown` /
    /// `PointerUp` / `PointerLeave`). Tests hold an `Arc` clone for
    /// assertions; the router moves the External into an
    /// `ExternalNode`.
    type EventLog = Arc<Mutex<Vec<String>>>;

    /// Shared move log alias — `(x_rel, y_rel)` tuples captured via
    /// `External::pointer_move` during capture lock. Only the
    /// `DragCaptureExternal` fixture appends here.
    type MoveLog = Arc<Mutex<Vec<(f32, f32)>>>;

    /// Drag-aware capture fixture. Opts in to pointer capture via
    /// [`External::wants_pointer_capture`] and records every
    /// [`External::pointer_move`] forward (so tests can assert the
    /// router fed the correct widget-relative normalised coords).
    /// Symbolic events (`PointerEnter` / `Down` / `Up` / `Leave`)
    /// share the same `events` log as the existing
    /// [`CaptureExternal`] for cross-correlation assertions in
    /// drag-end sequences.
    struct DragCaptureExternal {
        events: EventLog,
        moves: MoveLog,
        // R738 — when true, `capture_normalize` returns
        // `CaptureNormalize::Primary` (range-slider-style whole-widget normalization).
        normalize_primary: bool,
        // R880 — when true, opts in to the bare-target modifier wire
        // (`wants_bare_send_modifiers`), so a background release with held
        // modifiers reaches `send` as `":<EventName>:<token>"`.
        bare_send_modifiers: bool,
    }

    impl DragCaptureExternal {
        fn new() -> (Self, EventLog, MoveLog) {
            Self::with_normalize_primary(false)
        }

        fn with_normalize_primary(normalize_primary: bool) -> (Self, EventLog, MoveLog) {
            let events = Arc::new(Mutex::new(Vec::new()));
            let moves = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    events: Arc::clone(&events),
                    moves: Arc::clone(&moves),
                    normalize_primary,
                    bare_send_modifiers: false,
                },
                events,
                moves,
            )
        }

        /// R880 — fixture variant opted in to the bare-target modifier wire.
        fn with_bare_send_modifiers() -> (Self, EventLog, MoveLog) {
            let (mut fixture, events, moves) = Self::new();
            fixture.bare_send_modifiers = true;
            (fixture, events, moves)
        }
    }

    impl std::fmt::Debug for DragCaptureExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("DragCaptureExternal").finish()
        }
    }

    impl pinion_core::external::External for DragCaptureExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn wants_pointer_capture(&self) -> bool {
            true
        }
        fn capture_normalize(&self) -> CaptureNormalize<'_> {
            if self.normalize_primary {
                CaptureNormalize::Primary
            } else {
                CaptureNormalize::Target
            }
        }
        fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
            self.moves
                .lock()
                .expect("mutex poisoned")
                .push((x_rel, y_rel));
        }
        fn wants_bare_send_modifiers(&self) -> bool {
            self.bare_send_modifiers
        }
        fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
            Some(self)
        }
    }

    impl ExternalIntrospect for DragCaptureExternal {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(&[])
        }
        fn query(&self, _path: &str) -> Option<IntrospectValue> {
            None
        }
        fn intervene(
            &mut self,
            _path: &str,
            _value: IntrospectValue,
        ) -> Result<(), InterveneError> {
            Err(InterveneError::UnknownPath)
        }
        fn invoke(
            &mut self,
            method: &str,
            args: IntrospectValue,
        ) -> Result<IntrospectValue, InvokeError> {
            if method == "send" {
                if let IntrospectValue::Text(name) = args {
                    self.events.lock().expect("mutex poisoned").push(name);
                }
            }
            Ok(IntrospectValue::Null)
        }
    }

    fn read_moves(moves: &MoveLog) -> Vec<(f32, f32)> {
        moves.lock().expect("mutex poisoned").clone()
    }

    /// Paint scene mirroring [`paint_with_button`] but with the
    /// `main_slider` tag — the drag-widget counterpart of the
    /// button-like fixture.
    fn paint_with_slider(viewport_w: u32, viewport_h: u32, slider_rect: Rect) -> Scene {
        let slider = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag("main_slider")
                .with_style(BoxStyle::filled(Color::default())),
        );
        let mut slider_with_rect = slider;
        if let Scene::Container(c) = &mut slider_with_rect {
            c.rect = slider_rect;
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![slider_with_rect])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    fn state_with_slider() -> (Scene, EventLog, MoveLog) {
        let (capture, events, moves) = DragCaptureExternal::new();
        let scene = Scene::External(ExternalNode::new(Box::new(capture)).with_tag("main_slider"));
        (scene, events, moves)
    }

    #[test]
    fn capture_lock_pins_hover_during_drag() {
        // Drag-aware widget: cursor stray off rect during press must
        // NOT fire PointerLeave. The SCXML must stay in its `Dragging`
        // state through the strays, ending only on pointer_up.
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // PointerEnter
        router.pointer_down(PointerId::MOUSE, &mut state); // PointerDown + capture lock
        assert_eq!(
            router.captured_target(PointerId::MOUSE),
            Some("main_slider")
        );
        router.cursor_moved(PointerId::MOUSE, 200.0, 200.0, &mut state); // stray off
        // No PointerLeave during stray — capture lock keeps the
        // hover pinned. Only PointerEnter + PointerDown so far.
        assert_eq!(
            read(&events),
            vec!["PointerEnter".to_string(), "PointerDown".into()],
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // back over
        // Still no extra events (the router is in capture mode,
        // hover doesn't re-resolve).
        assert_eq!(
            read(&events),
            vec!["PointerEnter".to_string(), "PointerDown".into()],
        );
        router.pointer_up(PointerId::MOUSE, &mut state);
        // PointerUp lands now; capture clears; subsequent refresh
        // sees cursor (100, 100) IS on the rect — no PointerLeave.
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
    }

    #[test]
    fn r880_bare_send_modifiers_is_opt_in() {
        // R880 §5.35 §5.49 — a NON-composite (background) release with held
        // modifiers reaches the target as the empty-key three-segment wire
        // `":PointerUp:c"` — but ONLY for an External that opts in via
        // `wants_bare_send_modifiers` (the bare payload doubles as the SCXML
        // event name everywhere else, so the default wire must stay exact).
        let ctrl = Modifiers {
            shift: false,
            ctrl: true,
            alt: false,
            meta: false,
        };

        // Opted-in target: the modifier segment rides the bare wire.
        let mut router = InputRouter::new();
        let (capture, events, _moves) = DragCaptureExternal::with_bare_send_modifiers();
        let mut state =
            Scene::External(ExternalNode::new(Box::new(capture)).with_tag("main_slider"));
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up_with_modifiers(PointerId::MOUSE, &mut state, ctrl);
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                ":PointerUp:c".into()
            ],
        );
        // A modifier-free release stays the colon-free back-compat wire
        // even for an opted-in target.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(read(&events).last().map(String::as_str), Some("PointerUp"));

        // Non-opted target: held modifiers leave the bare wire untouched
        // (a Ctrl+click on a plain widget must stay an SCXML-matchable
        // "PointerUp").
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up_with_modifiers(PointerId::MOUSE, &mut state, ctrl);
        assert_eq!(read(&events).last().map(String::as_str), Some("PointerUp"));
    }

    #[test]
    fn capture_lock_forwards_pointer_move_normalized() {
        // During capture, cursor_moved must forward the cursor as
        // widget-relative normalised coords. Rect (80, 80, 40, 40)
        // means cursor (100, 100) → ((100 - 80) / 40, (100 - 80) / 40)
        // = (0.5, 0.5). The R51.35 click-to-position patch makes
        // pointer_down forward the press-time cursor too, so the
        // press at (100, 100) emits a (0.5, 0.5) entry before the
        // three drag-time moves below.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // PointerEnter (not capture-mode yet)
        assert!(read_moves(&moves).is_empty());
        router.pointer_down(PointerId::MOUSE, &mut state); // enter capture + click-point forward (0.5, 0.5)
        router.cursor_moved(PointerId::MOUSE, 80.0, 80.0, &mut state); // top-left
        router.cursor_moved(PointerId::MOUSE, 120.0, 120.0, &mut state); // bottom-right
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // centre
        let log = read_moves(&moves);
        assert_eq!(log.len(), 4);
        assert!((log[0].0 - 0.5).abs() < 1e-4 && (log[0].1 - 0.5).abs() < 1e-4);
        assert!((log[1].0 - 0.0).abs() < 1e-4 && (log[1].1 - 0.0).abs() < 1e-4);
        assert!((log[2].0 - 1.0).abs() < 1e-4 && (log[2].1 - 1.0).abs() < 1e-4);
        assert!((log[3].0 - 0.5).abs() < 1e-4 && (log[3].1 - 0.5).abs() < 1e-4);
    }

    #[test]
    fn pointer_down_forwards_initial_cursor() {
        // R51.35 §5.35 — click-without-drag still updates the
        // widget's value. The Slider UX precedent: clicking on the
        // track jumps the thumb to the click point even if the user
        // releases without moving the mouse.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Click at x = 110 → x_rel = (110 - 80) / 40 = 0.75.
        router.cursor_moved(PointerId::MOUSE, 110.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read_moves(&moves);
        // Exactly one pointer_move (the click-point); no drag moves
        // because the cursor never moved between down and up.
        assert_eq!(log.len(), 1);
        assert!((log[0].0 - 0.75).abs() < 1e-4);
        assert!((log[0].1 - 0.5).abs() < 1e-4);
    }

    #[test]
    fn capture_lock_allows_coords_outside_rect() {
        // Stray off the widget under capture lock — coords may exceed
        // [0, 1] or be negative; the consumer (Slider) clamps in its
        // own pointer_move impl. R51.35 click-to-position prepends a
        // (0.5, 0.5) press-time entry; the two strays follow.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state); // click-point (0.5, 0.5)
        router.cursor_moved(PointerId::MOUSE, 40.0, 100.0, &mut state); // x = -1.0
        router.cursor_moved(PointerId::MOUSE, 160.0, 100.0, &mut state); // x = 2.0
        let log = read_moves(&moves);
        assert_eq!(log.len(), 3);
        assert!((log[0].0 - 0.5).abs() < 1e-4);
        assert!((log[1].0 - (-1.0)).abs() < 1e-4);
        assert!((log[2].0 - 2.0).abs() < 1e-4);
    }

    #[test]
    fn cursor_left_during_capture_keeps_drag_alive() {
        // Cursor leaves the window while a drag is in flight (the
        // user dragged the mouse off-screen). The router must
        // suppress PointerLeave; the drag resumes when the cursor
        // re-enters. The eventual pointer_up still dispatches.
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_left(PointerId::MOUSE, &mut state); // off-screen
        // No PointerLeave; capture still pinned.
        assert_eq!(
            read(&events),
            vec!["PointerEnter".to_string(), "PointerDown".into()],
        );
        assert_eq!(
            router.captured_target(PointerId::MOUSE),
            Some("main_slider")
        );
        // Drag resumes when cursor re-enters.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
    }

    #[test]
    fn pointer_up_off_widget_dispatches_then_fires_leave() {
        // Drag ended outside the widget rect. PointerUp dispatches
        // to the captured tag (Slider observes Dragging → Hover →
        // value_committed); then the post-release refresh_hover
        // dispatches the deferred PointerLeave (Hover → Idle).
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state); // stray off
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
                "PointerLeave".into(),
            ],
        );
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn button_like_widget_preserves_pre_r51_34_cancel_by_leave() {
        // Regression: a non-capturing widget (default
        // wants_pointer_capture = false) must still cancel by leave
        // — cursor stray off during press fires PointerLeave, and
        // pointer_up off-button is a no-op (existing R47 behaviour).
        let mut router = InputRouter::new();
        let (mut state, events) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state); // PointerLeave
        router.pointer_up(PointerId::MOUSE, &mut state); // no dispatch (hover gone)
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerLeave".into(),
            ],
        );
    }

    #[test]
    fn capture_pointer_up_with_no_hover_or_capture_is_silent() {
        // pointer_up called with nothing pressed and no capture →
        // no dispatch (existing R47 behaviour). Defensive — winit
        // can replay key events on focus regain.
        let mut router = InputRouter::new();
        let (mut state, events) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(read(&events).is_empty());
    }

    #[test]
    fn normalize_cursor_handles_zero_size_rect() {
        // Degenerate layout (e.g. a Slider that hasn't laid out yet)
        // collapses to (0, 0); the router must not divide by zero.
        let rect = Rect::new(10, 10, 0, 0);
        let (x_rel, y_rel) = normalize_cursor(rect, 5.0, 5.0);
        assert!((x_rel - 0.0).abs() < f32::EPSILON);
        assert!((y_rel - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rect_for_tag_returns_inner_when_matched() {
        // rect_for_tag finds the tagged child's rect, not the root.
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        let found = rect_for_tag(&paint, "main_btn").expect("tag present");
        assert_eq!(found, Rect::new(80, 80, 40, 40));
        // Missing tag → None.
        assert!(rect_for_tag(&paint, "ghost").is_none());
    }

    #[test]
    fn capture_lock_skips_when_paint_scene_unset() {
        // Capture entered before the first paint (winit replay edge
        // case). cursor_moved finds no rect → pointer_move silent.
        // The router does not panic.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        // pointer_down without paint → hover_target is None →
        // no PointerDown → no capture. Verify that direct invariant
        // first.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
        // Now seed the paint scene + simulate a successful press to
        // enter capture; then drop the paint scene state by NOT
        // calling update_paint_scene with a fresh rect — the router
        // still holds last_paint_scene from the previous call.
        // (We can't easily clear last_paint_scene from outside; this
        // test instead validates pointer_down before paint does NOT
        // claim capture, exercising the same defensive path.)
        assert!(read_moves(&moves).is_empty());
    }

    // ─── R51.38 §5.35 multi-pointer fixtures + tests ───────────

    /// Paint scene with two drag-aware widgets — `slider_a` on the
    /// left and `slider_b` on the right. Used by the multi-touch
    /// drag tests to exercise the per-pointer capture map.
    fn paint_with_two_sliders(viewport_w: u32, viewport_h: u32) -> Scene {
        let slider_a = {
            let mut s = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag("slider_a")
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut s {
                c.rect = Rect::new(20, 20, 60, 60);
            }
            s
        };
        let slider_b = {
            let mut s = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag("slider_b")
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut s {
                c.rect = Rect::new(120, 20, 60, 60);
            }
            s
        };
        let mut root = Scene::Container(
            ContainerNode::new(vec![slider_a, slider_b])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    /// State scene with two drag-aware externals matching the paint
    /// fixture above. Returns both event + move logs so tests can
    /// distinguish which widget received what.
    #[allow(clippy::type_complexity)]
    fn state_with_two_sliders() -> (Scene, (EventLog, MoveLog), (EventLog, MoveLog)) {
        let (a, ea, ma) = DragCaptureExternal::new();
        let (b, eb, mb) = DragCaptureExternal::new();
        let root = Scene::Container(
            ContainerNode::new(vec![
                Scene::External(ExternalNode::new(Box::new(a)).with_tag("slider_a")),
                Scene::External(ExternalNode::new(Box::new(b)).with_tag("slider_b")),
            ])
            .with_style(BoxStyle::filled(Color::default())),
        );
        (root, (ea, ma), (eb, mb))
    }

    // ── R1098 §5.51 PR-33 — cross-window drop resolution ─────────────

    /// A one-window paint scene: a single opted-in drop-target panel tagged
    /// `tag` filling `rect` (window-local logical px), inside a root filling
    /// the window.
    fn window_with_drop_panel(tag: &str, rect: Rect) -> Scene {
        use pinion_core::style::LayoutStyle;
        let mut panel = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag(tag.to_string())
                .with_layout(LayoutStyle::new().with_drop_target(true))
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut panel {
            c.rect = rect;
        }
        let mut root = Scene::Container(ContainerNode::new(vec![panel]));
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 1000, 800);
        }
        root
    }

    #[test]
    fn r1098_resolves_a_drop_in_the_other_window() {
        // main at the desktop origin holds a dock panel; a floating window at
        // (800, 100) holds its own. An abs cursor over the MAIN panel resolves
        // to main even though the floating window is listed first (a settled
        // floating panel dragged back over main's dock — the PR-33 gap).
        let main = window_with_drop_panel("main_dock", Rect::new(500, 400, 100, 100));
        let floating = window_with_drop_panel("torn", Rect::new(10, 10, 80, 80));
        let windows = [
            ("torn", &floating, (800.0, 100.0)),
            ("main", &main, (0.0, 0.0)),
        ];
        let drop = resolve_cross_window_drop(windows, (550.0, 450.0)).expect("resolves main dock");
        assert_eq!(
            drop.window, "main",
            "the abs cursor maps into the main window"
        );
        assert_eq!(drop.point.tag, "main_dock");
    }

    #[test]
    fn r1118_drop_target_false_floater_rejects_cross_window_dock() {
        // R1118 — the LOAD-BEARING effect of a sole-floater's `drop_target=false`
        // (`DockPanelStyle::with_drop_target(false)`): the floating window exposes
        // NO drop target, so a panel dragged over it resolves nothing — a panel
        // cannot dock INTO a single-panel floater. (The window MOVE is a separate
        // `drag_to_at` branch, not this flag.)
        use pinion_core::style::LayoutStyle;
        let mut panel = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag("torn-viewport".to_string())
                .with_layout(LayoutStyle::new().with_drop_target(false))
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut panel {
            c.rect = Rect::new(10, 10, 80, 80);
        }
        let mut floater = Scene::Container(ContainerNode::new(vec![panel]));
        if let Scene::Container(c) = &mut floater {
            c.rect = Rect::new(0, 0, 1000, 800);
        }
        // Abs cursor (840,140) = floater-local (40,40), squarely inside the panel
        // rect — yet nothing resolves because the panel is not a drop target.
        let windows = [("torn-viewport", &floater, (800.0, 100.0))];
        assert!(
            resolve_cross_window_drop(windows, (840.0, 140.0)).is_none(),
            "a drop_target=false floater rejects a cross-window dock onto it",
        );
        // Control: the SAME geometry with drop_target=true DOES resolve, proving
        // the rejection is the flag's doing, not a geometry miss.
        let decorated = window_with_drop_panel("torn-viewport", Rect::new(10, 10, 80, 80));
        let control = [("torn-viewport", &decorated, (800.0, 100.0))];
        assert!(
            resolve_cross_window_drop(control, (840.0, 140.0)).is_some(),
            "a drop_target=true panel at the same rect DOES resolve (control)",
        );
    }

    #[test]
    fn r1098_transforms_abs_cursor_into_each_window_local_frame() {
        // The floating window sits at (800, 100); its panel is at LOCAL
        // (10, 10, 80, 80). An abs cursor at (840, 140) is floating-local
        // (40, 40) — inside the panel, 0.375 across each axis.
        let floating = window_with_drop_panel("torn", Rect::new(10, 10, 80, 80));
        let windows = [("torn", &floating, (800.0, 100.0))];
        let drop =
            resolve_cross_window_drop(windows, (840.0, 140.0)).expect("resolves the floater");
        assert_eq!(drop.window, "torn");
        assert!(
            (drop.point.x_rel - 0.375).abs() < 1e-4,
            "x_rel {}",
            drop.point.x_rel
        );
        assert!(
            (drop.point.y_rel - 0.375).abs() < 1e-4,
            "y_rel {}",
            drop.point.y_rel
        );
    }

    #[test]
    fn r1098_first_matching_window_wins_so_the_caller_orders_preference() {
        // Two windows whose drop targets both cover the abs cursor: the FIRST
        // in iteration order wins, so the shell controls preference (it lists
        // the non-source window first for a cross-window redock).
        let a = window_with_drop_panel("a_dock", Rect::new(0, 0, 100, 100));
        let b = window_with_drop_panel("b_dock", Rect::new(0, 0, 100, 100));
        let ab = [("a", &a, (0.0, 0.0)), ("b", &b, (0.0, 0.0))];
        assert_eq!(
            resolve_cross_window_drop(ab, (50.0, 50.0)).unwrap().window,
            "a"
        );
        let ba = [("b", &b, (0.0, 0.0)), ("a", &a, (0.0, 0.0))];
        assert_eq!(
            resolve_cross_window_drop(ba, (50.0, 50.0)).unwrap().window,
            "b"
        );
    }

    #[test]
    fn r1099_cursor_up_left_of_a_window_does_not_spuriously_hit() {
        // A cursor LEFT of / ABOVE a window must not resolve it: the hit-test
        // clamps a negative local coordinate to 0, which would otherwise hit
        // the top-left panel. (Regression: the per-window resolver never saw
        // out-of-window cursors; the cross-window one does.)
        let floating = window_with_drop_panel("torn", Rect::new(0, 0, 360, 360));
        let windows = [("torn", &floating, (1040.0, 200.0))];
        // abs (900, 50) is up-left of the floater at (1040, 200) → local
        // (-140, -150). The clamp would hit (0, 0) = the panel; the guard rejects it.
        assert!(resolve_cross_window_drop(windows, (900.0, 50.0)).is_none());
        // A genuinely-inside abs cursor still resolves.
        let inside = resolve_cross_window_drop(windows, (1100.0, 300.0)).expect("inside resolves");
        assert_eq!(inside.window, "torn");
        assert!(inside.point.x_rel >= 0.0 && inside.point.y_rel >= 0.0);
    }

    #[test]
    fn r1098_cursor_over_no_window_drop_target_is_none() {
        // The abs cursor maps into no window's drop target (a gap) → None: a
        // cross-window drop there floats, it does not redock.
        let main = window_with_drop_panel("main_dock", Rect::new(0, 0, 100, 100));
        let windows = [("main", &main, (0.0, 0.0))];
        assert!(resolve_cross_window_drop(windows, (500.0, 500.0)).is_none());
    }

    #[test]
    fn r1098_ignores_non_drop_target_tags_no_hover_fallback() {
        // A window whose node under the cursor is TAGGED but not an opted-in
        // drop target resolves to None — unlike the per-window resolver, the
        // cross-window path takes no hover-tag fallback (a redock must land on
        // a real dock zone, not an arbitrary tagged node in another window).
        let mut node =
            Scene::Container(ContainerNode::new(vec![]).with_tag("not_a_drop_target".to_string()));
        if let Scene::Container(c) = &mut node {
            c.rect = Rect::new(0, 0, 100, 100);
        }
        let mut root = Scene::Container(ContainerNode::new(vec![node]));
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 200, 200);
        }
        let windows = [("main", &root, (0.0, 0.0))];
        assert!(resolve_cross_window_drop(windows, (50.0, 50.0)).is_none());
    }

    #[test]
    fn r1156_outer_dock_zone_at_every_window_perimeter() {
        // A cursor in the outermost OUTER_DOCK_MARGIN band of the window content is
        // a FULL-SPAN outer dock (the container-edge gesture), NOT an inner panel
        // split — even though a panel fills the area under it. Pass 0 wins the
        // perimeter over the inner exact pass.
        let host = window_with_drop_panel("body", Rect::new(0, 0, 1000, 800));
        let windows = [("main", &host, (0.0, 0.0))];
        for (x, y, lbl) in [
            (100.0, 10.0, "top"),
            (10.0, 400.0, "left"),
            (990.0, 400.0, "right"),
            (500.0, 790.0, "bottom"),
        ] {
            let drop = resolve_cross_window_drop(windows, (x, y)).expect(lbl);
            assert_eq!(drop.window, "main");
            assert_eq!(
                drop.point.tag, OUTER_DOCK_ZONE_TAG,
                "{lbl} perimeter is outer"
            );
        }
    }

    #[test]
    fn r1156_interior_is_an_inner_panel_not_outer() {
        // Away from the perimeter the cursor resolves the inner panel (exact pass),
        // not an outer full-span dock — interior boundaries keep per-panel zones.
        let host = window_with_drop_panel("body", Rect::new(0, 0, 1000, 800));
        let windows = [("main", &host, (0.0, 0.0))];
        let drop = resolve_cross_window_drop(windows, (500.0, 400.0)).expect("center resolves");
        assert_eq!(
            drop.point.tag, "body",
            "the interior resolves the inner panel"
        );
    }

    #[test]
    fn r1156_beyond_the_perimeter_band_floats() {
        // Far outside every window → no dock (float), not an outer snap.
        let host = window_with_drop_panel("body", Rect::new(0, 0, 1000, 800));
        let windows = [("main", &host, (0.0, 0.0))];
        // 200px above the top edge, far beyond the 32px perimeter band.
        assert!(resolve_cross_window_drop(windows, (100.0, -200.0)).is_none());
    }

    #[test]
    fn r1156_outer_drop_point_normalised_over_the_whole_window() {
        // The outer DropPoint carries the cursor normalised over the WHOLE window
        // (not a panel) so the dock consumer (`outer_zone_for`) derives the nearest
        // edge: a top-perimeter cursor has a small y_rel and an x_rel = x / width.
        let host = window_with_drop_panel("body", Rect::new(0, 0, 1000, 800));
        let windows = [("main", &host, (0.0, 0.0))];
        let drop = resolve_cross_window_drop(windows, (300.0, 8.0)).expect("top perimeter");
        assert_eq!(drop.point.tag, OUTER_DOCK_ZONE_TAG);
        assert!(
            drop.point.y_rel < 0.05,
            "near the top → small y_rel ({})",
            drop.point.y_rel
        );
        assert!(
            (drop.point.x_rel - 0.3).abs() < 1e-4,
            "x_rel {}",
            drop.point.x_rel
        );
    }

    #[test]
    fn pointer_id_mouse_is_reserved_zero() {
        // Backwards-compat invariant: mouse pointer maps to the
        // reserved `PointerId(0)` slot; touch finger ids offset by
        // one so they never alias the mouse no matter what winit
        // hands the router.
        assert_eq!(PointerId::MOUSE.raw(), 0);
        assert_eq!(PointerId::touch(0).raw(), 1);
        assert_eq!(PointerId::touch(42).raw(), 43);
        assert_ne!(PointerId::MOUSE, PointerId::touch(0));
    }

    #[test]
    fn two_touches_drag_two_widgets_independently() {
        // Multi-touch first-design invariant: two fingers on two
        // widgets each enter capture lock on their own tag and
        // forward `pointer_move` only to their own widget. Single-
        // target capture (`Option<String>`) would alias here — the
        // R51.38 HashMap substrate makes this work without aliasing.
        let mut router = InputRouter::new();
        let (mut state, (ea, ma), (eb, mb)) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let t1 = PointerId::touch(0);
        let t2 = PointerId::touch(1);
        // Touch 1 lands on slider_a's centre (50, 50).
        router.cursor_moved(t1, 50.0, 50.0, &mut state);
        router.pointer_down(t1, &mut state);
        // Touch 2 lands on slider_b's centre (150, 50).
        router.cursor_moved(t2, 150.0, 50.0, &mut state);
        router.pointer_down(t2, &mut state);
        assert_eq!(router.captured_target(t1), Some("slider_a"));
        assert_eq!(router.captured_target(t2), Some("slider_b"));
        // Drag each in opposite directions. Each widget's
        // `pointer_move` only sees its own touch's coords; the
        // sequence below would alias under a single-target capture
        // implementation (the second touch would overwrite the
        // first's lock).
        router.cursor_moved(t1, 70.0, 50.0, &mut state); // slider_a right
        router.cursor_moved(t2, 130.0, 50.0, &mut state); // slider_b left
        router.pointer_up(t1, &mut state);
        router.pointer_up(t2, &mut state);
        // slider_a saw the click-point + one drag move.
        let log_a = read_moves(&ma);
        assert_eq!(log_a.len(), 2);
        assert!((log_a[0].0 - 0.5).abs() < 1e-4); // click point
        assert!((log_a[1].0 - 0.8333).abs() < 1e-3); // (70-20)/60
        // slider_b saw its own click-point + drag.
        let log_b = read_moves(&mb);
        assert_eq!(log_b.len(), 2);
        assert!((log_b[0].0 - 0.5).abs() < 1e-4); // click point
        assert!((log_b[1].0 - 0.1666).abs() < 1e-3); // (130-120)/60
        // PointerEnter / Down / Up streams independent per widget.
        assert_eq!(
            read(&ea),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(
            read(&eb),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(router.captured_target(t1), None);
        assert_eq!(router.captured_target(t2), None);
    }

    #[test]
    fn mouse_and_touch_dont_alias_hover() {
        // Mouse on slider_a, touch on slider_b — both pointers have
        // their own `hover_target` entry. Per-pointer dispatch means
        // each widget sees its own PointerEnter without aliasing.
        let mut router = InputRouter::new();
        let (mut state, (ea, _ma), (eb, _mb)) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let touch = PointerId::touch(0);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state);
        router.cursor_moved(touch, 150.0, 50.0, &mut state);
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("slider_a"));
        assert_eq!(router.hover_target(touch), Some("slider_b"));
        // Each widget observed exactly one PointerEnter — neither
        // saw the other pointer's transitions.
        assert_eq!(read(&ea), vec!["PointerEnter".to_string()]);
        assert_eq!(read(&eb), vec!["PointerEnter".to_string()]);
    }

    #[test]
    fn releasing_one_touch_does_not_release_other_capture() {
        // Per-pointer capture isolation: lifting one finger must
        // not break the other finger's drag. The shared single-
        // target `Option<String>` capture would collapse here (the
        // first pointer_up would clear the lock for both).
        let mut router = InputRouter::new();
        let (mut state, _a, _b) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let t1 = PointerId::touch(0);
        let t2 = PointerId::touch(1);
        router.cursor_moved(t1, 50.0, 50.0, &mut state);
        router.pointer_down(t1, &mut state);
        router.cursor_moved(t2, 150.0, 50.0, &mut state);
        router.pointer_down(t2, &mut state);
        assert_eq!(router.captured_target(t1), Some("slider_a"));
        assert_eq!(router.captured_target(t2), Some("slider_b"));
        // Lift touch 1 only.
        router.pointer_up(t1, &mut state);
        assert_eq!(router.captured_target(t1), None);
        // Touch 2's lock survives.
        assert_eq!(router.captured_target(t2), Some("slider_b"));
        router.pointer_up(t2, &mut state);
        assert_eq!(router.captured_target(t2), None);
    }

    #[test]
    fn cursor_left_for_one_pointer_keeps_other_state() {
        // Cursor leaves the window for the mouse pointer, but a
        // touch pointer's hover should be untouched. Per-pointer
        // `cursor_left` only drops the matching id's cursor.
        let mut router = InputRouter::new();
        let (mut state, (ea, _ma), (eb, _mb)) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let touch = PointerId::touch(0);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state);
        router.cursor_moved(touch, 150.0, 50.0, &mut state);
        router.cursor_left(PointerId::MOUSE, &mut state);
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
        assert_eq!(router.hover_target(touch), Some("slider_b"));
        // slider_a saw Enter + Leave; slider_b only Enter.
        assert_eq!(
            read(&ea),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(read(&eb), vec!["PointerEnter".to_string()]);
    }

    #[test]
    fn wants_capture_cache_co_locates_with_hover_walk() {
        // R51.40 §5.35 — `pointer_down` reads a cached bit
        // populated by the matching `refresh_hover` instead of
        // walking the state-scene tree itself. Behavioural
        // verification: a drag-aware widget still enters capture on
        // press, and a button-like widget still does not — same
        // observable outcome as the pre-R51.40 walk-on-click path,
        // exercising the cache lookup chain end-to-end.
        let mut router = InputRouter::new();
        let (mut state, _events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        // Pointer hovers a drag-aware widget — the cache hit makes
        // pointer_down lock capture without re-walking the scene.
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(
            router.captured_target(PointerId::MOUSE),
            Some("main_slider")
        );
        router.pointer_up(PointerId::MOUSE, &mut state);
        // Drop hover (cache cleared on PointerLeave path).
        let mut router2 = InputRouter::new();
        let (mut state2, _events) = state_with_button();
        let paint2 = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router2.update_paint_scene(paint2, &mut state2);
        router2.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state2);
        // Button-like widget cached as wants_capture=false — no
        // lock on press.
        router2.pointer_down(PointerId::MOUSE, &mut state2);
        assert_eq!(router2.captured_target(PointerId::MOUSE), None);
    }

    // ─── R51.42 §5.35 sub-index dispatch fixtures + tests ─────

    /// Paint scene with a single composite hit-target carrying the
    /// `"<primary>#<sub_index>"` tag convention from the R51.41 RFC.
    /// One container, fixed rect — mirrors the per-radio rect a real
    /// `RadioGroup` would lay out for index `sub_index` under primary
    /// `primary`.
    fn paint_with_subindex_tag(
        viewport_w: u32,
        viewport_h: u32,
        rect: Rect,
        primary: &str,
        sub_index: &str,
    ) -> Scene {
        let composite_tag = format!("{primary}#{sub_index}");
        let inner = {
            let mut s = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag(composite_tag)
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut s {
                c.rect = rect;
            }
            s
        };
        let mut root = Scene::Container(
            ContainerNode::new(vec![inner]).with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    /// State scene with one `CaptureExternal` tagged by the primary
    /// half of the composite hit-target convention (no `'#'`). Tests
    /// inspect `captures` to assert the wire payload the router
    /// forwards through `invoke("send", ...)`.
    fn state_with_primary_external(primary: &str) -> (Scene, Arc<Mutex<Vec<String>>>) {
        let (capture, captures) = CaptureExternal::new();
        let scene =
            Scene::External(ExternalNode::new(Box::new(capture)).with_tag(primary.to_string()));
        (scene, captures)
    }

    /// State scene with one `DragCaptureExternal` tagged by the
    /// primary half. Tests use this to verify the wiring of
    /// `widget_wants_capture` and `forward_pointer_move` through
    /// the sub-index split — even though `RadioGroup` itself returns
    /// `wants_pointer_capture = false`, a future composite drag
    /// widget would rely on the symmetric path landing here.
    fn state_with_primary_drag(primary: &str) -> (Scene, EventLog, MoveLog) {
        let (drag, events, moves) = DragCaptureExternal::new();
        let scene =
            Scene::External(ExternalNode::new(Box::new(drag)).with_tag(primary.to_string()));
        (scene, events, moves)
    }

    /// R738 §5.35 — paint fixture for a composite that paints BOTH a
    /// primary track rect and a sub-tagged thumb rect (the range-slider
    /// shape), so primary-rect vs sub-rect normalization differ.
    fn paint_with_primary_and_subtag(
        viewport_w: u32,
        viewport_h: u32,
        primary_rect: Rect,
        thumb_rect: Rect,
        primary: &str,
        sub_index: &str,
    ) -> Scene {
        let mut thumb = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag(format!("{primary}#{sub_index}"))
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut thumb {
            c.rect = thumb_rect;
        }
        let mut track = Scene::Container(
            ContainerNode::new(vec![thumb])
                .with_tag(primary.to_string())
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut track {
            c.rect = primary_rect;
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![track]).with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    #[test]
    fn capture_normalize_against_primary_uses_track_rect() {
        // R738 regression: a capture widget that returns
        // `CaptureNormalize::Primary` (the dual-thumb range
        // slider) normalizes the dragged cursor against the PRIMARY
        // (track) rect even though capture pinned a thumb sub-tag — so
        // x_rel maps across the whole track instead of saturating on the
        // thumb rect (the bug where grabbing the low thumb moved the
        // high thumb). The `DragCaptureExternal` mock here returns true.
        let mut router = InputRouter::new();
        let (drag, _events, moves) = DragCaptureExternal::with_normalize_primary(true);
        let mut state = Scene::External(ExternalNode::new(Box::new(drag)).with_tag("range"));
        // Track x 80..120 (width 40); thumb x 96..104 (width 8). Cursor
        // x=98 is on the thumb but off-centre so the two rects differ.
        let paint = paint_with_primary_and_subtag(
            200,
            200,
            Rect::new(80, 80, 40, 40),
            Rect::new(96, 80, 8, 40),
            "range",
            "low",
        );
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 98.0, 100.0, &mut state); // hover thumb
        router.pointer_down(PointerId::MOUSE, &mut state); // capture range#low + forward
        assert_eq!(router.captured_target(PointerId::MOUSE), Some("range#low"));
        let log = read_moves(&moves);
        assert_eq!(log.len(), 1, "click-to-position forwards once");
        // (98-80)/40 = 0.45 against the TRACK; against the 8px thumb it
        // would be (98-96)/8 = 0.25. Asserting 0.45 proves the opt-in
        // normalizes against the primary rect.
        assert!(
            (log[0].0 - 0.45).abs() < 1e-4,
            "x_rel normalized against the track (0.45), got {}",
            log[0].0
        );
    }

    #[test]
    fn capture_default_normalizes_against_subtag_even_with_primary_painted() {
        // R738 — the DEFAULT (false) normalizes against the captured
        // sub-tag's rect even when the primary IS painted. This is the
        // dock tear-off's exact shape (panel primary + header sub-tag):
        // its tear-off fraction is measured relative to the grabbed
        // header, so it must NOT normalize against the whole panel.
        let mut router = InputRouter::new();
        let (drag, _events, moves) = DragCaptureExternal::with_normalize_primary(false);
        let mut state = Scene::External(ExternalNode::new(Box::new(drag)).with_tag("panel"));
        let paint = paint_with_primary_and_subtag(
            200,
            200,
            Rect::new(80, 80, 40, 40),
            Rect::new(96, 80, 8, 40),
            "panel",
            "header",
        );
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 98.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(
            router.captured_target(PointerId::MOUSE),
            Some("panel#header")
        );
        let log = read_moves(&moves);
        assert_eq!(log.len(), 1);
        // (98-96)/8 = 0.25 against the 8px header sub-rect (NOT 0.45,
        // which would be the whole-panel normalization).
        assert!(
            (log[0].0 - 0.25).abs() < 1e-4,
            "x_rel normalized against the sub-tag header (0.25), got {}",
            log[0].0
        );
    }

    #[test]
    fn sub_index_dispatch_forwards_idx_prefixed_event_name() {
        // Paint `main_group#2` + state `main_group` → cursor on the
        // sub-region drives the composite External with the
        // `"2:<EventName>"` wire payload the RadioGroup invoke
        // handler parses (radio_group.rs:357 `split_once(':')`).
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_primary_external("main_group");
        let paint = paint_with_subindex_tag(200, 200, Rect::new(80, 80, 40, 40), "main_group", "2");
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&captures),
            vec![
                "2:PointerEnter".to_string(),
                "2:PointerDown".into(),
                "2:PointerUp".into(),
            ],
        );
        // The router stores the raw paint tag (with `#`) in its
        // hover map so subsequent leave-on-stray still routes to
        // the right sub-region.
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_group#2"));
    }

    #[test]
    fn r781_pointer_up_with_modifiers_appends_the_wire_token() {
        // R781 — a Shift+Ctrl release at the activate edge gains the third
        // `:sc` wire segment; the non-activation PointerDown (no modifier
        // variant) keeps the two-segment back-compat wire. This is the
        // emit side of the R773 encode↔decode pair (the decode side is
        // `composite_tag::split_send_payload`).
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_primary_external("main_group");
        let paint = paint_with_subindex_tag(200, 200, Rect::new(80, 80, 40, 40), "main_group", "2");
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        let mods = Modifiers {
            shift: true,
            ctrl: true,
            alt: false,
            meta: false,
        };
        router.pointer_up_with_modifiers(PointerId::MOUSE, &mut state, mods);
        assert_eq!(
            read(&captures),
            vec![
                "2:PointerEnter".to_string(),
                "2:PointerDown".into(),
                "2:PointerUp:sc".into(),
            ],
            "the activate edge carries the modifier token, hover/press do not",
        );
    }

    #[test]
    fn r781_pointer_up_no_modifiers_is_byte_identical_to_plain() {
        // Empty modifiers → the exact pre-R781 two-segment wire (every
        // existing composite consumer is unaffected).
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_primary_external("main_group");
        let paint = paint_with_subindex_tag(200, 200, Rect::new(80, 80, 40, 40), "main_group", "2");
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up_with_modifiers(PointerId::MOUSE, &mut state, Modifiers::empty());
        assert_eq!(
            read(&captures).last().map(String::as_str),
            Some("2:PointerUp")
        );
    }

    #[test]
    fn single_tag_backwards_compat() {
        // Plain `main_btn` tag (no `'#'`) routes verbatim — the
        // R51.34 / .37 / .38 / .40 fixtures all use this shape; this
        // test re-asserts the unsplit path under the R51.42 splitter
        // to lock in backwards compatibility.
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&captures),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
    }

    #[test]
    fn sub_index_capture_wires_to_primary() {
        // Composite drag-aware widget (paint `composite#0`, state
        // `composite` drag-capture). `widget_wants_capture` looks up
        // the primary half and returns `true`; `pointer_down` locks
        // capture on the *raw* paint tag (so the leave-deferred
        // refresh can find the sub-region rect again); the captured
        // `forward_pointer_move` normalises via the raw rect but
        // routes the call to the primary `External`. R51.41
        // composite hit-target convention is symmetric for drag-
        // aware composites even though `RadioGroup` itself opts out.
        // (R738 §5.35 — the dock tear-off relies on this raw-sub-rect
        // normalization: its tear-off fraction is measured relative to
        // the grabbed header, not the whole panel. The range slider,
        // which needs whole-track normalization, instead makes the
        // *track* its sole capture target rather than tagging thumbs.)
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_primary_drag("composite");
        let paint = paint_with_subindex_tag(200, 200, Rect::new(80, 80, 40, 40), "composite", "0");
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        // Captured target stores the raw paint tag (with `#`) so a
        // subsequent stray off the sub-region keeps the drag alive.
        assert_eq!(
            router.captured_target(PointerId::MOUSE),
            Some("composite#0"),
        );
        // Click-to-position forwarded one normalised entry — rect is
        // (80, 80, 40, 40) so cursor (100, 100) maps to (0.5, 0.5).
        let log = read_moves(&moves);
        assert_eq!(log.len(), 1);
        assert!((log[0].0 - 0.5).abs() < 1e-4 && (log[0].1 - 0.5).abs() < 1e-4);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
    }

    #[test]
    fn empty_subindex_treated_as_unsplit() {
        // Degenerate paint tag `main_btn#` (trailing `'#'` with empty
        // sub-index). The router must collapse to the unsplit path
        // so the wire payload is `PointerEnter`, not the malformed
        // `:PointerEnter` that composite widgets would reject. The
        // application's paint-tag schema is treated as opaque: the
        // router never panics, and the dispatch payload is well-
        // formed under either convention.
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button(); // tag = "main_btn"
        let paint = {
            // Hand-build a paint scene whose tagged container ends
            // with a literal `'#'` — `paint_with_button` uses the
            // plain tag so we inline the construction here.
            let mut inner = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag("main_btn#")
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut inner {
                c.rect = Rect::new(80, 80, 40, 40);
            }
            let mut root = Scene::Container(
                ContainerNode::new(vec![inner]).with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut root {
                c.rect = Rect::new(0, 0, 200, 200);
            }
            root
        };
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        // Unsplit path: payload is the raw event name, no `'<empty>:'`
        // prefix.
        assert_eq!(
            read(&captures),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
    }

    // ─── R51.186 §5.45 R55.C.2 wheel dispatch fixtures + tests ────

    use pinion_core::scene::{BoxNode, ScrollNode};
    use pinion_core::widgets::scroll::ScrollState;
    use std::rc::Rc;

    /// Build a paint scene with a single `Scene::Scroll` wrapping a
    /// `Scene::Box`. Optionally attach a `ScrollState` for wheel
    /// routing — `None` exercises the "declarative-only scroll"
    /// silent-drop path; `Some(rc)` exercises the dispatch path.
    fn paint_with_scroll(
        viewport_w: u32,
        viewport_h: u32,
        scroll_viewport: Rect,
        content_w: u32,
        content_h: u32,
        state: Option<Rc<ScrollState>>,
    ) -> Scene {
        let content = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, content_w, content_h),
            Color::default(),
        ));
        let mut scroll = ScrollNode::new(scroll_viewport, content);
        if let Some(s) = state {
            scroll = scroll.with_state(s);
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![Scene::Scroll(scroll)])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    #[test]
    fn r55_c2_wheel_no_op_without_cursor() {
        // R55.C.2 — wheel before any `cursor_moved` for this
        // pointer (cursor never entered the window) is a silent
        // drop. winit / web / iOS reuse the last stored cursor,
        // so an empty `cursors` map = no dispatch target.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            500,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        // No cursor_moved before wheel.
        let dispatched = router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 40.0 },
            &mut state_scene,
        );
        assert!(!dispatched);
        assert_eq!(state.offset(), (0, 0));
    }

    #[test]
    fn r55_c2_wheel_no_op_without_paint_scene() {
        // R55.C.2 — wheel before the first paint commit is a
        // silent drop. The router has no `last_paint_scene` to
        // hit-test against.
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        let dispatched = router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 40.0 },
            &mut state_scene,
        );
        assert!(!dispatched);
    }

    #[test]
    fn r55_c2_wheel_no_op_off_scroll_container() {
        // R55.C.2 — cursor on a non-scroll widget (the button
        // fixture) is a silent drop. The button is not a
        // wheel target.
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state_scene);
        let dispatched = router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 40.0 },
            &mut state_scene,
        );
        assert!(!dispatched);
    }

    #[test]
    fn r55_c2_wheel_silent_drop_on_stateless_scroll() {
        // R55.C.2 — the ScrollNode covers the cursor but has no
        // `state` link (declarative-only). The router drops
        // silently rather than panicking — the application can
        // ship a Scroll primitive without wiring input routing.
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(200, 200, Rect::new(0, 0, 100, 100), 200, 500, None);
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        let dispatched = router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 40.0 },
            &mut state_scene,
        );
        assert!(!dispatched);
    }

    #[test]
    fn r55_c2_wheel_pixels_scrolls_attached_state() {
        // R55.C.2 — Pixels deltas route through verbatim into
        // `ScrollState::scroll_by`. Positive `dy` scrolls
        // downward (W3C sign convention).
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        let dispatched = router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 40.0 },
            &mut state_scene,
        );
        assert!(dispatched);
        assert_eq!(state.offset(), (0, 40));
        // Second wheel — accumulates.
        router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 35.0 },
            &mut state_scene,
        );
        assert_eq!(state.offset(), (0, 75));
        // Horizontal axis routes too.
        router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 12.0, dy: 0.0 },
            &mut state_scene,
        );
        assert_eq!(state.offset(), (12, 75));
    }

    #[test]
    fn r55_c2_wheel_lines_multiplies_by_line_height_px() {
        // R55.C.2 — Lines deltas scale by `LINE_HEIGHT_PX`
        // (16, the W3C / browser default). One line = 16 px;
        // three lines = 48 px.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        let dispatched = router.wheel(
            PointerId::MOUSE,
            WheelDelta::Lines { dx: 0.0, dy: 3.0 },
            &mut state_scene,
        );
        assert!(dispatched);
        assert_eq!(state.offset(), (0, 48));
        // Negative line delta scrolls upward; clamped at zero.
        router.wheel(
            PointerId::MOUSE,
            WheelDelta::Lines { dx: 0.0, dy: -10.0 },
            &mut state_scene,
        );
        assert_eq!(state.offset(), (0, 0));
    }

    #[test]
    fn r55_c2_wheel_clamps_against_state_bounds() {
        // R55.C.2 — overshooting the declared bound clamps at the
        // bound rather than wrapping. ScrollState's own clamp
        // logic carries the policy; the router just feeds the
        // delta through.
        let state = Rc::new(ScrollState::new());
        state.set_max(100, 100);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels {
                dx: 0.0,
                dy: 9999.0,
            },
            &mut state_scene,
        );
        // Bound is 100 on the y axis.
        assert_eq!(state.offset(), (0, 100));
    }

    #[test]
    fn r55_c2_wheel_nan_delta_is_zero_offset() {
        // R55.C.2 — adversarial NaN delta clamps to zero (NaN
        // guard in `round_clamp_i32`); the dispatch still ran
        // (the router resolved a state target), so the return
        // is `true` — backends still consider this a
        // "dispatched" wheel even though the offset did not
        // move. Matches the R51.145 `clamp_frame_dt` NaN guard
        // precedent.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        let dispatched = router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels {
                dx: f32::NAN,
                dy: f32::NAN,
            },
            &mut state_scene,
        );
        assert!(dispatched, "NaN delta still counts as a dispatched wheel");
        assert_eq!(state.offset(), (0, 0));
    }

    #[test]
    fn r55_c2_wheel_routes_through_last_cursor_position() {
        // R55.C.2 — the router uses the *current* cursor stored
        // for the pointer, so moving the cursor away from the
        // scroll viewport before wheeling silently drops the
        // wheel. Mirrors winit's MouseWheel-without-position
        // contract: the surface owns the cursor; the wheel
        // applies to whatever is under it now.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        // Cursor enters scroll → wheel dispatches.
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        assert!(router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 30.0 },
            &mut state_scene,
        ));
        assert_eq!(state.offset(), (0, 30));
        // Cursor leaves the scroll viewport → wheel silently
        // drops. The stored cursor is still set (in fact the
        // refresh fired PointerLeave on the button-like state
        // scene's `main_btn` which is silent under
        // CaptureExternal), but it does not cover any scroll
        // viewport.
        router.cursor_moved(PointerId::MOUSE, 150.0, 150.0, &mut state_scene);
        assert!(!router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 30.0 },
            &mut state_scene,
        ));
        // Offset unchanged.
        assert_eq!(state.offset(), (0, 30));
    }

    #[test]
    fn r55_c2_two_pointers_each_route_through_their_own_cursor() {
        // R55.C.2 — multi-pointer: two pointers each track their
        // own cursor; a wheel for `PointerId::touch(0)` reads
        // that pointer's cursor, not the mouse's. The shared
        // `cursors` map keyed by `PointerId` keeps the lookups
        // independent.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        let t = PointerId::touch(0);
        // Mouse cursor over the scroll; touch cursor outside.
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        router.cursor_moved(t, 150.0, 150.0, &mut state_scene);
        // Wheel via touch — touch cursor is outside the scroll
        // viewport → silent drop.
        assert!(!router.wheel(
            t,
            WheelDelta::Pixels { dx: 0.0, dy: 20.0 },
            &mut state_scene
        ));
        assert_eq!(state.offset(), (0, 0));
        // Wheel via mouse — dispatches.
        assert!(router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 20.0 },
            &mut state_scene,
        ));
        assert_eq!(state.offset(), (0, 20));
    }

    // ─── R877 §5.15 §5.49 External wheel-offer tests ───────────────

    /// One recorded [`External::wheel`] call: `(x_rel, y_rel, dx, dy, modifiers)`.
    type WheelCall = (f32, f32, f32, f32, Modifiers);

    /// Records every [`External::wheel`] call; consumes (returns
    /// `true`) iff `consume` is set — the two sides of the R877
    /// innermost-listener-first contract.
    struct WheelExternal {
        calls: Arc<Mutex<Vec<WheelCall>>>,
        consume: bool,
    }

    impl WheelExternal {
        fn new(consume: bool) -> (Self, Arc<Mutex<Vec<WheelCall>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    calls: Arc::clone(&calls),
                    consume,
                },
                calls,
            )
        }
    }

    impl std::fmt::Debug for WheelExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WheelExternal").finish()
        }
    }

    impl pinion_core::external::External for WheelExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn wheel(
            &mut self,
            x_rel: f32,
            y_rel: f32,
            dx: f32,
            dy: f32,
            modifiers: Modifiers,
        ) -> bool {
            self.calls
                .lock()
                .expect("mutex poisoned")
                .push((x_rel, y_rel, dx, dy, modifiers));
            self.consume
        }
    }

    /// Paint: the `main_btn` rect (80..120 × 80..120, optionally with a
    /// composite `sub` child) inside a stateful scroll viewport covering
    /// the whole 200×200 window, so a declined offer has a live scroll
    /// fallback to land on. The tagged button sits inside an *untagged*
    /// content container — Scroll content is path-transparent (R55.A.3),
    /// so the content node itself is a wrapper layer, never a hover
    /// target (the real-binding shape).
    fn paint_with_button_over_scroll(state: Rc<ScrollState>, sub: Option<Scene>) -> Scene {
        let button = {
            let mut b = Scene::Container(
                ContainerNode::new(sub.into_iter().collect())
                    .with_tag("main_btn")
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut b {
                c.rect = Rect::new(80, 80, 40, 40);
            }
            b
        };
        let content = {
            let mut c = Scene::Container(
                ContainerNode::new(vec![button]).with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(node) = &mut c {
                node.rect = Rect::new(0, 0, 200, 1000);
            }
            c
        };
        let scroll = ScrollNode::new(Rect::new(0, 0, 200, 200), content).with_state(state);
        let mut root = Scene::Container(
            ContainerNode::new(vec![Scene::Scroll(scroll)])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 200, 200);
        }
        root
    }

    #[test]
    fn r877_wheel_offer_consumed_by_hovered_external_skips_scroll() {
        // The hovered External consumes the wheel → dispatched, scroll
        // state untouched, coordinates normalised over the Target rect
        // (cursor (100, 90) over rect 80..120 → rel (0.5, 0.25)), and
        // Lines deltas arrive pre-scaled by LINE_HEIGHT_PX at f32.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, calls) = WheelExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        assert!(router.wheel(
            PointerId::MOUSE,
            WheelDelta::Lines { dx: 0.0, dy: 2.0 },
            &mut state_scene,
        ));
        assert_eq!(
            scroll.offset(),
            (0, 0),
            "consumed wheel must not also scroll"
        );
        let recorded = calls.lock().expect("mutex poisoned").clone();
        assert_eq!(recorded.len(), 1);
        let (x_rel, y_rel, dx, dy, mods) = recorded[0];
        assert!((x_rel - 0.5).abs() < 1e-6, "x_rel {x_rel}");
        assert!((y_rel - 0.25).abs() < 1e-6, "y_rel {y_rel}");
        assert!((dx - 0.0).abs() < f32::EPSILON);
        assert!((dy - 2.0 * LINE_HEIGHT_PX).abs() < f32::EPSILON, "dy {dy}");
        assert!(mods.is_empty());
    }

    #[test]
    fn r877_wheel_offer_declined_falls_through_to_scroll() {
        // The hovered External declines (returns false) → the pre-R877
        // Scroll dispatch runs byte-identically underneath.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, calls) = WheelExternal::new(false);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        assert!(router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 40.0 },
            &mut state_scene,
        ));
        assert_eq!(
            scroll.offset(),
            (0, 40),
            "declined offer falls through to scroll"
        );
        assert_eq!(
            calls.lock().expect("mutex poisoned").len(),
            1,
            "offer was made first"
        );
    }

    #[test]
    fn r877_wheel_modifiers_reach_the_external() {
        // wheel_with_modifiers hands the held modifiers through — the
        // Ctrl bit a canvas zoom branches on.
        let scroll = Rc::new(ScrollState::new());
        let (ext, calls) = WheelExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(scroll, None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::empty()
        };
        assert!(router.wheel_with_modifiers(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: -10.0 },
            ctrl,
            &mut state_scene,
        ));
        let recorded = calls.lock().expect("mutex poisoned").clone();
        assert_eq!(recorded.len(), 1);
        assert!(
            recorded[0].4.control_key(),
            "ctrl modifier must reach the External"
        );
    }

    #[test]
    fn r877_wheel_composite_subtag_routes_to_primary_external() {
        // Hovering a composite `main_btn#sub_0` child still offers the
        // wheel to the `main_btn` primary External (split_subindex), so
        // wheeling over a node card reaches the canvas coordinator.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, calls) = WheelExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let sub = {
            let mut s = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag("main_btn#sub_0")
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut s {
                c.rect = Rect::new(90, 90, 20, 20);
            }
            s
        };
        let paint = paint_with_button_over_scroll(Rc::clone(&scroll), Some(sub));
        let mut router = InputRouter::new();
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state_scene);
        assert!(router.wheel(
            PointerId::MOUSE,
            WheelDelta::Pixels { dx: 0.0, dy: 8.0 },
            &mut state_scene,
        ));
        assert_eq!(scroll.offset(), (0, 0));
        assert_eq!(calls.lock().expect("mutex poisoned").len(), 1);
    }

    // ─── R881 §5.35 §5.49 middle-button drag-to-pan tests ─────────

    #[test]
    fn r881_middle_drag_pans_pinned_scroll_content_follows_cursor() {
        // The core arc: middle press over a scrollable, drag past the
        // dead zone → the pinned ScrollState pans by `last - current`
        // (content follows the cursor: dragging the cursor UP reveals
        // lower content, offset grows). Release resolves to Pan so the
        // shell never pastes.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        // (50, 90): inside the scroll viewport, off the tagged button.
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        // 2 px wobble: inside the dead zone — nothing pans yet.
        assert!(!router.cursor_moved(PointerId::MOUSE, 50.0, 88.0, &mut state_scene));
        assert_eq!(scroll.offset(), (0, 0), "dead-zone wobble must not pan");
        // 30 px up from the origin: latched. Dead-zone wobble never
        // advanced `last`, so the FULL displacement from the grab
        // origin (90 → 60 = 30) dispatches — total tracking, the
        // Blender grab semantic (no motion is lost to the dead zone).
        assert!(router.cursor_moved(PointerId::MOUSE, 50.0, 60.0, &mut state_scene));
        assert_eq!(scroll.offset(), (0, 30), "content follows the cursor");
        // Further horizontal move pans x too.
        assert!(router.cursor_moved(PointerId::MOUSE, 30.0, 60.0, &mut state_scene));
        assert_eq!(scroll.offset(), (20, 30));
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Pan);
    }

    #[test]
    fn r881_middle_press_release_in_place_is_click() {
        // A press-release inside the dead zone is the middle-*click*:
        // the shell runs its paste funnel on this verdict, and nothing
        // pans.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 52.0, 91.0, &mut state_scene);
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Click);
        assert_eq!(scroll.offset(), (0, 0));
        // The gesture is consumed: a second (spurious) release reports
        // NoPress, so the shell cannot double-paste.
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::NoPress);
    }

    #[test]
    fn r881_pointer_cancel_revokes_middle_gesture_no_paste() {
        // The R880.1 mandatory-cancel-arm discipline: a cancelled
        // gesture is "never happened" — the trailing OS release must
        // resolve to NoPress (no paste), and already-applied pan
        // deltas stay (incremental scrolling, not a journaled
        // transaction).
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 50.0, 60.0, &mut state_scene);
        assert_eq!(scroll.offset(), (0, 30));
        router.pointer_cancel(PointerId::MOUSE, &mut state_scene);
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::NoPress);
        assert_eq!(
            scroll.offset(),
            (0, 30),
            "applied pan deltas are not rolled back"
        );
    }

    #[test]
    fn r881_middle_press_before_any_cursor_degrades_to_click() {
        // A middle press that arrives before any cursor_moved has no
        // origin to latch against: pan is impossible, release degrades
        // to the click/paste path — the pre-R881 behaviour.
        let mut router = InputRouter::new();
        router.middle_down(PointerId::MOUSE);
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Click);
    }

    #[test]
    fn r881_middle_pan_targets_are_pinned_at_press() {
        // Gesture capture: the pan keeps driving the scrollable it
        // started on even when the cursor strays outside every scroll
        // viewport mid-drag (per-move re-resolution would drop or hop
        // the target — no native pan does that).
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        // March far outside the 200×200 paint root.
        assert!(router.cursor_moved(PointerId::MOUSE, 50.0, 400.0, &mut state_scene));
        // Content follows the cursor down: offset shrinks, clamped at 0
        // … so drag the other way to observe motion.
        assert!(router.cursor_moved(PointerId::MOUSE, 50.0, 350.0, &mut state_scene));
        assert_eq!(
            scroll.offset(),
            (0, 50),
            "pinned target pans outside its viewport"
        );
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Pan);
    }

    #[test]
    fn r881_middle_pan_offers_pinned_external_with_modifiers() {
        // Stage 1 of the pan dispatch is the SAME wheel-vocabulary
        // offer the wheel path makes (one dialect, two producers): a
        // consuming External receives the per-move delta + the held
        // modifiers (Ctrl+middle-drag = the canvas zoom chord), and
        // the scroll fallback must not also fire.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, calls) = WheelExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        // Over the tagged button → the hover tag is pinned at press.
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::empty()
        };
        assert!(router.cursor_moved_with_modifiers(
            PointerId::MOUSE,
            100.0,
            60.0,
            ctrl,
            &mut state_scene,
        ));
        let recorded = calls.lock().expect("mutex poisoned").clone();
        assert_eq!(recorded.len(), 1);
        let (_, _, dx, dy, mods) = recorded[0];
        assert!((dx - 0.0).abs() < f32::EPSILON);
        assert!(
            (dy - 30.0).abs() < f32::EPSILON,
            "delta = last - current, dy {dy}"
        );
        assert!(
            mods.control_key(),
            "held modifiers reach the External's wheel arm"
        );
        assert_eq!(
            scroll.offset(),
            (0, 0),
            "consumed offer skips the scroll fallback"
        );
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Pan);
    }

    #[test]
    fn r881_middle_pan_pins_hover_until_release() {
        // While a pan is live the pointer belongs to the gesture: the
        // free-mode hover walk is suppressed (no Enter/Leave churn as
        // content slides under the cursor), exactly like capture and
        // DnD. Hover resettles on the first move after release.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, _calls) = WheelExternal::new(false);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
        router.middle_down(PointerId::MOUSE);
        // Latch + stray far off the button: hover stays pinned.
        router.cursor_moved(PointerId::MOUSE, 30.0, 30.0, &mut state_scene);
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Pan);
        // First post-release move resettles hover normally.
        router.cursor_moved(PointerId::MOUSE, 30.0, 31.0, &mut state_scene);
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn r881_middle_pan_accumulates_sub_pixel_remainders() {
        // Integer scroll offsets round per move; the Qt-style
        // remainder carry keeps a slow high-DPI pan moving instead of
        // rounding every 0.4 px step to zero forever.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 100.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        // Latch with a clean 10 px pull.
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        assert_eq!(scroll.offset(), (0, 10));
        // Two 0.4 px creeps: the first rounds to 0 (remainder 0.4),
        // the second accumulates to 0.8 → rounds to 1 (remainder
        // -0.2). Motion is preserved across moves.
        router.cursor_moved(PointerId::MOUSE, 50.0, 89.6, &mut state_scene);
        assert_eq!(scroll.offset(), (0, 10), "first sub-pixel step carries");
        router.cursor_moved(PointerId::MOUSE, 50.0, 89.2, &mut state_scene);
        assert_eq!(scroll.offset(), (0, 11), "accumulated remainder lands");
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Pan);
    }

    // ─── R881.1 §5.35 adversarial-arm regressions (session audit) ──

    #[test]
    fn r881_1_repaint_hover_refresh_keeps_live_pan_pinned() {
        // The third hover-refresh producer: every repaint runs
        // `update_paint_scene` → `refresh_hover_for_all_active_pointers`,
        // and a live pan repaints every move. Pre-R881.1 that walk
        // skipped only capture, so panned content sliding under the
        // cursor churned Enter/Leave once per frame — the exact churn
        // the gesture suppression claims to prevent. One predicate
        // (`gesture_pins_hover`) now gates all three producers.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, _calls) = WheelExternal::new(false);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
        router.middle_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 30.0, 30.0, &mut state_scene);
        // Mid-pan repaint (the per-frame publish): hover must stay
        // pinned even though (30, 30) resolves to no tagged region.
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        assert_eq!(
            router.hover_target(PointerId::MOUSE),
            Some("main_btn"),
            "per-repaint hover refresh must not churn a live pan's pinned hover",
        );
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Pan);
        // The first post-release repaint resettles hover normally.
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn r881_1_left_press_and_release_swallowed_during_live_pan() {
        // Gesture exclusivity: while the pan owns the pointer, a left
        // press routes by a stale hover snapshot (content is moving),
        // so press AND release are swallowed — no `PointerDown`, and
        // no orphan `PointerUp` that an activation-edge decoder would
        // read as a click. After release, clicks work again.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, sends) = CaptureExternal::new();
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        // Latch while staying over the button rect (80..120).
        router.cursor_moved(PointerId::MOUSE, 110.0, 85.0, &mut state_scene);
        sends.lock().expect("mutex poisoned").clear();
        router.pointer_down(PointerId::MOUSE, &mut state_scene);
        router.pointer_up(PointerId::MOUSE, &mut state_scene);
        assert!(
            sends.lock().expect("mutex poisoned").is_empty(),
            "left press/release during a live pan dispatches nothing",
        );
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Pan);
        router.pointer_down(PointerId::MOUSE, &mut state_scene);
        let recorded = sends.lock().expect("mutex poisoned").clone();
        assert_eq!(
            recorded,
            vec!["PointerDown".to_owned()],
            "after the pan releases, the pointer routes normally again",
        );
    }

    #[test]
    fn r881_1_middle_during_left_gesture_never_pans_or_pastes() {
        // The other exclusivity direction: a pointer owned by a left
        // gesture (here: a tracked press) refuses the middle gesture —
        // its motion keeps feeding the left gesture only, and the
        // trailing middle release reports NoPress (no paste mid-drag).
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, _sends) = CaptureExternal::new();
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        router.pointer_down(PointerId::MOUSE, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 50.0, 30.0, &mut state_scene);
        assert_eq!(scroll.offset(), (0, 0), "no pan rides a left-owned pointer");
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::NoPress);
        router.pointer_up(PointerId::MOUSE, &mut state_scene);
    }

    #[test]
    fn r881_1_second_middle_press_cannot_reset_a_live_pan() {
        // First press wins: an RPC-injected middle press racing a
        // native hold must not replace the live gesture (a fresh
        // dead-zone latch would turn the user's pan into a paste at
        // the injected release).
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 50.0, 60.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        // R882.1 — the refused press is *counted*: its matching release
        // pairs with the refusal (`NoPress` — the injected pair is fully
        // inert, it can neither paste nor end the pan early) and only
        // the press that opened the gesture may close it.
        assert_eq!(
            router.middle_up(PointerId::MOUSE),
            PanRelease::NoPress,
            "the injected pair's release pairs with its refused press",
        );
        assert!(router.cursor_moved(PointerId::MOUSE, 50.0, 40.0, &mut state_scene));
        assert_eq!(scroll.offset(), (0, 50), "the native pan keeps panning");
        assert_eq!(
            router.middle_up(PointerId::MOUSE),
            PanRelease::Pan,
            "the owning press's release closes the gesture as a pan",
        );
    }

    #[test]
    fn r881_1_cursorless_press_lazy_seeds_on_first_move() {
        // A press before any cursor has no origin; the first move the
        // gesture learns about IS the origin — motion past it still
        // disambiguates pan from click (the degraded press is not
        // click-forever), and a dead-zone wobble still pastes.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.middle_down(PointerId::MOUSE);
        // First move seeds the origin (and pins targets there).
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 60.0, &mut state_scene);
        assert_eq!(
            scroll.offset(),
            (0, 30),
            "lazy-seeded pan pans from the seed origin"
        );
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Pan);
        // Dead-zone variant: seed then wobble → still a click.
        router.middle_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 50.0, 61.0, &mut state_scene);
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Click);
    }

    #[test]
    fn r881_1_shift_is_masked_from_the_pan_wheel_dialect() {
        // Shift+wheel is an axis REMAP for 1-D notch devices; a pan
        // delta is already 2-D, so the pan dispatch masks Shift
        // (Shift+middle-drag = plain pan, the Blender chord) while
        // zoom-class chords (Ctrl) pass through.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, calls) = WheelExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        let chord = Modifiers {
            shift: true,
            ctrl: true,
            ..Modifiers::empty()
        };
        assert!(router.cursor_moved_with_modifiers(
            PointerId::MOUSE,
            100.0,
            60.0,
            chord,
            &mut state_scene,
        ));
        let recorded = calls.lock().expect("mutex poisoned").clone();
        assert_eq!(recorded.len(), 1);
        assert!(
            !recorded[0].4.shift_key(),
            "Shift is masked out of the pan dispatch"
        );
        assert!(
            recorded[0].4.control_key(),
            "zoom-class chords pass through"
        );
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Pan);
    }

    #[test]
    fn r881_1_wheel_pixel_remainder_accumulates_and_resets_on_target_change() {
        // The R881.1 convergence: the wheel path shares the stage-2
        // remainder accumulator (pre-R881.1 a slow high-DPI PixelDelta
        // stream — 0.4 px/event — stalled forever on per-event
        // rounding), and the carry resets when the pointer's resolved
        // scroll target changes (a remainder never leaks containers).
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        let step = WheelDelta::Pixels { dx: 0.0, dy: 0.4 };
        assert!(
            !router.wheel(PointerId::MOUSE, step, &mut state_scene),
            "a sub-pixel step banks into the remainder (nothing visible moved)",
        );
        assert_eq!(scroll.offset(), (0, 0));
        assert!(router.wheel(PointerId::MOUSE, step, &mut state_scene));
        assert_eq!(scroll.offset(), (0, 1), "0.4 + 0.4 rounds to one pixel");
        // Wheeling with no scroll target under the cursor drops the
        // carry; returning restarts the accumulation from zero.
        router.cursor_moved(PointerId::MOUSE, 300.0, 300.0, &mut state_scene);
        assert!(!router.wheel(PointerId::MOUSE, step, &mut state_scene));
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        assert!(!router.wheel(PointerId::MOUSE, step, &mut state_scene));
        assert_eq!(
            scroll.offset(),
            (0, 1),
            "the carry reset on target change — 0.4 alone moves nothing",
        );
        assert!(router.wheel(PointerId::MOUSE, step, &mut state_scene));
        assert_eq!(scroll.offset(), (0, 2));
    }

    // ─── R882 §5.35 §5.39 left-button (Space-chord) pan tests ─────

    #[test]
    fn r882_left_pan_drag_pans_pinned_scroll() {
        // The R881 pan machinery driven from the left-drag channel:
        // identical dead zone, identical `last - current` grab
        // dispatch, identical pinning — one gesture engine, two
        // opening buttons.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.left_pan_down(PointerId::MOUSE);
        assert!(router.left_pan_in_flight(PointerId::MOUSE));
        // Dead-zone wobble: nothing pans yet.
        assert!(!router.cursor_moved(PointerId::MOUSE, 50.0, 88.0, &mut state_scene));
        assert_eq!(scroll.offset(), (0, 0), "dead-zone wobble must not pan");
        // Latched: full displacement from the grab origin dispatches.
        assert!(router.cursor_moved(PointerId::MOUSE, 50.0, 60.0, &mut state_scene));
        assert_eq!(scroll.offset(), (0, 30), "content follows the cursor");
        assert_eq!(router.left_pan_up(PointerId::MOUSE), PanRelease::Pan);
        assert!(!router.left_pan_in_flight(PointerId::MOUSE));
    }

    #[test]
    fn r882_left_pan_release_in_place_is_click_then_no_press() {
        // Release-in-place reports Click — which the shell treats as
        // inert for the left chord (Figma: Space+click does nothing).
        // The gesture is consumed: a second release is NoPress.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.left_pan_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 52.0, 91.0, &mut state_scene);
        assert_eq!(router.left_pan_up(PointerId::MOUSE), PanRelease::Click);
        assert_eq!(scroll.offset(), (0, 0));
        assert_eq!(router.left_pan_up(PointerId::MOUSE), PanRelease::NoPress);
    }

    #[test]
    fn r882_pan_release_requires_matching_button() {
        // Cross-button releases must not steal a live pan: a middle
        // release during a left-chord pan (or vice versa) reports
        // NoPress and leaves the gesture in flight — the release half
        // of the R881.1 exclusivity discipline.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.left_pan_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 50.0, 60.0, &mut state_scene);
        assert_eq!(
            router.middle_up(PointerId::MOUSE),
            PanRelease::NoPress,
            "a middle release must not close a left-opened pan",
        );
        assert!(
            router.left_pan_in_flight(PointerId::MOUSE),
            "the pan survives"
        );
        assert!(router.cursor_moved(PointerId::MOUSE, 50.0, 40.0, &mut state_scene));
        assert_eq!(
            scroll.offset(),
            (0, 50),
            "the pan keeps panning after the stray release"
        );
        assert_eq!(router.left_pan_up(PointerId::MOUSE), PanRelease::Pan);

        // The mirror direction: a left release cannot close a middle pan.
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 50.0, 60.0, &mut state_scene);
        assert_eq!(router.left_pan_up(PointerId::MOUSE), PanRelease::NoPress);
        assert!(
            !router.left_pan_in_flight(PointerId::MOUSE),
            "middle gesture ≠ left pan"
        );
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Pan);
    }

    #[test]
    fn r882_left_pan_swallows_routed_press_and_pins_hover() {
        // While the left-chord pan is live the pointer belongs to the
        // gesture: an injected routed press/release pair dispatches
        // nothing (same R881.1 exclusivity middle pans get), and the
        // hover stays pinned across moves and repaints.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, sends) = CaptureExternal::new();
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
        router.left_pan_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 30.0, 30.0, &mut state_scene);
        assert_eq!(
            router.hover_target(PointerId::MOUSE),
            Some("main_btn"),
            "a live left pan pins the hover exactly as a middle pan does",
        );
        sends.lock().expect("mutex poisoned").clear();
        router.pointer_down(PointerId::MOUSE, &mut state_scene);
        router.pointer_up(PointerId::MOUSE, &mut state_scene);
        assert!(
            sends.lock().expect("mutex poisoned").is_empty(),
            "routed press/release during a live left pan dispatches nothing",
        );
        assert_eq!(router.left_pan_up(PointerId::MOUSE), PanRelease::Pan);
    }

    #[test]
    fn r882_middle_press_rejected_while_left_pan_owns_the_pointer() {
        // One pan-class gesture per pointer, first press wins: a
        // middle press during a left-chord pan is refused outright, so
        // its trailing release can neither paste nor reset the latch.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.left_pan_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 50.0, 60.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::NoPress);
        assert_eq!(
            router.left_pan_up(PointerId::MOUSE),
            PanRelease::Pan,
            "the refused middle press left the live pan untouched",
        );
        // And the press-side mirror: a left pan cannot open while a
        // routed left gesture (tracked press) owns the pointer.
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        router.pointer_down(PointerId::MOUSE, &mut state_scene);
        router.left_pan_down(PointerId::MOUSE);
        assert!(
            !router.left_pan_in_flight(PointerId::MOUSE),
            "a tracked press refuses the pan channel",
        );
        router.pointer_up(PointerId::MOUSE, &mut state_scene);
    }

    #[test]
    fn r882_pointer_cancel_revokes_left_pan() {
        // The R880.1 mandatory-cancel-arm discipline applies to the
        // left chord too: a cancelled gesture is "never happened";
        // applied deltas stay (incremental scrolling).
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.left_pan_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 50.0, 60.0, &mut state_scene);
        assert_eq!(scroll.offset(), (0, 30));
        router.pointer_cancel(PointerId::MOUSE, &mut state_scene);
        assert!(!router.left_pan_in_flight(PointerId::MOUSE));
        assert_eq!(router.left_pan_up(PointerId::MOUSE), PanRelease::NoPress);
        assert_eq!(
            scroll.offset(),
            (0, 30),
            "applied pan deltas are not rolled back"
        );
    }

    #[test]
    fn r882_left_pan_offers_pinned_external_with_modifiers() {
        // The left chord rides the same two-stage wheel dispatch:
        // a consuming External receives the per-move delta plus the
        // held zoom-class chord (Ctrl+Space-drag = canvas zoom), and
        // the scroll fallback stays untouched.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, calls) = WheelExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        router.left_pan_down(PointerId::MOUSE);
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::empty()
        };
        assert!(router.cursor_moved_with_modifiers(
            PointerId::MOUSE,
            100.0,
            60.0,
            ctrl,
            &mut state_scene,
        ));
        let recorded = calls.lock().expect("mutex poisoned").clone();
        assert_eq!(recorded.len(), 1);
        let (_, _, dx, dy, mods) = recorded[0];
        assert!((dx - 0.0).abs() < f32::EPSILON);
        assert!(
            (dy - 30.0).abs() < f32::EPSILON,
            "delta = last - current, dy {dy}"
        );
        assert!(
            mods.control_key(),
            "held chords reach the External's wheel arm"
        );
        assert_eq!(
            scroll.offset(),
            (0, 0),
            "consumed offer skips the scroll fallback"
        );
        assert_eq!(router.left_pan_up(PointerId::MOUSE), PanRelease::Pan);
    }

    // ─── R882.1 §5.35 session-audit regressions ───────────────────

    #[test]
    fn r882_1_injected_left_pair_cannot_steal_a_live_left_pan() {
        // The same-button half of release exclusivity: an RPC click
        // (press + release on the shared pointer) racing a native
        // Space-chord hold must be fully inert — the press is refused
        // AND counted, so its release pairs with the refusal instead
        // of consuming the user's gesture; the user's real release
        // still resolves as the pan. Pre-R882.1 the injected release
        // ended the pan early and the user's physical release fell
        // through as an orphan free-mode `PointerUp` (phantom
        // activation).
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.left_pan_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 50.0, 60.0, &mut state_scene);
        // Injected pair, chord-held flavour (both halves enter the pan
        // channel): press refused + counted, release drains the count.
        router.left_pan_down(PointerId::MOUSE);
        assert_eq!(router.left_pan_up(PointerId::MOUSE), PanRelease::NoPress);
        assert!(
            router.left_pan_in_flight(PointerId::MOUSE),
            "the native pan survives"
        );
        // Injected pair, chordless flavour (the routed arc): press
        // swallowed + counted by `pointer_down`, release drained by
        // `pointer_up` — each release travels exactly one channel.
        router.pointer_down(PointerId::MOUSE, &mut state_scene);
        router.pointer_up(PointerId::MOUSE, &mut state_scene);
        assert!(
            router.left_pan_in_flight(PointerId::MOUSE),
            "still in flight"
        );
        // The pan still works and the OWNING release closes it.
        assert!(router.cursor_moved(PointerId::MOUSE, 50.0, 40.0, &mut state_scene));
        assert_eq!(scroll.offset(), (0, 50));
        assert_eq!(router.left_pan_up(PointerId::MOUSE), PanRelease::Pan);
        assert!(!router.left_pan_in_flight(PointerId::MOUSE));
    }

    #[test]
    fn r882_1_mixed_channel_pair_drains_one_count_per_release() {
        // A swallowed press counted via `pointer_down` may be released
        // via `left_pan_up` (the shell front door) and vice versa —
        // the counter is per-press, not per-channel, so a mixed pair
        // still balances and the owning release still closes.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let mut state_scene = Scene::Container(ContainerNode::new(Vec::new()));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 50.0, 90.0, &mut state_scene);
        router.left_pan_down(PointerId::MOUSE);
        router.cursor_moved(PointerId::MOUSE, 50.0, 60.0, &mut state_scene);
        router.pointer_down(PointerId::MOUSE, &mut state_scene);
        assert_eq!(
            router.left_pan_up(PointerId::MOUSE),
            PanRelease::NoPress,
            "the front-door release drains the routed-arc count",
        );
        assert_eq!(router.left_pan_up(PointerId::MOUSE), PanRelease::Pan);
    }

    #[test]
    fn r882_1_dead_zone_pan_press_refuses_routed_press_symmetrically() {
        // The press-side guards must be symmetric: `pan_down` refuses
        // while a press tracker exists, so `pointer_down` must refuse
        // while a pan gesture exists — even in its dead zone. Letting
        // a routed press open a capture / press tracker beside a
        // dead-zone pan would feed both gestures the same motion the
        // moment the pan latches.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, sends) = CaptureExternal::new();
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        // Over the button so a routed press WOULD capture it.
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        router.middle_down(PointerId::MOUSE);
        // Still inside the dead zone — no pan yet, but the gesture
        // candidate owns the pointer.
        sends.lock().expect("mutex poisoned").clear();
        router.pointer_down(PointerId::MOUSE, &mut state_scene);
        router.pointer_up(PointerId::MOUSE, &mut state_scene);
        assert!(
            sends.lock().expect("mutex poisoned").is_empty(),
            "a routed press/release is refused even during the dead zone",
        );
        // The middle gesture is unaffected: stray past the dead zone
        // and it pans; release resolves Pan.
        assert!(router.cursor_moved(PointerId::MOUSE, 100.0, 60.0, &mut state_scene));
        assert_eq!(router.middle_up(PointerId::MOUSE), PanRelease::Pan);
    }

    // ─── R51.187 §5.45 R55.C.3 keyboard scroll dispatch tests ─────

    #[test]
    fn r55_c3_arrow_keys_step_one_line_each_axis() {
        // R55.C.3 — arrow keys translate to ±LINE_HEIGHT_PX (16)
        // on the matching axis. Mirrors the W3C `WheelEvent`
        // sign convention positive `dy` = scroll downward.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowDown"));
        assert_eq!(state.offset(), (0, 16));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowDown"));
        assert_eq!(state.offset(), (0, 32));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowUp"));
        assert_eq!(state.offset(), (0, 16));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowRight"));
        assert_eq!(state.offset(), (16, 16));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowLeft"));
        assert_eq!(state.offset(), (0, 16));
    }

    #[test]
    fn r55_c3_page_keys_step_one_viewport() {
        // R55.C.3 — PageDown / PageUp step by the scroll
        // container's viewport extent on the matching axis.
        // Viewport height = 100 px → PageDown adds 100, PageUp
        // subtracts 100.
        let state = Rc::new(ScrollState::new());
        state.set_max(1000, 1000);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            2000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        assert!(router.scroll_key(PointerId::MOUSE, "PageDown"));
        assert_eq!(state.offset(), (0, 100));
        assert!(router.scroll_key(PointerId::MOUSE, "PageDown"));
        assert_eq!(state.offset(), (0, 200));
        assert!(router.scroll_key(PointerId::MOUSE, "PageUp"));
        assert_eq!(state.offset(), (0, 100));
    }

    #[test]
    fn r55_c3_home_end_jump_to_y_extremes() {
        // R55.C.3 — Home resets y to 0; End jumps y to max_y.
        // The horizontal offset is preserved (W3C "vertical
        // extreme" semantics — Ctrl-Home / Ctrl-End for corner
        // jumps is R55.C.4 carry).
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 800);
        state.scroll_to(50, 400);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            1000,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        assert!(router.scroll_key(PointerId::MOUSE, "Home"));
        assert_eq!(state.offset(), (50, 0), "Home preserves x, resets y");
        assert!(router.scroll_key(PointerId::MOUSE, "End"));
        assert_eq!(state.offset(), (50, 800), "End preserves x, jumps to max_y");
    }

    #[test]
    fn r55_c3_unknown_key_returns_false() {
        // R55.C.3 — keys not in the recognised set (Tab, Enter,
        // Escape, Space, character keys) return false so the
        // caller's regular `apply_key` arm stays the primary
        // path for widget-bound shortcuts.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            1000,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        for key in ["Space", "Enter", "Tab", "Escape", "a", "F1"] {
            assert!(
                !router.scroll_key(PointerId::MOUSE, key),
                "unrecognised key {key} must return false",
            );
        }
        assert_eq!(state.offset(), (0, 0), "no key advanced the offset");
    }

    #[test]
    fn r55_c3_no_op_when_cursor_off_scroll() {
        // R55.C.3 — same router-state guard as `wheel`: cursor
        // outside any scroll container is a silent drop. The
        // application's regular `apply_key` arm still runs (the
        // backend's `handle_named_key` fallback fires only after
        // `apply_key` returns unhandled, and `scroll_key`'s false
        // simply lets the key dispatch sequence end without an
        // unintended scroll).
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state_scene);
        // No scroll node under cursor → silent drop on every key.
        for key in ["ArrowDown", "ArrowUp", "PageDown", "PageUp", "Home", "End"] {
            assert!(!router.scroll_key(PointerId::MOUSE, key));
        }
        let _ = state; // unused but kept symmetric with other tests
    }

    #[test]
    fn r55_c3_arrow_clamps_against_bounds() {
        // R55.C.3 — ArrowUp from offset 0 clamps at 0 (lower
        // bound); ArrowDown past max_y clamps at max_y. The
        // ScrollState clamp logic carries the policy.
        let state = Rc::new(ScrollState::new());
        state.set_max(0, 40);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        let paint = paint_with_scroll(
            200,
            200,
            Rect::new(0, 0, 100, 100),
            200,
            140,
            Some(Rc::clone(&state)),
        );
        router.update_paint_scene(paint, &mut state_scene);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state_scene);
        // ArrowUp at zero — clamp lower.
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowUp"));
        assert_eq!(state.offset(), (0, 0));
        // Three ArrowDowns at +16 each = 48; clamp at max_y = 40.
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowDown"));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowDown"));
        assert!(router.scroll_key(PointerId::MOUSE, "ArrowDown"));
        assert_eq!(state.offset(), (0, 40));
    }

    #[test]
    fn r55_c2_line_height_px_constant_is_w3c_default() {
        // R55.C.2 — pin the constant value so a future override
        // (R55.C.4 per-widget line-height) shows up as an
        // explicit test edit, not a silent regression.
        assert!((LINE_HEIGHT_PX - 16.0).abs() < f32::EPSILON);
    }

    #[test]
    fn update_paint_scene_refreshes_every_active_pointer() {
        // After a layout change, every active pointer's hover_target
        // must re-resolve. With two pointers active (mouse + touch),
        // both should observe the layout shift independently.
        let mut router = InputRouter::new();
        let (mut state, (ea, _ma), (eb, _mb)) = state_with_two_sliders();
        router.update_paint_scene(paint_with_two_sliders(200, 200), &mut state);
        let touch = PointerId::touch(0);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state);
        router.cursor_moved(touch, 150.0, 50.0, &mut state);
        // Now repaint with both sliders shifted out from under both
        // cursors. paint_with_two_sliders uses fixed rects; build a
        // bare root with no children to simulate "both widgets
        // moved away".
        let bare_root = Scene::Container(
            ContainerNode::new(vec![]).with_style(BoxStyle::filled(Color::default())),
        );
        router.update_paint_scene(bare_root, &mut state);
        // Both pointers lost their hover — each sees PointerLeave.
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
        assert_eq!(router.hover_target(touch), None);
        assert_eq!(
            read(&ea),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(
            read(&eb),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
    }

    // ─── R664 §5.49 W3C double-click detection ─────────────────────

    /// Two consecutive presses with no cursor move in between (the
    /// `DeferredInput::DoubleClick` drain shape) fire one synthetic
    /// `DoubleClick` named event on top of the second `PointerDown`.
    /// The first press only emits `PointerDown` — `DoubleClick`
    /// requires the prior `last_press` snapshot.
    #[test]
    fn r664_back_to_back_presses_emit_double_click_event() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        // First click cycle: PointerEnter + PointerDown + PointerUp.
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        // Second press at the same coord — well inside W3C 300ms / 5px.
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&captures),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
                "PointerDown".into(),
                "DoubleClick".into(),
                "PointerUp".into(),
            ],
        );
    }

    /// Position delta exceeding [`DOUBLE_CLICK_DIST_PX`] between the
    /// two presses disqualifies the W3C `dblclick` heuristic — the
    /// second press is a normal `PointerDown` with no synthetic
    /// `DoubleClick`. 10 px on the same widget is enough; the Material
    /// 3 / Cocoa convention rejects 6 px+.
    #[test]
    fn r664_spatially_separated_presses_do_not_double_click() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        // Cursor moves 10 px before second press — over the 5 px window.
        router.cursor_moved(PointerId::MOUSE, 110.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(
            !read(&captures).iter().any(|s| s == "DoubleClick"),
            "second press 10 px away must not double-click",
        );
    }

    /// A press on widget A then a press on widget B does not produce
    /// `DoubleClick` even if temporally adjacent — the `last_press`
    /// guard tracks the target tag, not just the timestamp. Mirror of
    /// the W3C `target` equality requirement in the `dblclick`
    /// dispatch contract.
    #[test]
    fn r664_cross_target_presses_do_not_double_click() {
        let mut router = InputRouter::new();
        let (mut state, (ea, _ma), (eb, _mb)) = state_with_two_sliders();
        router.update_paint_scene(paint_with_two_sliders(200, 200), &mut state);
        let touch = PointerId::touch(0);
        // Single pointer crosses widgets — same PointerId.
        router.cursor_moved(touch, 50.0, 50.0, &mut state); // on slider A
        router.pointer_down(touch, &mut state);
        router.pointer_up(touch, &mut state);
        router.cursor_moved(touch, 150.0, 50.0, &mut state); // on slider B
        router.pointer_down(touch, &mut state);
        router.pointer_up(touch, &mut state);
        assert!(
            !read(&ea).iter().any(|s| s == "DoubleClick"),
            "slider A press → release must not see DoubleClick after target switch",
        );
        assert!(
            !read(&eb).iter().any(|s| s == "DoubleClick"),
            "slider B first press must not see stale DoubleClick from slider A",
        );
    }

    /// R875 §5.49 — a capture drag is a *drag*, not a click. A press that
    /// strays beyond [`DOUBLE_CLICK_DIST_PX`] while the capture lock is
    /// held must not seed a `DoubleClick` for the next same-spot press.
    /// Two numeric-scrub drags back and forth over one property row land
    /// two presses at the *same* coordinate within the 300 ms window;
    /// without invalidating the strayed `last_press` they would read as a
    /// double-click and spuriously open the inline editor (the R875 regress
    /// where a later commit-on-blur then reverts the scrubbed value). The
    /// native "drag cancels the double-click cycle" rule, enforced at the
    /// framework tier so no scrub binding re-derives it.
    #[test]
    fn r875_capture_drag_does_not_seed_double_click() {
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // First gesture: press, drag 18 px (a scrub), release — a drag.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 118.0, 100.0, &mut state); // strays > 5 px
        router.pointer_up(PointerId::MOUSE, &mut state);
        // Second press back at the original spot, well inside 300 ms.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(
            !read(&events).iter().any(|s| s == "DoubleClick"),
            "a capture drag must not seed a DoubleClick for the next press",
        );
    }

    /// R875 §5.49 — the companion guarantee: a capture press that stays
    /// *within* [`DOUBLE_CLICK_DIST_PX`] (a click-in-place, the cursor
    /// only jitters) still double-clicks. The drag-invalidation must not
    /// over-fire on the sub-threshold motion a real click carries, so a
    /// genuine double-click on a capture widget (e.g. double-click a
    /// numeric row to open its editor) keeps working.
    #[test]
    fn r875_capture_click_in_place_still_double_clicks() {
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 102.0, 101.0, &mut state); // 2 px jitter < 5
        router.pointer_up(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(
            read(&events).iter().any(|s| s == "DoubleClick"),
            "a click-in-place on a capture widget must still double-click",
        );
    }

    /// Triple-click resets after detail=2 — the third press starts a
    /// fresh single/double cycle. The 4th press completes another
    /// double-click. Confirms the substrate pins binary single/double
    /// per `[[abstraction-needs-second-consumer]]` until a triple
    /// consumer surfaces.
    #[test]
    fn r664_triple_press_resets_after_double() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        // 1st press — single. 2nd press — double. 3rd press — single
        // again (no triple). 4th press — double again.
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&captures);
        let double_count = log.iter().filter(|s| s.as_str() == "DoubleClick").count();
        assert_eq!(
            double_count, 2,
            "exactly two DoubleClick fires across 4 presses"
        );
    }

    // ----- R1080 §5.51 drop-target resolution -----

    /// R1080 §5.51 — `resolve_drop_target_tag` climbs to the nearest
    /// `LayoutStyle::drop_target` ancestor, so a drag coordinator gets the
    /// semantic drop region (a dock panel) rather than the deeper tagged
    /// content leaf the cursor is literally over. `resolve_hover_tag` stays
    /// on the deepest tag (hover unchanged), and `resolve_drop_point`
    /// normalises over the PANEL rect — what the dock zone classifier needs.
    #[test]
    fn r1080_drop_target_climbs_over_deeper_content_tag() {
        use pinion_core::scene::{BoxNode, ContainerNode};
        use pinion_core::style::{Color, LayoutStyle};

        // A panel (drop target, tag "panel", 0..400) whose content is a
        // deeper tagged child ("panel#content", 50..350) — the dock shape.
        let content = Scene::Box(
            BoxNode::filled(Rect::new(50, 50, 300, 300), Color::default())
                .with_tag("panel#content"),
        );
        let mut panel = ContainerNode::new(vec![content])
            .with_tag("panel")
            .with_layout(LayoutStyle::new().with_drop_target(true));
        panel.rect = Rect::new(0, 0, 400, 400);
        let scene = Scene::Container(panel);

        // Cursor at (200, 200): inside both the content and the panel.
        assert_eq!(
            resolve_drop_target_tag(&scene, 200.0, 200.0).as_deref(),
            Some("panel"),
            "drop resolution climbs to the drop-target panel",
        );
        assert_eq!(
            resolve_hover_tag(&scene, 200.0, 200.0).as_deref(),
            Some("panel#content"),
            "hover stays on the deepest tag (unchanged)",
        );

        // The DropPoint names the panel, normalised over the PANEL rect
        // (400 wide): (200 - 0) / 400 = 0.5 — the panel centre, what the
        // dock zone classifier reads, not a content-relative coordinate.
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        router.update_paint_scene(scene, &mut state_scene);
        let dp = router
            .resolve_drop_point(200.0, 200.0)
            .expect("over a drop target");
        assert_eq!(dp.tag, "panel");
        assert!(
            (dp.x_rel - 0.5).abs() < 1e-6 && (dp.y_rel - 0.5).abs() < 1e-6,
            "normalised over the panel rect, not the content",
        );
    }

    /// R1080 §5.51 — with no drop-target ancestor the drop resolution is
    /// bit-identical to pre-R1080: `resolve_drop_target_tag` is `None` and
    /// `resolve_drop_point` falls back to the deepest tagged hit.
    #[test]
    fn r1080_drop_resolution_falls_back_to_deepest_tag_without_drop_target() {
        use pinion_core::scene::{BoxNode, ContainerNode};
        use pinion_core::style::Color;

        let content = Scene::Box(
            BoxNode::filled(Rect::new(50, 50, 300, 300), Color::default())
                .with_tag("panel#content"),
        );
        let mut panel = ContainerNode::new(vec![content]).with_tag("panel");
        panel.rect = Rect::new(0, 0, 400, 400);
        let scene = Scene::Container(panel);

        assert_eq!(resolve_drop_target_tag(&scene, 200.0, 200.0), None);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        router.update_paint_scene(scene, &mut state_scene);
        let dp = router
            .resolve_drop_point(200.0, 200.0)
            .expect("over a tagged region");
        // Deepest tag = the content; normalised over the CONTENT rect
        // (300 wide at offset 50): (200 - 50) / 300 = 0.5.
        assert_eq!(dp.tag, "panel#content");
        assert!((dp.x_rel - 0.5).abs() < 1e-6);
    }

    /// R1152 §5.51 — a cursor OUTSIDE the window (negative window-local) has no
    /// own-window drop target. Pre-R1152 `floor_clamp_u32` clamped the negative to
    /// `(0,0)` and resolved a SPURIOUS hit on the top-left panel — which made a
    /// FLOATER's cross-window drop free-move instead of redock (its pointer grab
    /// delivers negative floater-local cursors while over the dock host, and the
    /// spurious own hit masked the resolved cross-window redock). Non-tautological:
    /// the negative case returns `Some("panel")` under the old clamp, `None` now.
    #[test]
    fn r1152_resolve_drop_point_rejects_out_of_window_negative_cursor() {
        use pinion_core::scene::{BoxNode, ContainerNode};
        use pinion_core::style::{Color, LayoutStyle};

        let content = Scene::Box(
            BoxNode::filled(Rect::new(50, 50, 300, 300), Color::default())
                .with_tag("panel#content"),
        );
        let mut panel = ContainerNode::new(vec![content])
            .with_tag("panel")
            .with_layout(LayoutStyle::new().with_drop_target(true));
        panel.rect = Rect::new(0, 0, 400, 400);
        let scene = Scene::Container(panel);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        router.update_paint_scene(scene, &mut state_scene);
        // In-bounds over the panel resolves it.
        assert!(router.resolve_drop_point(200.0, 200.0).is_some());
        // A negative (out-of-window) cursor resolves NOTHING.
        assert!(
            router.resolve_drop_point(-50.0, 200.0).is_none(),
            "a cursor left of the window has no own-window drop target",
        );
        assert!(
            router.resolve_drop_point(200.0, -50.0).is_none(),
            "a cursor above the window has no own-window drop target",
        );
    }

    /// R1167 §5.51 — the same-window OUTER dock override (debt B): a DOCK-PANEL
    /// drag whose cursor is in this window's outer band resolves to the full-span
    /// OUTER sentinel (so `resolve_drop` → `OuterDock` → `dock_panel_outer`),
    /// reaching the window-edge full-span dock WITHOUT leaving the window — the
    /// asymmetry that made same-window outer docking unreachable before. The
    /// interior keeps the inner panel hit-test; a non-dock kind (the outliner tree
    /// reparent) keeps the plain hit-test near the edge too (the dock-kind gate);
    /// a cursor OUTSIDE the window is an escape (→ float), not an outer dock.
    /// Non-tautological: the SAME edge cursor returns the inner panel under a
    /// non-dock kind but the OUTER sentinel under the dock kind.
    #[test]
    fn r1167_same_window_outer_dock_override_for_dock_panel_drag() {
        use pinion_core::scene::{BoxNode, ContainerNode};
        use pinion_core::style::{Color, LayoutStyle};
        use std::borrow::Cow;

        let content = Scene::Box(
            BoxNode::filled(Rect::new(0, 0, 400, 400), Color::default()).with_tag("panel#content"),
        );
        let mut panel = ContainerNode::new(vec![content])
            .with_tag("panel")
            .with_layout(LayoutStyle::new().with_drop_target(true));
        panel.rect = Rect::new(0, 0, 400, 400);
        let scene = Scene::Container(panel);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        router.update_paint_scene(scene, &mut state_scene);

        let dock = DragPayload {
            kind: Cow::Borrowed(DOCK_PANEL_DRAG_KIND),
            value: IntrospectValue::Text("panel".into()),
        };
        let tree = DragPayload {
            kind: Cow::Borrowed("tree-node"),
            value: IntrospectValue::Text("n".into()),
        };

        // A cursor within OUTER_DOCK_MARGIN of the LEFT edge → the full-span OUTER
        // sentinel (normalised over the WHOLE window: 10/400, 200/400).
        let outer = router
            .resolve_own_outer_dock(10.0, 200.0)
            .expect("left band is outer");
        assert_eq!(outer.tag, OUTER_DOCK_ZONE_TAG);
        assert!((outer.x_rel - 0.025).abs() < 1e-4 && (outer.y_rel - 0.5).abs() < 1e-4);

        // The override fires for a dock-panel drag near the edge...
        assert_eq!(
            router
                .resolve_drag_own_over(&dock, 10.0, 200.0)
                .expect("dock outer")
                .tag,
            OUTER_DOCK_ZONE_TAG,
            "a dock-panel drag near the edge gets the full-span outer sentinel",
        );
        // ...but NOT for a non-dock drag (the gate): the inner hit-test wins.
        assert_eq!(
            router
                .resolve_drag_own_over(&tree, 10.0, 200.0)
                .expect("tree inner")
                .tag,
            "panel",
            "a non-dock drag near the edge keeps the inner hit-test (no dock sentinel)",
        );

        // The window INTERIOR is not outer (the inner panel hit-test applies).
        assert!(
            router.resolve_own_outer_dock(200.0, 200.0).is_none(),
            "the centre is interior, not an outer dock",
        );
        assert_eq!(
            router
                .resolve_drag_own_over(&dock, 200.0, 200.0)
                .expect("inner")
                .tag,
            "panel",
        );

        // A cursor OUTSIDE the window is an ESCAPE (→ float via the empty own-over),
        // not an outer dock — the inside-only band preserves drag-out-to-float.
        assert!(
            router.resolve_own_outer_dock(-10.0, 200.0).is_none(),
            "left of the window is an escape, not an outer dock",
        );
        assert!(
            router.resolve_own_outer_dock(410.0, 200.0).is_none(),
            "right of the window is an escape, not an outer dock",
        );
    }

    #[test]
    fn r1203_outer_dock_margin_is_proportional_capped() {
        // A large / normal window keeps the ~32px cap; a small window shrinks
        // proportionally (a fixed 32px was an oversized fraction of it).
        assert!((super::outer_dock_margin(Rect::new(0, 0, 1200, 800)) - 32.0).abs() < 1e-9);
        assert!((super::outer_dock_margin(Rect::new(0, 0, 400, 400)) - 32.0).abs() < 1e-9);
        // 200px window: 10% of the smaller dim = 20 < the 32 cap.
        assert!((super::outer_dock_margin(Rect::new(0, 0, 200, 200)) - 20.0).abs() < 1e-9);
        // Sized by the SMALLER dimension (a wide-but-short window).
        assert!((super::outer_dock_margin(Rect::new(0, 0, 1600, 100)) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn r1203_dock_area_rect_insets_the_top() {
        let root = Rect::new(0, 0, 400, 600);
        assert_eq!(
            super::dock_area_rect(root, 0),
            root,
            "no chrome → unchanged"
        );
        assert_eq!(super::dock_area_rect(root, 32), Rect::new(0, 32, 400, 568));
        // A chrome taller than the window clamps to an in-bounds empty rect.
        assert_eq!(super::dock_area_rect(root, 700), Rect::new(0, 600, 400, 0));
    }

    #[test]
    fn r1203_resolve_own_outer_dock_measures_the_dock_area_below_chrome() {
        use pinion_core::scene::{BoxNode, ContainerNode};
        use pinion_core::style::{Color, LayoutStyle};
        let content = Scene::Box(
            BoxNode::filled(Rect::new(0, 0, 400, 600), Color::default()).with_tag("panel#content"),
        );
        let mut panel = ContainerNode::new(vec![content])
            .with_tag("panel")
            .with_layout(LayoutStyle::new().with_drop_target(true));
        panel.rect = Rect::new(0, 0, 400, 600);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        router.update_paint_scene(Scene::Container(panel), &mut state_scene);
        // A 32px client-side chrome strip: the dock area is y ∈ [32, 600].
        router.set_dock_area_top_inset(32);
        // A cursor IN the chrome strip (y=10 < 32) has left the dock area upward —
        // an escape, NOT the top outer band (it would land on the min/max/close
        // controls, not the dock).
        assert!(
            router.resolve_own_outer_dock(200.0, 10.0).is_none(),
            "the chrome strip is above the dock area, not the top outer band",
        );
        // The dock's TOP edge (y=40, 8px below the chrome) IS the top outer band,
        // normalised over the DOCK area (so `outer_zone_for` derives Top).
        let top = router
            .resolve_own_outer_dock(200.0, 40.0)
            .expect("the dock top edge is the outer band");
        assert_eq!(top.tag, OUTER_DOCK_ZONE_TAG);
        assert!(
            top.y_rel < 0.1,
            "normalised over the dock area → near its top (y_rel={})",
            top.y_rel,
        );
        // Non-tautological: WITHOUT the inset the SAME chrome-strip cursor is the
        // window's top band — the inset is exactly what moves the band off the
        // controls (the R1202 preview and this band now agree on the dock area).
        router.set_dock_area_top_inset(0);
        assert!(
            router.resolve_own_outer_dock(200.0, 10.0).is_some(),
            "no chrome inset → the window's top band includes y=10",
        );
    }

    // ----- R742 §5.51 drag-and-drop session -----

    /// A composite reorder-list-shaped drag source: a single
    /// `ExternalNode` tagged `dnd` with two paint sub-regions
    /// `dnd#0` / `dnd#1`. `send "{i}:PointerDown"` records the pressed
    /// row so [`begin_drag`] can arm; `drag_to` / `drag_release` append a
    /// readable trace so the router-side wiring can be asserted.
    struct DragExternal {
        pressed: std::cell::Cell<Option<usize>>,
        log: Arc<Mutex<Vec<String>>>,
        /// When true, also opts into `wants_pointer_capture` — the
        /// (currently hypothetical) widget that is *both* a capture widget
        /// and a drag source, used to prove a drag release clears any
        /// vestigial capture lock.
        capture: bool,
        /// (R1113) When `Some`, [`begin_drag`] emits a TEXT payload carrying
        /// this label (the dock-panel shape) instead of the default `Int` row
        /// index — so `active_drag_label` (the drag-image projection) has a
        /// label to surface. `None` keeps the legacy `Int` payload.
        label: Option<&'static str>,
    }

    impl DragExternal {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            Self::with_capture(false)
        }

        fn with_capture(capture: bool) -> (Self, Arc<Mutex<Vec<String>>>) {
            let log = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    pressed: std::cell::Cell::new(None),
                    log: Arc::clone(&log),
                    capture,
                    label: None,
                },
                log,
            )
        }

        /// (R1113) A text-payload drag source (the dock-panel shape): its
        /// [`begin_drag`] payload value is `Text(label)`, so `active_drag_label`
        /// surfaces it as the drag-image follower's label.
        fn with_label(label: &'static str) -> (Self, Arc<Mutex<Vec<String>>>) {
            let (mut s, log) = Self::with_capture(false);
            s.label = Some(label);
            (s, log)
        }
    }

    impl std::fmt::Debug for DragExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("DragExternal").finish()
        }
    }

    impl pinion_core::external::External for DragExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
            Some(self)
        }
        fn wants_pointer_capture(&self) -> bool {
            self.capture
        }
        fn begin_drag(&self) -> Option<DragPayload> {
            self.pressed.get().map(|i| {
                self.log
                    .lock()
                    .expect("poisoned")
                    .push(format!("begin:{i}"));
                DragPayload {
                    kind: std::borrow::Cow::Borrowed("dnd-row"),
                    // R1113 — a labelled source emits a Text payload (dock-panel
                    // shape); the default reorder source keeps its Int row index.
                    value: match self.label {
                        Some(l) => IntrospectValue::Text(l.to_string()),
                        None => IntrospectValue::Int(i64::try_from(i).unwrap_or(0)),
                    },
                }
            })
        }
        fn drag_to(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
            let dst = over.map_or_else(|| "none".to_string(), |p| p.tag);
            self.log
                .lock()
                .expect("poisoned")
                .push(format!("to:{}:{dst}", payload.value.as_i64().unwrap_or(-1)));
        }
        fn drag_release(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
            let dst = over.map_or_else(|| "none".to_string(), |p| p.tag);
            self.log.lock().expect("poisoned").push(format!(
                "drop:{}:{dst}",
                payload.value.as_i64().unwrap_or(-1)
            ));
        }
        // R1093 — record the absolute cursor the router now forwards, then
        // delegate to the cursor-less hooks so the pre-R1093 `to:`/`drop:`
        // log assertions still hold (this stub deliberately exercises BOTH).
        // R1101 — the cursor / over / window now ride one [`DragUpdate`].
        fn drag_to_at(&mut self, payload: &DragPayload, update: &DragUpdate) {
            // Format the f64 directly (whole values print without a decimal);
            // no truncating `as i64` cast, so this stays clippy-pedantic clean.
            // R1102 — append the cross-window `over_window` id when Some (empty
            // when None, so the pre-R1102 `at:x:y` assertions still hold).
            let win = update
                .over_window
                .map_or(String::new(), |w| format!(":{w}"));
            self.log
                .lock()
                .expect("poisoned")
                .push(format!("at:{}:{}{win}", update.cursor.0, update.cursor.1));
            self.drag_to(payload, update.over.clone());
        }
        fn drag_release_at(&mut self, payload: &DragPayload, update: &DragUpdate) {
            let win = update
                .over_window
                .map_or(String::new(), |w| format!(":{w}"));
            self.log.lock().expect("poisoned").push(format!(
                "drop_at:{}:{}{win}",
                update.cursor.0, update.cursor.1
            ));
            self.drag_release(payload, update.over.clone());
        }
        fn drag_cancel(&mut self, payload: &DragPayload) {
            self.log
                .lock()
                .expect("poisoned")
                .push(format!("cancel:{}", payload.value.as_i64().unwrap_or(-1)));
        }
    }

    impl ExternalIntrospect for DragExternal {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(&[])
        }
        fn query(&self, _path: &str) -> Option<IntrospectValue> {
            None
        }
        fn intervene(
            &mut self,
            _path: &str,
            _value: IntrospectValue,
        ) -> Result<(), InterveneError> {
            Err(InterveneError::UnknownPath)
        }
        fn invoke(
            &mut self,
            method: &str,
            args: IntrospectValue,
        ) -> Result<IntrospectValue, InvokeError> {
            if method == "send" {
                if let IntrospectValue::Text(name) = args {
                    // Composite "{idx}:{Event}" wire form (R51.42).
                    if let Some((idx, event)) = name.split_once(':') {
                        if event == "PointerDown" {
                            if let Ok(i) = idx.parse::<usize>() {
                                self.pressed.set(Some(i));
                            }
                        }
                        self.log.lock().expect("poisoned").push(name.clone());
                    }
                }
            }
            Ok(IntrospectValue::Null)
        }
    }

    /// Paint scene: two stacked rows `dnd#0` (y 0..40) and `dnd#1`
    /// (y 40..80) inside a 200x200 root — the reorder-list shape.
    fn paint_with_two_rows() -> Scene {
        let mut row0 = Scene::Container(ContainerNode::new(vec![]).with_tag("dnd#0"));
        if let Scene::Container(c) = &mut row0 {
            c.rect = Rect::new(0, 0, 200, 40);
        }
        let mut row1 = Scene::Container(ContainerNode::new(vec![]).with_tag("dnd#1"));
        if let Scene::Container(c) = &mut row1 {
            c.rect = Rect::new(0, 40, 200, 40);
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![row0, row1]).with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 200, 200);
        }
        root
    }

    fn state_with_dnd() -> (Scene, Arc<Mutex<Vec<String>>>) {
        let (drag, log) = DragExternal::new();
        let scene = Scene::External(ExternalNode::new(Box::new(drag)).with_tag("dnd"));
        (scene, log)
    }

    #[test]
    fn r742_drag_row0_onto_row1_forwards_resolved_drop_to_source() {
        let mut router = InputRouter::new();
        let (mut state, log) = state_with_dnd();
        router.update_paint_scene(paint_with_two_rows(), &mut state);
        // Press inside row 0, drag down into row 1, release there.
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(
            router.captured_target(PointerId::MOUSE),
            None,
            "a reorder source does not opt into pointer capture"
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 60.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        // PointerDown reached row 0, the drag armed, drag_to saw the
        // absolute cursor resolve to row 1, the drop committed on row 1,
        // and the trailing PointerUp still reached the statechart.
        assert!(log.contains(&"0:PointerDown".to_string()), "{log:?}");
        assert!(log.contains(&"begin:0".to_string()), "{log:?}");
        assert!(
            log.iter().any(|s| s == "to:0:dnd#1"),
            "drag_to over row 1: {log:?}"
        );
        assert!(
            log.iter().any(|s| s == "drop:0:dnd#1"),
            "drop on row 1: {log:?}"
        );
        // R794 — a real (moved) drag is NOT also a click: the drop committed
        // via drag_release, so the router does not synthesize the trailing
        // PointerUp (Qt startDragDistance / DOM no-click-after-drag). This is
        // what lets a file move / tab reorder not also activate the source.
        assert!(
            !log.contains(&"0:PointerUp".to_string()),
            "a moved drag must not synthesize a click: {log:?}"
        );
        // Hover was pinned *during* the drag — no `PointerLeave` reaches
        // the source between arming and the drop commit (the capture-
        // equivalent guarantee). A leave *after* the drop is correct: the
        // post-gesture `refresh_hover` resettles hover onto row 1, where
        // the cursor genuinely ended (mirrors capture's `pointer_up`).
        let drop_at = log
            .iter()
            .position(|s| s == "drop:0:dnd#1")
            .expect("drop logged");
        assert!(
            !log[..drop_at].iter().any(|s| s.contains("PointerLeave")),
            "no stray leave mid-drag: {log:?}"
        );
    }

    #[test]
    fn r1093_router_forwards_absolute_cursor_to_drag_source() {
        // R1093 §5.15 — the router must hand the drag source the ABSOLUTE
        // window-logical cursor on every move + the release, via
        // `drag_to_at`/`drag_release_at`. The DropPoint is rect-relative and
        // goes `None` once the cursor escapes every tag, so the absolute
        // cursor is the only live pointer signal a follow-the-cursor
        // coordinator can read.
        let mut router = InputRouter::new();
        let (mut state, log) = state_with_dnd();
        router.update_paint_scene(paint_with_two_rows(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        // Move into row 1 (still over a tag) then OFF every tag (escape):
        // the cursor must be forwarded in BOTH cases, even when `over` is None.
        router.cursor_moved(PointerId::MOUSE, 100.0, 60.0, &mut state);
        router.cursor_moved(PointerId::MOUSE, 400.0, 400.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        // The in-tag move forwarded the cursor at (100, 60)…
        assert!(
            log.iter().any(|s| s == "at:100:60"),
            "cursor at in-tag move: {log:?}"
        );
        // …and the escape move forwarded the cursor at (400, 400) even though
        // the DropPoint there is None (off every tag) — the whole point.
        assert!(
            log.iter().any(|s| s == "at:400:400"),
            "absolute cursor must be forwarded even when over is None: {log:?}"
        );
        // The release forwarded the final cursor too.
        assert!(
            log.iter().any(|s| s == "drop_at:400:400"),
            "release cursor forwarded: {log:?}"
        );
    }

    #[test]
    fn r1113_active_drag_label_tracks_a_text_payload_drag() {
        // R1113 §5.51 §5.33 — the projection the shell injects as the drag-image
        // follower (the way it reads focus state for the focus ring). Driven
        // through the REAL router input path (press → move → release), not a
        // mock: `Some(label, cursor)` only once a press becomes a real drag with
        // a non-empty text payload; `None` for an idle pointer, a pending click,
        // and after release.
        let mut router = InputRouter::new();
        let (drag, _log) = DragExternal::with_label("outliner");
        let mut state = Scene::External(ExternalNode::new(Box::new(drag)).with_tag("dnd"));
        router.update_paint_scene(paint_with_two_rows(), &mut state);
        // Idle (no press) → no follower.
        assert_eq!(
            router.active_drag_label(PointerId::MOUSE),
            None,
            "idle pointer: no follower",
        );
        // Press inside row 0 — the session opens (begin_drag emits the Text
        // payload), but it is still a CLICK until the cursor moves past the
        // click→drag threshold, so no follower yet.
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(
            router.active_drag_label(PointerId::MOUSE),
            None,
            "a pending click (pressed, not yet dragged) shows no follower",
        );
        // Move well past the threshold → the follower appears AT the cursor.
        router.cursor_moved(PointerId::MOUSE, 140.0, 120.0, &mut state);
        assert_eq!(
            router.active_drag_label(PointerId::MOUSE),
            Some(("outliner".to_string(), (140.0, 120.0))),
            "a real drag with a text payload floats the follower at the cursor",
        );
        // Release ends the session → no follower.
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            router.active_drag_label(PointerId::MOUSE),
            None,
            "after release: no follower",
        );
    }

    #[test]
    fn r1113_non_text_payload_drag_has_no_follower_label() {
        // A drag whose payload is not text (the default Int reorder rows)
        // carries no label, so the shell shows NO drag-image — only a labelled
        // drag gets a follower (the gate that keeps an opaque-payload drag from
        // flashing a meaningless chip).
        let mut router = InputRouter::new();
        let (mut state, _log) = state_with_dnd();
        router.update_paint_scene(paint_with_two_rows(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 140.0, 120.0, &mut state);
        assert!(
            router.press_became_drag(PointerId::MOUSE),
            "a real drag IS in flight (control: the gate is the payload, not the verdict)",
        );
        assert_eq!(
            router.active_drag_label(PointerId::MOUSE),
            None,
            "a non-text payload yields no follower label",
        );
    }

    #[test]
    fn r1102_resolve_drag_targets_is_own_window_first() {
        // R1102 §5.51 PR-33 — the pure precedence rule: an own-window hit wins
        // (over_window None); a cross-window drop applies only when the own
        // resolution is empty; both empty → both None.
        let own = DropPoint {
            tag: "local".to_string(),
            x_rel: 0.5,
            y_rel: 0.5,
        };
        let cross = CrossWindowDrop {
            window: "main".to_string(),
            point: DropPoint {
                tag: "main_dock".to_string(),
                x_rel: 0.25,
                y_rel: 0.75,
            },
        };
        // A NON-self own hit wins even when a cross-window drop is present
        // (same-window reorganize — the cursor is over a DIFFERENT own target).
        let (over, win) = resolve_drag_targets(Some(own.clone()), false, Some(cross.clone()));
        assert_eq!(over.as_ref().map(|p| p.tag.as_str()), Some("local"));
        assert_eq!(
            win, None,
            "a non-self own-window hit suppresses over_window"
        );
        // No own hit → the cross-window drop applies (target window + its zone).
        let (over, win) = resolve_drag_targets(None, false, Some(cross.clone()));
        assert_eq!(over.as_ref().map(|p| p.tag.as_str()), Some("main_dock"));
        assert_eq!(win.as_deref(), Some("main"));
        // Neither → both None (the escape-to-empty-space tear-off case).
        let (over, win) = resolve_drag_targets(None, false, None);
        assert!(over.is_none() && win.is_none());
        // R1124 — a SELF-DROP own hit (the dragged source's own subtree) YIELDS to
        // a cross-window redock: dragging a floater back over another window has
        // the cursor over the floater's own content, which must not mask it.
        let (over, win) = resolve_drag_targets(Some(own.clone()), true, Some(cross));
        assert_eq!(
            over.as_ref().map(|p| p.tag.as_str()),
            Some("main_dock"),
            "a self-drop yields to the cross-window redock target",
        );
        assert_eq!(
            win.as_deref(),
            Some("main"),
            "the redock names the target window"
        );
        // R1124 — but a self-drop with NO cross-window keeps own-window-first, so a
        // plain same-window self-release still snaps back (no spurious float).
        let (over, win) = resolve_drag_targets(Some(own.clone()), true, None);
        assert_eq!(over.as_ref().map(|p| p.tag.as_str()), Some("local"));
        assert_eq!(
            win, None,
            "a self-drop without a cross-window stays same-window"
        );
    }

    #[test]
    fn r1124_own_over_is_self_drop_discriminates_subtree_from_sibling() {
        // R1124 §5.51 PR-33 — the self-drop discriminator that lets a floater's own
        // header / content yield to a cross-window redock WITHOUT misclassifying a
        // same-window reorder. Scene: a "panel" with a "panel#header" child and a
        // "panel_row" grandchild (the floater's intra-panel drop targets), plus a
        // SIBLING "other" panel (a genuine reorganize target).
        let content = Scene::Container(
            ContainerNode::new(vec![Scene::Container(
                ContainerNode::new(vec![]).with_tag("panel_row".to_string()),
            )])
            .with_tag("panel#content".to_string()),
        );
        let panel = Scene::Container(
            ContainerNode::new(vec![
                Scene::Container(ContainerNode::new(vec![]).with_tag("panel#header".to_string())),
                content,
            ])
            .with_tag("panel".to_string()),
        );
        let other = Scene::Container(ContainerNode::new(vec![]).with_tag("other".to_string()));
        let paint = Scene::Container(ContainerNode::new(vec![panel, other]));

        // A drag started on the panel header. Its own header (the press tag), the
        // panel root, and a content row are ALL inside the source subtree → self.
        assert!(own_over_is_self_drop(
            &paint,
            "panel#header",
            "panel#header"
        ));
        assert!(own_over_is_self_drop(&paint, "panel", "panel#header"));
        assert!(own_over_is_self_drop(&paint, "panel", "panel_row"));
        // A genuine same-window reorganize target (a sibling panel) is NOT a self-
        // drop — the own-window-first reorder behaviour is preserved.
        assert!(!own_over_is_self_drop(&paint, "panel", "other"));
        // The reorder-row case: dragging one row onto a SIBLING row shares no
        // ancestry (siblings, not subtree), so it stays a same-window reorganize.
        let rows = Scene::Container(ContainerNode::new(vec![
            Scene::Container(ContainerNode::new(vec![]).with_tag("dnd#0".to_string())),
            Scene::Container(ContainerNode::new(vec![]).with_tag("dnd#1".to_string())),
        ]));
        assert!(!own_over_is_self_drop(&rows, "dnd#0", "dnd#1"));
        assert!(own_over_is_self_drop(&rows, "dnd#0", "dnd#0"));
    }

    #[test]
    fn r1125_drag_cross_window_getter_round_trips() {
        // R1125 §5.51 PR-33 — the READ peer of `set_drag_cross_window` the shell
        // scans (via `CoreShell::cross_window_drag_into`) to paint a TARGET window's
        // incoming drop-zone preview. None before any stash / for no session; the
        // stashed drop after; None again once cleared (the cursor left the window).
        let mut router = InputRouter::new();
        let (mut state, _log) = state_with_dnd();
        router.update_paint_scene(paint_with_two_rows(), &mut state);
        assert!(
            router.drag_cross_window(PointerId::MOUSE).is_none(),
            "no session → no cross-window"
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state); // arms a drag session
        assert!(
            router.drag_cross_window(PointerId::MOUSE).is_none(),
            "session armed but nothing stashed yet"
        );
        let drop = CrossWindowDrop {
            window: "main".to_string(),
            point: DropPoint {
                tag: "main_dock".to_string(),
                x_rel: 0.5,
                y_rel: 0.5,
            },
        };
        router.set_drag_cross_window(PointerId::MOUSE, Some(drop));
        assert_eq!(
            router
                .drag_cross_window(PointerId::MOUSE)
                .map(|d| (d.window.as_str(), d.point.tag.as_str())),
            Some(("main", "main_dock")),
            "the stashed cross-window drop reads back",
        );
        router.set_drag_cross_window(PointerId::MOUSE, None);
        assert!(
            router.drag_cross_window(PointerId::MOUSE).is_none(),
            "cleared when the cursor leaves every other window"
        );
    }

    #[test]
    fn r1102_cross_window_over_window_threaded_on_move_and_release() {
        // R1102 §5.51 PR-33 — the shell stashes a cross-window drop on the
        // session via `set_drag_cross_window`; the drag dispatch then fills
        // `DragUpdate.over_window` on every move AND the release while the cursor
        // is off every OWN-window target (the source's log carries the `:main`
        // window suffix). This is the live wiring the per-window router could
        // never resolve — the shell composes it, the router consumes it.
        let mut router = InputRouter::new();
        let (mut state, log) = state_with_dnd();
        router.update_paint_scene(paint_with_two_rows(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state); // arms the drag
        assert!(
            router.drag_session_active(PointerId::MOUSE),
            "a reorder source opens a session begin_drag can stash a cross-window on"
        );
        // The shell resolved the abs cursor onto ANOTHER window's dock zone and
        // pushed it down (source window excluded — done in the shell).
        router.set_drag_cross_window(
            PointerId::MOUSE,
            Some(CrossWindowDrop {
                window: "main".to_string(),
                point: DropPoint {
                    tag: "main_dock".to_string(),
                    x_rel: 0.5,
                    y_rel: 0.5,
                },
            }),
        );
        // Move + release OFF every own tag (own resolve = None) → the cross-
        // window drop applies on both the move and the commit.
        router.cursor_moved(PointerId::MOUSE, 400.0, 400.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        assert!(
            log.iter().any(|s| s == "at:400:400:main"),
            "move off own targets carries the cross-window id: {log:?}"
        );
        assert!(
            log.iter().any(|s| s == "drop_at:400:400:main"),
            "release off own targets redocks into the cross-window: {log:?}"
        );
    }

    #[test]
    fn r1102_own_window_hit_suppresses_a_stale_cross_window() {
        // R1102 §5.51 PR-33 — own-window-first is robust to a stale cross-window
        // resolution: even with one stashed, a move that lands on THIS window's
        // own drop target is a same-window reorganize (no `:main` suffix). So a
        // cursor returning over its own window never spuriously redocks.
        let mut router = InputRouter::new();
        let (mut state, log) = state_with_dnd();
        router.update_paint_scene(paint_with_two_rows(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.set_drag_cross_window(
            PointerId::MOUSE,
            Some(CrossWindowDrop {
                window: "main".to_string(),
                point: DropPoint {
                    tag: "main_dock".to_string(),
                    x_rel: 0.5,
                    y_rel: 0.5,
                },
            }),
        );
        // Move onto row 1 — an OWN-window drop target.
        router.cursor_moved(PointerId::MOUSE, 100.0, 60.0, &mut state);
        let log = read(&log);
        assert!(
            log.iter().any(|s| s == "at:100:60"),
            "own-window hit keeps over_window None: {log:?}"
        );
        assert!(
            !log.iter().any(|s| s == "at:100:60:main"),
            "an own-window hit must NOT carry the stale cross-window id: {log:?}"
        );
        // …and the rect-relative own drop still reached the source.
        assert!(
            log.iter().any(|s| s == "to:0:dnd#1"),
            "own-window drop forwarded rect-relative: {log:?}"
        );
    }

    #[test]
    fn r742_drag_release_clears_any_vestigial_capture_lock() {
        // A widget that opts into BOTH wants_pointer_capture and
        // begin_drag: the drag session supersedes capture, and the
        // release must not leave a stale captured_targets entry that would
        // pin every future cursor_moved to forward_pointer_move.
        let mut router = InputRouter::new();
        let (drag, _log) = DragExternal::with_capture(true);
        let mut state = Scene::External(ExternalNode::new(Box::new(drag)).with_tag("dnd"));
        router.update_paint_scene(paint_with_two_rows(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        // Both maps armed on press (capture set, then a drag session).
        assert_eq!(router.captured_target(PointerId::MOUSE), Some("dnd#0"));
        router.cursor_moved(PointerId::MOUSE, 100.0, 60.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        // The drag committed AND the capture lock is gone.
        assert_eq!(
            router.captured_target(PointerId::MOUSE),
            None,
            "drag release must clear the vestigial capture lock"
        );
    }

    #[test]
    fn r937_1_pointer_cancel_aborts_drag_session_without_committing() {
        // R937.1 (session-review) — an OS gesture revoke (TouchPhase::Cancelled)
        // during an in-flight drag must ABORT it: tell the source to discard
        // (`drag_cancel`), NOT commit a drop, and remove the session so a later
        // move never routes to a dead `update_drag`.
        let mut router = InputRouter::new();
        let (mut state, log) = state_with_dnd();
        router.update_paint_scene(paint_with_two_rows(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_cancel(PointerId::MOUSE, &mut state);
        let snap = read(&log);
        assert!(
            snap.contains(&"begin:0".to_string()),
            "the drag armed at press: {snap:?}"
        );
        assert!(
            snap.contains(&"cancel:0".to_string()),
            "cancel aborts via drag_cancel: {snap:?}"
        );
        assert!(
            !snap.iter().any(|s| s.starts_with("drop:")),
            "a cancel must NOT commit a drop: {snap:?}"
        );
        // The session is gone: a subsequent move does not route to `drag_to`.
        router.cursor_moved(PointerId::MOUSE, 100.0, 60.0, &mut state);
        assert!(
            !read(&log).iter().any(|s| s.starts_with("to:")),
            "no update_drag after a cancelled session"
        );
    }

    #[test]
    fn r742_press_release_in_place_drops_on_self_and_still_clicks() {
        let mut router = InputRouter::new();
        let (mut state, log) = state_with_dnd();
        router.update_paint_scene(paint_with_two_rows(), &mut state);
        // Press and release on row 0 without moving.
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        // The drop resolves to the same row (no reorder); PointerUp still
        // reaches the statechart so press-to-select stays reachable.
        assert!(
            log.iter().any(|s| s == "drop:0:dnd#0"),
            "drop on self: {log:?}"
        );
        assert!(log.contains(&"0:PointerUp".to_string()), "{log:?}");
    }

    #[test]
    fn r794_subthreshold_jiggle_is_a_click_suprathreshold_is_a_drag() {
        // A press with a tiny (< DRAG_CLICK_THRESHOLD_PX) wobble before release
        // is still a click: the trailing PointerUp fires.
        let mut router = InputRouter::new();
        let (mut state, log) = state_with_dnd();
        router.update_paint_scene(paint_with_two_rows(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 102.0, 21.0, &mut state); // ~2.2px < 4px
        router.pointer_up(PointerId::MOUSE, &mut state);
        let clicked = read(&log);
        assert!(
            clicked.contains(&"0:PointerUp".to_string()),
            "a sub-threshold jiggle is a click: {clicked:?}"
        );

        // A press that wanders past the threshold and *returns* to the press
        // point is still a drag (the latch): no trailing PointerUp.
        let (mut state2, log2) = state_with_dnd();
        router.update_paint_scene(paint_with_two_rows(), &mut state2);
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state2);
        router.pointer_down(PointerId::MOUSE, &mut state2);
        router.cursor_moved(PointerId::MOUSE, 100.0, 60.0, &mut state2); // past threshold
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state2); // back to press
        router.pointer_up(PointerId::MOUSE, &mut state2);
        let dragged = read(&log2);
        assert!(
            !dragged.contains(&"0:PointerUp".to_string()),
            "a drag that returns to the press point is still a drag, not a click: {dragged:?}"
        );
    }
}
