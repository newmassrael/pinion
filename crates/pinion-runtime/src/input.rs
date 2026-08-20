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

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::Instant;

use pinion_core::composite_tag::{compose_send_payload, split_subindex};
use pinion_core::drop_target::{DropActions, DropContract, DropOffer, DropStanding, DropVerdict};
use pinion_core::event::WheelDelta;
use pinion_core::external::{
    CaptureNormalize, DOCK_PANEL_DRAG_KIND, DragPayload, DragUpdate, DropPoint, ExternalIntrospect,
    IntrospectValue, OUTER_DOCK_MARGIN, OUTER_DOCK_ZONE_TAG,
};
use pinion_core::input::{
    GesturePhase, PointerButton, PointerButtons, PointerEdge, PointerReading, PointerWireEvent,
    RawPointerButton,
};
use pinion_core::scene::{ExternalNode, Rect, Scene};
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::wheel::WheelReading;

/// R664 §5.49 — W3C UI Events `dblclick` time threshold (milliseconds).
/// Two consecutive `pointer_down` calls within this window on the same
/// target with a position delta under [`DOUBLE_CLICK_DIST_PX`] dispatch
/// a synthetic `DoubleClick` named event in addition to the second
/// `PointerDown`. 300 ms is the W3C-canonical default
/// (Web `UIEvent.detail` definition, Windows `GetDoubleClickTime`'s
/// system-tunable default, macOS `NSEvent.doubleClickInterval` default).
/// ★ R1701 — the number is [`pinion_core::input::DoubleClickWindow::TIME_MS`]
/// now. Kept as a name here because three readers in this file cite it, and
/// aliased rather than copied because a third reader is exactly how the two
/// this file already had came to be two.
const DOUBLE_CLICK_TIME_MS: u128 = pinion_core::input::DoubleClickWindow::TIME_MS;

/// R664 §5.49 — W3C UI Events `dblclick` position tolerance (logical
/// pixels). Two consecutive presses within [`DOUBLE_CLICK_TIME_MS`] on
/// the same target must land within this Manhattan-distance window per
/// axis to qualify as a double-click; a small drag between the two
/// presses disqualifies (mirrors the Material 3 "intentional gesture"
/// + Cocoa `NSEvent.mouseLocation` tolerance). 5 logical px is the
///   `Material 3` + Cocoa convention.
const DOUBLE_CLICK_DIST_PX: f64 = pinion_core::input::DoubleClickWindow::DIST_PX;

/// R794 §5.51 — drag-vs-click distance (logical pixels). A pressed drag source
/// whose cursor moves more than this from the press point before release is a
/// **drag**, not a click: the router commits the drop via
/// [`External::drag_release`](pinion_core::external::External::drag_release) and does *not*
/// synthesize the trailing `PointerUp` (a drag and a click are mutually exclusive —
/// the toolkit `startDragDistance`, the DOM "no `click` after a drag" rule). A press-release under
/// this threshold is a click: the drop resolves to the source (a no-op) and
/// the `PointerUp` fires so press-to-activate stays reachable. Owning this once makes
/// click-vs-drag a framework SSOT, so no click-activatable drag surface (file
/// tree, asset browser, kanban) re-derives it per binding. R879 relocated the
/// constant itself to `pinion-core::input` (the contract crate): a capture-path External
/// judging its own click-vs-drag (the node graph) measures against the same
/// value ([[helper-crate-home-ssot-axis]]).
use pinion_core::{AutoRepeat, DragLatch};

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
///
/// R1549 — `Ord` as well, so a census over the per-pointer routing tables
/// (`scene/auto_repeat`'s in-flight holds) reads back in a stable order
/// rather than `HashMap` iteration order. The mouse (`PointerId(0)`)
/// sorts first, then touches in the order the platform numbered them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// keeping `PointerId(0)` reserved for [`MOUSE`](PointerId::MOUSE). Wrapping
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
    /// routes mouse + touch should prefer the [`MOUSE`](PointerId::MOUSE) constant and
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
    /// R1423 §5.35 — the contact FORCE, normalised `0.0..=1.0`, the W3C `PointerEvent.pressure` / the
    /// toolkit `pressure()` source (winit `Touch::force`, already normalised at the shell
    /// boundary). `None` when the platform reports no force (a plain touchscreen
    /// without pressure, or a synthesised touch); the router then leaves the
    /// pointer's pressure unchanged rather than forcing it to zero.
    pub force: Option<f32>,
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
/// Widget catalog R47+ (`Slider` / `Toggle` / `TextField`) plugs in by attaching a tag on its
/// paint Container and a matching tag on its state [`ExternalNode`]. No application-level
/// hit-test code is needed — adding a new widget cannot reintroduce the
/// R47-class bug because the routing primitive is framework-owned. R1430 §5.35
/// — the non-positional pointer axis bundle: the toolkit tablet event / W3C
/// `PointerEvent` scalar axes a surface reads alongside the cursor position. One value
/// struct so the router stores one map, forwards one bundle, and adds a new
/// axis as a field — not a parallel `HashMap` + a fifth copy of the note/set/forward
/// plumbing (the R1423 pressure + R1429 tilt duplication this lift resolves).
/// All-zero default is a plain mouse: no force, no lean, no barrel, wheel at
/// rest, in contact.
#[derive(Debug, Clone, Copy, Default)]
struct PointerAxisValues {
    /// W3C `pressure` / the toolkit `pressure()`, `0.0..=1.0`.
    pressure: f32,
    /// W3C `tiltX` / the toolkit `xTilt()`, degrees `-90.0..=90.0`.
    tilt_x: f32,
    /// W3C `tiltY` / the toolkit `yTilt()`, degrees `-90.0..=90.0`.
    tilt_y: f32,
    /// W3C `twist` / the toolkit `rotation()`, degrees `0.0..=360.0` (wrapped).
    twist: f32,
    /// W3C `tangentialPressure` / the toolkit `tangentialPressure()`, `-1.0..=1.0`.
    tangential: f32,
    /// The toolkit `z()` — hover height above the surface, `>= 0.0` (no W3C peer).
    height: f32,
    /// W3C `pointerType` / the toolkit `pointerType()` — the producing device
    /// (`Mouse` default / `Pen` / `Eraser` / `Touch`).
    kind: pinion_core::PointerKind,
}

impl PointerAxisValues {
    /// Forward every axis in the bundle to `handle` — the SINGLE delivery site
    /// both a forwarded `pointer_move` and a standalone `set_pointer_<axis>` call
    /// through, so a new axis is wired in one place and a surface can never see a
    /// half-updated bundle. Each hook defaults to a no-op, so a surface that
    /// reacts to only one axis pays nothing for the rest.
    fn forward_to(self, handle: &mut dyn pinion_core::external::External) {
        handle.pointer_pressure(self.pressure);
        handle.pointer_tilt(self.tilt_x, self.tilt_y);
        handle.pointer_twist(self.twist);
        handle.pointer_tangential_pressure(self.tangential);
        handle.pointer_height(self.height);
        handle.pointer_kind(self.kind);
    }
}

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
    /// `pointer_down` via [`External::wants_pointer_capture`](pinion_core::External::wants_pointer_capture). While
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
    /// [`External::wants_pointer_capture`](pinion_core::External::wants_pointer_capture) flag for the widget under
    /// the corresponding `hover_targets` entry. Refreshed in the
    /// same hover walk as [`refresh_hover`](InputRouter::refresh_hover), so [`pointer_down`](InputRouter::pointer_down)
    /// reads a bit instead of re-walking the scene tree. The cache
    /// stays consistent with the hover lifecycle: dropped when a
    /// pointer's hover clears, replaced when it moves between
    /// tagged widgets, never read while capture is in flight (the
    /// `captured_targets` map already pins the answer for that
    /// pointer). Relies on [`External::wants_pointer_capture`](pinion_core::External::wants_pointer_capture)
    /// being effectively constant per widget instance — the
    /// documented industry precedent (Button=false, Slider=true)
    /// and pinion's own widget catalog all return static bools.
    hover_wants_capture: HashMap<PointerId, bool>,
    /// R1405 §5.35 — per-pointer cache of the hover target's
    /// [`External::wants_hover_move`](pinion_core::External::wants_hover_move),
    /// resolved once on the Enter (the `hover_wants_capture` sibling) so a
    /// per-move forward is a map read, not a scene walk. `true` means each
    /// plain-hover move is also forwarded to the target as `pointer_move`.
    hover_wants_move: HashMap<PointerId, bool>,
    /// R664 §5.49 — per-pointer "last `pointer_down` we dispatched"
    /// snapshot used by [`pointer_down`](Self::pointer_down) to detect
    /// the W3C `UIEvent.detail == 2` double-click pattern: the next
    /// press lands within [`DOUBLE_CLICK_TIME_MS`] on the same target
    /// with a position delta below the same window's per-axis tolerance.
    ///
    /// ★★ R1701 — the thresholds and the pairing rule are
    /// [`pinion_core::input::DoubleClickWindow`] now, one per pointer. They
    /// were two constants and a comment here, and the window chrome needed the
    /// same rule for a press this router never sees (the shell consumes a title
    /// bar's press before routing it), so the promise that the two would not
    /// drift is a type rather than a sentence.
    ///
    /// Keyed by the resolved hit-test target inside the window, so a 2nd press
    /// on a *different* widget never triggers a stale double-click; cleared by
    /// the window itself after a double-click fires, so the next cycle starts
    /// fresh (a triple-click is *not* detail=2 + detail=3 in the W3C spec —
    /// pinion sticks to binary single/double until a 2nd consumer requests
    /// triple, per `[[abstraction-needs-second-consumer]]`).
    last_press: HashMap<PointerId, pinion_core::input::DoubleClickWindow>,
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
    ///
    /// R1549 §5.35 §5.38 — the entry widened from a bare [`DragLatch`] to
    /// a [`PressRecord`], because a *hold* is the same fact as a press:
    /// the press-and-hold auto-repeat run is created and destroyed by the
    /// two statements that already opened and closed this entry, so a
    /// repeat cannot outlive its press.
    press_gestures: HashMap<PointerId, PressRecord>,
    /// R881 §5.35 §5.49 — per-pointer in-flight pan-class gesture. Opened by
    /// [`middle_down`](Self::middle_down) (the middle-button chord) or
    /// [`left_pan_down`](Self::left_pan_down) (R882 — the shell's Space-hold chord routing
    /// a left press into the pan channel), advanced by
    /// [`cursor_moved`](Self::cursor_moved) (the pan arm), consumed by the
    /// matching-button release ([`middle_up`](Self::middle_up) /
    /// [`left_pan_up`](Self::left_pan_up)), revoked by [`pointer_cancel`](Self::pointer_cancel). A
    /// pan press is a *gesture chord*, not a routed widget event: a latched
    /// move is drag-to-pan (the DCC / the engine / the design tool hand-tool
    /// family); a release-in-place is button policy — the middle chord's X11
    /// PRIMARY paste (deferred to release — xterm / the toolkit convention),
    /// the left chord's no-op (the design tool: Space+click is inert). One map
    /// for every opening button so gesture exclusivity (one pan-class gesture
    /// per pointer, first press wins) needs no cross-map bookkeeping. See
    /// [`DragPan`].
    drag_pans: HashMap<PointerId, DragPan>,
    /// R881.1 §5.35 — per-pointer wheel-side sub-pixel remainder (the stage-2
    /// carry of [`dispatch_wheel_two_stage`]). Keyed to the scroll container it accumulated against
    /// via a [`Weak`](std::rc::Weak) handle: the carry resets when the pointer's
    /// resolved scroll target changes (a remainder must never leak across
    /// containers — the toolkit's accumulator discipline) and drops with the
    /// cursor on [`cursor_left`](Self::cursor_left). The middle pan keeps its remainder
    /// in its own gesture state instead — one carry per contiguous delta
    /// stream, whichever producer owns the stream.
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
    /// R1418 §5.35 §5.15 — per-pointer IMPLICIT GRAB for a raw multi-button
    /// sink (a widget that opts into
    /// [`External::wants_raw_pointer_buttons`](pinion_core::external::External::wants_raw_pointer_buttons)).
    ///
    /// A raw sink bypasses [`pointer_down`](Self::pointer_down) — the shell routes its
    /// button edges straight through [`deliver_raw_pointer_button`](Self::deliver_raw_pointer_button) —
    /// so it never populates `captured_targets` and would otherwise lose the release of a
    /// press-drag that strays off its rect (the raw edges would resolve to
    /// whatever widget the cursor left onto, and the sink would see a DOWN
    /// with no matching UP — a "stuck button" for an SGR mouse consumer). This
    /// is the framework's implicit mouse grab (the toolkit `grabMouse` / Win32 `SetCapture` /
    /// DOM implicit pointer capture): on a raw sink's first button press the
    /// router pins that tag, routing EVERY later button edge AND
    /// [`cursor_moved`](Self::cursor_moved) position to it regardless of the cursor
    /// location, and releases only when the LAST held button lifts (`held`
    /// reaches 0). Keyed by [`PointerId`] so two touch streams stay independent,
    /// mirroring `captured_targets`.
    raw_grabs: HashMap<PointerId, RawGrab>,
    /// R1422 §5.35 — per-(pointer, button) last-press mark for the RAW
    /// stream's double-click synthesis
    /// ([`RawPointerButton::click_count`](pinion_core::input::RawPointerButton::click_count), the toolkit
    /// `MouseButtonDblClick` peer). Distinct from `last_press` (the send-wire, target-tag-keyed W3C `dblclick`
    /// path) because a raw sink owns the whole stream — there is no per-target
    /// equality to gate on — but it SHARES the [`DOUBLE_CLICK_TIME_MS`] + [`DOUBLE_CLICK_DIST_PX`] thresholds so the
    /// two double-click rules cannot drift (the `r47`-class one-vocabulary
    /// discipline). Keyed by the button too so a left double-click and a right
    /// double-click count independently, the toolkit per-button rule.
    raw_click_marks: HashMap<(PointerId, PointerButton), RawClickMark>,
    /// R1430 §5.35 — the current non-positional pointer AXES per pointer (the
    /// toolkit tablet event / W3C `PointerEvent` scalar axis set: pressure, tilt, twist,
    /// tangential pressure, hover height). Set from the `scene/pointer_*` RPCs (and, for
    /// pressure, the platform `Touch::force`), and forwarded WHOLE to a surface alongside
    /// each `pointer_move` (every axis travels WITH position, the W3C `pointermove` model). Absent
    /// → [`PointerAxisValues`] default (all zero), so a plain mouse forwards a neutral bundle.
    /// One map, not one-per-axis, so a new axis is a struct field, not a new
    /// `HashMap` (R1423 pressure + R1429 tilt were the first two consumers; R1430
    /// lifts the storage + forward + target-resolution the copies shared).
    axes: HashMap<PointerId, PointerAxisValues>,
    /// R1619 §5.35 §5.41 — the SET of pointer buttons currently held, per
    /// pointer: the W3C `PointerEvent.buttons` state — the same fact the
    /// toolkit hangs off its single-point event base — and the one every drag
    /// gesture is built on.
    ///
    /// Written **only** by [`InputRouter::note_button_edge`], which every
    /// button-edge entry point calls before it dispatches — so the send wire's
    /// stamp is the state *after* the transition (a press includes its button,
    /// a release excludes it), the DOM / toolkit convention [`RawPointerButton`]
    /// already followed. The write is idempotent (a bit assignment, not a
    /// count), which is what lets the left channel note itself, the pan channel
    /// note the same edge, and the shell's one button seam note all three
    /// buttons, without a census of which of those overlap on any given path.
    ///
    /// Absent → [`PointerButtons::empty`]: a pointer that has never reported an
    /// edge holds nothing. Cleared wholesale on window blur
    /// ([`InputRouter::clear_held_buttons`]) for the [`HeldKeys`] reason — a
    /// release that raced the focus loss never arrives, and a stranded press
    /// would make every later hover read as a drag.
    ///
    /// R1619 folded the pre-existing per-grab copy into this map: `RawGrab`
    /// used to keep its own set, seeded EMPTY at the grab's first press, so a
    /// raw sink grabbed while another button was already down was told that
    /// button was up. One home, one answer.
    ///
    /// [`HeldKeys`]: pinion_core::input::HeldKeys
    held_buttons: HashMap<PointerId, PointerButtons>,
    /// R1619 §5.35 §5.41 — the keyboard modifiers held right now, stamped onto
    /// every dispatched pointer event.
    ///
    /// Written **only** by [`InputRouter::set_held_modifiers`], from the same
    /// out-of-band absolute-state funnel the shell already fed
    /// (`ModifiersChanged` natively, `scene/modifiers` over RPC) — one writer,
    /// so the press's answer and the release's cannot come from two caches.
    ///
    /// Before R1619 only the RELEASE carried modifiers
    /// ([`pointer_up_with_modifiers`](InputRouter::pointer_up_with_modifiers),
    /// which still takes its own parameter and is unaffected): the press went
    /// through the zero-modifier path, so a `Ctrl`-press was
    /// **indistinguishable from a plain one** on the wire. That was invisible
    /// while every chord-aware widget acted on the release edge, and became
    /// load-bearing the moment a gesture began at the press — a drag sweep is a
    /// function of the chord held when the finger went down, and there was no
    /// way to read it. `RawPointerButton` had already fixed this on its own
    /// channel (R1416 carries modifiers on both edges); this is the same fix
    /// for the wire every other widget listens on.
    held_modifiers: Modifiers,
    /// R1620 §5.45 §5.35 — the sub-pixel remainder each pointer's auto-scroll
    /// has accrued but not yet spent, per axis.
    ///
    /// [`ScrollState::scroll_by`](pinion_core::widgets::scroll::ScrollState::scroll_by)
    /// moves whole pixels, and a velocity integrated against a frame's `dt`
    /// rarely lands on one: at 60 fps a 30 px/s crawl is half a pixel a frame,
    /// which truncates to zero and stalls the gesture completely at exactly the
    /// speeds a user reaches for when they are being careful. Carrying the
    /// remainder makes the distance travelled a function of elapsed time rather
    /// than of frame boundaries — the same reason the wheel path keeps
    /// `wheel_remainders`.
    auto_scroll_frac: HashMap<PointerId, (f64, f64)>,
    /// R1620 §5.45 §5.35 — the scroll region a held pointer's gesture began in,
    /// pinned for the gesture's whole life.
    ///
    /// Auto-scroll's most useful moment is when the pointer is dragged PAST the
    /// edge — that is where the ramp saturates and the view moves fastest — and
    /// at that moment the cursor is outside the viewport, so resolving the
    /// region by hit-test finds nothing. Pinning at the press is also what
    /// makes the gesture belong to ONE region: a drag that begins in a list and
    /// wanders over a neighbouring one must keep scrolling the list it started
    /// in, exactly as its selection keeps belonging there.
    ///
    /// Held as a [`Weak`](std::rc::Weak) so a region torn down mid-gesture
    /// (a panel closed by a shortcut) ends the auto-scroll instead of keeping
    /// its state alive. The viewport rect is snapshotted beside it, because the
    /// ramp needs an edge to measure against and the node it came from may no
    /// longer be reachable by hit-test; a resize mid-drag therefore measures
    /// against the press-time rect until the pointer returns inside, which is
    /// stated rather than hidden.
    auto_scroll_pin: HashMap<PointerId, AutoScrollPin>,
}

/// R1620 §5.45 §5.35 — the scroll region a held pointer's gesture opened over,
/// captured at the press: the region itself (weakly, so a torn-down panel ends
/// the gesture rather than being kept alive by it), the viewport rect the ramp
/// measures against, and the policy that region declared.
///
/// All three are snapshotted because all three are read at the moment the
/// pointer is OUTSIDE the region, where a hit-test finds nothing — and that is
/// not an edge case, it is where auto-scroll does its work.
#[derive(Debug)]
struct AutoScrollPin {
    state: std::rc::Weak<ScrollState>,
    viewport: Rect,
    policy: pinion_core::widgets::scroll::AutoScroll,
}

/// R1418 §5.35 — one pointer's implicit grab on a raw multi-button sink: the
/// grabbed tag. The grab lives from the first button press until the last
/// release, so a multi-button chord (press left, press right, release left,
/// release right) keeps the grab through the whole span — the toolkit
/// implicit-grab discipline (grab holds until every button is up).
///
/// R1619 — the held SET that decides that lifetime moved to
/// [`InputRouter::held_buttons`], because it was never a property of the grab:
/// it is a property of the pointer, and keeping a second copy here meant the
/// grab's answer and the pointer's answer could differ (they did — the grab's
/// set was seeded empty at its first press, ignoring anything already down).
#[derive(Debug)]
struct RawGrab {
    tag: String,
}

/// R1549 §5.35 §5.38 — everything one in-flight press knows about itself:
/// the R876 click-vs-drag latch it always carried, the target it landed
/// on, and the press-and-hold auto-repeat run.
///
/// # Why the hold lives *in* the press record
///
/// A repeat that outlives its press is the classic runaway-button bug —
/// the toolkit's abstract button keeps a basic timer beside `isDown`, so a
/// missed release / hide / disable path leaves it firing. Here there is
/// no separate place for a run to live: the record is created by
/// [`InputRouter::pointer_down`] and removed by
/// [`InputRouter::pointer_up`], the exact two statements that already
/// existed, so "repeating while nothing is pressed" is not a state the
/// router can represent.
///
/// The *cadence* is not stored at all — it is re-asked of the widget every
/// frame through [`External::auto_repeat`](pinion_core::external::External::auto_repeat). So the
/// two facts the toolkit has to keep in agreement (armed, and down) are one
/// fact here, and it is the widget's own statechart.
#[derive(Debug)]
struct PressRecord {
    /// R876 §5.49 §5.51 — the press-to-drag determination, unchanged.
    latch: DragLatch,
    /// The (possibly composite) paint tag this press landed on. Recorded
    /// rather than re-derived from `hover_targets` / `captured_targets` at
    /// use time: the press's target is its own fact, and the hover can
    /// have moved on ([[drag-latch-router-owns-not-re-derive]]).
    target: String,
    /// Seconds this press has been held *while armed*. Published; also
    /// the honest denominator for "how long before it started repeating".
    held_secs: f32,
    /// Seconds since the last repeat fired (or since the press opened,
    /// while `fires == 0` and the delay is still running).
    since_last_fire: f32,
    /// Repeats fired so far. `0` for an ordinary click.
    fires: u32,
}

impl PressRecord {
    /// Open a record for a press at `origin` on `target`.
    fn new(origin: (f64, f64), target: String) -> Self {
        Self {
            latch: DragLatch::new(origin),
            target,
            held_secs: 0.0,
            since_last_fire: 0.0,
            fires: 0,
        }
    }

    /// The widget answered "not repeating" — rewind the ramp so a press that
    /// strays off its target and comes back restarts from the delay instead of
    /// resuming at speed (the toolkit's `mouseMoveEvent` does the same). The press itself
    /// is untouched: this is not a release.
    fn disarm(&mut self) {
        self.held_secs = 0.0;
        self.since_last_fire = 0.0;
        self.fires = 0;
    }

    /// Seconds that must accrue before the NEXT repeat fires: the delay
    /// while no repeat has fired yet, then the ramped interval that
    /// follows the last one.
    fn next_threshold(&self, policy: AutoRepeat) -> f32 {
        if self.fires == 0 {
            policy.delay_secs()
        } else {
            policy.interval_after(self.fires - 1)
        }
    }
}

/// R1549 §5.35 §5.12 — one in-flight press as published data: what a
/// `scene/auto_repeat` reader sees.
///
/// The toolkit has no peer. `autoRepeat()` answers a *static*
/// property of one widget you already have a pointer to; the in-flight
/// run — is it repeating right now, how many times has it fired, when
/// does the next one land — lives in a private basic timer and is
/// observable only through its side effects. An agent driving a pinion
/// app reads the run itself.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoRepeatHold {
    /// The pointer holding this press.
    pub pointer: PointerId,
    /// The (possibly composite) paint tag under the press.
    pub target: String,
    /// Whether the target declares a repeat cadence *right now*. `false`
    /// is a real answer, not an absence: a press on a non-repeating
    /// widget, on a spin arrow already at its bound, or on a button that
    /// disabled itself mid-hold all report a hold that is not repeating.
    pub repeating: bool,
    /// Seconds held while armed (`0.0` when not repeating).
    pub held_secs: f32,
    /// Repeats fired so far during this press.
    pub fires: u32,
    /// Declared cadence, when the target is repeating.
    pub policy: Option<AutoRepeat>,
    /// Seconds until the next repeat fires, when the target is repeating.
    pub next_fire_in_secs: Option<f32>,
}

/// R1422 §5.35 — one button's last-press mark on the RAW stream, the state the
/// double-click synthesiser compares the next press against. `at` + `(x, y)` are
/// the press instant and cursor position (the time + distance thresholds), and
/// `count` is the ordinal that press reported (`1` or `2`) — kept so a press
/// that already reached `2` starts the next cycle fresh (no rolling triple), and
/// so the matching release can echo the press's count.
#[derive(Debug, Clone, Copy)]
struct RawClickMark {
    at: Instant,
    x: f64,
    y: f64,
    count: u8,
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
    /// R882 Space-hold chord (the design tool / the raster editor hand tool).
    Left,
    /// The middle (W3C auxiliary) button — the chord-free R881 pan.
    Middle,
}

impl PanButton {
    /// R1619 §5.35 — the physical button this pan channel is riding. The pan
    /// vocabulary is a *routing* choice (which channel owns the press); the
    /// held set is a *physical* fact, so the two are related by this one
    /// projection rather than by a parallel set of `note` calls.
    const fn physical(self) -> PointerButton {
        match self {
            PanButton::Left => PointerButton::Left,
            PanButton::Middle => PointerButton::Middle,
        }
    }
}

/// R881 §5.35 §5.49 — one held pan-channel press. `pan` is `None`
/// only when the press arrived before any `cursor_moved` seeded a
/// cursor for the pointer (then the press can never pan — there is no
/// origin to latch against — and release degrades to the click
/// path, the pre-R881 behaviour).
///
/// R1434 rename (was `PanGesture`): a held-button DRAG that pans is not a *gesture* in
/// the native-gesture vocabulary. The name now belongs to
/// [`External::pan_gesture`](pinion_core::external::External::pan_gesture) — the trackpad native
/// gesture event / winit `PanGesture` axis, which carries its own `GesturePhase` and never touches
/// this drag latch. The toolkit draws the same line (pan gesture recogniser vs
/// `PanNativeGesture`); pinion states it in the type names so the two can never be read as
/// one family.
#[derive(Debug)]
struct DragPan {
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
    /// [`ScrollState::scroll_by`] branch (the toolkit wheel-remainder
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

/// R881 §5.35 — what a pan-channel release resolved to (renamed from `MiddleRelease` in
/// R882, when the left button gained the Space-hold chord entry into the same
/// channel). The router owns the click-vs-pan determination (the [`DragLatch`] SSOT);
/// the *action* on `Click` is per-button shell policy — the middle chord pastes
/// (`ShellCore::middle_click`, the X11 PRIMARY funnel), the left Space-chord is inert (the design
/// tool: Space+click does nothing) — substrate decides the gesture, backend
/// decides the action.
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
    /// R1734 §5.51 — the TARGET this drag is currently over: the primary paint
    /// tag of the surface whose declared
    /// [`pinion_core::drop_target::DropContract`] admitted the
    /// drag, and the verdict that surface last returned.
    ///
    /// One field holding both, because they are one fact — "who is being
    /// offered this, and what did they say" — and two fields could disagree.
    /// `None` while the cursor is over no declaring surface, which is where
    /// every pre-R1734 drag stays for its whole life.
    ///
    /// The verdict is retained rather than recomputed at the release for the
    /// reason [`DropAccept`](pinion_core::drop_target::DropAccept) exists: the
    /// commit takes the acceptance the preview produced, so a target cannot
    /// commit somewhere it did not show.
    drop_target: Option<DropTargetState>,
}

/// R1734 §5.51 — who a live drag is currently being offered to, and what they
/// said about it.
#[derive(Debug)]
struct DropTargetState {
    /// Primary paint tag of the surface receiving the offers.
    tag: String,
    /// The actions the source and this surface's declaration have in common,
    /// as [`DropContract::admits`] narrowed them.
    ///
    /// Kept rather than re-derived at the release, because re-deriving is a
    /// second reading of one fact and the whole shape of this contract is that
    /// the preview and the commit read the same one.
    actions: DropActions,
    /// That surface's answer to the most recent offer.
    verdict: DropVerdict,
}

/// R1734 §5.15 §5.51 — where a drop released at `(x, y)` would land, resolved
/// exactly as an in-flight drag resolves it.
///
/// `InputRouter::resolve_drop_point` delegates here (it is private, so this
/// names it in prose), so the point
/// `scene/drop_targets` answers about is the point the router routes by rather
/// than a second opinion about it. That is the property R1703 made
/// load-bearing for the wheel: a published answer that is *computed* the same
/// way cannot drift from the behaviour, and one that is merely *documented*
/// the same way has drifted every time this workspace has measured it.
///
/// The negative guard, the opted-in-drop-target preference and the
/// deepest-tag fallback are all R1099 / R1152 / R1080's, unchanged — this
/// function is where they moved to, not a re-derivation of them.
#[must_use]
pub fn drop_point_at(paint_scene: &Scene, x: f64, y: f64) -> Option<DropPoint> {
    // R1152 §5.51 — a cursor OUTSIDE this window has no own-window drop
    // target; guard before the hit-test, whose clamp would resolve a spurious
    // top-left hit.
    if x < 0.0 || y < 0.0 {
        return None;
    }
    // R1080 §5.51 — prefer the nearest opted-in drop target (a dock panel, a
    // tab strip — `LayoutStyle::drop_target`) so the coordinator receives the
    // semantic drop region with the cursor normalised over THAT region's rect.
    // Falls back to the deepest tagged hit when no node in the path opted in
    // (the reorder-row case, where the drop target is itself the deepest tag),
    // so every pre-R1080 R742 consumer is bit-identical.
    let tag = resolve_drop_target_tag(paint_scene, x, y)
        .or_else(|| resolve_pointer_tag(paint_scene, x, y))?;
    let rect = rect_for_tag(paint_scene, &tag)?;
    let (x_rel, y_rel) = normalize_cursor(rect, x, y);
    Some(DropPoint { tag, x_rel, y_rel })
}

/// R1734 §5.51 — the [`DropContract`] the surface at `primary` publishes, or
/// [`DropContract::EMPTY`] when the tag resolves to nothing or the surface
/// opted out of introspection.
///
/// Empty is the right answer for BOTH absences, and deliberately so: a surface
/// that cannot say what it accepts is not a drop target, which is the same
/// rule §2 #2 applies everywhere else in this tree. It is also what makes the
/// pre-R1734 tree bit-identical under this round — nothing declares, so
/// nothing is offered anything.
#[must_use]
pub fn declared_drop_contract(state_scene: &Scene, primary: &str) -> DropContract {
    state_scene
        .find_external_with_tag(primary)
        .map_or(DropContract::EMPTY, published_contract)
}

/// R1735 — the contract a surface ALREADY RESOLVED publishes.
///
/// Split out at its second caller: the router now reads the declaration off the
/// same node it is about to offer the drag to (one walk instead of two), while
/// the wire's census resolves surfaces by name. Two lookups, and deliberately
/// ONE reading — a second spelling of "what does this node declare" is the
/// drift this module keeps its rule in a single `admits` to avoid.
fn published_contract(node: &ExternalNode) -> DropContract {
    node.handle
        .introspect()
        .map_or(DropContract::EMPTY, ExternalIntrospect::drop_contract)
}

/// R1734 §5.51 — offer the live drag at `over` to whatever surface is under
/// the cursor, maintaining the enter / leave pairing.
///
/// The order is leave-then-enter, matching
/// [`InputRouter::refresh_hover`]'s `PointerLeave` before `PointerEnter`, so a
/// target that clears its preview on leave cannot wipe the preview its
/// successor has already drawn.
///
/// A surface is offered the drag **only** when its own published declaration
/// admits it ([`DropContract::admits`] — right kind, covered part, an action in
/// common). The three structural refusals are therefore derived from the
/// declaration and never asked of the widget, which is what keeps a claim and
/// its outcome from drifting: the list that says yes is the list that says no.
///
/// A free function rather than a method because the release path owns its
/// session (it was removed from the map before the commit) while the move path
/// borrows one — and one behaviour written twice is the drift this file has
/// paid for before.
/// ★★★★★ R1735 — and it ANSWERS with the standing, so the source can be told
/// what a release would do by the same call that decides it. A second
/// derivation for the source's benefit would be the two-computations class this
/// module's target half was built to close, one party over.
fn offer_drag_to_target(
    state_scene: &mut Scene,
    target: &mut Option<DropTargetState>,
    payload: &DragPayload,
    over: Option<&DropPoint>,
    cursor: (f64, f64),
    modifiers: Modifiers,
) -> DropStanding {
    // ★★★★★ R1735 — leave the OLD target first, and only when it is a different
    // surface. R1734 decided this from the admissibility verdict, which meant
    // the verdict had to be computed before the node could be borrowed — and
    // that forced a SECOND full walk of the state scene to reach the same node
    // again for the offer. The change of surface is answerable from the tag
    // alone; a surface that is still under the cursor but no longer admits the
    // drag is the same node, so its preview is dropped below, inside the one
    // borrow. Cost, per move sample with a target under the cursor: two
    // depth-first walks became one.
    if let Some(previous) = target.as_ref()
        && over
            .map(|point| split_subindex(&point.tag).0)
            .is_none_or(|primary| primary != previous.tag)
    {
        if let Some(node) = state_scene.find_external_with_tag_mut(&previous.tag) {
            node.handle.drop_left();
        }
        *target = None;
    }
    let Some(point) = over else {
        return DropStanding::Nowhere;
    };
    let (primary, part) = split_subindex(&point.tag);
    let Some(node) = state_scene.find_external_with_tag_mut(primary) else {
        // Something is painted here and no surface stands behind it (or it
        // vanished in a rebuild mid-gesture): nothing to offer anything to.
        *target = None;
        return DropStanding::Nowhere;
    };
    // The declaration is read off the SAME node that is about to be asked, so
    // the contract that gated dispatch and the surface that received it cannot
    // be two different resolutions of one tag.
    let contract = published_contract(node);
    // R1735 — the structural verdict is kept whole rather than discarded by
    // `.ok()`: its `Err` half is the reason a refused source is now told, and
    // the contract's emptiness is what separates "nothing here takes a drop"
    // from "something here said no". The floor collapses those two.
    let actions = match contract.admits(&payload.kind, payload.actions, part) {
        Ok(actions) => actions,
        Err(refusal) => {
            // The same surface is still under the cursor and no longer admits
            // this drag (the cursor crossed onto an undeclared part). Whatever
            // it was previewing goes, and this is the node to tell.
            if target.take().is_some() {
                node.handle.drop_left();
            }
            return if contract.is_empty() {
                DropStanding::Nowhere
            } else {
                DropStanding::Refused {
                    tag: primary.to_owned(),
                    refusal,
                }
            };
        }
    };
    let verdict = node
        .handle
        .drop_offered(&DropOffer::new(payload, point, actions, cursor, modifiers));
    let standing = match &verdict {
        DropVerdict::Accept(accept) => DropStanding::Accepted {
            tag: primary.to_owned(),
            accept: accept.clone(),
        },
        DropVerdict::Refuse(refusal) => DropStanding::Refused {
            tag: primary.to_owned(),
            refusal: refusal.clone(),
        },
    };
    *target = Some(DropTargetState {
        tag: primary.to_owned(),
        actions,
        verdict,
    });
    standing
}

/// R1734 §5.51 — the release. Re-offer at the release point, commit when that
/// verdict is an acceptance, and leave either way.
///
/// The re-offer is not a second opinion: it is the SAME call the last move
/// made, at the point the person let go, and its acceptance is what
/// [`External::drop_commit`](pinion_core::external::External::drop_commit)
/// receives as its witness. So the target commits the landing it previewed,
/// and there is no arithmetic here that could disagree with the arithmetic
/// there — the class R1668 paid for, and the one a floor that hands its target
/// a bare pixel leaves permanently open.
///
/// `drop_left` runs after a commit as well as after a refusal, so a target
/// never has to clear its own preview in two places.
fn release_drop_target(
    state_scene: &mut Scene,
    target: &mut Option<DropTargetState>,
    payload: &DragPayload,
    over: Option<&DropPoint>,
    cursor: (f64, f64),
    modifiers: Modifiers,
) -> DropStanding {
    // R1735 — and it is the standing the SOURCE is told for the release: the
    // same value the move path forwards, so "what would happen" and "what
    // happened" are one vocabulary rather than two.
    let standing = offer_drag_to_target(state_scene, target, payload, over, cursor, modifiers);
    // Nothing under the cursor declared for this drag; whatever was previewing
    // was already left by the offer above.
    let Some(state) = target.take() else {
        return standing;
    };
    let Some(node) = state_scene.find_external_with_tag_mut(&state.tag) else {
        return standing;
    };
    if let (DropVerdict::Accept(accept), Some(point)) = (&state.verdict, over) {
        let offer = DropOffer::new(payload, point, state.actions, cursor, modifiers);
        node.handle.drop_commit(&offer, accept);
    }
    // Unconditional: a target that committed and a target that refused both
    // stop previewing, so no impl has to clear itself in two places.
    node.handle.drop_left();
    standing
}

/// R1734 §5.51 — the drag ended without a drop over anybody (an OS cancel).
/// Whatever was previewing clears.
fn cancel_drop_target(state_scene: &mut Scene, target: &mut Option<DropTargetState>) {
    if let Some(state) = target.take()
        && let Some(node) = state_scene.find_external_with_tag_mut(&state.tag)
    {
        node.handle.drop_left();
    }
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

    /// R1619 §5.35 §5.41 — record one pointer-button **edge** into `id`'s held
    /// set: the single writer of the W3C `PointerEvent.buttons` state this
    /// router stamps onto every dispatched pointer event.
    ///
    /// Call it **before** dispatching the edge's own event, so the stamp is the
    /// state *after* the transition — a `PointerDown` reports its button held,
    /// the matching `PointerUp` reports it released. That is the DOM and the
    /// toolkit convention, and it is what makes "buttons is empty" mean the
    /// gesture is over rather than about to be.
    ///
    /// **Idempotent**: it assigns a bit rather than counting, so every entry
    /// point that represents a button edge may note its own edge without
    /// coordinating with the others. That is deliberate — the alternative is a
    /// census of which of `pointer_down` / `left_pan_down` / the shell's button
    /// seam / the raw channel run on any given press, and a census like that is
    /// exactly the thing that goes stale silently.
    pub fn note_button_edge(&mut self, id: PointerId, button: PointerButton, edge: PointerEdge) {
        let held = self.held_buttons.entry(id).or_default();
        let was_empty = held.is_empty();
        *held = match edge {
            PointerEdge::Down => held.with(button),
            PointerEdge::Up => held.without(button),
        };
        let now_empty = held.is_empty();
        // R1620 — the gesture's boundaries are exactly the transitions of this
        // set, which is why the auto-scroll pin is taken and released here
        // rather than in one of the several press arms: those are per-button
        // and per-channel, and a chord would open two gestures out of one.
        if was_empty && !now_empty {
            self.pin_auto_scroll_region(id);
        } else if now_empty {
            self.auto_scroll_pin.remove(&id);
            self.auto_scroll_frac.remove(&id);
        }
    }

    /// R1620 §5.45 — remember which scroll region this pointer is over as its
    /// gesture opens. `None` under the cursor simply leaves no pin, and the
    /// gesture then auto-scrolls nothing.
    fn pin_auto_scroll_region(&mut self, id: PointerId) {
        let Some(&(x, y)) = self.cursors.get(&id) else {
            return;
        };
        let Some(paint) = self.last_paint_scene.as_ref() else {
            return;
        };
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a window-local logical cursor; negatives cannot hit a viewport rect"
        )]
        let hit = paint.scroll_target_at(x.max(0.0) as u32, y.max(0.0) as u32);
        if let Some(node) = hit
            && let Some(state) = node.state.as_ref()
        {
            self.auto_scroll_pin.insert(
                id,
                AutoScrollPin {
                    state: std::rc::Rc::downgrade(state),
                    viewport: node.viewport,
                    policy: node.auto_scroll,
                },
            );
        }
    }

    /// R1619 §5.35 — the set of buttons `id` currently holds, the READ peer of
    /// [`note_button_edge`](Self::note_button_edge). A pointer that has never
    /// reported an edge holds nothing.
    #[must_use]
    pub fn held_buttons(&self, id: PointerId) -> PointerButtons {
        self.held_buttons.get(&id).copied().unwrap_or_default()
    }

    /// R1619 §5.35 §5.39 — forget every held button on every pointer: the
    /// window-blur arm, the
    /// [`HeldKeys::clear`](pinion_core::input::HeldKeys::clear) peer.
    ///
    /// A release that raced the focus loss never arrives, so the alternative to
    /// forgetting is a permanently stranded press — and the two errors are not
    /// symmetric. A forgotten button ends a drag early, which the user sees and
    /// can redo; a phantom one leaves every later hover reading as a drag, with
    /// no gesture the user can perform to clear it. R1610's rule: when the
    /// model can go stale, make it stale in the direction that self-corrects.
    pub fn clear_held_buttons(&mut self) {
        self.held_buttons.clear();
    }

    /// R1619 §5.35 §5.41 — set the keyboard modifiers every dispatched pointer
    /// event is stamped with, from the shell's out-of-band absolute-state cache
    /// (winit `ModifiersChanged`, the `scene/modifiers` RPC).
    ///
    /// Absolute, not an edge: the platform reports the whole state, so this
    /// replaces rather than merges. One writer by design — see
    /// [`held_modifiers`](Self::held_modifiers).
    pub fn set_held_modifiers(&mut self, modifiers: Modifiers) {
        self.held_modifiers = modifiers;
    }

    /// R1619 §5.35 — the modifiers this router stamps, the READ peer of
    /// [`set_held_modifiers`](Self::set_held_modifiers).
    #[must_use]
    pub fn held_modifiers(&self) -> Modifiers {
        self.held_modifiers
    }

    /// R1620 §5.35 §5.38 §5.45 — advance everything a held press keeps doing,
    /// by one frame, and answer whether any of it is still going.
    ///
    /// The two continuations are asked **unconditionally**: a press can be
    /// repeating a step AND auto-scrolling at the same time (a stepper inside
    /// a scrolling panel), so `a() || b()` would stop asking the second the
    /// moment the first said yes and silently halve the gesture. Written with
    /// both results bound before they are combined, and the composition lives
    /// HERE rather than in the shell so it sits beside the fixtures that can
    /// exercise both at once — a counterfactual found the shell-side version
    /// untestable in practice, which is the same thing as untested.
    pub fn tick_pointer_hold(&mut self, dt: f32, state_scene: &mut Scene) -> bool {
        let repeating = self.tick_auto_repeat(dt, state_scene);
        let scrolling = self.tick_auto_scroll(dt);
        repeating || scrolling
    }

    /// R1620 §5.45 §5.35 — advance every held pointer's **auto-scroll** by one
    /// frame, and answer whether any of them is still scrolling (so the
    /// backend knows to schedule another).
    ///
    /// A drag reaches the addresses it can see and no further: the pointer
    /// leaves the viewport and the rows past the edge are never entered, so a
    /// sweep stops at the last painted one. This is what lets it keep going —
    /// the reference's `autoScroll`, and the reason its abstract item view can
    /// select past its own bottom edge.
    ///
    /// ## Gated on a HELD BUTTON, which is why this round follows R1619
    ///
    /// A hovering pointer resting near an edge must not drag the view out from
    /// under the reader. The reference gates on its own drag states; here the
    /// gate is [`held_buttons`](Self::held_buttons) — the fact R1619 put on
    /// every event and in this router. Before that there was nothing to gate
    /// on outside a capture, which is the same absence that blocked
    /// drag-select itself.
    ///
    /// ## The selection follows WITHOUT a synthetic event
    ///
    /// Scrolling moves content under a stationary cursor, so the address the
    /// pointer is over changes with no input at all. The reference solves that
    /// by **fabricating a mouse-move** and posting it to the viewport, flagged
    /// as synthesised-by-the-framework — observable to the application and,
    /// at the widget, indistinguishable from the user having moved. Here nothing is fabricated: the scroll marks the region dirty,
    /// the next paint re-runs
    /// [`refresh_hover_for_all_active_pointers`](Self::refresh_hover_for_all_active_pointers),
    /// and the new hover target is a DERIVATION of the new picture. That path
    /// already existed and R1620 proved it end to end before relying on it.
    ///
    /// Returns `true` while any pointer's ramp is live. The value feeds the
    /// same "another frame, please" answer
    /// [`tick_auto_repeat`](Self::tick_auto_repeat) gives, because both are
    /// continuations of one held press.
    pub fn tick_auto_scroll(&mut self, dt: f32) -> bool {
        let mut live = false;
        // R1620 — iterate the PINS, not every pointer with a cursor. A pin
        // exists exactly while a gesture is open (`note_button_edge` takes one
        // on the empty -> non-empty transition and drops it on the way back),
        // so "is a button held" needs no second spelling here. A draft had
        // both and a counterfactual proved the extra one redundant: nothing
        // could catch its removal, because it could never disagree with the
        // pin. Two readers of one fact is the drift this codebase keeps
        // paying for, so the answer was to delete a check rather than to test
        // it.
        let ids: Vec<PointerId> = self.auto_scroll_pin.keys().copied().collect();
        for id in ids {
            let Some(step) = self.auto_scroll_step(id, dt) else {
                self.auto_scroll_frac.remove(&id);
                continue;
            };
            live = true;
            let (state, dx, dy) = step;
            let frac = self.auto_scroll_frac.entry(id).or_insert((0.0, 0.0));
            frac.0 += dx;
            frac.1 += dy;
            let (whole_x, whole_y) = (frac.0.trunc(), frac.1.trunc());
            frac.0 -= whole_x;
            frac.1 -= whole_y;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "whole_* is a trunc()ed per-frame pixel step, far inside i32"
            )]
            let (whole_x, whole_y) = (whole_x as i32, whole_y as i32);
            if whole_x != 0 || whole_y != 0 {
                state.scroll_by(whole_x, whole_y);
            }
        }
        live
    }

    /// R1620 §5.45 §5.16 — what `id`'s auto-scroll is doing right now, for the
    /// `scene/input_state` READ peer. `None` when no gesture holds a region.
    ///
    /// Derived from the same `auto_scroll_step` the tick integrates, at a
    /// notional one-second `dt` so the reported numbers ARE the velocities —
    /// one derivation, so the published answer cannot describe a ramp
    /// different from the one moving the view.
    #[must_use]
    pub fn auto_scroll_state(&self, id: PointerId) -> Option<pinion_core::input::AutoScrollState> {
        if self.held_buttons(id).is_empty() {
            return None;
        }
        let pin = self.auto_scroll_pin.get(&id)?;
        pin.state.upgrade()?;
        let (velocity_x, velocity_y) = self
            .auto_scroll_step(id, 1.0)
            .map_or((0.0, 0.0), |(_, dx, dy)| (dx, dy));
        Some(pinion_core::input::AutoScrollState {
            velocity_x,
            velocity_y,
            margin: pin.policy.margin,
            max_speed: pin.policy.max_speed,
        })
    }

    /// R1620 §5.45 — the scroll region under `id`'s cursor and the distance its
    /// auto-scroll wants to travel this frame, or `None` when the pointer is
    /// over no scrollable region, the region declares auto-scroll off, or the
    /// cursor sits outside every edge band.
    ///
    /// Split out so the borrow of `last_paint_scene` ends before the caller
    /// mutates the remainder map, and so the geometry is testable without a
    /// clock.
    fn auto_scroll_step(
        &self,
        id: PointerId,
        dt: f32,
    ) -> Option<(
        std::rc::Rc<pinion_core::widgets::scroll::ScrollState>,
        f64,
        f64,
    )> {
        let &(x, y) = self.cursors.get(&id)?;
        let paint = self.last_paint_scene.as_ref()?;
        let pin = self.auto_scroll_pin.get(&id)?;
        // Prefer the LIVE node when the cursor is still inside it — its rect and
        // its policy are this frame's. Outside it (the ramp's most useful
        // moment) fall back to the press-time snapshot, which is the whole
        // reason the pin exists.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a window-local logical cursor; negatives cannot hit a viewport rect"
        )]
        let live = paint
            .scroll_target_at(x.max(0.0) as u32, y.max(0.0) as u32)
            .filter(|node| {
                node.state
                    .as_ref()
                    .is_some_and(|s| std::rc::Weak::ptr_eq(&std::rc::Rc::downgrade(s), &pin.state))
            });
        let state = pin.state.upgrade()?;
        // The POLICY is snapshotted in the pin as well, not re-derived from a
        // default: a region that declared auto-scroll OFF must stay off once
        // the pointer wanders outside it, and a default-valued fallback would
        // have switched it on at exactly the moment the pin starts being used.
        let (policy, rect) = live.map_or((pin.policy, pin.viewport), |node| {
            (node.auto_scroll, node.viewport)
        });
        if !policy.is_enabled() {
            return None;
        }
        let state = &state;
        let (lo_x, hi_x) = (f64::from(rect.x), f64::from(rect.x + rect.w));
        let (lo_y, hi_y) = (f64::from(rect.y), f64::from(rect.y + rect.h));
        let vx = policy.speed_at(x, lo_x, hi_x);
        let vy = policy.speed_at(y, lo_y, hi_y);
        if vx == 0.0 && vy == 0.0 {
            return None;
        }
        let dt = f64::from(dt);
        Some((std::rc::Rc::clone(state), vx * dt, vy * dt))
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

    /// (R1196 §5.16 §5.39) The hover [`CursorHint`](pinion_core::style::CursorHint) the deepest hinted node
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
    /// drag-image overlay (`pinion_overlay::inject_drag_image`), the way it
    /// reads focus state to inject the focus ring. `Some` only once the press
    /// became a REAL drag (the `press_became_drag`
    /// click-vs-drag SSOT, so a pending click shows no follower) AND the
    /// payload carries a non-empty text label AND a cursor is known. A
    /// capture-drag (a splitter resize — no [`begin_drag`](pinion_core::external::External::begin_drag) session) has no
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
    /// (`update_drag` / [`pointer_up`](Self::pointer_up))
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
    /// [`External::wants_pointer_capture`](pinion_core::external::External::wants_pointer_capture) on its most recent
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
    /// capture lock (R51.34), a `DnD` drag session (R742), a live
    /// middle pan (R881), or an R1418 raw-sink implicit grab. While
    /// true, every hover-refresh producer must leave the pinned hover
    /// untouched — the gesture classes share ONE predicate so no
    /// producer can drift to a subset (the R873 one-gate discipline
    /// applied to hover).
    fn gesture_pins_hover(&self, id: PointerId) -> bool {
        self.captured_targets.contains_key(&id)
            || self.drag_sessions.contains_key(&id)
            || self.pan_live(id)
            || self.raw_grabs.contains_key(&id)
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
            gesture.latch.advance((x, y));
        }
    }

    /// R876 §5.49 §5.51 — whether the in-flight press for `id` has strayed
    /// into a drag. `false` when no press is tracked (already released, or a
    /// press that never reached a tagged target). The click-vs-drag SSOT
    /// query: a moved drag must neither activate its source on release (R794)
    /// nor seed a `DoubleClick` (R875).
    fn press_became_drag(&self, id: PointerId) -> bool {
        self.press_gestures
            .get(&id)
            .is_some_and(|press| press.latch.live())
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
    /// wheel arm exactly as a held `Ctrl`+wheel would (the DCC
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
            // R1701 — `forget`, not `remove`: the window is the per-pointer
            // detector and survives the gesture; what is dropped is the
            // pending press it would have paired with.
            if let Some(window) = self.last_press.get_mut(&id) {
                window.forget();
            }
        }
        if self.drag_sessions.contains_key(&id) {
            // R742 §5.51 — a drag started on this pointer: resolve the
            // drop location under the absolute cursor and forward it to
            // the source. Takes precedence over capture/free so the
            // source's hover stays pinned (no spurious mid-drag leave).
            self.update_drag(id, x, y, state_scene);
        } else if let Some(tag) = self.raw_grab_tag(id) {
            // R1418 §5.35 — a raw sink holds an implicit grab: forward every
            // move to it regardless of the cursor location (so it keeps a fresh
            // position to correlate its button edges against, even off its
            // rect) and suppress hover re-resolution — the same pin the
            // capture-lock and DnD paths get.
            self.forward_pointer_move(state_scene, &tag, id, x, y);
        } else if let Some(tag) = self.captured_targets.get(&id).cloned() {
            self.forward_pointer_move(state_scene, &tag, id, x, y);
        } else if !pan_live {
            self.refresh_hover(id, state_scene);
            // R1405 §5.35 — a hover target that opted into hover-move (a
            // TextGrid tracking the OSC-8 link cell under the pointer) gets the
            // position on a plain hover too, not only under capture. Re-read
            // AFTER `refresh_hover` (which may have just entered a new target),
            // and forward every move — including moves WITHIN the same target,
            // where `refresh_hover` early-returns with no Enter to piggyback on.
            if self.hover_wants_move.get(&id).copied().unwrap_or(false) {
                if let Some(tag) = self.hover_targets.get(&id).cloned() {
                    self.forward_pointer_move(state_scene, &tag, id, x, y);
                }
            }
        }
        pan_dispatched
    }

    /// R881 §5.35 §5.49 — open a middle-button gesture for `id` (winit
    /// `MouseInput { Middle, Pressed }`). Dispatches **nothing**: the
    /// press is ambiguous between a paste-click and a drag-to-pan until
    /// the [`DragLatch`] resolves it, so the X11 PRIMARY paste that
    /// pre-R881 fired here is deferred to a release-in-place
    /// ([`middle_up`](Self::middle_up) → [`PanRelease::Click`]) —
    /// the xterm / the toolkit release-paste convention.
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

    /// R882 §5.35 §5.39 — open the pan channel for a **left** press: the shell
    /// routes a `MouseInput { Left, Pressed }` here instead of [`pointer_down`](Self::pointer_down) while its Space
    /// chord is held (the design tool / the raster editor / Krita hand tool).
    /// The press dispatches nothing to widgets — no `PointerDown`, no focus steal, no
    /// caret move — and pan targets pin exactly as a middle press would. The
    /// chord policy (which key arms the channel) is the shell's; the router
    /// only knows a left press entered the pan channel.
    pub fn left_pan_down(&mut self, id: PointerId) {
        self.pan_down(id, PanButton::Left);
    }

    /// R881 / R882 §5.35 — the shared pan-channel press arm.
    fn pan_down(&mut self, id: PointerId, button: PanButton) {
        // R1619 §5.35 — a pan press is a button press. Noted here rather than
        // at the two public arms so the middle and left channels cannot answer
        // differently, and ahead of the exclusivity guard for the same reason
        // `pointer_down` does it: routing refusals do not un-press a button.
        self.note_button_edge(id, button.physical(), PointerEdge::Down);
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
        // see [`DragPan::swallowed_presses`].
        if let Some(gesture) = self.drag_pans.get_mut(&id) {
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
        self.drag_pans.insert(
            id,
            DragPan {
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
    /// (release-in-place) is inert for the left chord (the design tool:
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
        self.drag_pans
            .get(&id)
            .is_some_and(|g| g.button == PanButton::Left)
    }

    /// R882.1 §5.35 — whether ANY drag-pan (either opening button,
    /// latched or dead-zone) owns `id`. The shell-tier press front
    /// door reads this to skip its press follow-ups (click-to-focus /
    /// caret positioning / immediate-mode forward) for a press the
    /// router is about to swallow — pre-R882.1 those follow-ups ran
    /// on the pinned (stale) hover target and stole focus during a
    /// live pan.
    ///
    /// R1434 rename (was `pan_gesture_in_flight`): this predicate is about the
    /// held-button drag latch (the private `DragPan`), NOT the native
    /// [`pan_gesture`](Self::pan_gesture) axis, which is stateless at the router
    /// and has nothing in flight to ask about.
    #[must_use]
    pub fn drag_pan_in_flight(&self, id: PointerId) -> bool {
        self.drag_pans.contains_key(&id)
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
        // R1619 — the release half, noted before every `NoPress` early return
        // below: those mean "this channel had no gesture to close", not "the
        // button is still down".
        self.note_button_edge(id, button.physical(), PointerEdge::Up);
        let Some(gesture) = self.drag_pans.get_mut(&id) else {
            return PanRelease::NoPress;
        };
        if gesture.button != button {
            return PanRelease::NoPress;
        }
        if gesture.swallowed_presses > 0 {
            gesture.swallowed_presses -= 1;
            return PanRelease::NoPress;
        }
        match self.drag_pans.remove(&id).map(|g| g.pan) {
            Some(Some(pan)) if pan.latch.live() => PanRelease::Pan,
            Some(_) => PanRelease::Click,
            None => PanRelease::NoPress,
        }
    }

    /// R881 §5.35 — whether a *latched* pan (any opening button) is in
    /// flight for `id` (a non-latched pan press is still a click
    /// candidate and does not pin the hover).
    fn pan_live(&self, id: PointerId) -> bool {
        self.drag_pans
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
        if self.drag_pans.get(&id).is_some_and(|g| g.pan.is_none()) {
            let pan = self.pin_pan_targets(id, (x, y));
            if let Some(gesture) = self.drag_pans.get_mut(&id) {
                gesture.pan = Some(pan);
            }
            return (false, false);
        }
        // Stage 0: advance the latch + compute the delta, then release
        // the gesture borrow before touching the paint scene.
        let (dx, dy, tag, scroll, frac) = {
            let Some(pan) = self.drag_pans.get_mut(&id).and_then(|g| g.pan.as_mut()) else {
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
        // Shift+middle-drag a plain pan — exactly the DCC's chord set.
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
                // A middle-button pan is a continuous drag, and this arm is
                // one of its moves: the gesture's end arrives as a release,
                // not as a wheel event.
                phase: GesturePhase::Update,
                frac,
            },
        );
        if let Some(pan) = self.drag_pans.get_mut(&id).and_then(|g| g.pan.as_mut()) {
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
        let (modifiers, buttons) = (self.held_modifiers, self.held_buttons(id));
        if let Some(tag) = self.hover_targets.remove(&id) {
            dispatch_send(
                state_scene,
                &tag,
                PointerWireEvent::Leave.as_wire_name(),
                modifiers,
                buttons,
            );
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
    /// [`cursor_moved`](Self::cursor_moved) forwards the cursor to the widget through
    /// [`External::pointer_move`](pinion_core::external::External::pointer_move)
    /// and suppresses hover / leave dispatch for this pointer.
    /// R741 §5.35: button-like widgets now also opt in (so a click is
    /// jitter-robust) and pair it with
    /// [`External::cancel_on_release_off_target`](pinion_core::external::External::cancel_on_release_off_target) so a release off the
    /// widget still cancels — see [`pointer_up`](Self::pointer_up).
    ///
    /// R664 §5.49 — W3C UI Events `dblclick` detection. After the
    /// standard `PointerDown` dispatch, the router compares this
    /// press against `last_press` for `id`: if the
    /// previous press hit the same `target_tag` within
    /// `DOUBLE_CLICK_TIME_MS` and the cursor moved less than
    /// `DOUBLE_CLICK_DIST_PX` per axis, the router synthesises a
    /// second named event `DoubleClick` to the same target on top of
    /// the normal `PointerDown`. Widgets that distinguish single from
    /// double activation handle the `DoubleClick` arm in their
    /// `invoke("send", ...)` `match`; widgets that don't (the entire
    /// pre-R664 catalogue) silently ignore the extra event so the
    /// extension is fully additive. The
    /// `DeferredInput::DoubleClick`
    /// RPC drain reaches this same detection because its expansion
    /// fires two consecutive `pointer_down` calls with zero cursor
    /// move in between — the threshold check trivially fires for the
    /// second press, unifying the native winit and RPC-injected paths
    /// at the framework tier per [[r47-class-incident-prevention]].
    pub fn pointer_down(&mut self, id: PointerId, state_scene: &mut Scene) {
        // R1619 §5.35 — the physical button is down whatever routing decides,
        // so the held set is written BEFORE the exclusivity guard below can
        // return. A gesture that swallows the press does not un-press it, and
        // a held set that skipped those paths would report a released button
        // to every widget the pointer later crosses.
        self.note_button_edge(id, PointerButton::Left, PointerEdge::Down);
        // R881.1 §5.35 — gesture exclusivity: while a pan-class gesture
        // (middle drag or R882 Space-chord left drag) owns the pointer, a
        // routed press is swallowed. R882.1 widened the guard from
        // latched-only (`pan_live`) to ANY in-flight pan gesture: a dead-zone pan
        // press is already a gesture candidate, and letting a routed press
        // open a capture / press tracker beside it would feed both gestures
        // the same motion once the pan latches — the exact coexistence `pan_down`'s
        // own guard refuses in the mirror direction (the guards must be
        // symmetric or the exclusivity is one-way). The hover snapshot a press
        // would route by is also stale the moment content slides under the
        // cursor (the toolkit ignores secondary-button presses during an
        // active gesture); the matching release is swallowed in `pointer_up_with_modifiers` so no
        // widget sees an Up without its Down. A swallowed press on a
        // LEFT-owned gesture is counted so its release pairs with the refusal
        // instead of consuming the gesture (see [`DragPan::swallowed_presses`]; this arc IS the
        // left-button channel — middle has its own).
        if let Some(gesture) = self.drag_pans.get_mut(&id) {
            if gesture.button == PanButton::Left {
                gesture.swallowed_presses += 1;
            }
            return;
        }
        if let Some(tag) = self.hover_targets.get(&id).cloned() {
            dispatch_send(
                state_scene,
                &tag,
                PointerWireEvent::Down.as_wire_name(),
                self.held_modifiers,
                self.held_buttons(id),
            );
            // R876 §5.49 §5.51 — open the click-vs-drag tracker for this
            // press (origin = the press cursor). Every press over a tagged
            // target is a click *candidate*; `cursor_moved` latches it to a
            // drag once it strays, and `pointer_up` closes it. One record per
            // pointer feeds both the trailing-click suppression and the
            // double-click detector.
            // R1549 — the record also opens the press-and-hold auto-repeat
            // run and remembers the target it landed on. Gated on a known
            // cursor exactly as before: `hover_targets[id]` is only ever
            // populated by a `cursor_moved` that also seeded `cursors[id]`,
            // so the gate is satisfied on every real path, and defaulting
            // the origin instead would make the very first move latch a
            // drag against a phantom `(0, 0)`.
            if let Some(&origin) = self.cursors.get(&id) {
                self.press_gestures
                    .insert(id, PressRecord::new(origin, tag.clone()));
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
                // value at the click point (Material / `SwiftUI` / the toolkit
                // Slider click-jumps-to-position UX). Without this
                // forward the value would not update unless the user
                // also dragged the cursor at least one pixel.
                if let Some(&(x, y)) = self.cursors.get(&id) {
                    self.forward_pointer_move(state_scene, &tag, id, x, y);
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
                        // R1734 — nothing has been offered anything yet; the
                        // first move resolves the target under the cursor.
                        drop_target: None,
                    },
                );
            }

            // R664 §5.49 — double-click detection. Same target +
            // within W3C `dblclick` time + space window → synthesise
            // a `DoubleClick` named event on top of `PointerDown`.
            //
            // ★★ R1701 — the WINDOW is `pinion_core::input::DoubleClickWindow`
            // now, not two constants and a comment here. The window chrome runs
            // a second detector (a title-bar press is consumed before this
            // router ever sees it), and R1422's doc already promised the two
            // would not drift; this is that promise as a type.
            let cursor = self.cursors.get(&id).copied();
            let is_double = match cursor {
                Some((cx, cy)) => {
                    self.last_press
                        .entry(id)
                        .or_default()
                        .press(Instant::now(), cx, cy, &tag)
                        == 2
                }
                // No held cursor is no position to compare, so no pairing.
                None => false,
            };
            if is_double {
                dispatch_send(
                    state_scene,
                    &tag,
                    "DoubleClick",
                    self.held_modifiers,
                    self.held_buttons(id),
                );
            }
        }
    }

    /// R1549 §5.35 §5.38 — advance every in-flight press's **auto-repeat**
    /// by `dt` seconds and fire whatever repeats that crosses. Returns
    /// whether any hold is currently *armed* — the backend's "keep
    /// painting" cue, the [`Tickable::is_at_rest`] peer for a gesture that
    /// lives in the router rather than the owner's animation registry.
    ///
    /// [`Tickable::is_at_rest`]: pinion_core::animation::Tickable::is_at_rest
    ///
    /// # The clock is the frame, not a wall-clock timer
    ///
    /// The toolkit's auto-repeat is a basic timer on the event loop: a test
    /// has to sleep, and there is no way to *express* "hold this for 900 ms"
    /// to a running application. This rides the same `dt` the paint cycle and
    /// the `scene/tick` RPC already supply, so a hold is reproducible to the fire —
    /// which is also what keeps the §2 #3 `dry_run` determinism invariant intact (a
    /// wall-clock timer inside input routing would have broken it).
    ///
    /// # Boundaries
    ///
    /// The guarantee is *one fire per threshold crossed*, accumulated in
    /// `f32`. A `dt` landing exactly ON a fire instant may or may not
    /// include that fire, because the running remainder is a float
    /// subtraction — the same latitude every float clock has, and orders
    /// of magnitude tighter than the toolkit's millisecond basic timer under
    /// event-loop jitter. Callers who need an exact count should tick past
    /// the instant, not onto it; `AutoRepeatHold::next_fire_in_secs`
    /// publishes exactly how far that is.
    ///
    /// # Each fire re-asks
    ///
    /// The widget is consulted before the accumulation *and* before every
    /// individual fire, so a large `dt` that crosses several thresholds
    /// stops exactly where the widget stops answering — a spin arrow that
    /// reaches its bound mid-catch-up does not overshoot. A `None` answer
    /// rewinds the ramp (`PressRecord::disarm`) without ending the
    /// press.
    ///
    /// # A fire is the widget's own activation
    ///
    /// `PointerUp` then `PointerDown` — the toolkit's `released(); clicked();
    /// pressed();` in statechart vocabulary. No repeat-specific event
    /// exists, so a repeat cannot come to mean something a click does
    /// not, and no widget needs an SCXML transition to become repeatable.
    /// Net interaction state is unchanged (`Pressed` before and after).
    pub fn tick_auto_repeat(&mut self, dt: f32, state_scene: &mut Scene) -> bool {
        if !dt.is_finite() || dt <= 0.0 {
            // A frozen clock still reports whether a hold is armed, so a
            // `scene/tick 0` does not read as "the hold ended".
            return self.any_auto_repeat_armed(state_scene);
        }
        let mut armed_any = false;
        let ids: Vec<PointerId> = self.press_gestures.keys().copied().collect();
        for id in ids {
            let Some(target) = self
                .press_gestures
                .get(&id)
                .map(|press| press.target.clone())
            else {
                continue;
            };
            // `target` is cloned out because the fire below needs
            // `&mut state_scene` while the record is borrowed for the
            // accumulate; the QUERY itself is a shared read.
            let (mods, held) = (self.held_modifiers, self.held_buttons(id));
            if widget_auto_repeat(state_scene, &target).is_none() {
                if let Some(press) = self.press_gestures.get_mut(&id) {
                    press.disarm();
                }
                continue;
            }
            if let Some(press) = self.press_gestures.get_mut(&id) {
                press.held_secs += dt;
                press.since_last_fire += dt;
            }
            // Catch-up loop: re-ask, then fire, for as long as the accrued
            // time keeps crossing thresholds. `AutoRepeat` floors every
            // interval at `MIN_INTERVAL_FLOOR_SECS`, so the loop is bounded
            // by `dt / floor` for any declaration.
            //
            // The loop's exit reason IS the armed answer, which is why it
            // is captured here rather than set before the loop: a hold that
            // ran out of range mid-catch-up (a spin arrow reaching its
            // bound on a large `scene/tick`) is no longer armed by the time
            // this frame ends, and reporting the pre-loop answer would ask
            // the backend for a frame that has nothing left to do.
            loop {
                let Some(policy) = widget_auto_repeat(state_scene, &target) else {
                    break;
                };
                let Some(press) = self.press_gestures.get_mut(&id) else {
                    break;
                };
                let threshold = press.next_threshold(policy);
                if press.since_last_fire < threshold {
                    armed_any = true;
                    break;
                }
                press.since_last_fire -= threshold;
                press.fires += 1;
                // R1619 — a repeat is a synthetic press cycle while the
                // physical button is still down, so both halves carry the
                // real held set (the finger never left the button).
                dispatch_send(
                    state_scene,
                    &target,
                    PointerWireEvent::Up.as_wire_name(),
                    mods,
                    held,
                );
                dispatch_send(
                    state_scene,
                    &target,
                    PointerWireEvent::Down.as_wire_name(),
                    mods,
                    held,
                );
            }
        }
        armed_any
    }

    /// R1549 §5.35 — whether any in-flight press is on a target that
    /// declares a repeat cadence right now. The read-only half of
    /// [`Self::tick_auto_repeat`], used when the clock did not advance.
    fn any_auto_repeat_armed(&self, state_scene: &Scene) -> bool {
        self.press_gestures
            .values()
            .any(|press| widget_auto_repeat(state_scene, &press.target).is_some())
    }

    /// R1549 §5.35 §5.12 — every in-flight press as published data, for
    /// the `scene/auto_repeat` introspection method. Ordered by pointer so
    /// two simultaneous touch-holds read back deterministically.
    ///
    /// A press on a widget that does not repeat is still reported (with
    /// `repeating: false`): "this press is held and nothing will come of
    /// it" is the answer an agent needs, and omitting it would make a
    /// non-repeating hold indistinguishable from no hold at all.
    pub fn auto_repeat_holds(&self, state_scene: &Scene) -> Vec<AutoRepeatHold> {
        let mut ids: Vec<PointerId> = self.press_gestures.keys().copied().collect();
        ids.sort_unstable();
        ids.into_iter()
            .filter_map(|pointer| {
                let press = self.press_gestures.get(&pointer)?;
                let policy = widget_auto_repeat(state_scene, &press.target);
                Some(AutoRepeatHold {
                    pointer,
                    target: press.target.clone(),
                    repeating: policy.is_some(),
                    held_secs: press.held_secs,
                    fires: press.fires,
                    policy,
                    next_fire_in_secs: policy
                        .map(|p| (press.next_threshold(p) - press.since_last_fire).max(0.0)),
                })
            })
            .collect()
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
    /// widget's [`External::cancel_on_release_off_target`](pinion_core::external::External::cancel_on_release_off_target) policy:
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
    /// `refresh_hover` re-runs to resettle the
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
        // R1619 §5.35 — the release edge, recorded before any guard returns
        // (the press half's argument, mirrored). Read back immediately so
        // every dispatch below stamps ONE set: the release's own event must
        // report the button as no longer held.
        self.note_button_edge(id, PointerButton::Left, PointerEdge::Up);
        let buttons = self.held_buttons(id);
        // R1619 — this arm's explicit `modifiers` parameter predates the
        // router's cache and carries the same absolute state, so it WRITES the
        // cache rather than being read beside it: one source, or the press and
        // the release could disagree about a chord the user never changed.
        self.set_held_modifiers(modifiers);
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
        if let Some(gesture) = self.drag_pans.get_mut(&id) {
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
        if let Some(mut session) = self.drag_sessions.remove(&id) {
            self.captured_targets.remove(&id);
            let cursor = self.cursors.get(&id).copied();
            // R1167 §5.51 — same-window OUTER-dock override (dock-panel drag only),
            // shared with the move path so preview (`update_drag`) == result here.
            let own_over = cursor.and_then(|(x, y)| {
                self.resolve_drag_own_over(&session.payload, &session.source_tag, x, y, state_scene)
            });
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
                resolve_drag_targets(own_over, own_is_self_drop, session.cross_window.take());
            // R1734 §5.51 — the TARGET half of the release, before the source's
            // own `drag_release_at`. The target re-judges the release point and
            // commits the acceptance that judgement produced, so what it applies
            // is what the preview last showed. It runs FIRST because a source
            // that also owns the destination model (every pre-R1734 consumer)
            // must see the world the drop already changed, not the one before.
            let standing = release_drop_target(
                state_scene,
                &mut session.drop_target,
                &session.payload,
                over.as_ref(),
                cursor.unwrap_or(session.press_cursor),
                // The CACHE, not this arm's parameter, even though
                // `set_held_modifiers` above has just made them equal. R1619's
                // own note is why: the cache is the one source, and a reader
                // that takes the parameter instead is a second reader that a
                // later refactor can leave behind. The move path reads the same
                // field, so the two halves of one gesture cannot disagree about
                // a chord the person never changed.
                self.held_modifiers,
            );
            let (primary, _) = split_subindex(&session.source_tag);
            if let Some(external) = state_scene.find_external_with_tag_mut(primary) {
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
                            // R1735 — what the release actually resolved to,
                            // from the same call that performed it.
                            standing,
                        },
                    ),
                    None => external.handle.drag_release(&session.payload, over),
                }
            }
            // R794 §5.51 — a drag and a click are mutually exclusive. Only a
            // press-release *in place* (the cursor never left the press point
            // by DRAG_CLICK_THRESHOLD_PX) synthesizes the trailing `PointerUp` click;
            // a real moved drag committed via `drag_release` above and must NOT also
            // activate the source (the row a file move relocated, the tab a
            // reorder shifted). This is the framework SSOT for click-vs-drag —
            // the toolkit `startDragDistance`, the DOM no-`click`-after- drag rule — so no drag
            // source re-derives it per binding. R876: `became_drag` is the unified
            // press-to-drag determination (`track_press_drag`), shared with the double-click
            // detector.
            if !became_drag {
                dispatch_send(
                    state_scene,
                    &session.source_tag,
                    PointerWireEvent::Up.as_wire_name(),
                    modifiers,
                    buttons,
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
            dispatch_send(
                state_scene,
                &cap_tag,
                event.as_wire_name(),
                modifiers,
                buttons,
            );
            self.captured_targets.remove(&id);
            self.refresh_hover(id, state_scene);
        } else if let Some(tag) = self.hover_targets.get(&id).cloned() {
            // Free (no-capture) release: the cursor is over the target
            // (a mid-press stray already drove the SCXML out of Pressed
            // via `cursor_moved`'s `PointerLeave`).
            dispatch_send(
                state_scene,
                &tag,
                PointerWireEvent::Up.as_wire_name(),
                modifiers,
                buttons,
            );
        }
    }

    /// R741 §5.35 — whether the cursor for `id` currently resolves to
    /// `tag` (full-tag equality, so a composite `group#0` press that is
    /// released over `group#1` reads as off-target — the W3C "press and
    /// release on the same control" rule). `false` when the pointer has
    /// no tracked cursor or no last paint scene.
    ///
    /// R1497 — resolves through [`resolve_pointer_tag`], the same answer
    /// [`Self::refresh_hover`] stores. Pre-R1497 it took the deepest TAG, so a
    /// capture widget whose own label sat under the release cursor read as
    /// off-target and cancelled instead of activating — a press released exactly
    /// where it started, refused because the widget paints its own name there.
    fn cursor_over_tag(&self, id: PointerId, tag: &str) -> bool {
        match (self.cursors.get(&id), self.last_paint_scene.as_ref()) {
            (Some(&(x, y)), Some(scene)) => {
                resolve_pointer_tag(scene, x, y).as_deref() == Some(tag)
            }
            _ => false,
        }
    }

    /// R1416 §5.35 §5.15 — route a raw mouse-button edge to a widget that owns
    /// the multi-button pointer stream, returning `true` when a raw sink
    /// consumed it (so the shell suppresses the GUI default for that button).
    /// `false` when the target is not a raw sink, so the shell runs the standard
    /// per-button arc (left = focus, middle = paste, right = context menu)
    /// unchanged — the non-capture invariant.
    ///
    /// R1418 §5.35 — IMPLICIT GRAB (the toolkit `grabMouse` / DOM implicit pointer
    /// capture). A raw sink bypasses [`pointer_down`](Self::pointer_down), so it never
    /// engages `captured_targets`; this method supplies the equivalent press-to-release grab
    /// itself:
    ///
    /// * **While a grab is held** (`raw_grabs[id]`), the edge goes to the
    ///   GRABBED tag regardless of the cursor location, and the held-button
    ///   count tracks the edge (a press raises it, a release lowers it). The
    ///   grab releases when the last button lifts (`held` → 0), after which
    ///   `refresh_hover` re-settles against the cursor.
    ///   This is what pairs a press-drag-release that strayed off the sink's
    ///   rect — without it an SGR mouse consumer would see a stuck button (the
    ///   §5.15 forcing case). A stale grab (the tag reconciled away) is dropped
    ///   and the edge falls through to a fresh resolve.
    /// * **With no grab**, the target is the captured tag (a raw sink that also
    ///   holds a left-drag capture) else the hover target under the cursor. If
    ///   that is a raw sink the edge is delivered, and a PRESS opens a fresh
    ///   grab (`held` = 1); a lone release (a button pressed elsewhere, lifted
    ///   over the sink) delivers without opening one.
    ///
    /// POSITION rides the separate [`pointer_move`](pinion_core::external::External::pointer_move)
    /// channel — a raw sink opts into `wants_hover_move`, and while grabbed
    /// [`cursor_moved`](Self::cursor_moved) forwards each move to the grabbed
    /// tag (suppressing hover churn), so the sink keeps a fresh position to
    /// correlate the button edge against even off its rect.
    pub fn deliver_raw_pointer_button(
        &mut self,
        id: PointerId,
        button: PointerButton,
        edge: PointerEdge,
        modifiers: Modifiers,
        state_scene: &mut Scene,
    ) -> bool {
        // R1422 — synthesise the click-count (the toolkit `MouseButtonDblClick`) for this edge.
        // On a press it is derived from the prior mark; `pending_mark` is COMMITTED only
        // once the edge is actually delivered (below), so a press that
        // resolves to no raw sink cannot poison the next real press's
        // double-click window.
        let (click_count, pending_mark) = self.raw_click_for(id, button, edge);
        // R1619 — the held-button SET after this edge is now the router's own
        // per-pointer state, noted once here (the toolkit `buttons()`
        // semantics: a press adds, a release removes). Pre-R1619 this arm
        // applied the edge to a copy kept on the grab, seeded EMPTY at the
        // grab's first press — so a raw sink grabbed while another button was
        // already down was told that button was up, and the router's answer
        // and the grab's answer could differ. One writer, one answer.
        self.note_button_edge(id, button, edge);
        let buttons = self.held_buttons(id);
        // An active grab pins the target regardless of the cursor location.
        if let Some(grab) = self.raw_grabs.get(&id) {
            let tag = grab.tag.clone();
            let event = RawPointerButton {
                button,
                edge,
                modifiers,
                buttons,
                click_count,
            };
            if dispatch_raw_button(state_scene, &tag, event) {
                self.commit_raw_click_mark(id, button, pending_mark);
                if buttons.is_empty() {
                    // Last button lifted — release the grab and re-settle hover.
                    self.raw_grabs.remove(&id);
                    self.refresh_hover(id, state_scene);
                }
                return true;
            }
            // The grabbed tag no longer resolves to a raw sink (the scene
            // reconciled it away mid-gesture) — drop the stale grab and let the
            // edge resolve fresh below rather than swallow it silently.
            self.raw_grabs.remove(&id);
        }
        // No grab: resolve the target the same way hover / capture does. The
        // held set is the pointer's, so a press over a fresh sink reports
        // everything the pointer holds — including a button pressed before the
        // sink was reached, which the pre-R1619 per-grab set could not see.
        let Some(target) = self
            .captured_targets
            .get(&id)
            .or_else(|| self.hover_targets.get(&id))
            .cloned()
        else {
            return false;
        };
        let event = RawPointerButton {
            button,
            edge,
            modifiers,
            buttons,
            click_count,
        };
        if !dispatch_raw_button(state_scene, &target, event) {
            return false;
        }
        self.commit_raw_click_mark(id, button, pending_mark);
        // A press on a fresh raw sink opens its implicit grab (seeded with the
        // held set); a lone release (no matching press held) delivers without
        // opening one.
        if edge == PointerEdge::Down {
            self.raw_grabs.insert(id, RawGrab { tag: target });
        }
        true
    }

    /// R1422 §5.35 — the RAW stream's double-click synthesiser: compute the
    /// [`RawPointerButton::click_count`](pinion_core::input::RawPointerButton::click_count)
    /// for one edge, plus the mark to commit if that edge is delivered.
    ///
    /// * A **press** ([`PointerEdge::Down`]) reports `2` when it repeats the same
    ///   button as the prior press within [`DOUBLE_CLICK_TIME_MS`] and under
    ///   [`DOUBLE_CLICK_DIST_PX`] per axis of the prior press (the toolkit
    ///   `MouseButtonDblClick`), else `1`. It caps there — a press that already
    ///   reported `2` starts the next cycle fresh (`prev.count == 1` guard), the
    ///   send-wire `DoubleClick`'s "no rolling triple-click" rule. The returned
    ///   mark carries the reported count so the release can echo it. With no
    ///   tracked cursor (a press before any move seeded a position) the double
    ///   cannot be confirmed, so it is a fresh single with no mark.
    /// * A **release** ([`PointerEdge::Up`]) echoes the count of the press it
    ///   releases (the DOM `MouseEvent.detail` model — a consistent press/release
    ///   pair), or `1` when no tracked press matches; it never mutates the mark.
    ///
    /// Reads `self.cursors` (the same live position the send-wire `pointer_down`
    /// double-click reads) and the shared thresholds, so the raw and send-wire
    /// double-click rules stay one vocabulary.
    fn raw_click_for(
        &self,
        id: PointerId,
        button: PointerButton,
        edge: PointerEdge,
    ) -> (u8, Option<RawClickMark>) {
        match edge {
            PointerEdge::Up => (
                self.raw_click_marks
                    .get(&(id, button))
                    .map_or(1, |m| m.count),
                None,
            ),
            PointerEdge::Down => {
                let now = Instant::now();
                let Some(&(cx, cy)) = self.cursors.get(&id) else {
                    return (1, None);
                };
                let count = match self.raw_click_marks.get(&(id, button)) {
                    Some(prev)
                        if prev.count == 1
                            && now.duration_since(prev.at).as_millis() < DOUBLE_CLICK_TIME_MS
                            && (prev.x - cx).abs() < DOUBLE_CLICK_DIST_PX
                            && (prev.y - cy).abs() < DOUBLE_CLICK_DIST_PX =>
                    {
                        2
                    }
                    _ => 1,
                };
                (
                    count,
                    Some(RawClickMark {
                        at: now,
                        x: cx,
                        y: cy,
                        count,
                    }),
                )
            }
        }
    }

    /// R1422 §5.35 — store the double-click mark a delivered
    /// [`deliver_raw_pointer_button`](Self::deliver_raw_pointer_button) press
    /// computed. Called only after a successful dispatch, so an edge that reached
    /// no raw sink leaves the mark untouched; a `None` mark (a release, or a
    /// press with no tracked cursor) is a no-op.
    fn commit_raw_click_mark(
        &mut self,
        id: PointerId,
        button: PointerButton,
        mark: Option<RawClickMark>,
    ) {
        if let Some(mark) = mark {
            self.raw_click_marks.insert((id, button), mark);
        }
    }

    /// R1418 §5.35 — is `id` currently holding an implicit raw grab? The
    /// [`cursor_moved`](Self::cursor_moved) fast-path reads this to forward the
    /// move to the grabbed sink (and suppress hover churn) before the ordinary
    /// hover / capture resolution.
    fn raw_grab_tag(&self, id: PointerId) -> Option<String> {
        self.raw_grabs.get(&id).map(|g| g.tag.clone())
    }

    /// (R51.186 §5.45 R55.C.2) Mouse wheel input dispatch.
    /// Zero-modifier wrapper around
    /// [`wheel_with_modifiers`](Self::wheel_with_modifiers) (native
    /// notched wheels without held keys, the TUI shell, tests) —
    /// mirrors the [`pointer_up`](Self::pointer_up) /
    /// [`pointer_up_with_modifiers`](Self::pointer_up_with_modifiers)
    /// pair.
    /// The phase is [`GesturePhase::Update`] — what a notched mouse wheel is:
    /// an event that neither begins nor ends a continuous gesture, which is
    /// also the phase winit reports for one.
    pub fn wheel(&mut self, id: PointerId, delta: WheelDelta, state_scene: &mut Scene) -> bool {
        self.wheel_with_modifiers(
            id,
            delta,
            Modifiers::empty(),
            GesturePhase::Update,
            state_scene,
        )
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
    /// [`WheelDelta`] and the
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
        phase: GesturePhase,
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
        // R881.1 — the wheel-side sub-pixel remainder (the same carry the pan
        // gesture holds in its state): a slow high-DPI `PixelDelta` stream (0.4
        // px/event) must accumulate instead of rounding to zero forever. Per
        // the toolkit's accumulator discipline the carry resets when the
        // resolved scroll target changes — a remainder must never leak across
        // containers.
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
                phase,
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

    /// R1432 §5.35 §5.15 — offer a native PINCH (magnify) gesture to the
    /// [`External`](pinion_core::external::External) under this pointer's cursor.
    /// Mirrors the External-offer leg of [`wheel_with_modifiers`](Self::wheel_with_modifiers):
    /// resolve the hover target under the stored cursor, normalise the cursor
    /// over the widget's capture rect (the SAME basis a `wheel` / `pointer_move` reads), and
    /// forward the incremental `magnification` + `phase`. There is deliberately NO `Scene::Scroll`
    /// fallback — a native gesture has no default scroll action, so the
    /// toolkit delivers native gesture event only to the widget under the
    /// cursor. Returns `true` if that widget consumed it.
    ///
    /// No-op (`false`) under the same router-state guards
    /// [`wheel_with_modifiers`](Self::wheel_with_modifiers) checks: no stored
    /// cursor for `id`, no retained paint scene, or no hover target covering the
    /// cursor.
    pub fn pinch_gesture(
        &mut self,
        id: PointerId,
        magnification: f64,
        phase: GesturePhase,
        modifiers: Modifiers,
        state_scene: &mut Scene,
    ) -> bool {
        let Some((paint, target_tag, cursor)) = self.hovered_gesture_target(id) else {
            return false;
        };
        offer_pinch_to_external(
            paint,
            state_scene,
            &target_tag,
            cursor,
            magnification,
            phase,
            modifiers,
        )
    }

    /// R1434 §5.35 — the state the three native-gesture legs
    /// ([`pinch_gesture`](Self::pinch_gesture) /
    /// [`rotation_gesture`](Self::rotation_gesture) /
    /// [`pan_gesture`](Self::pan_gesture)) each need before they can offer
    /// anything: the pointer's stored cursor, the retained paint scene, and the
    /// hover target covering that cursor. Any one missing = the gesture is a
    /// clean no-op, the router-state guard
    /// [`wheel_with_modifiers`](Self::wheel_with_modifiers) states too.
    ///
    /// Lifted when the pan axis made this scaffold its THIRD verbatim copy — it
    /// is mechanical wiring with no per-gesture opinion, so it belongs in one
    /// place; what genuinely differs (the payload) stays in each caller's offer.
    /// The [`offer_to_hovered_external`] lift did the same for the delivery half.
    fn hovered_gesture_target(&self, id: PointerId) -> Option<(&Scene, String, (f64, f64))> {
        let &(x, y) = self.cursors.get(&id)?;
        let paint = self.last_paint_scene.as_ref()?;
        let target_tag = self.hover_targets.get(&id).cloned()?;
        Some((paint, target_tag, (x, y)))
    }

    /// R1433 §5.35 §5.15 — offer a native ROTATION gesture to the widget under
    /// pointer `id`'s cursor, the [`pinch_gesture`](Self::pinch_gesture) sibling with `rotation`
    /// (degrees) in place of `magnification`. Same offer-to-hovered-only delivery — NO `Scene::Scroll`
    /// fallback, a native gesture reaches only the widget under the cursor
    /// (the toolkit's contract). Returns `true` if that widget consumed it.
    ///
    /// No-op (`false`) under the same router-state guards
    /// [`pinch_gesture`](Self::pinch_gesture) checks: no stored cursor for `id`,
    /// no retained paint scene, or no hover target covering the cursor.
    pub fn rotation_gesture(
        &mut self,
        id: PointerId,
        rotation: f64,
        phase: GesturePhase,
        modifiers: Modifiers,
        state_scene: &mut Scene,
    ) -> bool {
        let Some((paint, target_tag, cursor)) = self.hovered_gesture_target(id) else {
            return false;
        };
        offer_rotation_to_external(
            paint,
            state_scene,
            &target_tag,
            cursor,
            rotation,
            phase,
            modifiers,
        )
    }

    /// R1434 §5.35 §5.15 — offer a native PAN gesture to the widget under
    /// pointer `id`'s cursor, the [`pinch_gesture`](Self::pinch_gesture) /
    /// [`rotation_gesture`](Self::rotation_gesture) sibling with a two-dimensional `(delta_x, delta_y)` in
    /// logical pixels in place of a single scalar. Same offer-to-hovered-only
    /// delivery — NO `Scene::Scroll` fallback: a native gesture reaches only the widget
    /// under the cursor (the toolkit's contract), and unlike a wheel it is
    /// direct manipulation, so the delta is forwarded with the platform's own
    /// sign, never flipped. Returns `true` if that widget consumed it.
    ///
    /// This is the NATIVE trackpad axis, unrelated to the held-button drag latch
    /// [`drag_pan_in_flight`](Self::drag_pan_in_flight) reports on: the two
    /// never interact, and R1434 renamed the latch's type to `DragPan` so the
    /// names say so.
    ///
    /// No-op (`false`) under the same router-state guards the sibling gestures
    /// check: no stored cursor for `id`, no retained paint scene, or no hover
    /// target covering the cursor.
    pub fn pan_gesture(
        &mut self,
        id: PointerId,
        delta_x: f32,
        delta_y: f32,
        phase: GesturePhase,
        modifiers: Modifiers,
        state_scene: &mut Scene,
    ) -> bool {
        let Some((paint, target_tag, cursor)) = self.hovered_gesture_target(id) else {
            return false;
        };
        offer_pan_to_external(
            paint,
            state_scene,
            &target_tag,
            cursor,
            (delta_x, delta_y),
            phase,
            modifiers,
        )
    }

    /// R1435 §5.35 §5.15 — offer a native SMART-ZOOM gesture to the widget
    /// under pointer `id`'s cursor: the toolkit `SmartZoomNativeGesture` / winit `DoubleTapGesture` peer. The
    /// family's phase-less member — one completed toggle, no arc to bracket
    /// and no delta to accumulate — so the cursor anchor and the modifiers are
    /// the whole offer. Same offer-to-hovered-only delivery as its siblings
    /// (no `Scene::Scroll` fallback). Returns `true` if the widget consumed it.
    ///
    /// Distinct from the pointer double-click path (two press/release cycles
    /// through the router's pointer-button arms): that is a button event with a
    /// click count, this is a buttonless trackpad gesture.
    ///
    /// No-op (`false`) under the same router-state guards the sibling gestures
    /// check: no stored cursor for `id`, no retained paint scene, or no hover
    /// target covering the cursor.
    pub fn smart_zoom_gesture(
        &mut self,
        id: PointerId,
        modifiers: Modifiers,
        state_scene: &mut Scene,
    ) -> bool {
        let Some((paint, target_tag, cursor)) = self.hovered_gesture_target(id) else {
            return false;
        };
        offer_smart_zoom_to_external(paint, state_scene, &target_tag, cursor, modifiers)
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
            // R1619 — the cancel still reports what was held when the gesture
            // was revoked; the set is cleared immediately after, because a
            // cancelled gesture's release is exactly the one that never comes.
            dispatch_send(
                state_scene,
                &tag,
                PointerWireEvent::Cancel.as_wire_name(),
                self.held_modifiers,
                self.held_buttons(id),
            );
        }
        self.held_buttons.remove(&id);
        // R937.1 §5.51 — a cancelled gesture revokes an in-flight drag this
        // pointer started: remove the session (so the next `cursor_moved` can
        // never route to a dead `update_drag` — `cursor_moved` checks
        // `drag_sessions` first) and tell the source to DISCARD it via
        // `drag_cancel` (clear its preview / arm WITHOUT applying the move — a
        // cancel is "the drag never happened", unlike the `pointer_up` drop which
        // commits). The session-review caught this: pre-R937.1 the session +
        // the source's reactive drop-preview both leaked, leaving a ghost
        // insertion line and a stale arm after an OS gesture revoke.
        if let Some(mut session) = self.drag_sessions.remove(&id) {
            // R1734 §5.51 — the TARGET's preview is revoked by the same cancel.
            // A target that is left holding a highlight after the gesture is
            // exactly the ghost this block was written for, one surface over.
            cancel_drop_target(state_scene, &mut session.drop_target);
            let (primary, _) = split_subindex(&session.source_tag);
            if let Some(external) = state_scene.find_external_with_tag_mut(primary) {
                external.handle.drag_cancel(&session.payload);
            }
        }
        // R881 §5.35 — revoke any in-flight middle gesture: a cancelled
        // press is "never happened", so the trailing OS `Released` (if
        // one still arrives) resolves to `PanRelease::NoPress` and
        // neither pastes nor pans (the R880.1 mandatory-cancel-arm
        // discipline). Pan deltas already applied stay applied — a pan
        // is incremental scrolling, not a journaled transaction.
        self.drag_pans.remove(&id);
        // R1418 §5.35 — a cancelled gesture also revokes a raw sink's implicit
        // grab: the abort is "never happened", so the grab must not outlive it
        // and strand a held-button count (the sink saw its `PointerCancel` via
        // the send wire above and reset its own edge state).
        let had_raw_grab = self.raw_grabs.remove(&id).is_some();
        if self.captured_targets.remove(&id).is_some() || had_raw_grab {
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
        id: PointerId,
        cursor_x: f64,
        cursor_y: f64,
    ) {
        let Some(paint) = self.last_paint_scene.as_ref() else {
            return;
        };
        let (primary, _) = split_subindex(target_tag);
        let Some(external) = state_scene.find_external_with_tag_mut(primary) else {
            return;
        };
        let Some(reading) =
            capture_rel_coords(paint, external, primary, target_tag, cursor_x, cursor_y)
        else {
            return;
        };
        external.handle.pointer_move(reading);
        // R1430 §5.35 — every non-positional axis (pressure / tilt / twist /
        // tangential / height) travels WITH the move (the W3C `pointermove`
        // model), forwarded as ONE bundle so a new axis is a struct field, not a
        // fresh forward line. Absent → the all-zero default (a plain mouse).
        self.axes
            .get(&id)
            .copied()
            .unwrap_or_default()
            .forward_to(&mut *external.handle);
    }

    /// R1430 §5.35 — the (sub-)tag a standalone axis change delivers to: an
    /// implicit raw grab, else a capture lock, else the hover target when it
    /// opted into hover-move — the SAME order [`cursor_moved`](Self::cursor_moved)
    /// forwards a move through. Shared by every `set_pointer_<axis>` so the
    /// resolution cannot drift per axis (the R1423/R1429 copies this lift folds).
    fn resolve_axis_target(&self, id: PointerId) -> Option<String> {
        self.raw_grab_tag(id)
            .or_else(|| self.captured_targets.get(&id).cloned())
            .or_else(|| {
                self.hover_wants_move
                    .get(&id)
                    .copied()
                    .unwrap_or(false)
                    .then(|| self.hover_targets.get(&id).cloned())
                    .flatten()
            })
    }

    /// R1430 §5.35 — deliver `id`'s current axis bundle to its resolved target at
    /// once (the standalone-change path every `set_pointer_<axis>` shares). A
    /// no-op when nothing under the pointer reads it.
    fn deliver_axes(&self, id: PointerId, state_scene: &mut Scene) {
        let Some(tag) = self.resolve_axis_target(id) else {
            return;
        };
        let (primary, _) = split_subindex(&tag);
        let axes = self.axes.get(&id).copied().unwrap_or_default();
        if let Some(external) = state_scene.find_external_with_tag_mut(primary) {
            axes.forward_to(&mut *external.handle);
        }
    }

    /// R1423 §5.35 — store `id`'s PRESSURE (W3C `PointerEvent.pressure` / the toolkit `pressure()`), clamped
    /// to `0.0..=1.0`, WITHOUT delivering it. The pen / touch bridge calls this before
    /// the accompanying [`cursor_moved`](Self::cursor_moved), whose forwarded `pointer_move` carries
    /// the new pressure to the surface (pressure travels with position).
    pub fn note_pointer_pressure(&mut self, id: PointerId, pressure: f32) {
        self.axes.entry(id).or_default().pressure = pressure.clamp(0.0, 1.0);
    }

    /// R1423 §5.35 — store `id`'s pressure AND deliver the axis bundle to the
    /// pointer's current move-target immediately (a pen pressing harder in place,
    /// the `scene/pointer_pressure` RPC path), so the change reaches the surface
    /// at once, not only on the next move. The stored value also rides every
    /// subsequent `pointer_move`.
    pub fn set_pointer_pressure(&mut self, id: PointerId, pressure: f32, state_scene: &mut Scene) {
        self.note_pointer_pressure(id, pressure);
        self.deliver_axes(id, state_scene);
    }

    /// R1429 §5.35 — store `id`'s TILT (W3C `PointerEvent.tiltX/tiltY` / the toolkit `xTilt/yTilt`), each axis
    /// clamped to `-90.0..=90.0` degrees, WITHOUT delivering it. Mirrors
    /// [`note_pointer_pressure`](Self::note_pointer_pressure).
    pub fn note_pointer_tilt(&mut self, id: PointerId, tilt_x: f32, tilt_y: f32) {
        let axes = self.axes.entry(id).or_default();
        axes.tilt_x = tilt_x.clamp(-90.0, 90.0);
        axes.tilt_y = tilt_y.clamp(-90.0, 90.0);
    }

    /// R1429 §5.35 — store `id`'s tilt AND deliver the bundle at once (a pen
    /// leaning in place, the `scene/pointer_tilt` RPC path).
    pub fn set_pointer_tilt(
        &mut self,
        id: PointerId,
        tilt_x: f32,
        tilt_y: f32,
        state_scene: &mut Scene,
    ) {
        self.note_pointer_tilt(id, tilt_x, tilt_y);
        self.deliver_axes(id, state_scene);
    }

    /// R1430 §5.35 — store `id`'s TWIST (W3C `PointerEvent.twist` / the toolkit `rotation()`), the barrel
    /// rotation in degrees, WRAPPED to `0.0..=360.0` (an angle folds rather than
    /// clamps), WITHOUT delivering it.
    pub fn note_pointer_twist(&mut self, id: PointerId, twist: f32) {
        self.axes.entry(id).or_default().twist = twist.rem_euclid(360.0);
    }

    /// R1430 §5.35 — store `id`'s twist AND deliver the bundle at once (a pen
    /// barrel turning in place, the `scene/pointer_twist` RPC path).
    pub fn set_pointer_twist(&mut self, id: PointerId, twist: f32, state_scene: &mut Scene) {
        self.note_pointer_twist(id, twist);
        self.deliver_axes(id, state_scene);
    }

    /// R1430 §5.35 — store `id`'s TANGENTIAL PRESSURE (W3C `PointerEvent.tangentialPressure` / the toolkit
    /// `tangentialPressure()`), the airbrush finger-wheel position clamped to `-1.0..=1.0`, WITHOUT
    /// delivering it.
    pub fn note_pointer_tangential_pressure(&mut self, id: PointerId, tangential: f32) {
        self.axes.entry(id).or_default().tangential = tangential.clamp(-1.0, 1.0);
    }

    /// R1430 §5.35 — store `id`'s tangential pressure AND deliver the bundle at
    /// once (the `scene/pointer_tangential_pressure` RPC path).
    pub fn set_pointer_tangential_pressure(
        &mut self,
        id: PointerId,
        tangential: f32,
        state_scene: &mut Scene,
    ) {
        self.note_pointer_tangential_pressure(id, tangential);
        self.deliver_axes(id, state_scene);
    }

    /// R1430 §5.35 — store `id`'s HEIGHT (the toolkit `z()`), the hover distance
    /// above the surface floored at `0.0` (a distance is non-negative; no W3C
    /// peer), WITHOUT delivering it.
    pub fn note_pointer_height(&mut self, id: PointerId, height: f32) {
        self.axes.entry(id).or_default().height = height.max(0.0);
    }

    /// R1430 §5.35 — store `id`'s height AND deliver the bundle at once (the
    /// `scene/pointer_height` RPC path).
    pub fn set_pointer_height(&mut self, id: PointerId, height: f32, state_scene: &mut Scene) {
        self.note_pointer_height(id, height);
        self.deliver_axes(id, state_scene);
    }

    /// R1431 §5.35 — store `id`'s device KIND (W3C `PointerEvent.pointerType` / the toolkit `pointerType()`)
    /// WITHOUT delivering it.
    pub fn note_pointer_kind(&mut self, id: PointerId, kind: pinion_core::PointerKind) {
        self.axes.entry(id).or_default().kind = kind;
    }

    /// R1431 §5.35 — store `id`'s device kind AND deliver the bundle at once (the
    /// `scene/pointer_type` RPC path — a stylus flipping to its eraser end).
    pub fn set_pointer_kind(
        &mut self,
        id: PointerId,
        kind: pinion_core::PointerKind,
        state_scene: &mut Scene,
    ) {
        self.note_pointer_kind(id, kind);
        self.deliver_axes(id, state_scene);
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
        let own_over = self.resolve_drag_own_over(&payload, &source, x, y, state_scene);
        let own_is_self_drop = own_over
            .as_ref()
            .is_some_and(|p| self.own_drop_is_self(p, &source));
        let (over, over_window) = resolve_drag_targets(own_over, own_is_self_drop, cross_window);
        // R1734 §5.51 — the TARGET half, before the source's own update. The
        // order matters for a surface that is both (the analysis shell's
        // palette and board live in one composite): the target's judgement of
        // this sample is what the source's `drag_to_at` may then paint, so the
        // verdict has to exist before the painter reads it.
        let modifiers = self.held_modifiers;
        // R1735 — the target's judgement of THIS sample, kept rather than
        // dropped: it is what the source is told below, so the preview a source
        // paints and the outcome a release commits are one value.
        let mut standing = DropStanding::Nowhere;
        if let Some(session) = self.drag_sessions.get_mut(&id) {
            let payload = session.payload.clone();
            standing = offer_drag_to_target(
                state_scene,
                &mut session.drop_target,
                &payload,
                over.as_ref(),
                (x, y),
                modifiers,
            );
        }
        let (primary, _) = split_subindex(&source);
        if let Some(external) = state_scene.find_external_with_tag_mut(primary) {
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
                    standing,
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
    ///
    /// R1497 — takes the state scene because the deepest-tag FALLBACK below now
    /// resolves through [`resolve_pointer_tag`]: one address, every method
    /// ([[r1484-one-address-every-method]]). A drop over a section's own label
    /// must land on the section, exactly as a press there does. The opted-in
    /// (`LayoutStyle::drop_target`) leg already skipped decoration — it demands a
    /// marker the label does not carry — which is why drops kept working while
    /// presses on the same pixel did not.
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
        drop_point_at(paint, x, y)
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
        // (R1205) Measure the band against the DOCK AREA — the laid-out rect of the
        // dock walker's `DOCK_SURFACE_TAG` wrapper (the whole workspace subtree). So
        // the top full-span band sits at the dock's top edge (below a client-side
        // chrome strip / toolbar / menu), not up in the chrome. The same-window peer
        // of R1202's cross-window preview, reading the SAME dock-area SSOT — both
        // agree on where the dock area is, and it tracks a toolbar the retired
        // chrome-height scalar was blind to.
        //
        // (R1322 §5.51) NO DOCK AREA → NO OUTER DOCK. This reads the tag directly
        // instead of `Scene::dock_surface_rect`, whose window-rect FALLBACK made every
        // window an outer-dock target — including a torn-off panel's own floating
        // window, which hosts no dock at all. The consequence was a silent regression
        // of the R1124 live floater→main redock: dragging a floater by its header put
        // the cursor inside the floater's OWN (fallback) edge band, so the router
        // resolved an own-window `OUTER_DOCK_ZONE_TAG` drop point; that sentinel is not
        // the dragged panel's own subtree, so `own_drop_is_self` said false,
        // `resolve_drag_targets` took the own-window-first arm, and the cross-window
        // redock (which needs `over_window: Some`) never fired — the gesture degraded to
        // a bare `window_move`. It is the exact bug class R1124 fixed (an own-window hit
        // masking the cross-window redock), reintroduced by R1167's NEW synthetic
        // own-window target, and R1203's proportional band made it near-certain (most of
        // a 420x320 floater is within its own band). A window with no dock area cannot
        // receive a dock, so it must not synthesize the zone in the first place —
        // fixing the CLASS, not just the floater instance.
        let root = paint.rect_for_tag_absolute(pinion_core::external::DOCK_SURFACE_TAG)?;
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
    ///
    /// (R1348 §5.51 PR-57) The claim is VETOABLE: the band is offered to the drag
    /// SOURCE ([`External::accepts_outer_dock`](pinion_core::external::External::accepts_outer_dock))
    /// before it is claimed, and a refusal falls through to the plain hit-test —
    /// the same path the band's INTERIOR already takes, so a vetoed perimeter
    /// behaves exactly like any other interior pixel (no new concept). Only the
    /// source knows whether a perimeter drop would reach anything (for a dock,
    /// whether an outer band at that edge is redundant against the live topology);
    /// the router holds geometry, not topology, and `pinion-widget-paint` is this
    /// crate's SIBLING, so the question travels through the `External` contract
    /// rather than a dependency this crate cannot have.
    ///
    /// Pre-R1348 the claim was UNCONDITIONAL and only the OUTCOME was suppressed
    /// (R1201's redundancy check inside `resolve_drop_checked`), so a redundant
    /// perimeter stayed claimed while resolving to a stay-put `SnapBack`: it
    /// previewed nothing, did nothing, and — because the sentinel replaced the
    /// hit-test — made the split bands of the panel BENEATH it unreachable. That
    /// dead strip is what this veto retires.
    ///
    /// WHAT THE FALL-THROUGH RESOLVES IS NOT GUARANTEED TO BE A DOCK. "The
    /// perimeter is just interior" is the whole rule, and the interior's outcome
    /// over a non-panel (a splitter gutter, a tab-strip background) or a dead-zone
    /// ring is `DropResolution::Float` — a TEAR-OFF for a floatable panel (R1158,
    /// deliberate). So a vetoed band inherits that too, where pre-R1348 it was
    /// inert. This is not a new class: measured on the R1348 demo's 2-slot shape,
    /// a cursor 40px in (INTERIOR, outside the band) over the same tab-strip
    /// background floats identically — the band now simply agrees with the pixel
    /// next to it. Suppressing only the band's float would restore the very
    /// perimeter-vs-interior asymmetry this round removes, so the float rule is
    /// R1158's to revisit, not this one's.
    ///
    /// The resolve-side `outer_redundant` arm in `resolve_drop_checked` STAYS, but
    /// NOT — as an earlier draft of this comment claimed — as "the cross-window
    /// path's guard": that path resolves through `resolve_drop`, which hardwires an
    /// always-`false` predicate, so the arm is inert there and guards nothing. Its
    /// real live role is the fallback for the case [`source_accepts_outer_dock`]
    /// cannot ask — an unresolvable source tag ACCEPTS (`is_none_or`), and the
    /// resolve-side check still suppresses the outcome — plus defence in depth for
    /// a third-party `External` that leaves `accepts_outer_dock` at its `true`
    /// default. (The false claim was inherited verbatim from `resolve_drop`'s
    /// docstring: a floater's panel is NOT generally absent from the topology —
    /// `FloatPolicy::Placeholder` is the DEFAULT and KEEPS the leaf, measured.)
    ///
    /// ★KNOWN GAP (carried, R1348): the CROSS-window perimeter
    /// ([`resolve_outer_dock_zone`]) is NOT vetoable and has the SAME claim/outcome
    /// gap — a floater redocking onto a 2-slot host previews a full-span band that
    /// `dock_panel_outer`'s redundancy no-op then refuses. Same bug class, sibling
    /// path, out of scope here (this round answers the same-window report). It needs
    /// its own answer because the redundancy question there belongs to the TARGET
    /// window's topology, not the source's — a target-side hook, which the
    /// `External::begin_drag` contract already anticipates.
    fn resolve_drag_own_over(
        &self,
        payload: &DragPayload,
        source_tag: &str,
        x: f64,
        y: f64,
        state_scene: &mut Scene,
    ) -> Option<DropPoint> {
        if payload.kind == DOCK_PANEL_DRAG_KIND {
            if let Some(outer) = self.resolve_own_outer_dock(x, y) {
                if source_accepts_outer_dock(state_scene, source_tag, payload, &outer) {
                    return Some(outer);
                }
                // Vetoed → fall through: the perimeter is just interior here.
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
    /// the prior `target == panel_id` self-drop rejection to the whole
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
    /// [`External::wants_pointer_capture`](pinion_core::External::wants_pointer_capture) is queried in the same
    /// pass and cached in `hover_wants_capture` so the next
    /// [`pointer_down`](InputRouter::pointer_down) reads a bit instead of re-walking the
    /// state-scene tree.
    fn refresh_hover(&mut self, id: PointerId, state_scene: &mut Scene) {
        let now = match (self.cursors.get(&id), &self.last_paint_scene) {
            // R1497 — the hover target is the deepest node that can RECEIVE the
            // event, so `pointer_down` cannot arm a tag `dispatch_send` will
            // then drop, and `hover_wants_capture` is read off the widget that
            // owns the region rather than off decoration painted over it.
            (Some(&(x, y)), Some(scene)) => resolve_pointer_tag(scene, x, y),
            _ => None,
        };
        let prev = self.hover_targets.get(&id).cloned();
        if prev == now {
            return;
        }
        // R1619 — read once: the crossing's leave and enter report the SAME
        // context, because a crossing is neither a button edge nor a key edge.
        let (modifiers, buttons) = (self.held_modifiers, self.held_buttons(id));
        if let Some(prev_tag) = prev {
            self.hover_targets.remove(&id);
            self.hover_wants_capture.remove(&id);
            self.hover_wants_move.remove(&id);
            dispatch_send(
                state_scene,
                &prev_tag,
                PointerWireEvent::Leave.as_wire_name(),
                modifiers,
                buttons,
            );
        }
        if let Some(target) = now {
            self.hover_targets.insert(id, target.clone());
            let wants = widget_wants_capture(state_scene, &target);
            self.hover_wants_capture.insert(id, wants);
            // R1405 — cache the hover-move opt-in once, on the Enter.
            self.hover_wants_move
                .insert(id, widget_wants_hover_move(state_scene, &target));
            dispatch_send(
                state_scene,
                &target,
                PointerWireEvent::Enter.as_wire_name(),
                modifiers,
                buttons,
            );
        }
    }
}

/// R1499 §5.35 §5.51 §2 #2 — hit-test `paint_scene` at `(x, y)` and return the
/// tag of the deepest node under the pointer. `None` when no node in the hit
/// path carries a tag (the cursor is over a fully untagged region — usually the
/// background).
///
/// The walk is deepest-first, sharing [`Scene::hit_test`]'s single descent the
/// way [`Scene::cursor_hint_at`] does: one hit-test, then
/// [`Scene::lookup_path_ref`] over the returned path.
///
/// ## Decoration declares itself, it is not inferred
///
/// A tagged decorative child — a header section's own label, an icon inside a
/// row — must not become the pointer target: `pointer_down` would dispatch to
/// it, `dispatch_send` would find no `External` for its primary half, and
/// the event would be **dropped silently**. Measured on `hello-column-reorder`
/// (R1497): `scene/click` on `colhdr#3` / `colhdr#4` was lost 100% of the time
/// while `#0` / `#1` / `#2` worked, and the discriminator was exactly whether the
/// cell's rect CENTRE — the point `scene/click {path}` presses — fell inside that
/// section's `colhdr_label#<n>` text rect. A centred label makes the most obvious
/// click point the one that cannot work.
///
/// The cure is the declaration the decoration itself carries:
/// [`LayoutStyle::pointer_transparent`](pinion_core::style::LayoutStyle::pointer_transparent) (R705), the
/// toolkit's `WA_TransparentForMouseEvents` and CSS's `pointer-events: none`. [`Scene::hit_test`] already skips such a node, so it never
/// reaches this walk and the widget beneath it — sibling or ancestor — is hit
/// exactly as CSS says.
///
/// **R1499 — this must not be inferred instead.** R1497 tried to derive it here,
/// answering the deepest tag whose primary resolves to an `External` and falling
/// back to the deepest tag when none did, on the reasoning that "is there an
/// `External` behind this tag" already answers the question. It does not, and the
/// justification was factually wrong: window chrome was said to be safe because
/// the controls and the eight resize regions "are injected as top-level SIBLINGS
/// of the content, so no `External` is ever an ancestor of one". But
/// `pinion_overlay`'s `wrap_into_container` returns an existing `Scene::Container`
/// unchanged, so when the app's root view IS that container and carries the
/// widget's tag — the chromeless / content-header floater shape — the regions
/// become CHILDREN of a tagged, `External`-backed node, as does a dock header's
/// own close control. Three `pinion-shell` window-chrome tests went red.
///
/// The two cases are structurally identical — `colhdr_label#3` inside `colhdr` (the ancestor
/// should win) and `window-resize#north` inside `r1121-content` (the descendant should win) — so no rule
/// reading the tree can tell them apart. Which one is decoration is a fact
/// only the paint site knows, which is why the toolkit and CSS both make it a
/// declaration and why this walk asks for none.
/// ★ R1724 — `pub` so a gate can ask the ROUTER what a press addresses instead
/// of writing the walk out again.
///
/// The test that found the defect below re-implemented this in four lines and
/// therefore did not see the repair — the R47 class, in the smallest possible
/// form. A hit test spelled twice is two hit tests.
pub fn resolve_pointer_tag(paint_scene: &Scene, x: f64, y: f64) -> Option<String> {
    let xu = floor_clamp_u32(x);
    let yu = floor_clamp_u32(y);
    let hit = paint_scene.hit_test(xu, yu)?;
    // Walk segments deepest-first: the longer the prefix, the deeper
    // the ancestor. The root (empty prefix) is the last fallback.
    for k in (0..=hit.segments.len()).rev() {
        let Some(scene) = paint_scene.lookup_path_ref(&hit.segments[..k]) else {
            continue;
        };
        if let Some(tag) = tag_beneath_a_viewport(scene) {
            return Some(tag.to_string());
        }
    }
    None
}

/// ★★★★★ R1724 §5.35 §5.45 — **a scroll viewport is a clip, not a target.**
///
/// [`Scene::lookup_path_ref`] is path-transparent at a `Scroll` — its own
/// comment says so, *"a wrapper, not a path-bearing layer"* — but only while
/// segments remain: handed an empty remainder it stops AT the scroll and
/// returns the wrapper. That is the case a press lands in whenever nothing
/// deeper is hittable, which is the normal state of a screen that makes its
/// own paint pointer-transparent and resolves every gesture at its root
/// (R1655, and three screens of this tree do it).
///
/// While the scroll is the WINDOW's own pan the wrapper sits above the paint
/// root and nothing is hurt: no external carries its tag, so the router drops a
/// press it was going to drop anyway. Put a screen inside a region that pans
/// and the wrapper sits between the screen and *its own root* — measured the
/// day the first screen was mounted: a press at the centre of a card the screen
/// painted resolved to `window.pan`, and the whole section was dead to a mouse
/// while its paint, its wire, its accessibility tree and `scene/pointer_reach`
/// were all correct.
///
/// Looking through cannot take a press away from anything: a `Scroll` tag is a
/// scroll ADDRESS (`scene/scroll {path}`), never an `External`'s, so a press
/// that resolved to one was already being forwarded nowhere.
fn tag_beneath_a_viewport(scene: &Scene) -> Option<&str> {
    match scene {
        Scene::Scroll(node) => tag_beneath_a_viewport(&node.content),
        other => other.tag(),
    }
}

/// R1650 §5.35 §2 #7 — a painted tag that the pointer reaches and **nothing can
/// receive**, together with the widget it takes the input away from.
///
/// See [`pointer_reach`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerShadow {
    /// The paint tag the router's own resolution answers at this node.
    pub tag: String,
    /// The node's address, as `scene/snapshot` and `scene/locate` spell it.
    pub path: Vec<String>,
    /// The tag of the nearest ancestor that **does** resolve to an `External`
    /// — the widget that would have received the press had this node not been
    /// painted over it.
    pub shadowed: String,
}

/// R1650 §5.35 §2 #7 — a widget whose centre **no widget answers**: a press
/// at the middle of its own painted rect resolves to a tag nothing receives,
/// so the router drops it.
///
/// Not merely "something else is on top". A row painted over its tree, a cell
/// over its grid, a button over its toolbar — all cover a container's centre,
/// and the press reaching the child is what is supposed to happen. This is the
/// case where it reaches **nothing**.
///
/// See [`pointer_reach`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerUnreachable {
    /// The widget's paint tag.
    pub tag: String,
    /// Its address.
    pub path: Vec<String>,
    /// The tag the router resolves at its centre instead, or `None` when the
    /// centre hits nothing at all (the widget is off-window or zero-area).
    pub blocked_by: Option<String>,
}

/// R1664 §5.35 §2 #7 — one `External` the state scene registers, paired with
/// the painted tag (if any) that routes a press to it.
///
/// The state scene is where a widget *exists*; the paint scene is where it is
/// *addressable*. The router joins them by name, and the join is two string
/// literals in two different functions with nothing checking that they agree.
/// When they do not, every press on that widget is dropped in silence — the
/// failure has no symptom other than a person pressing something and reporting
/// that nothing happened, which is exactly how it was found (R1663's
/// `hello-packet-view`: registered `packet_view`, painted `pv.root`).
///
/// [`PointerReach::deliverable`] already counted the join from the paint side,
/// so a total mismatch reported `0` — a number that reads identically to "this
/// screen has no widgets on it". This row is the same fact from the state side,
/// where the thing that needs repairing has a **name**.
///
/// See [`pointer_reach`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalRouting {
    /// The tag the `External` is registered under in the state scene.
    pub tag: String,
    /// The shallowest painted tag whose primary half resolves here — the region
    /// a press lands in — or `None` when nothing painted routes here at all.
    ///
    /// `None` is not by itself a defect: a data-only `External` (R1663's
    /// `pv.map`, a model with no surface) is registered for `scene/query` and
    /// is never meant to receive a press. It becomes one only when *no*
    /// external on the screen is routed — see
    /// [`PointerReach::is_dead_to_a_pointer`].
    pub routed_by: Option<String>,
}

/// R1650 §5.35 §2 #7 — the reachability of a painted surface: how much of it a
/// real pointer can drive, and what is stealing the rest.
///
/// See [`pointer_reach`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PointerReach {
    /// Painted tags whose primary half resolves to an `External` in the state
    /// scene — the nodes a press arrives at, *if* one can land on them.
    pub deliverable: usize,
    /// Painted tags that resolve to nothing **and take nothing away**: no
    /// ancestor of theirs is a widget either, so the region was never live.
    /// The decorative case, reported as a count so the numbers below sum to
    /// the tags examined rather than leaving a silent remainder.
    pub inert: usize,
    /// Every tag that swallows input a widget above it would have received.
    ///
    /// The **census**, not the verdict: a tagged container is an ancestor of
    /// the real targets inside it and only swallows presses in its own
    /// uncovered gaps, which a grid or a scroll region may legitimately do
    /// nothing with. Read [`unreachable`](Self::unreachable) for the defects.
    pub shadows: Vec<PointerShadow>,
    /// The widgets whose own centre no widget answers — the point
    /// `scene/click {path}` presses, and the point R1497 measured as the exact
    /// discriminator between the header cells that worked and the ones that
    /// silently did not.
    ///
    /// This is the gate-worthy half. A shadow says *some* input is being
    /// intercepted, which a container legitimately does in its own gaps; an
    /// entry here says a press lands and is dropped.
    pub unreachable: Vec<PointerUnreachable>,
    /// R1664 — every **tagged** `External` the state scene registers, and the
    /// painted tag that routes a press to it. Declaration order, deduplicated.
    ///
    /// The census the counts above could not give: `deliverable` and `inert`
    /// are read off the *paint* tree, so when the join fails completely they
    /// report `0` and a number, and neither of those names the widget that
    /// cannot be reached. This list does.
    ///
    /// **Tagged**, and that is the definition rather than an oversight: the
    /// router's only handle on a widget is the tag it is registered under, so
    /// an untagged `External` is unaddressable by construction — not by the
    /// router, not by `scene/click {path}`, not by `send`. Listing one would
    /// report a widget as unrouted when nothing was ever meant to route to it,
    /// and `hello-endpoint-identity` is that case in this tree: a display-only
    /// binding (PR-51's `primary_surface` opt-out) whose roster is therefore
    /// empty and whose screen is correctly not called dead.
    pub externals: Vec<ExternalRouting>,
}

impl PointerReach {
    /// R1664 §5.35 — the screen registers widgets and **not one of them** can
    /// receive a press: every point in the window is dropped by the router.
    ///
    /// # Why this is derived and not a field
    ///
    /// A stored verdict is a second place the answer lives, free to disagree
    /// with the census it summarises. This one is a function of
    /// [`externals`](Self::externals) alone, so it cannot drift from it.
    ///
    /// # Why "all", and not "any"
    ///
    /// A single unrouted `External` is ordinary: a model registered for
    /// `scene/query` with no surface of its own is unroutable by design, and
    /// this tree has them. What is never ordinary is a screen where the router
    /// resolves *nothing anywhere* while widgets are registered and a surface
    /// is painted — that screen is dead to a mouse, and the only report it used
    /// to produce was `deliverable: 0`, which is byte-identical to the answer a
    /// screen with no widgets gives.
    ///
    /// Measured, which is why it is here rather than in a comment: R1663 shipped
    /// `hello-packet-view` with 185 painted tags, 30 of them inert, `deliverable
    /// = 0`, an empty `shadows` list and an empty `unreachable` list — a report
    /// that reads as *clean* in every field — and a person pressing the window
    /// got nothing, anywhere.
    ///
    /// An empty [`externals`](Self::externals) is **not** dead: a screen with no
    /// widgets registered has nothing to fail to reach, and saying otherwise
    /// would make "no widgets yet" indistinguishable from "the wiring broke",
    /// which is the mistake this method exists to undo.
    #[must_use]
    pub fn is_dead_to_a_pointer(&self) -> bool {
        !self.externals.is_empty() && self.externals.iter().all(|e| e.routed_by.is_none())
    }
}

/// R1650 §5.35 §2 #7 — ask a painted surface which of its tags a pointer can
/// actually drive, by replaying the router's own tag-resolution rule over the
/// whole tree instead of at one cursor position.
///
/// # The failure this exists to make visible
///
/// Dispatch is two facts, and only one of them is normally asserted. A handler
/// can be **correct** — driven by `scene/click`, `scene/invoke` or a synthetic
/// `send`, every assertion passing — while it is not **wired**, because those
/// wire verbs call the widget's handler directly and the router does not. The
/// router resolves the deepest *tagged* node under the cursor and looks the
/// primary half of that tag up as an `External` in the state scene; when the
/// lookup fails it returns, silently. So a tagged decorative child painted over
/// a widget makes that widget dead to a real mouse and leaves every wire-driven
/// test green.
///
/// Measured twice in this tree before anything checked it:
///
/// * R1497 — `hello-column-reorder` lost 100% of the clicks on `colhdr#3` and
///   `colhdr#4` while `#0`–`#2` worked, the discriminator being whether the
///   press point fell inside that header's own `colhdr_label#<n>` text rect;
/// * R1649.1 — the analyzer shell tagged every card, panel and palette row for
///   addressing, so the root `External` was shadowed everywhere and **the whole
///   window was dead to a real mouse** while a 118-assertion demo passed.
///
/// Both are one shape, and this report names it: `shadows` is non-empty exactly
/// when some node swallows input that an `External`-backed ancestor would
/// otherwise have received.
///
/// # Why a shadow is a defect and an inert tag is not
///
/// Tags are addresses, not affordances (R1613) — most of them exist so
/// `scene/snapshot` can name a region, and a chart's mark or a label's text is
/// meant to be inert. The discriminator is therefore not "does this tag resolve
/// to a widget" but **"does an ancestor's widget lose input because of it"**.
/// A decorative tag over dead space costs nothing and is counted as
/// [`inert`](PointerReach::inert); the same tag over a button is a defect and
/// is reported by name.
///
/// # What is deliberately not reported
///
/// * **Pointer-transparent subtrees.** [`Scene::hit_test`] skips such a node
///   and everything under it, so nothing there is reachable and nothing there
///   can shadow — this is the declaration
///   ([`LayoutStyle::pointer_transparent`](pinion_core::style::LayoutStyle::pointer_transparent),
///   the toolkit's `WA_TransparentForMouseEvents`, CSS's `pointer-events:
///   none`) that repairs a shadow, so a repaired surface reports clean.
/// * **Disabled regions.** R1554 made a disabled region deliberately opaque to
///   the pointer: absorbing the press *is* its job, so it is not a defect.
/// * **Occlusion.** A tag hidden behind a later sibling is structurally able to
///   shadow and is reported; deciding it cannot would require a full paint-order
///   coverage analysis, and the wrong-`have` direction is the expensive one
///   (R1602).
///
/// # Past the reference toolkit
///
/// There, every widget is itself an event target, so the analogous silence —
/// a child that accepts a press and does nothing with it — is *unaskable*: the
/// per-widget `WA_TransparentForMouseEvents` attribute is readable one widget
/// at a time and nothing aggregates it, and a widget's willingness to handle a
/// press is a virtual function, not data. Here the whole answer is a pure
/// function of two scenes, so it is a wire read (`scene/pointer_reach`) an
/// agent can take before it decides a screen is operable.
#[must_use]
pub fn pointer_reach(paint_scene: &Scene, state_scene: &Scene) -> PointerReach {
    let mut out = PointerReach::default();
    // Widget-backed painted nodes, kept so each can be asked afterwards
    // whether a press at its own centre still reaches it. Collected during the
    // walk rather than re-walked, and probed after it because the probe
    // hit-tests from the root.
    let mut widgets: Vec<(String, Vec<String>)> = Vec::new();
    paint_scene.for_each_node(&mut |visit| {
        let node = visit.node;
        // A transparent node is skipped by `hit_test`, and so is everything
        // beneath it — the subtree is not reachable, so it cannot shadow.
        if node.is_pointer_transparent()
            || visit.ancestors.iter().any(|a| a.is_pointer_transparent())
        {
            return;
        }
        // R1554 — absorbing the press is what a disabled region is for.
        if node.declares_disabled() {
            return;
        }
        let Some(tag) = node.tag() else {
            return;
        };
        // A `Scroll` and its content share one address, and the router resolves
        // the SCROLL there (`lookup_path_ref` stops at it). Asking the address
        // which node it names keeps this report on the router's side of that
        // collapse rather than inventing a second answer.
        if !std::ptr::eq(
            paint_scene
                .lookup_path_ref(visit.path)
                .unwrap_or(paint_scene),
            node,
        ) {
            return;
        }
        let (primary, _) = split_subindex(tag);
        if state_scene.find_external_with_tag(primary).is_some() {
            out.deliverable += 1;
            widgets.push((tag.to_string(), visit.path.to_vec()));
            return;
        }
        // Nearest widget-backed ancestor, innermost first — the one whose input
        // this node is taking.
        let shadowed = visit.ancestors.iter().rev().find_map(|ancestor| {
            let ancestor_tag = ancestor.tag()?;
            let (ancestor_primary, _) = split_subindex(ancestor_tag);
            state_scene
                .find_external_with_tag(ancestor_primary)
                .map(|_| ancestor_tag.to_string())
        });
        match shadowed {
            Some(shadowed) => out.shadows.push(PointerShadow {
                tag: tag.to_string(),
                path: visit.path.to_vec(),
                shadowed,
            }),
            None => out.inert += 1,
        }
    });
    // The verdict half: ask the router itself, at the one point every path-form
    // press lands on and the one a person aims at.
    for (tag, path) in widgets {
        // WINDOW-absolute, via the one coordinate-translation authority — a
        // node's own `rect` inside a `Scroll` is content-intrinsic, and using
        // it here reported 59 false defects on the first surface that had one.
        // `None` means clipped fully out of its viewport: a virtualised row
        // that is scrolled away is not pressable and is not a defect, so the
        // remedy is to scroll rather than to repair, and it is skipped.
        let Some(rect) = rect_for_tag(paint_scene, &tag) else {
            continue;
        };
        if rect.w == 0 || rect.h == 0 {
            continue;
        }
        let cx = f64::from(rect.x) + f64::from(rect.w) / 2.0;
        let cy = f64::from(rect.y) + f64::from(rect.h) / 2.0;
        let resolved = resolve_pointer_tag(paint_scene, cx, cy);
        // A press at this widget's centre is DELIVERED when whatever the router
        // resolves there is a widget — this one, or another one painted over it.
        //
        // The second case is ordinary nesting and not a defect: a row inside a
        // tree, a cell inside a grid and a button inside a toolbar all cover
        // their container's centre, and the press reaching the row is the
        // point. Requiring the widget to answer for ITSELF was the first draft,
        // and the sweep found it flagging four such containers immediately.
        // What is left is the failure this exists for: the centre resolves to a
        // tag no `External` answers, so the router drops the press in silence.
        let delivered = resolved.as_deref().is_some_and(|hit| {
            state_scene
                .find_external_with_tag(split_subindex(hit).0)
                .is_some()
        });
        if !delivered {
            out.unreachable.push(PointerUnreachable {
                tag,
                path,
                blocked_by: resolved,
            });
        }
    }
    // R1664 — the same join from the other side, so the failure has a NAME.
    //
    // Walked over the state scene (where an `External` exists) and answered
    // against the paint scene (where it becomes addressable), which is the
    // direction the router itself does not go: it starts at a point. Nothing
    // else in this tree asks "is this registered widget on screen at all", and
    // the answer being `no` for every widget at once is the one report that
    // separates a dead screen from an empty one.
    let mut routed: HashMap<&str, Option<String>> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    state_scene.for_each_node(&mut |visit| {
        if let Scene::External(node) = visit.node
            && let Some(tag) = node.tag.as_deref()
            && !routed.contains_key(tag)
        {
            routed.insert(tag, None);
            order.push(tag);
        }
    });
    // Shallowest painted tag wins: a press lands in a region, and the region is
    // the outermost node carrying that address. Taking the deepest would name a
    // leaf whose parent is the thing a repair moves.
    paint_scene.for_each_node(&mut |visit| {
        let Some(tag) = visit.node.tag() else { return };
        let (primary, _) = split_subindex(tag);
        if let Some(slot @ None) = routed.get_mut(primary) {
            *slot = Some(tag.to_string());
        }
    });
    out.externals = order
        .into_iter()
        .map(|tag| ExternalRouting {
            tag: tag.to_string(),
            routed_by: routed.get(tag).cloned().flatten(),
        })
        .collect();
    out
}

/// R1080 §5.51 — hit-test `paint_scene` at `(x, y)` and return the nearest
/// ancestor that opted in as a drop target
/// ([`Scene::is_drop_target`](pinion_core::Scene::is_drop_target)) AND carries
/// a tag. `None` when no node in the hit path is a drop target — then
/// [`InputRouter::resolve_drop_point`] falls back to [`resolve_pointer_tag`]
/// (R1497; pre-R1497 the deepest tagged hit outright), which is the reorder-row
/// case where the drop target is itself the deepest ADDRESSABLE tag, so no
/// marking is needed.
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
/// (R1549 §5.35 §5.38) Ask the widget under `target_tag` for its
/// press-and-hold repeat cadence
/// ([`External::auto_repeat`](pinion_core::external::External::auto_repeat)).
/// Resolves the `ExternalNode` from the PRIMARY half of the (possibly
/// composite) tag exactly like [`widget_begin_drag`] — the widget already
/// recorded which sub-region the press reached, so it answers for that
/// sub-region and the router never parses a composite tag to decide a
/// cadence.
///
/// An unresolvable tag answers `None` (no repeat), which is also the
/// safe direction: a target that left the scene mid-hold stops repeating.
fn widget_auto_repeat(state_scene: &Scene, target_tag: &str) -> Option<AutoRepeat> {
    let (primary, _) = split_subindex(target_tag);
    state_scene
        .find_external_with_tag(primary)?
        .handle
        .auto_repeat()
}

fn widget_begin_drag(state_scene: &mut Scene, target_tag: &str) -> Option<DragPayload> {
    let (primary, _) = split_subindex(target_tag);
    state_scene
        .find_external_with_tag_mut(primary)?
        .handle
        .begin_drag()
}

/// (R1348 §5.51 PR-57) Ask the drag SOURCE at `source_tag` whether it accepts the
/// synthetic outer-dock zone at `point`
/// ([`External::accepts_outer_dock`](pinion_core::external::External::accepts_outer_dock))
/// — the veto [`InputRouter::resolve_drag_own_over`] consults BEFORE claiming the
/// perimeter. Resolves the source's `ExternalNode` from the primary half of its
/// (possibly composite) paint tag, exactly like [`widget_begin_drag`], so the
/// widget that opened the session is the one that answers.
///
/// An unresolvable source (an out-of-sync tag) ACCEPTS: the veto is a refinement
/// of the claim, so when the source cannot be asked the router keeps its
/// pre-R1348 behaviour rather than silently dropping the zone.
fn source_accepts_outer_dock(
    state_scene: &mut Scene,
    source_tag: &str,
    payload: &DragPayload,
    point: &DropPoint,
) -> bool {
    let (primary, _) = split_subindex(source_tag);
    state_scene
        .find_external_with_tag_mut(primary)
        .is_none_or(|node| node.handle.accepts_outer_dock(payload, point))
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
fn dispatch_send(
    state_scene: &mut Scene,
    target_tag: &str,
    event_name: &str,
    modifiers: Modifiers,
    buttons: PointerButtons,
) {
    let (primary, sub_index) = split_subindex(target_tag);
    let Some(external) = state_scene.find_external_with_tag_mut(primary) else {
        return;
    };
    // The bare wire doubles as the SCXML event name, so it only carries the
    // context under the target's opt-in; composite consumers all decode via
    // the `split_send_payload` SSOT, so they take it unconditionally.
    //
    // R1619 — the held-button axis rides the SAME gate as the modifier axis,
    // and deliberately so: the gate exists because a bare payload doubles as
    // an SCXML event name, which any extra segment breaks. That is a property
    // of the payload's shape, not of which context axis filled it, so a second
    // opt-in would be a second answer to one question.
    let (wire_mods, wire_buttons) =
        if sub_index.is_some() || external.handle.wants_bare_send_modifiers() {
            (modifiers, buttons)
        } else {
            (Modifiers::empty(), PointerButtons::empty())
        };
    let Some(intro) = external.handle.introspect_mut() else {
        return;
    };
    let payload = compose_send_payload(sub_index, event_name, wire_mods, wire_buttons);
    let _ = intro.invoke("send", IntrospectValue::Text(payload));
}

// R1497 — the router's private `find_external_by_tag` / `tag_matches` pair is
// retired here. `pinion_core::Scene::find_external_with_tag` had carried a doc
// note since R55.D.5 saying it "mirrors the `find_external_by_tag` private
// helper inside `pinion_runtime::input`", and R1497's addressability predicate
// would have been the THIRD copy of that walk. Every dispatch site now calls the
// core SSOT (`find_external_with_tag` / `_mut`), which also descends
// `Scroll.content` — the one branch the router's copy had missed, so an External
// inside a scroll region is now addressable by the same walk `contains_tag` and
// `hit_test` already used.

/// R51.34 §5.35 — the **window-absolute** post-layout rect of the tagged
/// primitive named by `target_tag`. `None` when no node carries the tag
/// or it is scrolled fully out of view.
///
/// R1098 §5.51 PR-33 — a drop resolved **across windows**: which window owns
/// the drop target the absolute desktop cursor landed on, plus the
/// [`DropPoint`] in that window's own local logical frame.
///
/// The per-window `InputRouter::resolve_drop_point` sees only its own
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
/// `InputRouter::resolve_drop_point`.
///
/// Each `windows` item is `(spec_id, scene, outer_position)` in **logical**
/// pixels (the same coordinate space the router resolves in). The abs cursor
/// is transformed into each window's local frame (`abs - outer`) and the SAME
/// opted-in drop-target hit-test (`resolve_drop_target_tag`) runs against
/// that window's scene. Windows are tried in iteration order; the FIRST that
/// resolves a drop target wins. Ordering — including any source-window
/// exclusion or topmost-first preference a live cross-window redock wants — is
/// the **caller's** concern: this resolver imposes none and simply takes the
/// iterator as given. (The current `scene/cross_window_drop` caller passes the
/// declared window order and does not yet exclude the source window; that
/// refinement lands with the live cross-window redock wiring, not here.)
///
/// Unlike `InputRouter::resolve_drop_point`, this takes NO hover-tag
/// fallback: a cross-window drop must land on a real opted-in drop region (a
/// dock zone), never an arbitrary tagged node in another window.
///
/// R1156 — resolution is two-pass: an OUTER-perimeter pass first
/// (`resolve_outer_dock_zone` — a cursor in the outermost [`OUTER_DOCK_MARGIN`]
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
    // Pass 0 (R1156) — OUTER perimeter: a cursor in the outermost
    // OUTER_DOCK_MARGIN band of a host window's content edge is a FULL-SPAN
    // outer dock (a row/column across EVERY pane), not an inner panel split.
    // Checked FIRST so the perimeter band wins over the inner panel at the
    // very edge — the toolkit ADS / VS outer-guide model. Interior panel
    // boundaries are untouched (they are not near the window perimeter, so
    // they keep their per-panel inner zones).
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
        // (R1322 §5.51) A window with NO DOCK AREA is not an outer-dock host — the
        // cross-window twin of the `resolve_own_outer_dock` rule. Pre-R1322 EVERY window
        // advertised a perimeter, so a torn-off panel's own floating window (which hosts
        // one panel, no dock, and whose panel deliberately opts OUT of being a drop
        // target — `DockPanelStyle::drop_target = false`, the R1118 "a panel cannot dock
        // into a sole floater" rule) still offered one. The synthesized zone BYPASSED
        // that opt-out: a second panel torn off near a floater already on screen
        // redocked INTO it instead of floating (`r1146_release_only_window_move` E), and
        // a floater dragged back over main resolved its OWN band instead of main's
        // redock (`r1124`). The dock area is the SSOT for "this window can receive a
        // dock": no wrapper, no zone.
        //
        // The BAND GEOMETRY deliberately stays the WINDOW rect (below), not the dock-area
        // rect the same-window band uses (R1205). Moving it here would pull the top band
        // below a client-side toolbar, and a floater approaching main's top edge from
        // OUTSIDE would then fall short of the straddle band entirely — losing top-edge
        // cross-window docking for any window with a toolbar (`r1156_outer_dock` pins
        // that gesture). The resolver-vs-preview geometry mismatch that follows from this
        // (the preview paints the band on the dock area) is PRE-EXISTING and out of scope
        // for a regression fix; it needs its own designed answer.
        if scene
            .rect_for_tag_absolute(pinion_core::external::DOCK_SURFACE_TAG)
            .is_none()
        {
            continue;
        }
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
/// `InputRouter::resolve_own_drop_excluding_source` so a floating panel's own
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
/// bounds when lowering `pinion_a11y::AccessNode` into
/// `accesskit::TreeUpdate`; also used by the router's pointer-capture
/// move.
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
    /// R1703 — where this event sits in the gesture, forwarded to the offer so
    /// a stepped consumer can drop its sub-notch remainder when the gesture
    /// ends. Dropped at the winit boundary until this round.
    phase: GesturePhase,
    /// Incoming sub-pixel remainder for the stage-2 integer rounding.
    frac: (f32, f32),
}

/// R881.1 §5.35 §5.49 — ONE home for the wheel-dialect dispatch policy:
/// offer the `External` first (a consuming canvas pans / zooms itself —
/// the W3C listener-before-default model), else apply the delta to the
/// scroll container through the sub-pixel remainder accumulator
/// (integer scroll offsets round per event; the carry keeps a slow
/// high-DPI stream moving — the toolkit's wheel-remainder discipline). Both
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
            args.phase,
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

/// R1433 §5.35 — the shared "offer an event to the widget under the cursor"
/// scaffold behind [`offer_wheel_to_external`] / [`offer_pinch_to_external`] /
/// [`offer_rotation_to_external`]. Resolve the (possibly composite)
/// `target_tag`'s primary `External` in the state scene, normalise `cursor` over
/// the widget's [`CaptureNormalize`] basis via the shared [`capture_rel_coords`]
/// (the SAME basis `pointer_move` uses), and hand the widget-relative
/// `(x_rel, y_rel)` to `offer` — whose closure applies the event-specific
/// payload and calls the matching `External` hook. Returns the widget's consume
/// verdict; `false` when nothing tagged covers the cursor.
///
/// Lifted at R1433 when the third native-input offer (rotation) would have been
/// a third verbatim copy of this resolve-and-normalise boilerplate: the
/// three-site internal-duplication substrate lift, the per-gesture payload left
/// in each caller's closure so the scaffold has one home.
fn offer_to_hovered_external(
    paint: &Scene,
    state_scene: &mut Scene,
    target_tag: &str,
    cursor: (f64, f64),
    offer: impl FnOnce(&mut dyn pinion_core::external::External, f32, f32) -> bool,
) -> bool {
    let (primary, _) = split_subindex(target_tag);
    let Some(external) = state_scene.find_external_with_tag_mut(primary) else {
        return false;
    };
    let Some(reading) =
        capture_rel_coords(paint, external, primary, target_tag, cursor.0, cursor.1)
    else {
        return false;
    };
    // R1727 — the wheel and the two-finger gestures take the FRACTION and are
    // right to: a notch does not resize what it is measured over. Only the
    // captured drag needed the rect, so only it reads the whole reading.
    offer(external.handle.as_mut(), reading.u(), reading.v())
}

/// R877 / R881 §5.35 §5.49 — the wheel-vocabulary `External` offer, stage 1 of
/// [`dispatch_wheel_two_stage`]. Offers the reading to
/// [`External::wheel`](pinion_core::external::External::wheel) on the widget
/// under the cursor via [`offer_to_hovered_external`]. `true` = consumed (no
/// scroll fallback may run).
///
/// ★★ R1703 — **the declaration is the precondition.** A widget is offered the
/// event only when its
/// [`wheel_intent`](pinion_core::external::External::wheel_intent) is `Some`;
/// otherwise the wheel falls straight through to the scroll chain as if the
/// widget were not there. That is what keeps `scene/wheel_intent`'s answer and
/// the behaviour one fact rather than two that can drift — the wire reads the
/// same value this line routes by, so a widget cannot claim a wheel it does not
/// take, nor take one it does not claim.
fn offer_wheel_to_external(
    paint: &Scene,
    state_scene: &mut Scene,
    target_tag: &str,
    cursor: (f64, f64),
    delta: (f32, f32),
    modifiers: Modifiers,
    phase: GesturePhase,
) -> bool {
    offer_to_hovered_external(paint, state_scene, target_tag, cursor, |h, x_rel, y_rel| {
        if h.wheel_intent((x_rel, y_rel)).is_none() {
            return false;
        }
        h.wheel(&WheelReading::new((x_rel, y_rel), delta, phase, modifiers))
    })
}

/// R1703 §5.45 §5.15 — **what a wheel at this window point would do**, answered
/// by the router's own resolution rather than by a second opinion about it.
///
/// The whole value of a published wheel intent is that it is the value the
/// dispatch reads, so this walks the identical path the wheel offer does — the
/// same pointer-tag resolution for the target, the same widget-selected
/// [`CaptureNormalize`] basis for the point — and stops one step short of
/// turning the wheel. `scene/wheel_intent` is its only consumer and holds no
/// geometry of its own for that reason.
///
/// Returns the surface's tag and its answer; the tag comes back even when the
/// answer is `None`, because "this surface is here and declines" and "nothing
/// is here" are different facts and a caller auditing a form needs both.
#[must_use]
pub fn wheel_intent_at(
    paint: &Scene,
    state_scene: &Scene,
    cursor: (f64, f64),
) -> Option<(String, Option<pinion_core::widgets::wheel::WheelIntent>)> {
    let target_tag = resolve_pointer_tag(paint, cursor.0, cursor.1)?;
    let (primary, _) = split_subindex(&target_tag);
    let external = state_scene.find_external_with_tag(primary)?;
    let reading = capture_rel_coords(paint, external, primary, &target_tag, cursor.0, cursor.1)?;
    Some((primary.to_owned(), external.handle.wheel_intent(reading.at)))
}

/// R1432 §5.35 — the External-offer leg for a native PINCH gesture, the
/// [`offer_wheel_to_external`] sibling minus the wheel's `Scene::Scroll`
/// fallback (a native gesture has no default scroll action). Forwards the
/// incremental `magnification` + `phase` to the widget under `cursor` via the
/// shared [`offer_to_hovered_external`]. Returns the widget's consume verdict;
/// `false` when nothing tagged covers the cursor.
fn offer_pinch_to_external(
    paint: &Scene,
    state_scene: &mut Scene,
    target_tag: &str,
    cursor: (f64, f64),
    magnification: f64,
    phase: GesturePhase,
    modifiers: Modifiers,
) -> bool {
    offer_to_hovered_external(paint, state_scene, target_tag, cursor, |h, x_rel, y_rel| {
        h.pinch_gesture(x_rel, y_rel, magnification, phase, modifiers)
    })
}

/// R1433 §5.35 — the External-offer leg for a native ROTATION gesture, the
/// [`offer_pinch_to_external`] sibling with rotation (degrees) in place of scale
/// (both share [`offer_to_hovered_external`], minus any scroll fallback).
/// Forwards the incremental `rotation` + `phase` to the widget under `cursor`.
/// Returns the widget's consume verdict; `false` when nothing tagged covers the
/// cursor.
fn offer_rotation_to_external(
    paint: &Scene,
    state_scene: &mut Scene,
    target_tag: &str,
    cursor: (f64, f64),
    rotation: f64,
    phase: GesturePhase,
    modifiers: Modifiers,
) -> bool {
    offer_to_hovered_external(paint, state_scene, target_tag, cursor, |h, x_rel, y_rel| {
        h.rotation_gesture(x_rel, y_rel, rotation, phase, modifiers)
    })
}

/// R1434 §5.35 — the External-offer leg for a native PAN gesture, the
/// [`offer_pinch_to_external`] / [`offer_rotation_to_external`] sibling with a
/// TWO-dimensional delta in place of a single scalar (all three share
/// [`offer_to_hovered_external`] — the payload is the only difference, which is
/// why it lives in the caller's closure). Forwards the incremental
/// `(delta_x, delta_y)` in logical pixels + `phase` to the widget under
/// `cursor`. Returns the widget's consume verdict; `false` when nothing tagged
/// covers the cursor.
fn offer_pan_to_external(
    paint: &Scene,
    state_scene: &mut Scene,
    target_tag: &str,
    cursor: (f64, f64),
    delta: (f32, f32),
    phase: GesturePhase,
    modifiers: Modifiers,
) -> bool {
    offer_to_hovered_external(paint, state_scene, target_tag, cursor, |h, x_rel, y_rel| {
        h.pan_gesture(x_rel, y_rel, delta.0, delta.1, phase, modifiers)
    })
}

/// R1435 §5.35 — the External-offer leg for a native SMART-ZOOM gesture, the
/// phase-less member of the family: the anchor IS the payload (it selects what
/// to fit), so this offer forwards nothing but the resolved coordinates and the
/// modifiers. The emptiest possible use of [`offer_to_hovered_external`] —
/// where [`offer_pan_to_external`] proved the closure carries a two-axis
/// payload, this proves it carries none.
fn offer_smart_zoom_to_external(
    paint: &Scene,
    state_scene: &mut Scene,
    target_tag: &str,
    cursor: (f64, f64),
    modifiers: Modifiers,
) -> bool {
    offer_to_hovered_external(paint, state_scene, target_tag, cursor, |h, x_rel, y_rel| {
        h.smart_zoom_gesture(x_rel, y_rel, modifiers)
    })
}

fn capture_rel_coords(
    paint: &Scene,
    external: &ExternalNode,
    primary: &str,
    target_tag: &str,
    cursor_x: f64,
    cursor_y: f64,
) -> Option<PointerReading> {
    let norm_tag = match external.handle.capture_normalize() {
        CaptureNormalize::Tag(tag) => tag,
        CaptureNormalize::Primary => primary,
        CaptureNormalize::Target => target_tag,
    };
    let rect = rect_for_tag(paint, norm_tag)?;
    // R1727 §5.35 — the rect travels WITH the fraction. It was resolved here and
    // dropped on the floor, which left every consumer to supply a divisor of its
    // own; a divisor the gesture itself moves is then a defect nobody can see
    // (see [`PointerReading`] for the measurement that opened this).
    #[allow(
        clippy::cast_precision_loss,
        reason = "a logical-pixel rect is small enough to round-trip f32 exactly"
    )]
    Some(PointerReading::new(
        normalize_cursor(rect, cursor_x, cursor_y),
        (rect.w as f32, rect.h as f32),
    ))
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

/// R1405 §5.35 — resolve a bool flag on the state-scene `ExternalNode` whose
/// tag matches `target_tag`'s primary half, reading it via `read`; `false`
/// when no matching node is found (an out-of-sync paint / state tag) so the
/// router never acts on a phantom widget.
///
/// R51.42 §5.35 — composite hit-target paint tags (`"group#i"`) route the
/// lookup through the primary half so the single composite `External` decides
/// once for the whole hit-region; the sub-index is discarded because the flag
/// is a property of the composite handle, not of any one sub-region.
///
/// R1405 lift — the shared resolver behind [`widget_wants_capture`] /
/// [`widget_cancels_on_release_off`] / [`widget_wants_hover_move`], three
/// byte-identical walks (differing only in the flag read) before this became
/// their 3rd consumer (R727).
///
/// R1497 — its own recursive `widget_flag_walk` is gone: it was a fourth copy of
/// [`Scene::find_external_with_tag`]'s walk that differed only in projecting a
/// flag out of the node it found. Finding and reading are now separate, so this
/// resolves through the same SSOT every dispatch site uses and a flag can no
/// longer be read off a different node than the event is delivered to.
fn widget_flag(
    state_scene: &Scene,
    target_tag: &str,
    read: fn(&dyn pinion_core::External) -> bool,
) -> bool {
    let (primary, _) = split_subindex(target_tag);
    state_scene
        .find_external_with_tag(primary)
        .is_some_and(|node| read(&*node.handle))
}

/// R51.34 / R51.42 §5.35 — does the external at `target_tag` opt in to pointer
/// capture ([`External::wants_pointer_capture`](pinion_core::external::External::wants_pointer_capture))?
fn widget_wants_capture(state_scene: &Scene, target_tag: &str) -> bool {
    widget_flag(state_scene, target_tag, |e| e.wants_pointer_capture())
}

/// R1405 §5.35 — does the external at `target_tag` opt in to hover-move
/// forwarding ([`External::wants_hover_move`](pinion_core::external::External::wants_hover_move))?
fn widget_wants_hover_move(state_scene: &Scene, target_tag: &str) -> bool {
    widget_flag(state_scene, target_tag, |e| e.wants_hover_move())
}

/// R741 §5.35 — resolve [`External::cancel_on_release_off_target`](pinion_core::External::cancel_on_release_off_target) for
/// the external registered at `target_tag`'s primary half. `false` when
/// the tag is not found or the widget keeps the drag-commit default.
fn widget_cancels_on_release_off(state_scene: &Scene, target_tag: &str) -> bool {
    widget_flag(state_scene, target_tag, |e| {
        e.cancel_on_release_off_target()
    })
}

/// R1416 §5.35 §5.15 — deliver a raw pointer-button edge to the external at
/// `target_tag` **iff** it owns the raw multi-button stream
/// ([`External::wants_raw_pointer_buttons`](pinion_core::external::External::wants_raw_pointer_buttons)).
/// Resolves the `ExternalNode` from the primary half of a (possibly composite)
/// paint tag — like [`dispatch_send`] — and calls
/// [`External::raw_pointer_button`](pinion_core::external::External::raw_pointer_button)
/// on a single scene walk that both TESTS the opt-in and DELIVERS. Returns
/// `true` when the raw sink consumed the edge (so the caller suppresses the GUI
/// default for it); `false` when the tag resolves to no external, or to one that
/// did not opt in — the standard GUI button semantics then run.
fn dispatch_raw_button(state_scene: &mut Scene, target_tag: &str, event: RawPointerButton) -> bool {
    let (primary, _) = split_subindex(target_tag);
    let Some(external) = state_scene.find_external_with_tag_mut(primary) else {
        return false;
    };
    if !external.handle.wants_raw_pointer_buttons() {
        return false;
    }
    external.handle.raw_pointer_button(event);
    true
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
/// [`WheelDelta`] into a `(dx, dy)`
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
    use pinion_core::external::ReadRefusal;
    use std::sync::{Arc, Mutex};

    use super::*;
    use pinion_core::drop_target::{DropAccept, DropAction, DropClause};
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
            IntrospectSchema::new(const { &[] })
        }
        fn query(&self, _path: &str) -> Result<IntrospectValue, ReadRefusal> {
            Err(ReadRefusal::UnknownPath)
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

    /// R1656 — an External that records every size the framework tells it,
    /// which is the fact `pointer_move`'s fraction is a fraction OF.
    /// The log of sizes a [`SizedExternal`] was told.
    type SizeLog = Arc<Mutex<Vec<(u32, u32)>>>;

    struct SizedExternal {
        sizes: SizeLog,
    }

    impl SizedExternal {
        fn new() -> (Self, SizeLog) {
            let sizes = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    sizes: Arc::clone(&sizes),
                },
                sizes,
            )
        }
    }

    impl std::fmt::Debug for SizedExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SizedExternal").finish()
        }
    }

    impl pinion_core::external::External for SizedExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn on_resize(&mut self, width: u32, height: u32) {
            self.sizes
                .lock()
                .expect("mutex poisoned")
                .push((width, height));
        }
    }

    /// How a `WidgetCore` binding's surface actually appears in the paint
    /// scene: an ordinary container carrying the surface's tag. NOT an
    /// `External` — asserting against one of those is what let the first
    /// version of this wiring pass its tests and do nothing on a real screen.
    fn painted_as(tag: &'static str, rect: Rect) -> Scene {
        let mut c = ContainerNode::new(Vec::new());
        c.rect = rect;
        c.tag = Some(tag.into());
        c.layout = pinion_core::style::LayoutStyle::new();
        Scene::Container(c)
    }

    fn external_at(tag: &'static str, rect: Rect, handle: SizedExternal) -> Scene {
        let mut node = pinion_core::scene::ExternalNode::new(Box::new(handle)).with_tag(tag);
        node.rect = rect;
        Scene::External(node)
    }

    /// ★★★★★ R1736 — **what a surface painted is recorded on the same pass**,
    /// in paint order, in the surface's own frame, and only for its own marks.
    ///
    /// The three properties a hit test reading this store depends on, asserted
    /// apart from any screen: a mark inside a nested surface belongs to the
    /// nested one (or a screen inside a screen would find its host's controls
    /// in its own stack), the coordinates are surface-local (or every lookup
    /// would be off by wherever the surface sits), and a surface that stops
    /// being painted stops answering (or a press would be resolved against a
    /// frame that is no longer on screen).
    #[test]
    fn r1736_a_surface_is_told_what_it_painted_and_only_that() {
        let (handle, _) = SizedExternal::new();
        let mut state = Scene::Container(ContainerNode::new(vec![external_at(
            "screen",
            Rect::new(0, 0, 0, 0),
            handle,
        )]));
        // A surface at an offset, holding two marks — one of them drawn later
        // and overlapping the first.
        let mut surface = ContainerNode::new(vec![
            painted_as("under", Rect::new(120, 140, 80, 40)),
            painted_as("over", Rect::new(150, 150, 20, 20)),
        ]);
        surface.rect = Rect::new(100, 100, 400, 300);
        surface.tag = Some("screen".into());
        surface.layout = pinion_core::style::LayoutStyle::new();
        let paint = Scene::Container(surface);
        let mut known = std::collections::HashMap::new();
        announce_external_sizes(&paint, &mut state, &mut known);

        let marks = pinion_core::painted::painted_regions("screen").expect("painted this frame");
        assert_eq!(
            marks.marks().collect::<Vec<_>>(),
            vec![
                ("under", Rect::new(20, 40, 80, 40)),
                ("over", Rect::new(50, 50, 20, 20)),
            ],
            "surface-local, in paint order, and the surface is not a mark inside itself",
        );
        // The overlap belongs to whichever was drawn last, which is what the
        // reader sees.
        assert_eq!(marks.topmost_at(55, 55), Some("over"));
        assert_eq!(marks.topmost_at(25, 45), Some("under"));
        assert_eq!(marks.topmost_at(300, 200), None);

        // A frame that does not paint it takes the record away, rather than
        // leaving a stale one to answer.
        let empty = painted_as("elsewhere", Rect::new(0, 0, 10, 10));
        announce_external_sizes(&empty, &mut state, &mut known);
        assert!(
            pinion_core::painted::painted_regions("screen").is_none(),
            "a surface that is not on screen answers nothing, not an empty set",
        );
    }

    /// ★ R1656 §5.15 — the widget is told its size, and told again when it
    /// changes.
    ///
    /// Written because the arm it exercises was **declared and never called**.
    /// `External::on_resize` has been one of the eight §5.15 contract items
    /// since the contract was written, and a search of every call site in this
    /// workspace found none — so a consumer implementing it, as the contract
    /// invites, waited forever. The visible cost was a screen whose pointer
    /// coordinates were scaled by opening-size over current-size after a
    /// maximise, because the only other way to learn the basis of
    /// `pointer_move`'s fraction is a reactive hook that does not answer from
    /// inside a pointer callback.
    #[test]
    fn r1656_an_external_is_told_the_size_its_fractions_are_of() {
        let (handle, sizes) = SizedExternal::new();
        let mut state = Scene::Container(ContainerNode::new(vec![external_at(
            "canvas",
            Rect::new(0, 0, 0, 0),
            handle,
        )]));
        let paint = painted_as("canvas", Rect::new(0, 0, 1440, 900));
        let mut known = std::collections::HashMap::new();

        assert_eq!(announce_external_sizes(&paint, &mut state, &mut known), 1);
        assert_eq!(
            *sizes.lock().expect("mutex poisoned"),
            vec![(1440, 900)],
            "the opening size arrives"
        );

        // A still window says nothing: the callback is an event, so a consumer
        // does not have to debounce it.
        assert_eq!(announce_external_sizes(&paint, &mut state, &mut known), 0);
        assert_eq!(sizes.lock().expect("mutex poisoned").len(), 1);

        // A maximise, which is the case a person reported.
        let grown = painted_as("canvas", Rect::new(0, 0, 2494, 1531));
        assert_eq!(announce_external_sizes(&grown, &mut state, &mut known), 1);
        assert_eq!(
            *sizes.lock().expect("mutex poisoned"),
            vec![(1440, 900), (2494, 1531)],
            "and the new one, so a fraction of it can become pixels"
        );
    }

    /// ★★★ R1684.4 §5.15 — **the size is READABLE on every frame, not only on
    /// the frame it changed**, and a surface that leaves the screen stops
    /// answering.
    ///
    /// `on_resize` is an event and is deliberately suppressed when nothing
    /// moved; `surface_size` is a QUESTION and must answer whenever the surface
    /// is painted. Recording it behind the suppression would leave the first
    /// steady frame after a resize correct and every later one silent, which is
    /// the worse failure — it looks like it works.
    ///
    /// Written because its counterfactual PASSED: dropping the forget on an
    /// unpainted surface was caught by nothing at all.
    ///
    /// ★ A third counterfactual — moving the record below the announcement's
    /// debounce — also passed, and that one is NOT a gap: the store is a map
    /// that persists across frames, so a debounced frame changes nothing and a
    /// changed frame is never debounced. The two orderings are equivalent, the
    /// comment that claimed otherwise was corrected rather than defended, and
    /// no test is written for a difference that does not exist.
    #[test]
    fn r1684_4_the_announced_size_is_readable_on_every_frame() {
        let (handle, _sizes) = SizedExternal::new();
        let mut state = Scene::Container(ContainerNode::new(vec![external_at(
            "canvas",
            Rect::new(0, 0, 0, 0),
            handle,
        )]));
        let paint = painted_as("canvas", Rect::new(0, 0, 1440, 900));
        let mut known = std::collections::HashMap::new();
        pinion_core::external::forget_surface_size("canvas");

        assert_eq!(
            pinion_core::external::surface_size("canvas"),
            None,
            "a surface nobody has painted has no size to report"
        );

        announce_external_sizes(&paint, &mut state, &mut known);
        assert_eq!(
            pinion_core::external::surface_size("canvas"),
            Some((1440, 900))
        );

        // A STILL window announces nothing — and the question still answers.
        assert_eq!(announce_external_sizes(&paint, &mut state, &mut known), 0);
        assert_eq!(
            pinion_core::external::surface_size("canvas"),
            Some((1440, 900)),
            "★ the suppression is on the announcement, not on the answer"
        );

        let grown = painted_as("canvas", Rect::new(0, 0, 2494, 1531));
        announce_external_sizes(&grown, &mut state, &mut known);
        assert_eq!(announce_external_sizes(&grown, &mut state, &mut known), 0);
        assert_eq!(
            pinion_core::external::surface_size("canvas"),
            Some((2494, 1531)),
            "★ and it is the size the surface is at now"
        );

        // Painted no more — a torn-off pane, a hidden one.
        let gone = painted_as("elsewhere", Rect::new(0, 0, 100, 100));
        announce_external_sizes(&gone, &mut state, &mut known);
        assert_eq!(
            pinion_core::external::surface_size("canvas"),
            None,
            "★ a surface that is not on screen does not answer a stale size"
        );
    }

    /// ★ The size announced is the WIDGET's rect, not the window's — because
    /// that is the rect `pointer_move` normalises over. A viewport inset by a
    /// toolbar would otherwise be handed a basis it never had.
    /// ★★★★★ R1724 §5.35 §5.45 — **a press resolves through a scroll viewport
    /// to what it is a viewport onto.**
    ///
    /// The gate this crate did not have, and its absence is a finding rather
    /// than an oversight: the repair landed with its only gate in a consuming
    /// example, so a counterfactual restoring the defect was caught by nothing
    /// `cargo test -p pinion-runtime` runs. A gate beside the layer a defect
    /// lives in is a gate the next person editing that layer will not trip.
    ///
    /// The defect it pins: [`Scene::lookup_path_ref`] is path-transparent at a
    /// `Scroll` only while segments remain, and returns the WRAPPER when handed
    /// an empty remainder — which is what a press produces whenever nothing
    /// deeper is hittable. That is the normal state of a screen that makes its
    /// own paint pointer-transparent and resolves every gesture at its root
    /// (R1655; three screens of this tree do it). Harmless while the scroll is
    /// a window's own pan, because nothing is below it; fatal once a pan sits
    /// between a screen and its own root, where it left a whole mounted
    /// section dead to the mouse.
    #[test]
    fn r1724_a_press_resolves_through_a_scroll_to_what_it_shows() {
        use pinion_core::scene::{ScrollAxis, ScrollNode};
        use pinion_core::style::{LayoutStyle, Size};

        let screen = Scene::Container(
            ContainerNode::new(vec![Scene::Container(
                // The screen's own paint, transparent to the pointer the way
                // `hello-node-lab` paints every absolutely-placed node so that
                // one root external resolves every gesture.
                ContainerNode::new(Vec::new())
                    .with_tag("screen.card")
                    .with_layout(
                        LayoutStyle::new()
                            .with_absolute_position(10, 10)
                            .with_size(Size::px(80, 40))
                            .with_pointer_transparent(true),
                    ),
            )])
            .with_tag("screen")
            .with_layout(LayoutStyle::new().with_size(Size::px(400, 300))),
        );
        let panned = Scene::Scroll(
            ScrollNode::new(Rect::new(0, 0, 200, 150), screen)
                .with_axis(ScrollAxis::Both)
                .with_tag("window.pan"),
        );
        let mut paint = Scene::Container(ContainerNode::new(vec![panned]));
        let mut cache = crate::LayoutCache::new();
        crate::compute_layout(&mut paint, &mut cache, 200, 150);

        assert_eq!(
            resolve_pointer_tag(&paint, 50.0, 50.0).as_deref(),
            Some("screen"),
            "the press reaches the screen the viewport shows, not the viewport",
        );
    }

    #[test]
    fn r1656_the_size_announced_is_the_widgets_own_rect() {
        let (handle, sizes) = SizedExternal::new();
        let mut state = Scene::Container(ContainerNode::new(vec![external_at(
            "viewport",
            Rect::new(0, 0, 0, 0),
            handle,
        )]));
        let paint = painted_as("viewport", Rect::new(280, 56, 860, 744));
        let mut known = std::collections::HashMap::new();
        announce_external_sizes(&paint, &mut state, &mut known);
        assert_eq!(
            *sizes.lock().expect("mutex poisoned"),
            vec![(860, 744)],
            "the pane it was laid out into, not the window around it"
        );
    }

    fn read(captures: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        captures.lock().expect("mutex poisoned").clone()
    }

    /// R1416 — raw multi-button pointer-sink fixture. Opts into
    /// [`External::wants_raw_pointer_buttons`] (and, like a realistic pane,
    /// [`External::wants_pointer_capture`] for stray-motion), and records every
    /// [`External::raw_pointer_button`] edge as a compact
    /// `"<button>:<edge>:<mods>"` string so a test can assert the router
    /// delivered the right button, edge, AND modifiers. Does NOT implement the
    /// `send` introspect wire — a raw sink trades the legacy `PointerDown` /
    /// `PointerUp` send stream for the raw one.
    struct RawButtonExternal {
        log: Arc<Mutex<Vec<String>>>,
        moves: MoveLog,
    }

    impl RawButtonExternal {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>, MoveLog) {
            let log = Arc::new(Mutex::new(Vec::new()));
            let moves = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    log: Arc::clone(&log),
                    moves: Arc::clone(&moves),
                },
                log,
                moves,
            )
        }
    }

    impl std::fmt::Debug for RawButtonExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RawButtonExternal").finish()
        }
    }

    impl pinion_core::external::External for RawButtonExternal {
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
        fn wants_hover_move(&self) -> bool {
            true
        }
        fn wants_raw_pointer_buttons(&self) -> bool {
            true
        }
        fn raw_pointer_button(&mut self, event: RawPointerButton) {
            // `button:edge:mods:buttons:clicks` — the 4th segment is the R1418 held set (the toolkit `buttons()`),
            // the 5th is the R1422 click-count (the toolkit `MouseButtonDblClick` = `2`), so a
            // chord test reads the set progression and a double-click test
            // reads the synthesised count.
            self.log.lock().expect("mutex poisoned").push(format!(
                "{}:{}:{}:{}:{}",
                event.button.as_wire_name(),
                event.edge.as_wire_name(),
                event.modifiers.as_wire_token(),
                event.buttons.as_wire_token(),
                event.click_count,
            ));
        }
        fn pointer_move(&mut self, at: PointerReading) {
            self.moves.lock().expect("mutex poisoned").push(at.at);
        }
    }

    /// A raw sink tagged `main_slider` so it reuses [`paint_with_slider`].
    fn state_with_raw_sink() -> (Scene, Arc<Mutex<Vec<String>>>, MoveLog) {
        let (sink, log, moves) = RawButtonExternal::new();
        let scene = Scene::External(ExternalNode::new(Box::new(sink)).with_tag("main_slider"));
        (scene, log, moves)
    }

    #[test]
    fn r1416_raw_sink_receives_all_three_buttons_both_edges_with_modifiers() {
        let mut router = InputRouter::new();
        let (mut state, log, _moves) = state_with_raw_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Cursor over the pane so it is the hover target the raw edges resolve to.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::empty()
        };
        // Every left / middle / right press + release routes to the raw sink,
        // carrying the button, the edge, AND the modifiers on BOTH edges (the
        // press-edge-drops-modifiers gap the legacy send wire had).
        for (button, edge, mods) in [
            (PointerButton::Left, PointerEdge::Down, Modifiers::empty()),
            (PointerButton::Left, PointerEdge::Up, Modifiers::empty()),
            (PointerButton::Middle, PointerEdge::Down, shift),
            (PointerButton::Middle, PointerEdge::Up, shift),
            (PointerButton::Right, PointerEdge::Down, Modifiers::empty()),
            (PointerButton::Right, PointerEdge::Up, Modifiers::empty()),
        ] {
            assert!(
                router.deliver_raw_pointer_button(PointerId::MOUSE, button, edge, mods, &mut state),
                "raw sink must consume {button:?} {edge:?}"
            );
        }
        assert_eq!(
            read(&log),
            vec![
                // Each is a single press/release of a distinct button, so the
                // R1422 click-count (5th segment) is 1 throughout.
                "left:down::l:1".to_string(),
                "left:up:::1".into(),
                "middle:down:s:m:1".into(),
                "middle:up:s::1".into(),
                "right:down::r:1".into(),
                "right:up:::1".into(),
            ],
        );
    }

    #[test]
    fn r1416_non_raw_widget_is_not_a_raw_sink() {
        // A plain button (does NOT opt into the raw stream): the router reports
        // "not consumed" so the shell falls through to the GUI arc, and no raw
        // edge is delivered.
        let mut router = InputRouter::new();
        let (mut state, _captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        assert!(!router.deliver_raw_pointer_button(
            PointerId::MOUSE,
            PointerButton::Right,
            PointerEdge::Down,
            Modifiers::empty(),
            &mut state,
        ));
    }

    #[test]
    fn r1416_no_target_under_cursor_returns_false() {
        // Cursor over the untagged background — no hover / capture target — so a
        // raw button edge resolves to nothing and is not consumed.
        let mut router = InputRouter::new();
        let (mut state, log, _moves) = state_with_raw_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 5.0, 5.0, &mut state); // off the pane
        assert!(!router.deliver_raw_pointer_button(
            PointerId::MOUSE,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
            &mut state,
        ));
        assert!(read(&log).is_empty());
    }

    #[test]
    fn r1416_captured_target_receives_raw_edge_after_cursor_strays() {
        // A raw sink that ALSO holds a capture lock (it opts into
        // wants_pointer_capture) keeps receiving raw button edges after the
        // cursor strays off its rect — the captured-target fallback, so a
        // press-drag-release beyond the pane still pairs on the same widget.
        let mut router = InputRouter::new();
        let (mut state, log, _moves) = state_with_raw_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state); // engages capture
        assert_eq!(
            router.captured_target(PointerId::MOUSE),
            Some("main_slider")
        );
        router.cursor_moved(PointerId::MOUSE, 300.0, 300.0, &mut state); // stray off
        assert!(router.deliver_raw_pointer_button(
            PointerId::MOUSE,
            PointerButton::Left,
            PointerEdge::Up,
            Modifiers::empty(),
            &mut state,
        ));
        assert_eq!(read(&log), vec!["left:up:::1".to_string()]);
    }

    #[test]
    fn r1418_implicit_grab_pairs_a_release_that_strayed_off_the_sink() {
        // The R1418 implicit grab: a press opens a grab, so the matching release
        // pairs on the SAME sink even after the cursor strayed off its rect and
        // over the untagged background — the fix for the "stuck button" an SGR
        // mouse consumer would otherwise see. The off-rect move is forwarded to
        // the grabbed sink too (a fresh position to correlate the edge against).
        let mut router = InputRouter::new();
        let (mut state, log, moves) = state_with_raw_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // over the sink
        assert!(router.deliver_raw_pointer_button(
            PointerId::MOUSE,
            PointerButton::Right,
            PointerEdge::Down,
            Modifiers::empty(),
            &mut state,
        ));
        // Stray far off the sink rect, over the background. Without the grab the
        // release below would resolve to no hover target and be LOST; the move
        // would not reach the sink either.
        router.cursor_moved(PointerId::MOUSE, 5.0, 5.0, &mut state);
        assert!(
            !read_moves(&moves).is_empty(),
            "the off-rect move is forwarded to the grabbed sink"
        );
        assert!(router.deliver_raw_pointer_button(
            PointerId::MOUSE,
            PointerButton::Right,
            PointerEdge::Up,
            Modifiers::empty(),
            &mut state,
        ));
        assert_eq!(
            read(&log),
            vec!["right:down::r:1".to_string(), "right:up:::1".into()],
            "the release paired on the sink despite the cursor being off it"
        );
        // The last button lifted, so the grab released: a fresh edge over the
        // background now resolves to no raw sink.
        assert!(!router.deliver_raw_pointer_button(
            PointerId::MOUSE,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
            &mut state,
        ));
    }

    #[test]
    fn r1418_grab_holds_across_a_multi_button_chord() {
        // The grab releases only when the LAST held button lifts — a press
        // left, press right, release left, release right chord keeps the grab
        // (and its target) through the whole span, the toolkit implicit-grab
        // rule.
        let mut router = InputRouter::new();
        let (mut state, log, _moves) = state_with_raw_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        let press = |r: &mut InputRouter, b, e, s: &mut Scene| {
            r.deliver_raw_pointer_button(PointerId::MOUSE, b, e, Modifiers::empty(), s)
        };
        assert!(press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Down,
            &mut state
        ));
        router.cursor_moved(PointerId::MOUSE, 5.0, 5.0, &mut state); // off the sink
        assert!(press(
            &mut router,
            PointerButton::Right,
            PointerEdge::Down,
            &mut state
        ));
        assert!(press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Up,
            &mut state
        ));
        // Left lifted but right still held: the grab persists, so this edge
        // still reaches the sink.
        assert!(press(
            &mut router,
            PointerButton::Right,
            PointerEdge::Up,
            &mut state
        ));
        // The 4th segment is the R1418 held set (the toolkit `buttons()`): it grows to
        // `{left, right}` = "lr" at the right press, then shrinks as each lifts — the
        // state a single changed `button` cannot express. The 5th is the R1422
        // click-count: every edge here is a distinct-button single, so 1.
        assert_eq!(
            read(&log),
            vec![
                "left:down::l:1".to_string(),
                "right:down::lr:1".into(),
                "left:up::r:1".into(),
                "right:up:::1".into(),
            ],
        );
        // Now every button is up: the grab released.
        assert!(!press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Down,
            &mut state
        ));
    }

    #[test]
    fn r1418_a_lone_release_does_not_open_a_grab() {
        // A release with no matching held press (a button pressed elsewhere,
        // lifted over the sink) delivers but must NOT open a grab — else a
        // subsequent stray would wrongly stay pinned to the sink.
        let mut router = InputRouter::new();
        let (mut state, log, _moves) = state_with_raw_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        assert!(router.deliver_raw_pointer_button(
            PointerId::MOUSE,
            PointerButton::Left,
            PointerEdge::Up,
            Modifiers::empty(),
            &mut state,
        ));
        assert_eq!(read(&log), vec!["left:up:::1".to_string()]);
        // No grab was opened, so a fresh edge over the background is not routed
        // back to the sink.
        router.cursor_moved(PointerId::MOUSE, 5.0, 5.0, &mut state);
        assert!(!router.deliver_raw_pointer_button(
            PointerId::MOUSE,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
            &mut state,
        ));
    }

    /// Press one button and read back the raw sink log — a shared helper for the
    /// R1422 double-click tests. The cursor must already sit over the sink.
    fn raw_press(
        router: &mut InputRouter,
        button: PointerButton,
        edge: PointerEdge,
        state: &mut Scene,
    ) {
        assert!(
            router.deliver_raw_pointer_button(
                PointerId::MOUSE,
                button,
                edge,
                Modifiers::empty(),
                state
            ),
            "the raw sink must consume {button:?} {edge:?}"
        );
    }

    #[test]
    fn r1422_a_second_press_on_the_same_spot_synthesises_a_double_click() {
        // Two presses of the same button, at the same spot, back-to-back (well
        // inside DOUBLE_CLICK_TIME_MS) → the router synthesises click_count =
        // 2 on the SECOND press (the toolkit `MouseButtonDblClick`), and the matching release
        // echoes that 2 (the DOM `detail` model). The first press/release stay 1.
        let mut router = InputRouter::new();
        let (mut state, log, _moves) = state_with_raw_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        raw_press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Down,
            &mut state,
        );
        raw_press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Up,
            &mut state,
        );
        raw_press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Down,
            &mut state,
        );
        raw_press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Up,
            &mut state,
        );
        assert_eq!(
            read(&log),
            vec![
                "left:down::l:1".to_string(),
                "left:up:::1".into(),
                "left:down::l:2".into(),
                "left:up:::2".into(),
            ],
            "the second press is a double-click; its release echoes the 2",
        );
    }

    #[test]
    fn r1422_a_moved_second_press_is_not_a_double_click() {
        // The second press strays beyond DOUBLE_CLICK_DIST_PX (10 px on x vs the
        // 5 px window) → NOT a double: click_count stays 1, the intentional-drag
        // tolerance shared with the send-wire `DoubleClick` path.
        let mut router = InputRouter::new();
        let (mut state, log, _moves) = state_with_raw_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        raw_press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Down,
            &mut state,
        );
        raw_press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Up,
            &mut state,
        );
        router.cursor_moved(PointerId::MOUSE, 110.0, 100.0, &mut state); // strayed 10 px
        raw_press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Down,
            &mut state,
        );
        assert_eq!(
            read(&log),
            vec![
                "left:down::l:1".to_string(),
                "left:up:::1".into(),
                "left:down::l:1".into(),
            ],
            "a press that strayed past the tolerance is a fresh single click",
        );
    }

    #[test]
    fn r1422_a_double_click_is_independent_per_button() {
        // A left double-click must not make a following RIGHT press read as a
        // double — the tracker keys on the button, the toolkit per-button
        // rule.
        let mut router = InputRouter::new();
        let (mut state, log, _moves) = state_with_raw_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        raw_press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Down,
            &mut state,
        );
        raw_press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Up,
            &mut state,
        );
        raw_press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Down,
            &mut state,
        ); // left double
        raw_press(
            &mut router,
            PointerButton::Left,
            PointerEdge::Up,
            &mut state,
        );
        raw_press(
            &mut router,
            PointerButton::Right,
            PointerEdge::Down,
            &mut state,
        ); // fresh button
        assert_eq!(
            read(&log),
            vec![
                "left:down::l:1".to_string(),
                "left:up:::1".into(),
                "left:down::l:2".into(),
                "left:up:::2".into(),
                "right:down::r:1".into(),
            ],
            "the right press is a fresh single despite the left double-click",
        );
    }

    #[test]
    fn r1422_a_third_press_starts_a_fresh_cycle_no_rolling_triple() {
        // A third back-to-back press does NOT read as 3 (or stay 2): once a press
        // reaches 2 the cycle resets, so the third is 1 — the send-wire
        // `DoubleClick`'s "no rolling triple-click" rule on the raw axis.
        let mut router = InputRouter::new();
        let (mut state, log, _moves) = state_with_raw_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        for _ in 0..3 {
            raw_press(
                &mut router,
                PointerButton::Left,
                PointerEdge::Down,
                &mut state,
            );
            raw_press(
                &mut router,
                PointerButton::Left,
                PointerEdge::Up,
                &mut state,
            );
        }
        let downs: Vec<u8> = read(&log)
            .iter()
            .filter(|s| s.contains(":down:"))
            .map(|s| s.rsplit(':').next().unwrap().parse().unwrap())
            .collect();
        assert_eq!(
            downs,
            vec![1, 2, 1],
            "press ordinals cycle 1 → 2 → 1, never a rolling triple",
        );
    }

    /// R1423 — a minimal pressure-recording sink: opts into hover-move (so the
    /// router forwards `pointer_move` and the R1423 pressure to it on a plain
    /// hover) and logs every `pointer_pressure` call.
    struct PressureSink {
        pressures: Arc<Mutex<Vec<f32>>>,
    }

    impl std::fmt::Debug for PressureSink {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PressureSink").finish()
        }
    }

    impl pinion_core::external::External for PressureSink {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn wants_hover_move(&self) -> bool {
            true
        }
        fn pointer_pressure(&mut self, pressure: f32) {
            self.pressures
                .lock()
                .expect("mutex poisoned")
                .push(pressure);
        }
    }

    fn state_with_pressure_sink() -> (Scene, Arc<Mutex<Vec<f32>>>) {
        let pressures = Arc::new(Mutex::new(Vec::new()));
        let sink = PressureSink {
            pressures: Arc::clone(&pressures),
        };
        let scene = Scene::External(ExternalNode::new(Box::new(sink)).with_tag("main_slider"));
        (scene, pressures)
    }

    #[test]
    fn r1423_pressure_rides_a_forwarded_hover_move() {
        // R1423 — a noted pressure rides the next forwarded `pointer_move` (the
        // W3C `pointermove` model: pressure travels with position).
        let mut router = InputRouter::new();
        let (mut state, pressures) = state_with_pressure_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.note_pointer_pressure(PointerId::MOUSE, 0.5);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // enter the sink
        router.cursor_moved(PointerId::MOUSE, 101.0, 100.0, &mut state); // move within it
        assert!(
            pressures
                .lock()
                .expect("mutex poisoned")
                .iter()
                .any(|p| (*p - 0.5).abs() < 1e-6),
            "the noted pressure rode a forwarded move, got {:?}",
            pressures.lock().expect("mutex poisoned")
        );
    }

    #[test]
    fn r1423_set_pressure_delivers_immediately_and_clamps() {
        // R1423 — `set_pointer_pressure` (the RPC path) delivers to the hover
        // target at once — no move required (a pen pressing harder in place) —
        // and clamps out-of-range input.
        let mut router = InputRouter::new();
        let (mut state, pressures) = state_with_pressure_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // hover the sink
        pressures.lock().expect("mutex poisoned").clear(); // drop the move-forwarded 0.0
        router.set_pointer_pressure(PointerId::MOUSE, 5.0, &mut state); // out of range
        assert_eq!(
            pressures.lock().expect("mutex poisoned").as_slice(),
            &[1.0],
            "a standalone pressure change delivers immediately, clamped to 1.0",
        );
    }

    /// R1429 — shared `(tilt_x, tilt_y)` log the [`TiltSink`] appends to and the
    /// test reads. Aliased so the sink field and the fixture signature stay under
    /// clippy's `type_complexity` bar.
    type TiltLog = Arc<Mutex<Vec<(f32, f32)>>>;

    /// R1429 — a minimal tilt-recording sink: opts into hover-move (so the router
    /// forwards `pointer_move` and the R1429 tilt to it on a plain hover) and logs
    /// every `pointer_tilt` call as a `(tilt_x, tilt_y)` pair.
    struct TiltSink {
        tilts: TiltLog,
    }

    impl std::fmt::Debug for TiltSink {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TiltSink").finish()
        }
    }

    impl pinion_core::external::External for TiltSink {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn wants_hover_move(&self) -> bool {
            true
        }
        fn pointer_tilt(&mut self, tilt_x: f32, tilt_y: f32) {
            self.tilts
                .lock()
                .expect("mutex poisoned")
                .push((tilt_x, tilt_y));
        }
    }

    fn state_with_tilt_sink() -> (Scene, TiltLog) {
        let tilts = Arc::new(Mutex::new(Vec::new()));
        let sink = TiltSink {
            tilts: Arc::clone(&tilts),
        };
        let scene = Scene::External(ExternalNode::new(Box::new(sink)).with_tag("main_slider"));
        (scene, tilts)
    }

    #[test]
    fn r1429_tilt_rides_a_forwarded_hover_move() {
        // R1429 — a noted tilt rides the next forwarded `pointer_move` (the W3C
        // `pointermove` model: tilt travels with position).
        let mut router = InputRouter::new();
        let (mut state, tilts) = state_with_tilt_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.note_pointer_tilt(PointerId::MOUSE, 30.0, -45.0);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // enter the sink
        router.cursor_moved(PointerId::MOUSE, 101.0, 100.0, &mut state); // move within it
        assert!(
            tilts
                .lock()
                .expect("mutex poisoned")
                .iter()
                .any(|(x, y)| (*x - 30.0).abs() < 1e-6 && (*y + 45.0).abs() < 1e-6),
            "the noted tilt rode a forwarded move, got {:?}",
            tilts.lock().expect("mutex poisoned")
        );
    }

    #[test]
    fn r1429_set_tilt_delivers_immediately_and_clamps_each_axis() {
        // R1429 — `set_pointer_tilt` (the RPC path) delivers to the hover target
        // at once — no move required (a pen leaning in place) — and clamps each
        // axis to -90..=90 independently.
        let mut router = InputRouter::new();
        let (mut state, tilts) = state_with_tilt_sink();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // hover the sink
        tilts.lock().expect("mutex poisoned").clear(); // drop the move-forwarded (0,0)
        router.set_pointer_tilt(PointerId::MOUSE, 120.0, -120.0, &mut state); // both out of range
        assert_eq!(
            tilts.lock().expect("mutex poisoned").as_slice(),
            &[(90.0, -90.0)],
            "a standalone tilt change delivers immediately, each axis clamped",
        );
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
                Ok(IntrospectValue::Bool(true))
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

    /// R1619 §5.35 — a paint scene of two composite sections of one
    /// `External`, the shape a header band / list body has: `sel#0` on the
    /// left half, `sel#1` on the right. Crossing from one to the other is the
    /// drag-select gesture's inner step.
    fn paint_with_two_sections() -> Scene {
        let section = |tag: &'static str, x: u32| {
            let mut node = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag(tag)
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut node {
                c.rect = Rect::new(x, 0, 100, 40);
            }
            node
        };
        let mut root = Scene::Container(
            ContainerNode::new(vec![section("sel#0", 0), section("sel#1", 100)])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 200, 40);
        }
        root
    }

    /// R1619 §5.35 §5.41 — **the pointer wire says which buttons are held.**
    ///
    /// The W3C `PointerEvent.buttons` axis (the toolkit's single-point event
    /// base carries the same set): a `PointerEnter` delivered while the primary button is *held* is a
    /// different fact from one delivered with nothing held — it is the inner
    /// step of every drag-select (a header band's `sectionEntered`, a list's
    /// range drag, a marquee outside a node canvas). Before this round the two
    /// were **byte-identical on the wire**, so no consumer could tell them
    /// apart and every drag-select was blocked at the substrate.
    ///
    /// Driven twice over the same geometry, changing only whether a press
    /// happened first — the negative control is the round's own claim, since a
    /// stamp that appears unconditionally would prove nothing.
    #[test]
    fn r1619_enter_says_which_buttons_are_held() {
        fn cross_sections(press_first: bool) -> Vec<String> {
            let (capture, captures) = CaptureExternal::new();
            let mut state = Scene::External(ExternalNode::new(Box::new(capture)).with_tag("sel"));
            let mut router = InputRouter::new();
            router.update_paint_scene(paint_with_two_sections(), &mut state);
            router.cursor_moved(PointerId::MOUSE, 50.0, 20.0, &mut state); // over sel#0
            if press_first {
                router.pointer_down(PointerId::MOUSE, &mut state);
            }
            router.cursor_moved(PointerId::MOUSE, 150.0, 20.0, &mut state); // over sel#1
            read(&captures)
        }

        let dragging = cross_sections(true);
        let hovering = cross_sections(false);
        // Both streams end with the crossing: leave 0, enter 1.
        assert_eq!(
            hovering,
            vec![
                "0:PointerEnter".to_string(),
                "0:PointerLeave".into(),
                "1:PointerEnter".into()
            ],
            "a plain hover crossing carries no button context",
        );
        assert_eq!(
            dragging,
            vec![
                "0:PointerEnter".to_string(),
                "0:PointerDown::l".into(),
                "0:PointerLeave::l".into(),
                "1:PointerEnter::l".into(),
            ],
            "every event delivered while the primary button is held says so",
        );
    }

    /// R1619 §5.35 §5.40 — the whole chain, driven through the REAL router
    /// against the REAL selection coordinator: press a row, drag across two
    /// more, release. Nothing here stubs the property under test — the router
    /// stamps the held set because a press happened, and
    /// [`VirtualSelectExternal`](pinion_core::widgets::virtual_select::VirtualSelectExternal)
    /// sweeps because it reads that stamp off the wire.
    ///
    /// The unit tests on either side of this one can each pass while the two
    /// halves disagree about the payload; this is the one that cannot.
    #[test]
    fn r1619_a_drag_across_rows_selects_the_range_end_to_end() {
        use pinion_core::external::IntrospectValue;
        use pinion_core::widgets::virtual_select::VirtualSelectExternal;

        // `"selection"` answers the selection as JSON **runs** (`[[lo, hi]]`)
        // — the wire an RPC
        // client reads, so this asserts against the published form rather than
        // a Rust-only accessor. Compared as its serialized text because
        // `pinion-runtime` does not depend on `serde_json` and should not
        // start to for a test.
        fn rows(state: &Scene) -> String {
            let Scene::External(node) = state else {
                panic!("external root")
            };
            match node
                .handle
                .introspect()
                .expect("introspect")
                .query("selection")
            {
                Ok(IntrospectValue::Json(list)) => list.to_string(),
                other => panic!("selection query answered {other:?}"),
            }
        }
        // Four stacked rows `sel#0..#3`, 40px each, over one coordinator.
        let paint = {
            let row = |i: u32| {
                let mut node = Scene::Container(
                    ContainerNode::new(vec![])
                        .with_tag(format!("sel#{i}"))
                        .with_style(BoxStyle::filled(Color::default())),
                );
                if let Scene::Container(c) = &mut node {
                    c.rect = Rect::new(0, i * 40, 200, 40);
                }
                node
            };
            let mut root = Scene::Container(
                ContainerNode::new((0..4).map(row).collect())
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut root {
                c.rect = Rect::new(0, 0, 200, 160);
            }
            root
        };
        let mut state = Scene::External(
            ExternalNode::new(Box::new(VirtualSelectExternal::new_multi(4))).with_tag("sel"),
        );
        let mut router = InputRouter::new();
        router.update_paint_scene(paint, &mut state);

        // Press row 0, drag through 1 into 2, release over 2.
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(rows(&state), "[]", "the press alone selects nothing");
        assert!(
            router
                .held_buttons(PointerId::MOUSE)
                .contains(pinion_core::PointerButton::Left),
            "the router knows the primary button is down",
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 60.0, &mut state);
        assert_eq!(rows(&state), "[[0,1]]", "crossing into row 1 sweeps");
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        assert_eq!(rows(&state), "[[0,2]]");
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(rows(&state), "[[0,2]]", "the release keeps the range");
        assert!(
            router.held_buttons(PointerId::MOUSE).is_empty(),
            "and the release empties the held set",
        );

        // NEGATIVE CONTROL — the identical cursor path with no press selects
        // nothing. Without it this test would pass against a router that
        // stamped every event unconditionally.
        let mut state = Scene::External(
            ExternalNode::new(Box::new(VirtualSelectExternal::new_multi(4))).with_tag("sel"),
        );
        router.refresh_hover_for_all_active_pointers(&mut state);
        for y in [20.0, 60.0, 100.0] {
            router.cursor_moved(PointerId::MOUSE, 100.0, y, &mut state);
        }
        assert_eq!(rows(&state), "[]", "hovering is not dragging");
    }

    /// R1620 §5.45 §5.35 — a scroll region built over `state`, `h` px tall,
    /// with `rows` 40-px rows of content. The row tags are the DATA index, so
    /// which rows are painted is a function of the offset — the shape a
    /// virtualized list has, and the one that makes "did the sweep reach a row
    /// that was off screen" answerable.
    fn scrolling_rows(
        state: &std::rc::Rc<pinion_core::widgets::scroll::ScrollState>,
        rows: u32,
        h: u32,
    ) -> Scene {
        let row = |i: u32| {
            let mut node = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag(format!("sel#{i}"))
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut node {
                // Content coordinates; the scroll node applies the offset.
                c.rect = Rect::new(0, i * 40, 200, 40);
            }
            node
        };
        let mut content = Scene::Container(ContainerNode::new((0..rows).map(row).collect()));
        if let Scene::Container(c) = &mut content {
            c.rect = Rect::new(0, 0, 200, rows * 40);
        }
        let offset = state.offset();
        let scroll = ScrollNode::new(Rect::new(0, 0, 200, h), content)
            .with_state(std::rc::Rc::clone(state))
            .with_offset(offset.0, offset.1);
        let mut root = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 200, h);
        }
        root
    }

    /// R1620 §5.45 §5.35 — **a drag-select reaches past the viewport.**
    ///
    /// The gesture the reference calls auto-scroll: hold the primary button
    /// near the bottom edge and the view keeps moving, so the sweep can select
    /// rows that were never painted when it began. Before this round a sweep
    /// stopped at the last painted row, because a row that is not painted is
    /// never entered.
    ///
    /// Driven through the real router against the real coordinator, with the
    /// selection read off the published wire form.
    #[test]
    fn r1620_a_held_pointer_at_the_edge_scrolls_and_the_sweep_follows() {
        use pinion_core::external::IntrospectValue;
        use pinion_core::widgets::scroll::ScrollState;
        use pinion_core::widgets::virtual_select::VirtualSelectExternal;

        fn rows(state: &Scene) -> String {
            let Scene::External(node) = state else {
                panic!("external root")
            };
            match node
                .handle
                .introspect()
                .expect("introspect")
                .query("selection")
            {
                Ok(IntrospectValue::Json(list)) => list.to_string(),
                other => panic!("selection query answered {other:?}"),
            }
        }
        let scroll = ScrollState::new();
        // 40 rows of 40 px in a 160-px viewport: four rows visible, 36 not.
        scroll.set_max(0, 40 * 40 - 160);
        let scroll = std::rc::Rc::new(scroll);
        let mut state = Scene::External(
            ExternalNode::new(Box::new(VirtualSelectExternal::new_multi(40))).with_tag("sel"),
        );
        let mut router = InputRouter::new();
        router.update_paint_scene(scrolling_rows(&scroll, 40, 160), &mut state);

        // Press row 0, then drag to the BOTTOM EDGE and hold there.
        router.cursor_moved(PointerId::MOUSE, 100.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 155.0, &mut state);
        assert_eq!(
            rows(&state),
            "[[0,3]]",
            "the sweep reaches the last PAINTED row and, without auto-scroll, \
             would stop there forever",
        );
        assert_eq!(scroll.offset().1, 0, "nothing has scrolled yet");

        // Now hold still and let frames pass. Each tick scrolls, the app
        // repaints, and the newly-arrived rows are entered.
        for _ in 0..12 {
            assert!(
                router.tick_auto_scroll(0.016),
                "the ramp is live while the pointer holds inside the margin",
            );
            router.update_paint_scene(scrolling_rows(&scroll, 40, 160), &mut state);
        }
        let scrolled = scroll.offset().1;
        assert!(scrolled > 0, "the view moved: offset {scrolled}");
        let reached = rows(&state);
        assert_ne!(
            reached, "[[0,3]]",
            "and the selection grew past the four rows that were painted",
        );
        // The range is still anchored at the press and contiguous — the sweep
        // followed the content rather than jumping.
        assert!(
            reached.starts_with("[[0,"),
            "still anchored where the finger went down: {reached}",
        );

        // NEGATIVE CONTROL 1 — the release stops it. Nothing about the cursor
        // changes; only the held set does.
        router.pointer_up(PointerId::MOUSE, &mut state);
        let after_release = scroll.offset().1;
        assert!(
            !router.tick_auto_scroll(0.016),
            "a pointer that holds nothing is hovering, not dragging",
        );
        assert_eq!(
            scroll.offset().1,
            after_release,
            "and the view does not move under a resting pointer",
        );
    }

    /// R1620 §5.45 — **the speed is a function of the pointer, not of elapsed
    /// time**, which is the one place this parts from the reference (whose
    /// counter ramps per timer tick and reads the margin as a boolean).
    #[test]
    fn r1620_the_ramp_is_proportional_to_depth_and_saturates_outside() {
        use pinion_core::widgets::scroll::AutoScroll;
        let policy = AutoScroll {
            margin: 20.0,
            max_speed: 100.0,
        };
        // Viewport 0..200 on this axis.
        let (lo, hi) = (0.0, 200.0);
        // `near`, not `assert_eq!`: these are computed f64s and the workspace
        // lints reject exact float comparison, for the usual reason.
        let near = |got: f64, want: f64| assert!((got - want).abs() < 1e-9, "{got} != {want}");
        near(policy.speed_at(100.0, lo, hi), 0.0); // the middle is still
        near(policy.speed_at(180.0, lo, hi), 0.0); // the band's inner edge
        assert!(
            (policy.speed_at(190.0, lo, hi) - 50.0).abs() < 1e-9,
            "halfway into the band is half speed — the reference cannot express \
             this at all, because its speed does not read the position",
        );
        assert!(
            (policy.speed_at(200.0, lo, hi) - 100.0).abs() < 1e-9,
            "at the edge"
        );
        assert!(
            (policy.speed_at(9_999.0, lo, hi) - 100.0).abs() < 1e-9,
            "and it SATURATES outside: a pointer dragged far past the window \
             asks for max speed, not for an unbounded one",
        );
        // The near edge is the mirror image, negative.
        assert!((policy.speed_at(10.0, lo, hi) + 50.0).abs() < 1e-9);
        assert!((policy.speed_at(0.0, lo, hi) + 100.0).abs() < 1e-9);
        assert!((policy.speed_at(-500.0, lo, hi) + 100.0).abs() < 1e-9);
        // A band wider than half the viewport folds to the two halves rather
        // than overlapping itself in the middle.
        let fat = AutoScroll {
            margin: 500.0,
            max_speed: 100.0,
        };
        near(fat.speed_at(100.0, lo, hi), 0.0);
        assert!(fat.speed_at(150.0, lo, hi) > 0.0);
        assert!(fat.speed_at(50.0, lo, hi) < 0.0);
        // Off is off, at every position.
        for pos in [0.0, 100.0, 200.0, -50.0] {
            near(AutoScroll::off().speed_at(pos, lo, hi), 0.0);
        }
        assert!(!AutoScroll::off().is_enabled());
        assert!(AutoScroll::default().is_enabled());
        // Either half at zero is off. `off()` zeroes both, so a test that only
        // uses it cannot tell which half is load-bearing — a counterfactual
        // dropping the band check from `is_enabled` passed against exactly
        // that. A band of zero width IS how a region declines, whatever speed
        // sits beside it.
        let no_band = AutoScroll {
            margin: 0.0,
            max_speed: 100.0,
        };
        assert!(!no_band.is_enabled(), "a zero-width band is off");
        for pos in [0.0, 1.0, 100.0, 199.0, 200.0] {
            near(no_band.speed_at(pos, lo, hi), 0.0);
        }
        let no_speed = AutoScroll {
            margin: 16.0,
            max_speed: 0.0,
        };
        assert!(!no_speed.is_enabled(), "and so is a zero speed");
        near(no_speed.speed_at(199.0, lo, hi), 0.0);
    }

    /// R1620 §5.45 — the gesture belongs to the region it STARTED in, and keeps
    /// scrolling once the pointer is dragged outside — which is where the ramp
    /// saturates and where a hit-test finds nothing.
    ///
    /// Also the negative control for the policy: a region that declared
    /// auto-scroll OFF must stay off once the pointer leaves it, which a
    /// default-valued fallback would silently have switched on.
    #[test]
    fn r1620_the_region_is_pinned_at_the_press_and_keeps_its_policy() {
        use pinion_core::widgets::scroll::{AutoScroll, ScrollState};
        let scroll = ScrollState::new();
        scroll.set_max(0, 1_000);
        let scroll = std::rc::Rc::new(scroll);
        let mut state = Scene::Container(ContainerNode::new(vec![]));

        let mut router = InputRouter::new();
        router.update_paint_scene(scrolling_rows(&scroll, 40, 160), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 80.0, &mut state); // mid-viewport
        router.pointer_down(PointerId::MOUSE, &mut state);
        // Drag WELL BELOW the window: no scroll node covers this point.
        router.cursor_moved(PointerId::MOUSE, 100.0, 900.0, &mut state);
        assert!(
            router.tick_auto_scroll(0.016),
            "the pinned region keeps scrolling with the pointer outside it",
        );
        assert!(scroll.offset().1 > 0);

        // The same gesture over a region that declared auto-scroll OFF.
        let quiet = std::rc::Rc::new(ScrollState::new());
        quiet.set_max(0, 1_000);
        let paint_off = |st: &std::rc::Rc<ScrollState>| {
            let inner = scrolling_rows(st, 40, 160);
            let Scene::Container(mut root) = inner else {
                panic!("container root")
            };
            if let Some(Scene::Scroll(node)) = root.children.first_mut() {
                node.auto_scroll = AutoScroll::off();
            }
            Scene::Container(root)
        };
        let mut router = InputRouter::new();
        router.update_paint_scene(paint_off(&quiet), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 80.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 900.0, &mut state);
        assert!(
            !router.tick_auto_scroll(0.016),
            "a region that declared OFF stays off outside its own rect",
        );
        assert_eq!(quiet.offset().1, 0);
    }

    /// R1620 §5.35 §5.38 §5.45 — **both continuations run in the same frame.**
    ///
    /// A press inside a scrolling panel can be repeating a step AND holding
    /// the view's edge. `tick_pointer_hold` must ask both every frame: a
    /// short-circuited `a() || b()` stops asking the second the moment the
    /// first says yes, and the gesture silently loses half of itself — which
    /// is a bug nobody would see in either mechanism's own tests.
    ///
    /// This test exists because a counterfactual found nothing catching it,
    /// and the composition was moved onto the router so it could be written at
    /// all: the shell-side version had no fixture that could drive both.
    #[test]
    fn r1620_a_repeating_press_still_auto_scrolls_in_the_same_frame() {
        use pinion_core::widgets::scroll::ScrollState;
        let scroll = std::rc::Rc::new(ScrollState::new());
        scroll.set_max(0, 1_000);
        // A real auto-repeating button, painted at the BOTTOM of a scroll
        // region so one press is inside both mechanisms at once.
        let mut state = state_with_real_button(Some(pinion_core::AutoRepeat::new(0.0, 0.01)));
        let paint = {
            let mut btn = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag("main_btn")
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut btn {
                c.rect = Rect::new(0, 120, 200, 40);
            }
            let mut content = Scene::Container(ContainerNode::new(vec![btn]));
            if let Scene::Container(c) = &mut content {
                c.rect = Rect::new(0, 0, 200, 1_160);
            }
            let node = ScrollNode::new(Rect::new(0, 0, 200, 160), content)
                .with_state(std::rc::Rc::clone(&scroll));
            let mut root = Scene::Container(ContainerNode::new(vec![Scene::Scroll(node)]));
            if let Scene::Container(c) = &mut root {
                c.rect = Rect::new(0, 0, 200, 160);
            }
            root
        };
        let mut router = InputRouter::new();
        router.update_paint_scene(paint, &mut state);
        // Press the button, which sits inside the bottom edge band.
        router.cursor_moved(PointerId::MOUSE, 100.0, 152.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        let _ = drain_clicks(&mut state);

        let armed = router.tick_pointer_hold(0.05, &mut state);
        assert!(armed, "the hold is live");
        let repeats = drain_clicks(&mut state);
        let scrolled = scroll.offset().1;
        assert!(repeats > 0, "the repeat fired in this frame");
        assert!(
            scrolled > 0,
            "and the SAME frame auto-scrolled: {scrolled} px — a short-circuit \
             here would have left this at 0 with the repeat still passing",
        );
    }

    /// R1620 §5.45 §5.16 — a region that declares auto-scroll **off** does not
    /// scroll even with the pointer pressed inside its own band, and the
    /// published state says the ramp is still — so an agent asking "why is my
    /// drag not scrolling" reads the declared band rather than guessing.
    ///
    /// Both halves exist because counterfactuals found nothing catching
    /// either: the off-inside case (the outside case was already covered), and
    /// the published velocity being a constant rather than the step that moves
    /// the view.
    #[test]
    fn r1620_off_inside_the_band_is_still_off_and_the_wire_agrees() {
        use pinion_core::widgets::scroll::{AutoScroll, ScrollState};
        let scroll = std::rc::Rc::new(ScrollState::new());
        scroll.set_max(0, 1_000);
        let mut state = Scene::Container(ContainerNode::new(vec![]));
        let paint = |st: &std::rc::Rc<ScrollState>, policy: AutoScroll| {
            let inner = scrolling_rows(st, 40, 160);
            let Scene::Container(mut root) = inner else {
                panic!("container root")
            };
            if let Some(Scene::Scroll(node)) = root.children.first_mut() {
                node.auto_scroll = policy;
            }
            Scene::Container(root)
        };

        // OFF, with the pointer pressed deep inside the bottom band.
        let mut router = InputRouter::new();
        router.update_paint_scene(paint(&scroll, AutoScroll::off()), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 155.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert!(!router.tick_auto_scroll(0.016), "off is off, inside too");
        assert_eq!(scroll.offset().1, 0);
        let reported = router
            .auto_scroll_state(PointerId::MOUSE)
            .expect("a gesture holds the region, so the axis is present");
        assert!(
            (reported.velocity_y).abs() < 1e-9,
            "the wire reports a still ramp: {reported:?}",
        );
        assert!(
            (reported.margin).abs() < 1e-9,
            "and publishes the band that explains WHY it is still",
        );
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(
            router.auto_scroll_state(PointerId::MOUSE).is_none(),
            "absent once no gesture holds a region — never a zeroed object",
        );

        // ON, same geometry: the published velocity is the one that moves it.
        let live = std::rc::Rc::new(ScrollState::new());
        live.set_max(0, 1_000);
        let policy = AutoScroll {
            margin: 16.0,
            max_speed: 500.0,
        };
        let mut router = InputRouter::new();
        router.update_paint_scene(paint(&live, policy), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 155.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        let reported = router
            .auto_scroll_state(PointerId::MOUSE)
            .expect("gesture open");
        assert!(reported.velocity_y > 0.0, "downward: {reported:?}");
        // One second of ticking must travel the velocity the wire published —
        // the read and the motion are one derivation or neither is trustworthy.
        for _ in 0..100 {
            router.tick_auto_scroll(0.01);
        }
        let travelled = f64::from(live.offset().1);
        assert!(
            (travelled - reported.velocity_y).abs() <= 2.0,
            "travelled {travelled} px in 1s against a published {} px/s",
            reported.velocity_y,
        );
    }

    /// R1620 §5.45 — a slow ramp still moves. The remainder is carried between
    /// frames, so a speed below one pixel per frame accumulates instead of
    /// truncating to nothing — which is what it would do at exactly the speeds
    /// a careful user reaches for.
    #[test]
    fn r1620_a_sub_pixel_speed_accumulates_instead_of_stalling() {
        use pinion_core::widgets::scroll::{AutoScroll, ScrollState};
        let scroll = std::rc::Rc::new(ScrollState::new());
        scroll.set_max(0, 1_000);
        let mut state = Scene::Container(ContainerNode::new(vec![]));
        let paint = |st: &std::rc::Rc<ScrollState>| {
            let inner = scrolling_rows(st, 40, 160);
            let Scene::Container(mut root) = inner else {
                panic!("container root")
            };
            if let Some(Scene::Scroll(node)) = root.children.first_mut() {
                // 30 px/s is half a pixel per 60 fps frame.
                node.auto_scroll = AutoScroll {
                    margin: 16.0,
                    max_speed: 30.0,
                };
            }
            Scene::Container(root)
        };
        let mut router = InputRouter::new();
        router.update_paint_scene(paint(&scroll), &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 80.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 160.0, &mut state); // full push
        for _ in 0..4 {
            router.tick_auto_scroll(0.016);
        }
        assert!(
            scroll.offset().1 >= 1,
            "four frames of half a pixel is two pixels, not zero: {}",
            scroll.offset().1,
        );
    }

    /// R1619 §5.35 §5.39 — the held set is forgotten on blur, and a    /// R1619 §5.35 §5.39 — the held set is forgotten on blur, and a
    /// [`PointerCancel`](pinion_core::input::PointerWireEvent::Cancel) clears
    /// it too. Both are the same rule: the release that would have cleared it
    /// is one this router will never see, and a stranded press is worse than a
    /// forgotten one.
    #[test]
    fn r1619_a_revoked_or_blurred_gesture_forgets_its_buttons() {
        let mut router = InputRouter::new();
        let (mut state, _captures) = state_with_button();
        router.update_paint_scene(
            paint_with_button(200, 200, Rect::new(80, 80, 40, 40)),
            &mut state,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert!(!router.held_buttons(PointerId::MOUSE).is_empty());
        router.pointer_cancel(PointerId::MOUSE, &mut state);
        assert!(
            router.held_buttons(PointerId::MOUSE).is_empty(),
            "a revoked gesture holds nothing",
        );
        // The blur arm, on a fresh press.
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert!(!router.held_buttons(PointerId::MOUSE).is_empty());
        router.clear_held_buttons();
        assert!(router.held_buttons(PointerId::MOUSE).is_empty());
    }

    /// R1619 §5.35 — every button edge the router can be told about moves the
    /// held set, and the note is idempotent so the seams that overlap on a real
    /// press cannot double-count. The census this replaces would have been a
    /// list of which entry points to check; the property is stated instead.
    #[test]
    fn r1619_every_button_edge_moves_the_held_set_idempotently() {
        use pinion_core::{PointerButton, PointerEdge};
        let mut router = InputRouter::new();
        let mut state = Scene::Container(ContainerNode::new(vec![]));
        // The three buttons are independent bits.
        for button in [
            PointerButton::Left,
            PointerButton::Middle,
            PointerButton::Right,
        ] {
            router.note_button_edge(PointerId::MOUSE, button, PointerEdge::Down);
            assert!(router.held_buttons(PointerId::MOUSE).contains(button));
        }
        assert_eq!(
            router.held_buttons(PointerId::MOUSE).as_wire_token(),
            "lmr",
            "a three-button chord is a set, not a last-writer",
        );
        // Idempotent: repeating an edge is not a second press.
        router.note_button_edge(PointerId::MOUSE, PointerButton::Left, PointerEdge::Down);
        router.middle_down(PointerId::MOUSE);
        assert_eq!(router.held_buttons(PointerId::MOUSE).as_wire_token(), "lmr");
        // The router's own left channel notes its own edges, so a driver that
        // never touches `note_button_edge` still gets a correct set.
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(router.held_buttons(PointerId::MOUSE).as_wire_token(), "mr");
        router.middle_up(PointerId::MOUSE);
        assert_eq!(router.held_buttons(PointerId::MOUSE).as_wire_token(), "r");
        // Per-pointer: a second pointer's set is its own.
        assert!(router.held_buttons(PointerId::touch(7)).is_empty());
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

    // ═════════════════════════════════════════════════════════════
    // R1549 §5.35 §5.38 — press-and-hold auto-repeat.
    //
    // Driven end-to-end through a REAL `ButtonExternal`: its own SCXML
    // decides whether it is `Pressed`, its own `auto_repeat()` answers the
    // cadence, and the fires are counted from the `"click"` intents its
    // own `WidgetTransition::detect` produces. Nothing here stubs the
    // property under test — a repeat is a click by the same derivation a
    // finger's click is.
    // ═════════════════════════════════════════════════════════════

    /// State scene holding one real [`ButtonExternal`] tagged `main_btn`,
    /// optionally declaring a repeat cadence.
    fn state_with_real_button(repeat: Option<pinion_core::AutoRepeat>) -> Scene {
        let mut btn = pinion_core::widgets::button::ButtonExternal::new();
        if let Some(r) = repeat {
            btn = btn.with_auto_repeat(r);
        }
        Scene::External(ExternalNode::new(Box::new(btn)).with_tag("main_btn"))
    }

    /// Drain and count the `"click"` intents the button has buffered — one
    /// per activation, whether a finger or the repeat driver caused it.
    fn drain_clicks(state: &mut Scene) -> usize {
        let Scene::External(node) = state else {
            return 0;
        };
        let mut n = 0;
        node.handle.drain_intents(&mut |intent| {
            if intent.tag_str() == "click" {
                n += 1;
            }
        });
        n
    }

    /// Press and hold `main_btn`, leaving the press in flight.
    fn press_and_hold(router: &mut InputRouter, state: &mut Scene) {
        router.update_paint_scene(
            paint_with_button(200, 200, Rect::new(80, 80, 40, 40)),
            state,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, state);
        router.pointer_down(PointerId::MOUSE, state);
    }

    /// The behaviour the round exists to add, and the exact cadence: the
    /// delay must elapse before the first repeat, then one per interval.
    #[test]
    fn r1549_held_button_repeats_after_the_delay_then_per_interval() {
        let mut router = InputRouter::new();
        let mut state = state_with_real_button(Some(pinion_core::AutoRepeat::new(0.30, 0.10)));
        press_and_hold(&mut router, &mut state);
        assert_eq!(drain_clicks(&mut state), 0, "a press is not yet a click");

        // Just short of the delay: nothing.
        assert!(router.tick_auto_repeat(0.29, &mut state), "armed");
        assert_eq!(drain_clicks(&mut state), 0, "0.29 < 0.30 delay");
        // Crossing it: exactly one.
        router.tick_auto_repeat(0.02, &mut state);
        assert_eq!(drain_clicks(&mut state), 1, "the delay elapsed");
        // Then one per interval, and no more.
        router.tick_auto_repeat(0.09, &mut state);
        assert_eq!(drain_clicks(&mut state), 0, "0.01 + 0.09 < 0.10");
        router.tick_auto_repeat(0.02, &mut state);
        assert_eq!(drain_clicks(&mut state), 1);
    }

    /// The pre-R1549 behaviour, still the default: a plain button held
    /// forever activates once, on release.
    #[test]
    fn r1549_undeclared_button_never_repeats_however_long_it_is_held() {
        let mut router = InputRouter::new();
        let mut state = state_with_real_button(None);
        press_and_hold(&mut router, &mut state);
        for _ in 0..100 {
            assert!(
                !router.tick_auto_repeat(0.1, &mut state),
                "an undeclared target arms nothing",
            );
        }
        assert_eq!(drain_clicks(&mut state), 0, "ten seconds, zero repeats");
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(drain_clicks(&mut state), 1, "one press, one click");
    }

    /// THE structural claim: a repeat cannot outlive its press. The
    /// release removes the press record, and the record is the only place
    /// a run can live — so this needs no un-arming code to be true, and no
    /// amount of ticking resurrects it.
    #[test]
    fn r1549_release_ends_the_repeat_with_no_unarm_path() {
        let mut router = InputRouter::new();
        let mut state = state_with_real_button(Some(pinion_core::AutoRepeat::new(0.10, 0.05)));
        press_and_hold(&mut router, &mut state);
        router.tick_auto_repeat(0.20, &mut state);
        assert!(drain_clicks(&mut state) >= 1, "it was repeating");
        router.pointer_up(PointerId::MOUSE, &mut state);
        let _ = drain_clicks(&mut state); // the release's own activation
        for _ in 0..50 {
            assert!(
                !router.tick_auto_repeat(0.1, &mut state),
                "nothing is held, so nothing is armed",
            );
        }
        assert_eq!(drain_clicks(&mut state), 0, "five seconds after release");
    }

    /// A large `dt` — an agent's `scene/tick 1.0` — crosses many
    /// thresholds and fires each of them, so injected time reproduces
    /// wall-clock time exactly rather than costing one fire per frame.
    #[test]
    fn r1549_one_large_tick_fires_every_threshold_it_crosses() {
        let mut router = InputRouter::new();
        let mut state = state_with_real_button(Some(pinion_core::AutoRepeat::new(0.30, 0.10)));
        press_and_hold(&mut router, &mut state);
        // 1.05s: fires at 0.30 then every 0.10 through 1.00 = 8, with
        // 0.05 to spare. Deliberately NOT a span that lands exactly on a
        // fire instant — see `tick_auto_repeat`'s note on boundaries.
        router.tick_auto_repeat(1.05, &mut state);
        assert_eq!(drain_clicks(&mut state), 8, "1 delay + 7 intervals");
    }

    /// Sixty small frames and one big tick over the same span fire the same
    /// number of times — the property that makes a hold reproducible without a
    /// wall clock, and the one the toolkit's basic timer cannot offer.
    #[test]
    fn r1549_many_small_frames_equal_one_large_tick() {
        let policy = pinion_core::AutoRepeat::new(0.30, 0.10);
        let mut a = InputRouter::new();
        let mut sa = state_with_real_button(Some(policy));
        press_and_hold(&mut a, &mut sa);
        a.tick_auto_repeat(63.0 / 60.0, &mut sa);
        let one_big = drain_clicks(&mut sa);

        let mut b = InputRouter::new();
        let mut sb = state_with_real_button(Some(policy));
        press_and_hold(&mut b, &mut sb);
        let mut many = 0;
        for _ in 0..63 {
            b.tick_auto_repeat(1.0 / 60.0, &mut sb);
            many += drain_clicks(&mut sb);
        }
        assert_eq!(one_big, many, "the two time bases reproduce each other");
        assert_eq!(one_big, 8, "and both are the arithmetic answer");
    }

    /// A frozen clock (`scene/tick 0`) fires nothing but still reports the
    /// hold as armed — otherwise a client polling at dt=0 would read a
    /// live hold as finished.
    #[test]
    fn r1549_zero_tick_fires_nothing_but_still_reports_armed() {
        let mut router = InputRouter::new();
        let mut state = state_with_real_button(Some(pinion_core::AutoRepeat::new(0.10, 0.05)));
        press_and_hold(&mut router, &mut state);
        assert!(router.tick_auto_repeat(0.0, &mut state), "armed");
        assert_eq!(drain_clicks(&mut state), 0, "a frozen clock fires nothing");
        // A non-finite delta takes the same road as a frozen one: it must
        // not poison the accumulator (a `NaN` there would compare `false`
        // against every threshold and silently kill the hold for good).
        assert!(
            router.tick_auto_repeat(f32::NAN, &mut state),
            "a malformed delta still reports the hold",
        );
        assert_eq!(drain_clicks(&mut state), 0);
        router.tick_auto_repeat(0.22, &mut state);
        assert_eq!(
            drain_clicks(&mut state),
            3,
            "and the clock still works afterwards (0.10, 0.15, 0.20)",
        );
    }

    /// A widget that stops declaring mid-hold rewinds the ramp: coming back
    /// does not resume at speed, it restarts from the delay (the toolkit's `mouseMoveEvent`
    /// behaves the same). Driven by DISABLING the button mid-hold — a `Disabled`
    /// button is not `Pressed`, so it answers `None` through the statechart rather than
    /// any repeat-specific hook.
    #[test]
    fn r1549_a_quiet_answer_rewinds_the_ramp_rather_than_pausing_it() {
        let mut router = InputRouter::new();
        let mut state = state_with_real_button(Some(pinion_core::AutoRepeat::new(0.30, 0.10)));
        press_and_hold(&mut router, &mut state);
        router.tick_auto_repeat(0.29, &mut state); // 0.01 short of firing
        assert_eq!(drain_clicks(&mut state), 0);

        // Disable, tick, re-enable: the accrued 0.29 is gone.
        send_to_button(&mut state, "Disable");
        assert!(!router.tick_auto_repeat(0.10, &mut state), "not armed");
        send_to_button(&mut state, "Enable");
        send_to_button(&mut state, "PointerEnter");
        send_to_button(&mut state, "PointerDown");
        let _ = drain_clicks(&mut state);
        router.tick_auto_repeat(0.29, &mut state);
        assert_eq!(
            drain_clicks(&mut state),
            0,
            "the delay restarted; a resumed ramp would have fired here",
        );
        router.tick_auto_repeat(0.02, &mut state);
        assert_eq!(drain_clicks(&mut state), 1, "and it fires 0.30 in");
    }

    /// The published census: an agent can see the run it is driving — which
    /// press, on what, at what cadence, how far in, and when the next one
    /// lands. The toolkit keeps every one of these in a private timer.
    #[test]
    fn r1549_the_wire_states_the_run_it_is_driving() {
        let mut router = InputRouter::new();
        let mut state = state_with_real_button(Some(pinion_core::AutoRepeat::new(0.30, 0.10)));
        assert!(
            router.auto_repeat_holds(&state).is_empty(),
            "nothing held yet",
        );
        press_and_hold(&mut router, &mut state);

        let holds = router.auto_repeat_holds(&state);
        assert_eq!(holds.len(), 1);
        assert_eq!(holds[0].target, "main_btn");
        assert!(holds[0].repeating);
        assert_eq!(holds[0].fires, 0);
        let next = holds[0].next_fire_in_secs.expect("a cadence is declared");
        assert!((next - 0.30).abs() < 1e-5, "the whole delay is still ahead");

        // Tick EXACTLY what the wire said, and a repeat lands: the census
        // is predictive, not merely descriptive.
        router.tick_auto_repeat(next, &mut state);
        assert_eq!(drain_clicks(&mut state), 1);
        let holds = router.auto_repeat_holds(&state);
        assert_eq!(holds[0].fires, 1);
        assert!((holds[0].held_secs - 0.30).abs() < 1e-5);
        let next = holds[0].next_fire_in_secs.expect("still repeating");
        assert!((next - 0.10).abs() < 1e-5, "now the interval");
    }

    /// A press on a NON-repeating widget is still published, with
    /// `repeating: false` and no cadence. Omitting it would make "held,
    /// and nothing will come of it" read identically to "not held".
    #[test]
    fn r1549_a_non_repeating_hold_is_reported_as_a_hold() {
        let mut router = InputRouter::new();
        let mut state = state_with_real_button(None);
        press_and_hold(&mut router, &mut state);
        let holds = router.auto_repeat_holds(&state);
        assert_eq!(holds.len(), 1, "the press IS in flight");
        assert!(!holds[0].repeating);
        assert_eq!(holds[0].policy, None);
        assert_eq!(holds[0].next_fire_in_secs, None);
    }

    /// A target that leaves the scene mid-hold stops repeating rather than
    /// dispatching into nothing — the router's own last line of defence,
    /// independent of any widget's answer.
    #[test]
    fn r1549_a_target_that_leaves_the_scene_stops_repeating() {
        let mut router = InputRouter::new();
        let mut state = state_with_real_button(Some(pinion_core::AutoRepeat::new(0.10, 0.05)));
        press_and_hold(&mut router, &mut state);
        router.tick_auto_repeat(0.20, &mut state);
        assert!(drain_clicks(&mut state) >= 1);
        // The binding re-composed its tree and the button is gone.
        let mut gone = Scene::Container(ContainerNode::new(vec![]));
        assert!(!router.tick_auto_repeat(1.0, &mut gone), "nothing to ask");
        assert!(
            router.auto_repeat_holds(&gone)[0].repeating.eq(&false),
            "the press is still in flight, but it repeats nothing",
        );
    }

    /// The R876 click-vs-drag latch still works after the press record
    /// grew a hold — the widening must not have cost the field its first
    /// job (a press that strays is a drag, and a drag suppresses the
    /// trailing click).
    #[test]
    fn r1549_press_record_still_answers_the_click_vs_drag_question() {
        let mut router = InputRouter::new();
        let mut state = state_with_real_button(None);
        press_and_hold(&mut router, &mut state);
        assert!(!router.press_became_drag(PointerId::MOUSE), "in place");
        router.cursor_moved(PointerId::MOUSE, 140.0, 100.0, &mut state);
        assert!(router.press_became_drag(PointerId::MOUSE), "strayed 40px");
    }

    /// A large tick against a COMPOSITE target that runs out of range
    /// mid-catch-up stops exactly at the bound. This is the property the
    /// per-fire re-ask buys: asking once and then firing N times would
    /// have driven the value past its own maximum, and it also proves the
    /// composite (`spin#inc`) path end-to-end — the widget answers for the
    /// sub-region it recorded, and the router never parses the tag.
    #[test]
    fn r1549_catch_up_stops_at_the_bound_it_reaches_mid_tick() {
        use pinion_core::widgets::spin_button::SpinButtonExternal;
        let mut router = InputRouter::new();
        // 3 of 10, single step: five repeats would reach 8, but a 10-second
        // tick would fire ~97 times if the router asked only once.
        let spin = SpinButtonExternal::new(3.0, 0.0, 10.0, 1.0);
        let mut state = Scene::External(ExternalNode::new(Box::new(spin)).with_tag("spin"));
        let mut paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        if let Scene::Container(root) = &mut paint {
            if let Some(Scene::Container(c)) = root.children.first_mut() {
                c.tag = Some("spin#inc".into());
            }
        }
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(
            router.auto_repeat_holds(&state)[0].target,
            "spin#inc",
            "the press landed on the sub-region",
        );

        assert!(
            !router.tick_auto_repeat(10.0, &mut state),
            "ran out mid-tick"
        );
        let Scene::External(node) = &state else {
            unreachable!()
        };
        let value = node
            .handle
            .introspect()
            .expect("spin introspects")
            .query("value");
        assert_eq!(
            value,
            Ok(pinion_core::external::IntrospectValue::Float(10.0)),
            "it stopped at max, not past it",
        );
        assert_eq!(
            router.auto_repeat_holds(&state)[0].fires,
            7,
            "3 -> 10 is seven steps and the eighth was never asked for",
        );
    }

    /// The census names the target the press LANDED on, not wherever the
    /// cursor has since drifted. The two coincide for a capturing widget
    /// (capture suppresses the mid-press hover change) and for most of the
    /// paths a non-capturing one takes, which is exactly why the press's
    /// target is *stored* rather than re-derived from `hover_targets`:
    /// their agreement is a consequence of three other invariants, and a
    /// hold that reported the drifted tag would send a stuck-press
    /// investigation to the wrong widget.
    #[test]
    fn r1549_the_hold_names_the_target_the_press_landed_on() {
        use pinion_core::widgets::spin_button::SpinButtonExternal;
        let mut router = InputRouter::new();
        let spin = SpinButtonExternal::new(3.0, 0.0, 10.0, 1.0);
        let mut state = Scene::External(ExternalNode::new(Box::new(spin)).with_tag("spin"));
        // Two sub-regions side by side: `spin#dec` at x 20..60, `spin#inc`
        // at x 80..120. A spin button does NOT take pointer capture, so the
        // hover really does move out from under the press.
        let dec = {
            let mut c = ContainerNode::new(vec![]).with_tag("spin#dec");
            c.rect = Rect::new(20, 80, 40, 40);
            Scene::Container(c)
        };
        let inc = {
            let mut c = ContainerNode::new(vec![]).with_tag("spin#inc");
            c.rect = Rect::new(80, 80, 40, 40);
            Scene::Container(c)
        };
        let mut root = ContainerNode::new(vec![dec, inc]);
        root.rect = Rect::new(0, 0, 200, 200);
        router.update_paint_scene(Scene::Container(root), &mut state);

        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(router.auto_repeat_holds(&state)[0].target, "spin#inc");

        // Drag across to the other arrow. The hover moves; the press does not.
        router.cursor_moved(PointerId::MOUSE, 40.0, 100.0, &mut state);
        assert_eq!(
            router.hover_target(PointerId::MOUSE),
            Some("spin#dec"),
            "precondition: the hover really moved",
        );
        let holds = router.auto_repeat_holds(&state);
        assert_eq!(
            holds[0].target, "spin#inc",
            "the press is still the one the user started",
        );
        assert!(
            !holds[0].repeating,
            "and it repeats nothing — the arrow it left is no longer Pressed",
        );
        assert!(
            !router.tick_auto_repeat(5.0, &mut state),
            "five seconds of held-but-strayed fires nothing",
        );
        let Scene::External(node) = &state else {
            unreachable!()
        };
        assert_eq!(
            node.handle
                .introspect()
                .expect("introspects")
                .query("value"),
            Ok(pinion_core::external::IntrospectValue::Float(3.0)),
            "and in particular it did not start stepping the OTHER arrow",
        );
    }

    /// Drive a named event straight into the state scene's button, the way
    /// a binding's own dispatch would.
    fn send_to_button(state: &mut Scene, event: &str) {
        let Scene::External(node) = state else {
            panic!("state root is the button");
        };
        let _ = node
            .handle
            .introspect_mut()
            .expect("button introspects")
            .invoke(
                "send",
                pinion_core::external::IntrospectValue::Text(event.to_owned()),
            );
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
        /// R1497 — which [`External`] opt-ins this variant declares. Four
        /// independent `bool` fields tripped `clippy::struct_excessive_bools`,
        /// and the lint was right: these were never four unrelated booleans but
        /// one SET, which is also how the trait presents them.
        opt_ins: OptIns,
    }

    /// R1497 — the set of contract opt-ins a [`DragCaptureExternal`] variant
    /// declares. One value instead of a widening row of flags, so a new variant
    /// adds a constant rather than a field.
    #[derive(Clone, Copy, Default)]
    struct OptIns(u8);

    impl OptIns {
        /// R738 — `capture_normalize` returns `CaptureNormalize::Primary`
        /// (range-slider-style whole-widget normalization).
        const NORMALIZE_PRIMARY: u8 = 1 << 0;
        /// R880 — the bare-target modifier wire
        /// (`wants_bare_send_modifiers`), so a background release with held
        /// modifiers reaches `send` as `":<EventName>:<token>"`.
        const BARE_SEND_MODIFIERS: u8 = 1 << 1;
        /// R1405 — hover-move forwarding (`wants_hover_move`), so a plain hover
        /// (no press) also forwards `pointer_move`.
        const HOVER_MOVE: u8 = 1 << 2;
        /// R1497 — the R741 button-like release policy
        /// (`cancel_on_release_off_target`): a release whose cursor is no longer
        /// over the captured tag cancels instead of activating.
        const CANCEL_OFF_TARGET: u8 = 1 << 3;

        fn with(self, flag: u8) -> Self {
            Self(self.0 | flag)
        }

        fn has(self, flag: u8) -> bool {
            self.0 & flag != 0
        }
    }

    impl DragCaptureExternal {
        fn new() -> (Self, EventLog, MoveLog) {
            Self::with_opt_ins(OptIns::default())
        }

        fn with_opt_ins(opt_ins: OptIns) -> (Self, EventLog, MoveLog) {
            let events = Arc::new(Mutex::new(Vec::new()));
            let moves = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    events: Arc::clone(&events),
                    moves: Arc::clone(&moves),
                    opt_ins,
                },
                events,
                moves,
            )
        }

        /// R738 — fixture variant whose `capture_normalize` is whole-widget.
        /// Takes a `bool` because two call sites contrast the two answers.
        fn with_normalize_primary(primary: bool) -> (Self, EventLog, MoveLog) {
            let mut opt_ins = OptIns::default();
            if primary {
                opt_ins = opt_ins.with(OptIns::NORMALIZE_PRIMARY);
            }
            Self::with_opt_ins(opt_ins)
        }

        /// R1405 — fixture variant opted in to hover-move forwarding.
        fn with_hover_move() -> (Self, EventLog, MoveLog) {
            Self::with_opt_ins(OptIns::default().with(OptIns::HOVER_MOVE))
        }

        /// R880 — fixture variant opted in to the bare-target modifier wire.
        fn with_bare_send_modifiers() -> (Self, EventLog, MoveLog) {
            Self::with_opt_ins(OptIns::default().with(OptIns::BARE_SEND_MODIFIERS))
        }

        /// R1497 — fixture variant with the R741 button-like release policy
        /// ([`External::cancel_on_release_off_target`]), the only shape in which
        /// `cursor_over_tag`'s answer changes the dispatched EVENT rather than
        /// just its target. `Button` / `Checkbox` / `Radio` / `Toggle` all set it.
        fn with_cancel_off_target() -> (Self, EventLog, MoveLog) {
            Self::with_opt_ins(OptIns::default().with(OptIns::CANCEL_OFF_TARGET))
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
        fn wants_hover_move(&self) -> bool {
            self.opt_ins.has(OptIns::HOVER_MOVE)
        }
        fn capture_normalize(&self) -> CaptureNormalize<'_> {
            if self.opt_ins.has(OptIns::NORMALIZE_PRIMARY) {
                CaptureNormalize::Primary
            } else {
                CaptureNormalize::Target
            }
        }
        fn pointer_move(&mut self, at: PointerReading) {
            self.moves.lock().expect("mutex poisoned").push(at.at);
        }
        fn wants_bare_send_modifiers(&self) -> bool {
            self.opt_ins.has(OptIns::BARE_SEND_MODIFIERS)
        }
        fn cancel_on_release_off_target(&self) -> bool {
            self.opt_ins.has(OptIns::CANCEL_OFF_TARGET)
        }
        fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
            Some(self)
        }
    }

    impl ExternalIntrospect for DragCaptureExternal {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(const { &[] })
        }
        fn query(&self, _path: &str) -> Result<IntrospectValue, ReadRefusal> {
            Err(ReadRefusal::UnknownPath)
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
                // R1619 — the held-button axis rides the SAME bare-target
                // opt-in as the modifier axis, so an opted-in target sees the
                // press's button on the empty-key wire.
                ":PointerDown::l".into(),
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
    fn hover_forwards_pointer_move_only_when_opted_in() {
        // R1405 §5.35 — a widget that opts into hover-move gets `pointer_move`
        // on a PLAIN hover (no press held): the router forwards the
        // widget-relative position, the OSC-8-hover seam. The default
        // (capture-only) case is the empty `read_moves` the sibling capture
        // test asserts before its `pointer_down`.
        let mut router = InputRouter::new();
        let (capture, _events, moves) = DragCaptureExternal::with_hover_move();
        let mut state =
            Scene::External(ExternalNode::new(Box::new(capture)).with_tag("main_slider"));
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Two hover moves INSIDE the rect (a hover forwards only while over the
        // widget — unlike capture, which forwards off-rect too). Rect
        // (80,80,40,40): (100,100) -> (0.5,0.5) [also the Enter], (110,110) ->
        // (0.75,0.75). Both forwarded because the widget opted in.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.cursor_moved(PointerId::MOUSE, 110.0, 110.0, &mut state);
        let log = read_moves(&moves);
        assert_eq!(log.len(), 2, "both hover moves forwarded (opted in)");
        assert!((log[0].0 - 0.5).abs() < 1e-4 && (log[0].1 - 0.5).abs() < 1e-4);
        assert!((log[1].0 - 0.75).abs() < 1e-4 && (log[1].1 - 0.75).abs() < 1e-4);
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

    /// (R1322) A window that HOSTS A DOCK — the panel wrapped in the dock walker's
    /// `DOCK_SURFACE_TAG` area, as every real dock window paints since R1205. Only such
    /// a window advertises an outer-dock perimeter; a window with no dock area (a
    /// torn-off panel's floating window) is not an outer-dock host.
    fn dock_window_with_panel(tag: &str, rect: Rect) -> Scene {
        use pinion_core::external::DOCK_SURFACE_TAG;
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
        let mut surface = Scene::Container(
            ContainerNode::new(vec![panel]).with_tag(DOCK_SURFACE_TAG.to_string()),
        );
        if let Scene::Container(c) = &mut surface {
            c.rect = Rect::new(0, 0, 1000, 800);
        }
        let mut root = Scene::Container(ContainerNode::new(vec![surface]));
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
        let host = dock_window_with_panel("body", Rect::new(0, 0, 1000, 800));
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
    fn r1322_a_sole_floater_is_not_a_cross_window_outer_dock_host() {
        // ★R1322 §5.51 — the CROSS-WINDOW twin of `r1322_no_dock_surface_no_outer_zone`.
        //
        // A torn-off panel's floating window hosts ONE panel and no dock area, and it
        // deliberately exposes NO drop target (`DockPanelStyle::drop_target = false` —
        // the R1118 "a panel cannot dock into a sole floater" rule). Pre-R1322
        // `resolve_outer_dock_zone` measured its band against EVERY window's
        // `scene.rect()`, so the floater still advertised a full outer perimeter and the
        // synthesized zone BYPASSED that opt-out: a second panel torn off toward a
        // floater already on screen redocked INTO it instead of floating
        // (`r1146_release_only_window_move` section E, red since R1167).
        use pinion_core::style::LayoutStyle;
        let mut panel = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag("properties".to_string())
                // The real floater's opt-out.
                .with_layout(LayoutStyle::new().with_drop_target(false))
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut panel {
            c.rect = Rect::new(0, 0, 360, 300);
        }
        let mut floater = Scene::Container(ContainerNode::new(vec![panel]));
        if let Scene::Container(c) = &mut floater {
            c.rect = Rect::new(0, 0, 360, 300);
        }
        let windows = [("torn-properties", &floater, (1000.0, 200.0))];
        // A cursor deep in the floater's perimeter band — where the old whole-window
        // fallback minted the sentinel — resolves NOTHING: no outer zone (no dock area)
        // and no inner target (the panel opted out). So the drag FLOATS, as it must.
        for (x, y, lbl) in [
            (1006.0, 350.0, "left band"),
            (1180.0, 206.0, "top band"),
            (1354.0, 350.0, "right band"),
        ] {
            assert!(
                resolve_cross_window_drop(windows, (x, y)).is_none(),
                "★{lbl}: a sole floater must not advertise an outer dock",
            );
        }
        // Non-tautological: the SAME band cursor over a window that DOES host a dock
        // area still resolves the outer sentinel.
        let host = dock_window_with_panel("body", Rect::new(0, 0, 1000, 800));
        let hosts = [("main", &host, (0.0, 0.0))];
        assert_eq!(
            resolve_cross_window_drop(hosts, (6.0, 350.0))
                .expect("a dock-hosting window still offers its perimeter")
                .point
                .tag,
            OUTER_DOCK_ZONE_TAG,
        );
    }

    #[test]
    fn r1156_interior_is_an_inner_panel_not_outer() {
        // Away from the perimeter the cursor resolves the inner panel (exact pass),
        // not an outer full-span dock — interior boundaries keep per-panel zones.
        let host = dock_window_with_panel("body", Rect::new(0, 0, 1000, 800));
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
        let host = dock_window_with_panel("body", Rect::new(0, 0, 1000, 800));
        let windows = [("main", &host, (0.0, 0.0))];
        // 200px above the top edge, far beyond the 32px perimeter band.
        assert!(resolve_cross_window_drop(windows, (100.0, -200.0)).is_none());
    }

    #[test]
    fn r1156_outer_drop_point_normalised_over_the_whole_window() {
        // The outer DropPoint carries the cursor normalised over the WHOLE window
        // (not a panel) so the dock consumer (`outer_zone_for`) derives the nearest
        // edge: a top-perimeter cursor has a small y_rel and an x_rel = x / width.
        let host = dock_window_with_panel("body", Rect::new(0, 0, 1000, 800));
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

    /// A capture widget that keeps the WHOLE reading, not just its fraction.
    struct ReadingExternal(Arc<Mutex<Vec<PointerReading>>>);

    impl std::fmt::Debug for ReadingExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ReadingExternal").finish()
        }
    }

    impl pinion_core::external::External for ReadingExternal {
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
            CaptureNormalize::Primary
        }
        fn pointer_move(&mut self, at: PointerReading) {
            self.0.lock().expect("mutex poisoned").push(at);
        }
    }

    /// ★★★★★ R1727 — **the law that makes a captured reading frame-independent.**
    ///
    /// `px()` is the cursor's offset inside the rectangle the reading was taken
    /// over, and therefore does NOT depend on that rectangle's size. Asserted by
    /// painting the SAME origin at two different sizes and requiring the same
    /// pixel out of both — which is exactly the situation a gesture that grows
    /// its own container creates between one frame and the next, and exactly
    /// what a consumer scaling the fraction by a live model count gets wrong.
    #[test]
    fn r1727_px_is_the_offset_in_the_rect_whatever_size_the_rect_is() {
        let mut readings = Vec::new();
        for height in [40_u32, 400_u32] {
            let log = Arc::new(Mutex::new(Vec::new()));
            let mut state = Scene::External(
                ExternalNode::new(Box::new(ReadingExternal(Arc::clone(&log)))).with_tag("board"),
            );
            let mut router = InputRouter::new();
            router.update_paint_scene(
                paint_with_primary_and_subtag(
                    600,
                    600,
                    Rect::new(80, 80, 40, height),
                    Rect::new(96, 80, 8, 8),
                    "board",
                    "cell",
                ),
                &mut state,
            );
            router.cursor_moved(PointerId::MOUSE, 98.0, 100.0, &mut state);
            router.pointer_down(PointerId::MOUSE, &mut state);
            let seen = log.lock().expect("mutex poisoned").clone();
            assert_eq!(seen.len(), 1, "one forwarded move per press");
            readings.push(seen[0]);
        }

        let (short, tall) = (readings[0], readings[1]);
        assert_eq!(
            short.extent,
            (40.0, 40.0),
            "the reading carries the rect it was normalised over"
        );
        assert_eq!(tall.extent, (40.0, 400.0), "and reports the taller one");
        // The FRACTION disagrees, which is the whole hazard: the same pixel is
        // half-way down a short board and a twentieth of the way down a tall one.
        assert!(
            (short.v() - 0.5).abs() < 1e-4 && (tall.v() - 0.05).abs() < 1e-4,
            "the fraction moves with the rect: {} vs {}",
            short.v(),
            tall.v(),
        );
        // And `px` does not.
        assert_eq!(
            short.px(),
            tall.px(),
            "px is cursor - origin, so it is the same pixel in both",
        );
        assert_eq!(short.px(), (18.0, 20.0), "cursor (98,100) - origin (80,80)");
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
                "2:PointerDown::l".into(),
                "2:PointerUp".into(),
            ],
        );
        // The router stores the raw paint tag (with `#`) in its
        // hover map so subsequent leave-on-stray still routes to
        // the right sub-region.
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_group#2"));
    }

    /// R1619 §5.35 §5.41 — the **press** carries the chord it was made with.
    ///
    /// R781 put modifiers on the release only, and that held up for 838 rounds
    /// because every chord-aware widget acted on the activation edge. It stops
    /// holding the moment a gesture *begins* at the press: a drag sweep is a
    /// function of the chord held when the finger went down, and there was no
    /// way to read it.
    ///
    /// This test exists because a counterfactual found nothing catching it —
    /// the round's own demo did, over the real wire, but a demo is not the gate
    /// a round is judged by (the R1618 lesson, recurring).
    #[test]
    fn r1619_the_press_carries_the_chord_it_was_made_with() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_primary_external("main_group");
        let paint = paint_with_subindex_tag(200, 200, Rect::new(80, 80, 40, 40), "main_group", "2");
        router.update_paint_scene(paint, &mut state);
        let ctrl = Modifiers {
            shift: false,
            ctrl: true,
            alt: false,
            meta: false,
        };
        // The chord is absolute state, set out of band exactly as the platform
        // reports it (winit `ModifiersChanged` / the `scene/modifiers` RPC) —
        // never inferred from a key event this router did not see.
        router.set_held_modifiers(ctrl);
        assert_eq!(router.held_modifiers(), ctrl);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up_with_modifiers(PointerId::MOUSE, &mut state, ctrl);
        assert_eq!(
            read(&captures),
            vec![
                // Every arm of the cycle, not only the release: a hover that
                // arrives mid-chord is as much a fact as a click.
                "2:PointerEnter:c".to_string(),
                "2:PointerDown:c:l".into(),
                "2:PointerUp:c".into(),
            ],
        );
        // NEGATIVE CONTROL — with no chord held the wire is the bare form, so
        // the stamp above is the modifier state and not an unconditional token.
        let (mut state, captures) = state_with_primary_external("main_group");
        let mut plain = InputRouter::new();
        plain.update_paint_scene(
            paint_with_subindex_tag(200, 200, Rect::new(80, 80, 40, 40), "main_group", "2"),
            &mut state,
        );
        plain.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        plain.pointer_down(PointerId::MOUSE, &mut state);
        plain.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&captures),
            vec![
                "2:PointerEnter".to_string(),
                "2:PointerDown::l".into(),
                "2:PointerUp".into(),
            ],
        );
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
                // R1619 — the press says the primary button is down; the
                // release says it is not, and carries the modifier token.
                "2:PointerDown::l".into(),
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
        /// R1703 — what this widget says a wheel over it does. `None` is a
        /// widget that declines the gesture entirely, and the router must then
        /// never call [`External::wheel`] on it at all.
        declares: Option<pinion_core::widgets::wheel::WheelIntent>,
    }

    impl WheelExternal {
        fn new(consume: bool) -> (Self, Arc<Mutex<Vec<WheelCall>>>) {
            Self::declaring(
                consume,
                Some(pinion_core::widgets::wheel::WheelIntent::Step(
                    pinion_core::widgets::wheel::StepUnit::Value,
                )),
            )
        }

        fn declaring(
            consume: bool,
            declares: Option<pinion_core::widgets::wheel::WheelIntent>,
        ) -> (Self, Arc<Mutex<Vec<WheelCall>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    calls: Arc::clone(&calls),
                    consume,
                    declares,
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
        fn wheel(&mut self, reading: &WheelReading) -> bool {
            self.calls.lock().expect("mutex poisoned").push((
                reading.at.0,
                reading.at.1,
                reading.dx(),
                reading.dy(),
                reading.modifiers,
            ));
            self.consume
        }

        /// R1703 — what makes this widget reachable at all: the router offers
        /// the event only to a widget that declares an intent.
        fn wheel_intent(
            &self,
            _at: (f32, f32),
        ) -> Option<pinion_core::widgets::wheel::WheelIntent> {
            self.declares
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

    /// ★★★★★ R1703 §5.45 §5.15 — **a widget that declares no wheel is never
    /// offered one**, and the scroll behind it keeps the gesture.
    ///
    /// This exists because a counterfactual PASSED without it. Deleting the
    /// router's precondition entirely — offering `External::wheel` to every
    /// hovered widget whatever it declared — left `cargo test -p hello-node-lab`
    /// and the whole integration demo green, and the reason is that every
    /// widget the demo drives ALSO re-checks its own condition inside `wheel`:
    /// the node canvas tests the canvas rectangle, a shut combo box's list is
    /// not painted, and a slider takes every wheel anyway. So the one
    /// mechanism this round's central claim rests on — the published answer and
    /// the dispatch are one fact — was guarded by nothing at all.
    ///
    /// It is deliberately NOT guarded twice. Making each widget re-check its
    /// own declaration inside `wheel` would hide the precondition rather than
    /// hold it: the rule would still be gone and everything would still pass.
    /// One mechanism, and a test that drives it.
    #[test]
    fn r1703_a_widget_that_declares_no_wheel_is_never_offered_one() {
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        // Consuming AND silent: if the router offers it at all, the wheel is
        // eaten and the scroll below stays put — so the assertion below fails
        // in the loudest possible way rather than by a subtle coordinate.
        let (ext, calls) = WheelExternal::declaring(true, None);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        assert!(
            router.wheel(
                PointerId::MOUSE,
                WheelDelta::Lines { dx: 0.0, dy: 2.0 },
                &mut state_scene,
            ),
            "the wheel still dispatched — to the scroll container"
        );
        assert!(
            calls.lock().expect("mutex poisoned").is_empty(),
            "a widget declaring no wheel was handed one anyway, so what \
             `scene/wheel_intent` publishes no longer describes what happens"
        );
        // Two lines at the framework's own line height, which is what the
        // scroll fallback rounds a `Lines` delta to.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "LINE_HEIGHT_PX is the integral constant 16; the product \
                      is an exact scroll offset"
        )]
        let two_lines = (2.0 * LINE_HEIGHT_PX) as i32;
        assert_eq!(
            scroll.offset(),
            (0, two_lines),
            "the wheel reached the scroll chain exactly as if the widget were \
             not there"
        );
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
            GesturePhase::Update,
            &mut state_scene,
        ));
        let recorded = calls.lock().expect("mutex poisoned").clone();
        assert_eq!(recorded.len(), 1);
        assert!(
            recorded[0].4.control_key(),
            "ctrl modifier must reach the External"
        );
    }

    // ─── R1432 §5.35 §5.15 native pinch-gesture offer tests ────────

    /// One recorded [`External::pinch_gesture`] call:
    /// `(x_rel, y_rel, magnification, phase, modifiers)`.
    type PinchCall = (f32, f32, f64, GesturePhase, Modifiers);

    /// Records every [`External::pinch_gesture`] call; consumes (returns `true`)
    /// iff `consume` is set.
    struct PinchExternal {
        calls: Arc<Mutex<Vec<PinchCall>>>,
        consume: bool,
    }

    impl PinchExternal {
        fn new(consume: bool) -> (Self, Arc<Mutex<Vec<PinchCall>>>) {
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

    impl std::fmt::Debug for PinchExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PinchExternal").finish()
        }
    }

    impl pinion_core::external::External for PinchExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn pinch_gesture(
            &mut self,
            x_rel: f32,
            y_rel: f32,
            magnification: f64,
            phase: GesturePhase,
            modifiers: Modifiers,
        ) -> bool {
            self.calls.lock().expect("mutex poisoned").push((
                x_rel,
                y_rel,
                magnification,
                phase,
                modifiers,
            ));
            self.consume
        }
    }

    #[test]
    fn r1432_pinch_gesture_offered_to_hovered_external() {
        // The External the cursor hovers receives the pinch: coordinates
        // normalised over its rect (cursor (100, 90) over rect 80..120 → rel
        // (0.5, 0.25)), the incremental magnification + phase + modifiers
        // forwarded verbatim, and the consume verdict returned.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, calls) = PinchExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::empty()
        };
        assert!(router.pinch_gesture(
            PointerId::MOUSE,
            0.25,
            GesturePhase::Update,
            ctrl,
            &mut state_scene,
        ));
        // A native gesture never touches the scroll fallback (there is none).
        assert_eq!(scroll.offset(), (0, 0), "pinch must not scroll");
        let recorded = calls.lock().expect("mutex poisoned").clone();
        assert_eq!(recorded.len(), 1);
        let (x_rel, y_rel, magnification, phase, mods) = recorded[0];
        assert!((x_rel - 0.5).abs() < 1e-6, "x_rel {x_rel}");
        assert!((y_rel - 0.25).abs() < 1e-6, "y_rel {y_rel}");
        assert!(
            (magnification - 0.25).abs() < 1e-9,
            "magnification {magnification}"
        );
        assert_eq!(phase, GesturePhase::Update);
        assert!(mods.control_key(), "ctrl modifier must reach the External");
    }

    #[test]
    fn r1432_pinch_gesture_off_target_is_noop() {
        // With the cursor over no tagged widget (10, 10 is outside the 80..120
        // button, and Scroll content is path-transparent), the pinch resolves
        // no target and is a clean no-op — false, nothing recorded.
        let scroll = Rc::new(ScrollState::new());
        let (ext, calls) = PinchExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(scroll, None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state_scene);
        assert!(
            !router.pinch_gesture(
                PointerId::MOUSE,
                0.5,
                GesturePhase::Begin,
                Modifiers::empty(),
                &mut state_scene,
            ),
            "no hovered target → no consume"
        );
        assert!(
            calls.lock().expect("mutex poisoned").is_empty(),
            "no target → no offer"
        );
    }

    // ─── R1433 §5.35 §5.15 native rotation-gesture offer tests ─────

    /// One recorded [`External::rotation_gesture`] call:
    /// `(x_rel, y_rel, rotation, phase, modifiers)`.
    type RotationCall = (f32, f32, f64, GesturePhase, Modifiers);

    /// Records every [`External::rotation_gesture`] call; consumes (returns
    /// `true`) iff `consume` is set. The [`PinchExternal`] rotation peer.
    struct RotationExternal {
        calls: Arc<Mutex<Vec<RotationCall>>>,
        consume: bool,
    }

    impl RotationExternal {
        fn new(consume: bool) -> (Self, Arc<Mutex<Vec<RotationCall>>>) {
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

    impl std::fmt::Debug for RotationExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RotationExternal").finish()
        }
    }

    impl pinion_core::external::External for RotationExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn rotation_gesture(
            &mut self,
            x_rel: f32,
            y_rel: f32,
            rotation: f64,
            phase: GesturePhase,
            modifiers: Modifiers,
        ) -> bool {
            self.calls
                .lock()
                .expect("mutex poisoned")
                .push((x_rel, y_rel, rotation, phase, modifiers));
            self.consume
        }
    }

    #[test]
    fn r1433_rotation_gesture_offered_to_hovered_external() {
        // The External the cursor hovers receives the rotation: coordinates
        // normalised over its rect (cursor (100, 90) over rect 80..120 → rel
        // (0.5, 0.25)), the incremental rotation (degrees) + phase + modifiers
        // forwarded verbatim, and the consume verdict returned.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, calls) = RotationExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::empty()
        };
        assert!(router.rotation_gesture(
            PointerId::MOUSE,
            15.0,
            GesturePhase::Update,
            shift,
            &mut state_scene,
        ));
        // A native gesture never touches the scroll fallback (there is none).
        assert_eq!(scroll.offset(), (0, 0), "rotation must not scroll");
        let recorded = calls.lock().expect("mutex poisoned").clone();
        assert_eq!(recorded.len(), 1);
        let (x_rel, y_rel, rotation, phase, mods) = recorded[0];
        assert!((x_rel - 0.5).abs() < 1e-6, "x_rel {x_rel}");
        assert!((y_rel - 0.25).abs() < 1e-6, "y_rel {y_rel}");
        assert!((rotation - 15.0).abs() < 1e-9, "rotation {rotation}");
        assert_eq!(phase, GesturePhase::Update);
        assert!(mods.shift_key(), "shift modifier must reach the External");
    }

    #[test]
    fn r1433_rotation_gesture_off_target_is_noop() {
        // With the cursor over no tagged widget (10, 10 is outside the 80..120
        // button), the rotation resolves no target and is a clean no-op — false,
        // nothing recorded.
        let scroll = Rc::new(ScrollState::new());
        let (ext, calls) = RotationExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(scroll, None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state_scene);
        assert!(
            !router.rotation_gesture(
                PointerId::MOUSE,
                30.0,
                GesturePhase::Begin,
                Modifiers::empty(),
                &mut state_scene,
            ),
            "no hovered target → no consume"
        );
        assert!(
            calls.lock().expect("mutex poisoned").is_empty(),
            "no target → no offer"
        );
    }

    /// R1434 — one recorded [`External::pan_gesture`] offer. TWO delta axes,
    /// where the pinch / rotation peers carry one scalar — the payload shape the
    /// shared `offer_to_hovered_external` had to stay agnostic of.
    type PanCall = (f32, f32, f32, f32, GesturePhase, Modifiers);

    /// Records every [`External::pan_gesture`] call; consumes (returns `true`)
    /// iff `consume` is set. The [`PinchExternal`] pan peer.
    struct PanExternal {
        calls: Arc<Mutex<Vec<PanCall>>>,
        consume: bool,
    }

    impl PanExternal {
        fn new(consume: bool) -> (Self, Arc<Mutex<Vec<PanCall>>>) {
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

    impl std::fmt::Debug for PanExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PanExternal").finish()
        }
    }

    impl pinion_core::external::External for PanExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn pan_gesture(
            &mut self,
            x_rel: f32,
            y_rel: f32,
            delta_x: f32,
            delta_y: f32,
            phase: GesturePhase,
            modifiers: Modifiers,
        ) -> bool {
            self.calls
                .lock()
                .expect("mutex poisoned")
                .push((x_rel, y_rel, delta_x, delta_y, phase, modifiers));
            self.consume
        }
    }

    #[test]
    fn r1434_pan_gesture_offered_to_hovered_external() {
        // The External the cursor hovers receives the pan: coordinates
        // normalised over its rect (cursor (100, 90) over rect 80..120 → rel
        // (0.5, 0.25)), BOTH delta axes + phase + modifiers forwarded verbatim
        // (the delta keeps the platform's sign — a pan is direct manipulation,
        // not a sign-flipped scroll command), and the consume verdict returned.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, calls) = PanExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::empty()
        };
        assert!(router.pan_gesture(
            PointerId::MOUSE,
            12.0,
            -7.5,
            GesturePhase::Update,
            shift,
            &mut state_scene,
        ));
        // A native gesture never touches the scroll fallback (there is none) —
        // the one behaviour that separates this from the wheel path.
        assert_eq!(scroll.offset(), (0, 0), "pan must not scroll");
        let recorded = calls.lock().expect("mutex poisoned").clone();
        assert_eq!(recorded.len(), 1);
        let (x_rel, y_rel, delta_x, delta_y, phase, mods) = recorded[0];
        assert!((x_rel - 0.5).abs() < 1e-6, "x_rel {x_rel}");
        assert!((y_rel - 0.25).abs() < 1e-6, "y_rel {y_rel}");
        assert!((delta_x - 12.0).abs() < 1e-6, "delta_x {delta_x}");
        assert!((delta_y + 7.5).abs() < 1e-6, "delta_y {delta_y}");
        assert_eq!(phase, GesturePhase::Update);
        assert!(mods.shift_key(), "shift modifier must reach the External");
    }

    #[test]
    fn r1434_pan_gesture_off_target_is_noop() {
        // With the cursor over no tagged widget (10, 10 is outside the 80..120
        // button), the pan resolves no target and is a clean no-op — false,
        // nothing recorded. The guard the three gestures now share
        // (`hovered_gesture_target`) is what makes this uniform.
        let scroll = Rc::new(ScrollState::new());
        let (ext, calls) = PanExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(scroll, None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state_scene);
        assert!(
            !router.pan_gesture(
                PointerId::MOUSE,
                10.0,
                10.0,
                GesturePhase::Begin,
                Modifiers::empty(),
                &mut state_scene,
            ),
            "no hovered target → no consume"
        );
        assert!(
            calls.lock().expect("mutex poisoned").is_empty(),
            "no target → no offer"
        );
    }

    #[test]
    fn r1434_native_pan_gesture_does_not_touch_the_drag_pan_latch() {
        // The R1434 rename's substance: the native pan axis and the held-button
        // DRAG pan (`DragPan`, reported by `drag_pan_in_flight`) are unrelated
        // state. A native pan mid-flight must leave the drag latch exactly as it
        // found it — before R1434 the two shared a name, which is the confusion
        // this asserts can never become behaviour.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, _calls) = PanExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        assert!(
            !router.drag_pan_in_flight(PointerId::MOUSE),
            "no press yet → no drag latch"
        );
        assert!(router.pan_gesture(
            PointerId::MOUSE,
            25.0,
            0.0,
            GesturePhase::Begin,
            Modifiers::empty(),
            &mut state_scene,
        ));
        assert!(
            !router.drag_pan_in_flight(PointerId::MOUSE),
            "a native pan gesture must not open a drag-pan latch"
        );
    }

    /// R1435 — one recorded [`External::smart_zoom_gesture`] offer: the anchor
    /// and the modifiers, and nothing else. The family's phase-less member, so
    /// the tuple is the SHORTEST of the four — the other end of the payload
    /// range `offer_to_hovered_external`'s closure has to span.
    type SmartZoomCall = (f32, f32, Modifiers);

    /// Records every [`External::smart_zoom_gesture`] call; consumes (returns
    /// `true`) iff `consume` is set.
    struct SmartZoomExternal {
        calls: Arc<Mutex<Vec<SmartZoomCall>>>,
        consume: bool,
    }

    impl SmartZoomExternal {
        fn new(consume: bool) -> (Self, Arc<Mutex<Vec<SmartZoomCall>>>) {
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

    impl std::fmt::Debug for SmartZoomExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SmartZoomExternal").finish()
        }
    }

    impl pinion_core::external::External for SmartZoomExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn smart_zoom_gesture(&mut self, x_rel: f32, y_rel: f32, modifiers: Modifiers) -> bool {
            self.calls
                .lock()
                .expect("mutex poisoned")
                .push((x_rel, y_rel, modifiers));
            self.consume
        }
    }

    #[test]
    fn r1435_smart_zoom_gesture_offered_to_hovered_external() {
        // The External the cursor hovers receives the toggle with the cursor
        // normalised over its rect (cursor (100, 90) over rect 80..120 → rel
        // (0.5, 0.25)). The anchor is the entire payload — it is what selects
        // the object to fit — so the assertion that matters most is that it
        // arrives intact.
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(500, 500);
        let (ext, calls) = SmartZoomExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(Rc::clone(&scroll), None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 90.0, &mut state_scene);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::empty()
        };
        assert!(router.smart_zoom_gesture(PointerId::MOUSE, shift, &mut state_scene));
        assert_eq!(scroll.offset(), (0, 0), "smart zoom must not scroll");
        let recorded = calls.lock().expect("mutex poisoned").clone();
        assert_eq!(recorded.len(), 1);
        let (x_rel, y_rel, mods) = recorded[0];
        assert!((x_rel - 0.5).abs() < 1e-6, "x_rel {x_rel}");
        assert!((y_rel - 0.25).abs() < 1e-6, "y_rel {y_rel}");
        assert!(mods.shift_key(), "shift modifier must reach the External");
        // Each call is one completed toggle — two gestures are two offers, with
        // nothing accumulated in between (there is no arc to accumulate over).
        assert!(router.smart_zoom_gesture(PointerId::MOUSE, Modifiers::empty(), &mut state_scene));
        assert_eq!(calls.lock().expect("mutex poisoned").len(), 2);
    }

    #[test]
    fn r1435_smart_zoom_gesture_off_target_is_noop() {
        // With the cursor over no tagged widget (10, 10 is outside the 80..120
        // button), the gesture resolves no target and is a clean no-op — the
        // same shared `hovered_gesture_target` guard the other three use.
        let scroll = Rc::new(ScrollState::new());
        let (ext, calls) = SmartZoomExternal::new(true);
        let mut state_scene =
            Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_btn"));
        let mut router = InputRouter::new();
        router.update_paint_scene(
            paint_with_button_over_scroll(scroll, None),
            &mut state_scene,
        );
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state_scene);
        assert!(
            !router.smart_zoom_gesture(PointerId::MOUSE, Modifiers::empty(), &mut state_scene),
            "no hovered target → no consume"
        );
        assert!(
            calls.lock().expect("mutex poisoned").is_empty(),
            "no target → no offer"
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
        // DCC grab semantic (no motion is lost to the dead zone).
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
        // Integer scroll offsets round per move; the toolkit-style
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
        // (Shift+middle-drag = plain pan, the DCC chord) while
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
        // Release-in-place reports Click — which the shell treats as inert for
        // the left chord (the design tool: Space+click does nothing). The
        // gesture is consumed: a second release is NoPress.
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
        let (mut state_scene, _) = state_with_button();
        assert_eq!(
            resolve_pointer_tag(&scene, 200.0, 200.0).as_deref(),
            Some("panel#content"),
            "hover stays on the deepest tag (unchanged)",
        );

        // The DropPoint names the panel, normalised over the PANEL rect
        // (400 wide): (200 - 0) / 400 = 0.5 — the panel centre, what the
        // dock zone classifier reads, not a content-relative coordinate.
        let mut router = InputRouter::new();
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
        use pinion_core::external::DOCK_SURFACE_TAG;
        use pinion_core::scene::{BoxNode, ContainerNode};
        use pinion_core::style::{Color, LayoutStyle};

        let content = Scene::Box(
            BoxNode::filled(Rect::new(0, 0, 400, 400), Color::default()).with_tag("panel#content"),
        );
        let mut panel = ContainerNode::new(vec![content])
            .with_tag("panel")
            .with_layout(LayoutStyle::new().with_drop_target(true));
        panel.rect = Rect::new(0, 0, 400, 400);
        // (R1322) The band is measured against the DOCK AREA, so the scene must carry
        // the walker's `DOCK_SURFACE_TAG` wrapper — as every real dock-hosting window
        // does since R1205. A window WITHOUT one hosts no dock and gets no outer zone
        // (pinned by `r1322_no_dock_surface_no_outer_zone`).
        let mut surface = ContainerNode::new(vec![Scene::Container(panel)])
            .with_tag(DOCK_SURFACE_TAG.to_string());
        surface.rect = Rect::new(0, 0, 400, 400);
        let scene = Scene::Container(surface);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        router.update_paint_scene(scene, &mut state_scene);

        let dock = DragPayload::new(DOCK_PANEL_DRAG_KIND, IntrospectValue::Text("panel".into()));
        let tree = DragPayload::new("tree-node", IntrospectValue::Text("n".into()));

        // A cursor within OUTER_DOCK_MARGIN of the LEFT edge → the full-span OUTER
        // sentinel (normalised over the WHOLE window: 10/400, 200/400).
        let outer = router
            .resolve_own_outer_dock(10.0, 200.0)
            .expect("left band is outer");
        assert_eq!(outer.tag, OUTER_DOCK_ZONE_TAG);
        assert!((outer.x_rel - 0.025).abs() < 1e-4 && (outer.y_rel - 0.5).abs() < 1e-4);

        // The override fires for a dock-panel drag near the edge...
        // (R1348) `state_with_button`'s source is not a dock panel, so it takes the
        // default `accepts_outer_dock` (accept) — the claim is unchanged here.
        assert_eq!(
            router
                .resolve_drag_own_over(&dock, "panel", 10.0, 200.0, &mut state_scene)
                .expect("dock outer")
                .tag,
            OUTER_DOCK_ZONE_TAG,
            "a dock-panel drag near the edge gets the full-span outer sentinel",
        );
        // ...but NOT for a non-dock drag (the gate): the inner hit-test wins.
        assert_eq!(
            router
                .resolve_drag_own_over(&tree, "panel", 10.0, 200.0, &mut state_scene)
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
                .resolve_drag_own_over(&dock, "panel", 200.0, 200.0, &mut state_scene)
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

    /// (R1348) A dock-panel-shaped drag source that refuses the LEFT perimeter when
    /// `refuse_left` — the router-side stand-in for
    /// `DockPanelExternal::accepts_outer_dock`'s live redundancy answer (the real
    /// predicate is topology-owned and lives in the sibling crate; what the ROUTER
    /// must be pinned on is that it HONOURS a refusal).
    struct VetoSource {
        refuse_left: bool,
    }

    impl std::fmt::Debug for VetoSource {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("VetoSource").finish()
        }
    }

    impl pinion_core::external::External for VetoSource {
        // §5.15 mandatory declarations (inert for a pure drag-source stub).
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn begin_drag(&self) -> Option<DragPayload> {
            Some(DragPayload::new(
                DOCK_PANEL_DRAG_KIND,
                IntrospectValue::Text("panel".into()),
            ))
        }
        fn accepts_outer_dock(&self, _payload: &DragPayload, point: &DropPoint) -> bool {
            assert_eq!(
                point.tag, OUTER_DOCK_ZONE_TAG,
                "the veto is only ever asked about the perimeter sentinel",
            );
            // "Left" ONLY for the two points this test feeds (x_rel near 0 / near 1
            // at mid-height). Deliberately NOT `outer_zone_for`'s real nearest-edge
            // classification — that lives in the sibling crate this one cannot dep
            // on, and the router's contract under test is "a refusal is honoured",
            // not "the source classifies edges correctly".
            !(point.x_rel < 0.5 && self.refuse_left)
        }
    }

    /// (R1348) A dock surface filling a 400x400 window with ONE drop-target panel —
    /// so a perimeter the router does not claim resolves to `"panel"` instead.
    fn r1348_dock_paint_scene() -> Scene {
        use pinion_core::external::DOCK_SURFACE_TAG;
        use pinion_core::scene::{BoxNode, ContainerNode};
        use pinion_core::style::{Color, LayoutStyle};
        let content = Scene::Box(
            BoxNode::filled(Rect::new(0, 0, 400, 400), Color::default()).with_tag("panel#content"),
        );
        let mut panel = ContainerNode::new(vec![content])
            .with_tag("panel")
            .with_layout(LayoutStyle::new().with_drop_target(true));
        panel.rect = Rect::new(0, 0, 400, 400);
        let mut surface = ContainerNode::new(vec![Scene::Container(panel)])
            .with_tag(DOCK_SURFACE_TAG.to_string());
        surface.rect = Rect::new(0, 0, 400, 400);
        Scene::Container(surface)
    }

    /// ★R1348 §5.51 PR-57 — the drag SOURCE vetoes the OUTER perimeter claim, and
    /// the vetoed band falls through to the panel beneath it.
    ///
    /// The bug this pins: R1201 declared "an outer drop indicator is offered only
    /// when the outcome differs" but enforced it at RESOLVE only — the router
    /// claimed the perimeter UNCONDITIONALLY, so a redundant edge kept the claim
    /// (the sentinel replaced the inner hit-test) while the outcome died as a
    /// `SnapBack`. The band previewed nothing, did nothing, AND made the split
    /// bands of the panel underneath unreachable: a dead strip. With exactly 2
    /// pane slots EVERY edge is redundant (R1338), so the ENTIRE perimeter of a
    /// 2-pane dock was dead — the most common IDE / terminal layout there is.
    ///
    /// The claim now asks the source first, so "claimed but inert" is
    /// unrepresentable rather than merely unwanted.
    #[test]
    fn r1348_a_vetoed_outer_claim_falls_through_to_the_panel_beneath() {
        use pinion_core::scene::ExternalNode;

        let paint = r1348_dock_paint_scene;
        let dock = DragPayload::new(DOCK_PANEL_DRAG_KIND, IntrospectValue::Text("panel".into()));

        // ── ACCEPTING source: the claim stands (the R1167 baseline) ──────────
        let mut router = InputRouter::new();
        let mut state = Scene::External(
            ExternalNode::new(Box::new(VetoSource { refuse_left: false })).with_tag("src"),
        );
        router.update_paint_scene(paint(), &mut state);
        assert_eq!(
            router
                .resolve_drag_own_over(&dock, "src", 10.0, 200.0, &mut state)
                .expect("accepted claim")
                .tag,
            OUTER_DOCK_ZONE_TAG,
            "an accepting source keeps the full-span outer sentinel (R1167 unchanged)",
        );

        // ── VETOING source: the band becomes ordinary interior ───────────────
        let mut router = InputRouter::new();
        let mut state = Scene::External(
            ExternalNode::new(Box::new(VetoSource { refuse_left: true })).with_tag("src"),
        );
        router.update_paint_scene(paint(), &mut state);
        // The geometric band is UNCHANGED — the veto is a claim decision, not a
        // band-geometry change (so a non-vetoed edge is unaffected, below).
        assert_eq!(
            router
                .resolve_own_outer_dock(10.0, 200.0)
                .expect("the left band is still geometrically outer")
                .tag,
            OUTER_DOCK_ZONE_TAG,
        );
        assert_eq!(
            router
                .resolve_drag_own_over(&dock, "src", 10.0, 200.0, &mut state)
                .expect("★the vetoed band resolves the panel underneath")
                .tag,
            "panel",
            "★a vetoed perimeter falls through to the inner hit-test — the panel \
             beneath keeps its own split bands instead of a dead strip",
        );
        // ★Non-tautological: the SAME source still claims an edge it does NOT
        // refuse, so the veto is per-edge, not a blanket opt-out of outer docking.
        assert_eq!(
            router
                .resolve_drag_own_over(&dock, "src", 390.0, 200.0, &mut state)
                .expect("right claim")
                .tag,
            OUTER_DOCK_ZONE_TAG,
            "an un-refused edge still claims the perimeter",
        );
        // A source the state scene cannot resolve accepts (the claim is unchanged
        // when the source cannot be asked) — an out-of-sync tag must not silently
        // drop the zone.
        assert_eq!(
            router
                .resolve_drag_own_over(&dock, "gone", 10.0, 200.0, &mut state)
                .expect("unresolvable source")
                .tag,
            OUTER_DOCK_ZONE_TAG,
            "an unresolvable source keeps the pre-R1348 claim",
        );
    }

    #[test]
    fn r1322_no_dock_surface_no_outer_zone() {
        // ★R1322 §5.51 — a window with NO dock area synthesizes NO outer-dock zone.
        //
        // The regression this pins: `resolve_own_outer_dock` measured its band against
        // `Scene::dock_surface_rect()`, which FALLS BACK to the window rect when the
        // scene carries no `DOCK_SURFACE_TAG`. A torn-off panel's floating window has
        // no dock area at all, so its whole edge band became an own-window outer-dock
        // target — and since that sentinel is not the dragged panel's own subtree,
        // `own_drop_is_self` was false, `resolve_drag_targets` kept own-window-first,
        // and the R1124 live floater→main redock silently degraded to a bare
        // `window_move` (demo `r1124_floater_drag_back_redock`, red since R1167).
        //
        // The floater scene below is exactly what `view_floating_panel` paints: a lone
        // dock panel, no dock-surface wrapper.
        use pinion_core::scene::{BoxNode, ContainerNode};
        use pinion_core::style::{Color, LayoutStyle};

        let content = Scene::Box(
            BoxNode::filled(Rect::new(0, 0, 420, 320), Color::default()).with_tag("panel#content"),
        );
        let mut panel = ContainerNode::new(vec![content])
            .with_tag("panel")
            .with_layout(LayoutStyle::new().with_drop_target(true));
        panel.rect = Rect::new(0, 0, 420, 320);
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        router.update_paint_scene(Scene::Container(panel), &mut state_scene);

        // A cursor deep in the floater's edge band — where the fallback used to mint the
        // sentinel — resolves NO outer zone…
        assert!(
            router.resolve_own_outer_dock(6.0, 160.0).is_none(),
            "★a window with no dock area must not synthesize an outer-dock zone",
        );
        // …so the own-over stays the plain hit-test, which `own_drop_is_self` recognises
        // as the dragged panel's own subtree and therefore yields to the cross-window
        // redock (the R1124 rule, reachable again).
        let dock = DragPayload::new(DOCK_PANEL_DRAG_KIND, IntrospectValue::Text("panel".into()));
        assert_eq!(
            router
                .resolve_drag_own_over(&dock, "panel", 6.0, 160.0, &mut state_scene)
                .expect("the inner hit-test still resolves")
                .tag,
            "panel",
            "★the own hit is the panel itself (a self-drop), not an outer sentinel",
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
    fn r1205_dock_surface_rect_reads_the_tagged_wrapper() {
        use pinion_core::external::DOCK_SURFACE_TAG;
        use pinion_core::scene::ContainerNode;
        // A dock-area wrapper (below a 32px chrome strip) nested in the window root.
        let mut surface = ContainerNode::new(vec![]).with_tag(DOCK_SURFACE_TAG.to_string());
        surface.rect = Rect::new(0, 32, 400, 568);
        let mut root = ContainerNode::new(vec![Scene::Container(surface)]);
        root.rect = Rect::new(0, 0, 400, 600);
        assert_eq!(
            Scene::Container(root).dock_surface_rect(),
            Rect::new(0, 32, 400, 568),
            "the dock surface tag's laid-out rect is the dock area (below the chrome)",
        );
        // No dock surface (a naked / decorated window) → the whole window rect.
        let mut bare = ContainerNode::new(vec![]);
        bare.rect = Rect::new(0, 0, 400, 600);
        assert_eq!(
            Scene::Container(bare).dock_surface_rect(),
            Rect::new(0, 0, 400, 600),
            "no dock surface → fall back to the window rect",
        );
    }

    #[test]
    fn r1205_resolve_own_outer_dock_measures_the_dock_surface_below_chrome() {
        use pinion_core::external::DOCK_SURFACE_TAG;
        use pinion_core::scene::{BoxNode, ContainerNode};
        use pinion_core::style::{Color, LayoutStyle};
        // A window whose dock surface (`DOCK_SURFACE_TAG` wrapper) is inset 32px
        // below a client-side chrome strip: the dock area is y ∈ [32, 600].
        fn chromed_paint_scene() -> Scene {
            let content = Scene::Box(
                BoxNode::filled(Rect::new(0, 32, 400, 568), Color::default())
                    .with_tag("panel#content"),
            );
            let mut surface = ContainerNode::new(vec![content])
                .with_tag(DOCK_SURFACE_TAG.to_string())
                .with_layout(LayoutStyle::new().with_drop_target(true));
            surface.rect = Rect::new(0, 32, 400, 568);
            let mut root = ContainerNode::new(vec![Scene::Container(surface)]);
            root.rect = Rect::new(0, 0, 400, 600);
            Scene::Container(root)
        }
        let mut router = InputRouter::new();
        let (mut state_scene, _) = state_with_button();
        router.update_paint_scene(chromed_paint_scene(), &mut state_scene);
        // A cursor IN the chrome strip (y=10 < 32) has left the dock area upward —
        // an escape, NOT the top outer band (it would land on the min/max/close
        // controls, not the dock).
        assert!(
            router.resolve_own_outer_dock(200.0, 10.0).is_none(),
            "the chrome strip is above the dock surface, not the top outer band",
        );
        // The dock surface's TOP edge (y=40, 8px below the chrome) IS the top outer
        // band, normalised over the DOCK surface (so `outer_zone_for` derives Top).
        let top = router
            .resolve_own_outer_dock(200.0, 40.0)
            .expect("the dock surface top edge is the outer band");
        assert_eq!(top.tag, OUTER_DOCK_ZONE_TAG);
        assert!(
            top.y_rel < 0.1,
            "normalised over the dock surface → near its top (y_rel={})",
            top.y_rel,
        );
        // (R1322) WITHOUT a dock surface there is NO band at all — not, as pre-R1322,
        // the whole window rect via `dock_surface_rect`'s fallback. That fallback is
        // what let a torn-off panel's own floating window mint an outer-dock sentinel
        // and mask the R1124 cross-window redock; a window with no dock area cannot
        // receive a dock. (This assertion previously pinned the fallback — i.e. it
        // pinned the bug. See `r1322_no_dock_surface_no_outer_zone`.)
        let mut bare = ContainerNode::new(vec![]);
        bare.rect = Rect::new(0, 0, 400, 600);
        router.update_paint_scene(Scene::Container(bare), &mut state_scene);
        assert!(
            router.resolve_own_outer_dock(200.0, 10.0).is_none(),
            "no dock surface → no outer-dock zone anywhere in the window",
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
                DragPayload::new(
                    "dnd-row",
                    // R1113 — a labelled source emits a Text payload (dock-panel
                    // shape); the default reorder source keeps its Int row index.
                    match self.label {
                        Some(l) => IntrospectValue::Text(l.to_string()),
                        None => IntrospectValue::Int(i64::try_from(i).unwrap_or(0)),
                    },
                )
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
            IntrospectSchema::new(const { &[] })
        }
        fn query(&self, _path: &str) -> Result<IntrospectValue, ReadRefusal> {
            Err(ReadRefusal::UnknownPath)
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
                    // Composite "{idx}:{Event}[:<mods>[:<buttons>]]" wire form
                    // (R51.42 / R781 / R1619), decoded through the grammar SSOT.
                    // R1619 — this fixture used to `split_once(':')` and compare
                    // the remainder to `"PointerDown"`, which stopped matching
                    // the moment the wire grew its fourth segment. A fixture
                    // that re-derives the grammar is exactly what
                    // `split_send_payload` exists to prevent, and it is not
                    // exempt from that argument for being a fixture.
                    if let Some(sent) = pinion_core::composite_tag::split_send_payload(&name) {
                        if sent.event == PointerWireEvent::Down.as_wire_name()
                            && let Ok(i) = sent.key.parse::<usize>()
                        {
                            self.pressed.set(Some(i));
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
        assert!(log.contains(&"0:PointerDown::l".to_string()), "{log:?}");
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
        // PointerUp (the toolkit startDragDistance / DOM no-click-after-drag).
        // This is what lets a file move / tab reorder not also activate the
        // source.
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

    // ── R1734 §5.51 — the TARGET side of a drag ────────────────────────────
    //
    // Four SEPARATE `External`s in one scene: a palette that is only a source,
    // a board and a bin that are only targets, and a trash well that declares
    // an action the palette cannot offer. Before this round that arrangement
    // could not be spelled at all — every event of a session went back to the
    // surface that opened it, so a destination could not be asked anything,
    // could not preview and could not receive a drop.
    //
    // Driven through the ROUTER's public entry points (a real press, real
    // moves, a real release), never by calling the dispatch helpers: a helper
    // called directly proves the helper, and the claim here is about routing.

    /// The payload kind the fixture palette hands the fixture board.
    const FIXTURE_KIND: &str = "board-widget";

    /// A source and nothing else: it declares no drop contract, so the router
    /// must never offer it anything — including its own drag, when the cursor
    /// passes back over it.
    struct PaletteExternal {
        log: Arc<Mutex<Vec<String>>>,
        actions: DropActions,
    }

    impl std::fmt::Debug for PaletteExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PaletteExternal").finish()
        }
    }

    impl pinion_core::external::External for PaletteExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn begin_drag(&self) -> Option<DragPayload> {
            self.log.lock().expect("poisoned").push("begin".to_owned());
            Some(
                DragPayload::new(FIXTURE_KIND, IntrospectValue::Text("throughput".to_owned()))
                    .with_actions(self.actions),
            )
        }
        fn drop_offered(&mut self, _offer: &DropOffer) -> DropVerdict {
            self.log
                .lock()
                .expect("poisoned")
                .push("palette:offered".to_owned());
            DropVerdict::decline("a palette is not a drop target")
        }
        /// ★★★★★ R1735 — the SOURCE writes down what it was told a release
        /// would do. The floor tells a source in this position an object
        /// identity and an action; this records the whole standing, the
        /// sentence it carries and the live cursor beside it, so a test can
        /// assert each of the three things that measurement says are missing.
        fn drag_to_at(&mut self, _payload: &DragPayload, update: &DragUpdate) {
            let mut log = self.log.lock().expect("poisoned");
            log.push(format!("palette:told:{}", standing_line(&update.standing)));
            log.push(format!("palette:why:{}", update.standing.sentence()));
            #[expect(
                clippy::cast_possible_truncation,
                reason = "fixture cursors are whole pixels"
            )]
            log.push(format!(
                "palette:cursor:{}:{}",
                update.cursor.0 as i64, update.cursor.1 as i64
            ));
        }
        fn drag_release_at(&mut self, _payload: &DragPayload, update: &DragUpdate) {
            self.log.lock().expect("poisoned").push(format!(
                "palette:release-told:{}",
                standing_line(&update.standing)
            ));
        }
    }

    /// R1735 — one line naming everything a standing carries, so an assertion
    /// reads as the sentence the source was handed.
    fn standing_line(standing: &DropStanding) -> String {
        match standing {
            DropStanding::Nowhere => "nowhere".to_owned(),
            DropStanding::Refused { tag, refusal } => {
                format!("refused:{tag}:{}", refusal.as_wire_name())
            }
            DropStanding::Accepted { tag, accept } => {
                let landing = match &accept.landing {
                    IntrospectValue::Text(t) => t.clone(),
                    other => other.kind().to_owned(),
                };
                format!("accepted:{tag}:{}:{landing}", accept.action.as_wire_name())
            }
        }
    }

    /// How a target answers an offer its declaration already admitted.
    #[derive(Clone, Copy)]
    enum BoardPolicy {
        /// Accept, landing on the slot the cursor is over.
        Land,
        /// Decline for a reason only live state knows.
        Full,
    }

    /// A target and nothing else. Declares a contract, previews a landing, and
    /// commits the acceptance the router hands back.
    struct BoardExternal {
        name: &'static str,
        log: Arc<Mutex<Vec<String>>>,
        contract: DropContract,
        policy: BoardPolicy,
    }

    impl std::fmt::Debug for BoardExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("BoardExternal").finish()
        }
    }

    impl BoardExternal {
        fn new(
            name: &'static str,
            log: &Arc<Mutex<Vec<String>>>,
            contract: DropContract,
            policy: BoardPolicy,
        ) -> Self {
            Self {
                name,
                log: Arc::clone(log),
                contract,
                policy,
            }
        }

        fn say(&self, line: String) {
            self.log.lock().expect("poisoned").push(line);
        }
    }

    impl pinion_core::external::External for BoardExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
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
        fn drop_offered(&mut self, offer: &DropOffer) -> DropVerdict {
            let part = offer.part().unwrap_or("surface").to_owned();
            self.say(format!("{}:offered:{part}", self.name));
            match self.policy {
                BoardPolicy::Land => DropVerdict::accept(
                    offer.actions.first(),
                    IntrospectValue::Text(format!("landing-{part}")),
                ),
                BoardPolicy::Full => DropVerdict::decline("every slot is taken"),
            }
        }
        fn drop_left(&mut self) {
            self.say(format!("{}:left", self.name));
        }
        fn drop_commit(&mut self, offer: &DropOffer, accept: &DropAccept) {
            // Deliberately reads the WITNESS and not the offer's geometry: the
            // whole point of the signature is that the commit applies what the
            // preview showed.
            let landing = match &accept.landing {
                IntrospectValue::Text(t) => t.clone(),
                other => other.kind().to_owned(),
            };
            self.say(format!(
                "{}:commit:{landing}:{}:{}",
                self.name,
                accept.action.as_wire_name(),
                offer.kind(),
            ));
        }
    }

    impl ExternalIntrospect for BoardExternal {
        fn schema(&self) -> pinion_core::external::IntrospectSchema {
            pinion_core::external::IntrospectSchema::new(&[])
        }
        fn drop_contract(&self) -> DropContract {
            self.contract
        }
        fn query(
            &self,
            _path: &str,
        ) -> Result<IntrospectValue, pinion_core::external::ReadRefusal> {
            Err(pinion_core::external::ReadRefusal::UnknownPath)
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
            _method: &str,
            _args: IntrospectValue,
        ) -> Result<IntrospectValue, InvokeError> {
            Err(InvokeError::UnknownPath)
        }
    }

    const FIXTURE_SLOTS: DropContract = DropContract::new(
        const {
            &[DropClause::parts(
                FIXTURE_KIND,
                DropActions::one(DropAction::Copy).with(DropAction::Move),
                const { &["slot-a", "slot-b"] },
            )]
        },
    );

    const FIXTURE_MOVE_ONLY: DropContract = DropContract::new(
        const {
            &[DropClause::surface(
                FIXTURE_KIND,
                DropActions::one(DropAction::Move),
            )]
        },
    );

    const FIXTURE_WHOLE: DropContract = DropContract::new(
        const {
            &[DropClause::surface(
                FIXTURE_KIND,
                DropActions::one(DropAction::Copy).with(DropAction::Move),
            )]
        },
    );

    /// The paint scene the R1734 tests hit-test against.
    ///
    /// `palette#chart` 0..100 | `board` 100..300 (three sub-parts, only two of
    /// them declared) | `trash` 300..450 | `bin` 450..600, all 400 tall.
    fn r1734_paint() -> Scene {
        fn tagged(tag: &str, rect: Rect) -> Scene {
            let mut node = Scene::Container(ContainerNode::new(vec![]).with_tag(tag.to_string()));
            if let Scene::Container(c) = &mut node {
                c.rect = rect;
            }
            node
        }
        let mut board = Scene::Container(
            ContainerNode::new(vec![
                tagged("board#slot-a", Rect::new(100, 0, 200, 150)),
                tagged("board#slot-b", Rect::new(100, 150, 200, 150)),
                tagged("board#rim", Rect::new(100, 300, 200, 100)),
            ])
            .with_tag("board"),
        );
        if let Scene::Container(c) = &mut board {
            c.rect = Rect::new(100, 0, 200, 400);
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![
                tagged("palette#chart", Rect::new(0, 0, 100, 400)),
                board,
                tagged("trash", Rect::new(300, 0, 150, 400)),
                tagged("bin", Rect::new(450, 0, 150, 400)),
            ])
            .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 600, 400);
        }
        root
    }

    /// The state scene: four externals, one shared log.
    fn r1734_state(source_actions: DropActions) -> (Scene, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::External(
                ExternalNode::new(Box::new(PaletteExternal {
                    log: Arc::clone(&log),
                    actions: source_actions,
                }))
                .with_tag("palette"),
            ),
            Scene::External(
                ExternalNode::new(Box::new(BoardExternal::new(
                    "board",
                    &log,
                    FIXTURE_SLOTS,
                    BoardPolicy::Land,
                )))
                .with_tag("board"),
            ),
            Scene::External(
                ExternalNode::new(Box::new(BoardExternal::new(
                    "trash",
                    &log,
                    FIXTURE_MOVE_ONLY,
                    BoardPolicy::Land,
                )))
                .with_tag("trash"),
            ),
            Scene::External(
                ExternalNode::new(Box::new(BoardExternal::new(
                    "bin",
                    &log,
                    FIXTURE_WHOLE,
                    BoardPolicy::Full,
                )))
                .with_tag("bin"),
            ),
        ]));
        (scene, log)
    }

    const COPY_ONLY: DropActions = DropActions::one(DropAction::Copy);

    #[test]
    fn r1734_a_drag_between_two_externals_reaches_the_target() {
        // ★ The claim of this round. The palette and the board are two
        // separate `External`s; before R1734 the board received nothing at
        // all, and the palette would have had to resolve and apply the drop on
        // the board's behalf.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 60.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        assert!(log.contains(&"begin".to_owned()), "{log:?}");
        assert!(
            log.contains(&"board:offered:slot-a".to_owned()),
            "the board is asked about the drag it did not start: {log:?}",
        );
        assert!(
            log.contains(&"board:commit:landing-slot-a:copy:board-widget".to_owned()),
            "the board receives the drop, with the action IT chose: {log:?}",
        );
        assert!(
            log.contains(&"board:left".to_owned()),
            "the preview is cleared for the target by the router: {log:?}",
        );
    }

    #[test]
    fn r1734_the_commit_applies_the_landing_the_last_preview_produced() {
        // The commit reads the acceptance, not the geometry. Moving from
        // slot-a to slot-b and releasing there commits slot-b — and, more to
        // the point, commits the STRING the preview built, so the two cannot
        // be computed differently.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 60.0, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 220.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        let offers: Vec<&String> = log
            .iter()
            .filter(|s| s.starts_with("board:offered"))
            .collect();
        assert_eq!(
            offers,
            [
                "board:offered:slot-a",
                "board:offered:slot-b",
                "board:offered:slot-b",
            ],
            "one offer per move, plus the release's own re-offer: {log:?}",
        );
        assert!(
            log.contains(&"board:commit:landing-slot-b:copy:board-widget".to_owned()),
            "{log:?}",
        );
        assert!(
            !log.iter()
                .any(|s| s.starts_with("board:commit:landing-slot-a")),
            "the abandoned preview is not what commits: {log:?}",
        );
    }

    #[test]
    fn r1734_an_undeclared_part_is_refused_by_the_declaration_and_never_asked() {
        // `board#rim` is painted and is NOT in the contract's part list. The
        // router must not ask the board about it — the declaration is the
        // gate, so a widget cannot accept somewhere it did not declare.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 60.0, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 350.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        assert!(
            !log.iter().any(|s| s.contains("offered:rim")),
            "an undeclared part is never offered: {log:?}",
        );
        assert!(
            !log.iter().any(|s| s.starts_with("board:commit")),
            "and nothing commits there: {log:?}",
        );
        // Leaving the declared part still paired, so no highlight is stranded.
        assert_eq!(
            log.iter().filter(|s| *s == "board:left").count(),
            1,
            "the slot-a preview is left exactly once: {log:?}",
        );
    }

    #[test]
    fn r1734_a_target_with_no_action_in_common_is_never_asked() {
        // `trash` declares the kind and only `move`; the palette offers only
        // `copy`. Refusing this from the DECLARATION is what keeps a claim and
        // its outcome from drifting — the widget is not consulted at all.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 380.0, 200.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        assert!(
            !log.iter().any(|s| s.starts_with("trash:")),
            "no action in common → the widget is never reached: {log:?}",
        );
        // The same drag with a source that CAN move reaches it, which is what
        // shows the silence above was the action check and not the wiring.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(DropActions::one(DropAction::Move));
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 380.0, 200.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        assert!(
            log.contains(&"trash:commit:landing-surface:move:board-widget".to_owned()),
            "{log:?}",
        );
    }

    #[test]
    fn r1734_a_targets_own_refusal_blocks_the_commit_and_still_clears() {
        // `bin` declares the drag and its live state declines it. The
        // declaration cannot predict that, so the widget IS asked — and its
        // refusal must stop the commit while still ending the preview.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 520.0, 200.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        assert!(log.contains(&"bin:offered:surface".to_owned()), "{log:?}");
        assert!(
            !log.iter().any(|s| s.starts_with("bin:commit")),
            "a declined offer does not commit: {log:?}",
        );
        assert!(log.contains(&"bin:left".to_owned()), "{log:?}");
    }

    #[test]
    fn r1734_crossing_from_one_target_to_another_leaves_before_it_enters() {
        // The pairing rule the hover path already keeps: the surface being
        // abandoned clears BEFORE the next one previews, so a late leave
        // cannot wipe a highlight its successor has already drawn.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 60.0, &mut state);
        router.cursor_moved(PointerId::MOUSE, 520.0, 200.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        let left = log
            .iter()
            .position(|s| s == "board:left")
            .expect("the board is left");
        let entered = log
            .iter()
            .position(|s| s == "bin:offered:surface")
            .expect("the bin is entered");
        assert!(left < entered, "leave precedes enter: {log:?}");
    }

    #[test]
    fn r1734_a_cancelled_drag_leaves_the_target_it_was_over() {
        // An OS revoke must revoke the TARGET's preview too — the ghost R937.1
        // fixed for the source, one surface over.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 60.0, &mut state);
        router.pointer_cancel(PointerId::MOUSE, &mut state);
        let log = read(&log);
        assert!(log.contains(&"board:offered:slot-a".to_owned()), "{log:?}");
        assert!(
            log.contains(&"board:left".to_owned()),
            "a cancel clears the target's preview: {log:?}",
        );
        assert!(
            !log.iter().any(|s| s.starts_with("board:commit")),
            "a cancel is not a drop: {log:?}",
        );
    }

    #[test]
    fn r1734_a_surface_that_declares_nothing_is_offered_nothing() {
        // The palette declares no contract, so dragging back over itself
        // offers it nothing — which is also why every `External` written
        // before this round is untouched by it.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 60.0, &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 300.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        assert!(
            !log.contains(&"palette:offered".to_owned()),
            "an undeclared surface is never asked: {log:?}",
        );
        assert!(
            log.contains(&"board:left".to_owned()),
            "and the target it came from is still left: {log:?}",
        );
        assert!(
            !log.iter().any(|s| s.contains(":commit")),
            "releasing over nothing commits nothing: {log:?}",
        );
    }

    // ── R1735 §5.51 — what the SOURCE is told while its own drag is in
    // flight ───────────────────────────────────────────────────────────────
    //
    // The same four-`External` fixture, read from the other end. The floor
    // was built and driven for this axis against its 6.11.1 release: crossing
    // an accepting region, bare background and a refusing region — eleven
    // pointer samples — a source received FOUR notifications carrying an
    // object identity and an action, its own pointer handler ran zero times,
    // and the refusing region was reported identically to the bare
    // background. These tests assert the three things that measurement says
    // are missing.

    /// Everything the source was told under one prefix, in order.
    fn said(log: &[String], prefix: &str) -> Vec<String> {
        log.iter()
            .filter_map(|line| line.strip_prefix(prefix).map(ToOwned::to_owned))
            .collect()
    }

    /// Every standing the source was told, in order.
    fn told(log: &[String]) -> Vec<String> {
        said(log, "palette:told:")
    }

    #[test]
    fn r1735_the_source_is_told_the_landing_the_commit_will_receive() {
        // ★ The claim. A source painting from this standing draws the landing
        // the release will apply, because it is the SAME `DropAccept` value —
        // not a second computation over the same pixel.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 60.0, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 220.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        assert_eq!(
            told(&log),
            [
                "accepted:board:copy:landing-slot-a",
                "accepted:board:copy:landing-slot-b",
            ],
            "the source is told a landing on every move: {log:?}",
        );
        // And the landing it was told LAST is the one that committed.
        assert!(
            log.contains(&"board:commit:landing-slot-b:copy:board-widget".to_owned()),
            "{log:?}",
        );
    }

    #[test]
    fn r1735_a_refusing_target_and_bare_background_are_different_answers() {
        // ★★ The hole the floor cannot fill. Measured there, a refusing
        // widget and empty space both report the null object with the ignore
        // action, so a source cannot tell "something is here and it said no"
        // from "nothing is here". Here they are two arms, and only one of
        // them carries a reason.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        // `bin` declares this kind and its live state refuses…
        router.cursor_moved(PointerId::MOUSE, 500.0, 200.0, &mut state);
        // …`board#rim` is painted inside a declaring surface but is not a
        // declared part — a structural refusal, derived, never asked…
        router.cursor_moved(PointerId::MOUSE, 200.0, 350.0, &mut state);
        // …`trash` declares the kind with no action in common…
        router.cursor_moved(PointerId::MOUSE, 380.0, 200.0, &mut state);
        // …and the palette itself declares nothing at all.
        router.cursor_moved(PointerId::MOUSE, 50.0, 300.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            told(&read(&log)),
            [
                "refused:bin:declined",
                "refused:board:part-not-accepted",
                "refused:trash:no-common-action",
                "nowhere",
            ],
            "four crossings, four distinct answers",
        );
    }

    #[test]
    fn r1735_a_refusal_reaches_the_source_as_a_sentence() {
        // The reason travels, not just the fact. The floor's source-side
        // notification has no room for one.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 500.0, 200.0, &mut state);
        router.cursor_moved(PointerId::MOUSE, 380.0, 200.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let why = said(&read(&log), "palette:why:");
        assert!(
            why.iter().any(|s| s.contains("every slot is taken")),
            "the target's own words reach the source: {why:?}",
        );
        assert!(
            why.iter().any(|s| s.contains("move") && s.contains("copy")),
            "a derived refusal names what would have worked: {why:?}",
        );
    }

    #[test]
    fn r1735_the_release_tells_the_source_what_it_resolved_to() {
        // The release is not a separate vocabulary: the source hears the same
        // standing for "what happened" that it heard for "what would happen".
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 60.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        assert!(
            log.contains(&"palette:release-told:accepted:board:copy:landing-slot-a".to_owned()),
            "{log:?}",
        );
        // A release over nothing says so rather than repeating the last hover.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 200.0, 60.0, &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 300.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&log);
        assert!(
            log.contains(&"palette:release-told:nowhere".to_owned()),
            "{log:?}",
        );
    }

    #[test]
    fn r1735_a_source_keeps_being_told_where_the_cursor_is() {
        // Measured on the floor: a source's own pointer handler runs ZERO
        // times while its drag is in flight, and no member of the drag object
        // carries a point — so a self-hit-testing screen there has no live
        // cursor at all. Here every move reaches the source with the absolute
        // cursor beside the standing.
        let mut router = InputRouter::new();
        let (mut state, log) = r1734_state(COPY_ONLY);
        router.update_paint_scene(r1734_paint(), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 200.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        for x in [120.0, 200.0, 260.0, 340.0, 500.0] {
            router.cursor_moved(PointerId::MOUSE, x, 200.0, &mut state);
        }
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            said(&read(&log), "palette:cursor:"),
            ["120:200", "200:200", "260:200", "340:200", "500:200"],
            "one live cursor per move, for the whole gesture",
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

    /// R1497 — a paint scene in the shape that exposed the defect: two
    /// External-backed composite cells, each wrapping its own **tagged** label
    /// text. The label is a presentational name (a11y / introspection / demo
    /// assertions) with no `External` behind it, and it covers the middle of its
    /// cell — including the rect CENTRE, which is the point `scene/click {path}`
    /// presses.
    ///
    /// Cell 0 spans x 0..100, its label x 30..70; cell 1 spans x 100..200, its
    /// label x 130..170. Both cells span y 0..40, both labels y 10..30.
    ///
    /// The cells sit inside a strip container tagged with the BARE primary, which
    /// is `hello-column-reorder`'s real shape (`colhdr` wraps `colhdr#<n>`) and is
    /// what makes these fixtures discriminate the walk DIRECTION: the strip is
    /// addressable too, so a resolution that took the shallowest addressable tag
    /// would answer `main_btn` and lose the sub-region the widget acts on.
    fn paint_with_labelled_cells(primary: &str) -> Scene {
        let cell = |i: u32, x: u32| {
            // R1499 — the label declares itself decoration, as the toolkit's
            // `WA_TransparentForMouseEvents` and CSS's `pointer-events: none`
            // do and as the real consumer's paint site now does. Drop the
            // declaration and the press lands on a tag no `External` backs.
            let label = Scene::Box(
                BoxNode::filled(Rect::new(x + 30, 10, 40, 20), Color::default())
                    .with_tag(format!("{primary}_label#{i}"))
                    .with_layout(
                        pinion_core::style::LayoutStyle::new().with_pointer_transparent(true),
                    ),
            );
            let mut c = ContainerNode::new(vec![label]).with_tag(format!("{primary}#{i}"));
            c.rect = Rect::new(x, 0, 100, 40);
            Scene::Container(c)
        };
        let mut strip =
            ContainerNode::new(vec![cell(0, 0), cell(1, 100)]).with_tag(primary.to_string());
        strip.rect = Rect::new(0, 0, 200, 40);
        let mut root = ContainerNode::new(vec![Scene::Container(strip)]);
        root.rect = Rect::new(0, 0, 200, 100);
        Scene::Container(root)
    }

    /// R1497 §5.35 §2 #2 — THE defect. A press whose coordinate falls on a
    /// tagged decorative child reaches the widget that owns the region.
    ///
    /// Pre-R1497 `resolve_hover_tag` answered the deepest TAG, so hover settled
    /// on `main_btn_label#0`; `pointer_down` dispatched there,
    /// `dispatch_send` split off the primary `main_btn_label`, found no
    /// `External`, and returned — dropping the press with no diagnostic. On
    /// `hello-column-reorder` that made `scene/click` on two of five header
    /// sections a silent no-op, and the discriminator was exactly whether the
    /// section's own label covered the cell centre.
    #[test]
    fn r1497_a_tagged_decoration_does_not_swallow_the_press() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        router.update_paint_scene(paint_with_labelled_cells("main_btn"), &mut state);
        // (50, 20) is inside cell 0 AND inside its label — the cell centre.
        router.cursor_moved(PointerId::MOUSE, 50.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&captures);
        assert!(
            // R1619 — the press carries the button it was made with.
            log.contains(&"0:PointerDown::l".to_string()),
            "the press reaches the widget that owns the region: {log:?}"
        );
        assert!(
            log.contains(&"0:PointerUp".to_string()),
            "and so does the release — the pair is the click: {log:?}"
        );
        // The subindex is the CELL's, not the label's — the widget is told which
        // of its own sub-regions was pressed, so an identically-suffixed label
        // cannot make it act on the wrong one.
        assert_eq!(
            router.hover_target(PointerId::MOUSE),
            Some("main_btn#0"),
            "hover names the widget's sub-region, not the decoration over it",
        );
    }

    /// R1497 — the enter / leave stream is the widget's, not its decoration's.
    /// A cursor crossing from one cell's label to the next cell's label must
    /// produce exactly one Leave + one Enter, both naming cells: pre-R1497 they
    /// named `*_label#*` tags that could receive nothing, so a widget watching
    /// its own hover saw neither edge.
    #[test]
    fn r1497_hover_edges_name_the_widget_when_the_cursor_crosses_labels() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        router.update_paint_scene(paint_with_labelled_cells("main_btn"), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 20.0, &mut state);
        router.cursor_moved(PointerId::MOUSE, 150.0, 20.0, &mut state);
        assert_eq!(
            read(&captures),
            vec![
                "0:PointerEnter".to_string(),
                "0:PointerLeave".to_string(),
                "1:PointerEnter".to_string(),
            ],
            "leave-before-enter, both naming cells",
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn#1"));
    }

    /// R1499 §5.35 §5.16 — THE regression R1497 shipped, as a router test.
    ///
    /// A window-chrome control with no `External` behind it, nested inside a
    /// tagged container that DOES have one. R1497 answered the deepest tag whose
    /// primary resolves to an `External`, falling back to the deepest tag only
    /// when none on the path did — so this resolved the container and the
    /// shell's chrome interception stopped seeing the control. R1497 believed
    /// the shape impossible ("injected as top-level SIBLINGS of the content"),
    /// but `pinion_overlay::wrap_into_container` returns an existing
    /// `Scene::Container` unchanged, so a chromeless window whose root view IS
    /// that container hosts the regions as its CHILDREN. Three `pinion-shell`
    /// window-chrome tests went red on it.
    ///
    /// The control declares nothing, so the deepest tag is the answer. The
    /// discriminator against the R1497 rule is that `main_btn` — the container —
    /// is genuinely `External`-backed here, which is what made it win.
    #[test]
    fn r1499_a_chrome_tag_inside_an_external_backed_container_still_wins() {
        let mut close =
            ContainerNode::new(vec![]).with_tag("ai-overlay/window-controls#close".to_string());
        close.rect = Rect::new(160, 0, 40, 30);
        // Tagged `main_btn`, which the state scene DOES back with an External —
        // the content-hosted shape, not a top-level sibling.
        let mut content =
            ContainerNode::new(vec![Scene::Container(close)]).with_tag("main_btn".to_string());
        content.rect = Rect::new(0, 0, 200, 100);
        let paint = Scene::Container(content);
        let (state, _) = state_with_button();
        assert!(
            state.find_external_with_tag("main_btn").is_some(),
            "the fixture's premise: the ANCESTOR is the External-backed one",
        );
        assert_eq!(
            resolve_pointer_tag(&paint, 170.0, 10.0).as_deref(),
            Some("ai-overlay/window-controls#close"),
            "a tag the shell intercepts is not shadowed by the widget it sits in",
        );
        // And the widget still owns everywhere the control is not.
        assert_eq!(
            resolve_pointer_tag(&paint, 20.0, 50.0).as_deref(),
            Some("main_btn"),
        );

        // The shape R1497 assumed was the only one — the control as a top-level
        // sibling, with no External anywhere on the path. It has to keep working
        // too; the rule is the same one, which is the point.
        let mut sibling =
            ContainerNode::new(vec![]).with_tag("ai-overlay/window-controls#close".to_string());
        sibling.rect = Rect::new(160, 0, 40, 30);
        let mut root = ContainerNode::new(vec![Scene::Container(sibling)]);
        root.rect = Rect::new(0, 0, 200, 100);
        assert_eq!(
            resolve_pointer_tag(&Scene::Container(root), 170.0, 10.0).as_deref(),
            Some("ai-overlay/window-controls#close"),
        );
    }

    /// R1497 — the second, smaller widening the SSOT switch brings. The router's
    /// retired private walk recursed `Container.children` only, so an `External`
    /// the state scene wraps in a [`Scene::Scroll`] was invisible to every
    /// dispatch site — a press on it went nowhere for the same reason a press on
    /// a label did. [`Scene::find_external_with_tag`] descends `Scroll.content`
    /// (the branch set `contains_tag` and `hit_test` already walked), so both the
    /// predicate and the dispatch now reach it.
    ///
    /// Non-tautological: under the retired walk the press logs nothing.
    #[test]
    fn r1497_an_external_inside_a_scroll_is_addressable() {
        use pinion_core::scene::ScrollNode;

        let (capture, captures) = CaptureExternal::new();
        let inner = Scene::External(ExternalNode::new(Box::new(capture)).with_tag("main_btn"));
        let mut state = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 200, 100), inner));
        assert!(
            state.find_external_with_tag("main_btn").is_some(),
            "a scroll region does not hide the widget it holds",
        );
        let mut router = InputRouter::new();
        router.update_paint_scene(paint_with_labelled_cells("main_btn"), &mut state);
        router.cursor_moved(PointerId::MOUSE, 50.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&captures);
        assert!(
            log.contains(&"0:PointerDown::l".to_string())
                && log.contains(&"0:PointerUp".to_string()),
            "and the press reaches it: {log:?}",
        );
    }

    /// R1497 §5.35 R741 — a capture widget's release read through the SAME
    /// resolution. `Button` / `Checkbox` / `Radio` / `Toggle` all set
    /// `cancel_on_release_off_target`, so `cursor_over_tag`'s answer decides
    /// between activating (`PointerUp`) and cancelling (`PointerLeave`).
    ///
    /// Press on the cell's plain area, slide onto the cell's own LABEL, release.
    /// Pre-R1497 `cursor_over_tag` resolved the label tag, compared it against the
    /// captured cell tag, found them different, and cancelled — a press released
    /// exactly where the widget paints its own name, refused for that reason.
    #[test]
    fn r1497_a_capture_release_over_its_own_label_still_activates() {
        let mut router = InputRouter::new();
        let (capture, events, _moves) = DragCaptureExternal::with_cancel_off_target();
        let mut state =
            Scene::External(ExternalNode::new(Box::new(capture)).with_tag("main_slider"));
        router.update_paint_scene(paint_with_labelled_cells("main_slider"), &mut state);
        // Press at x=10: inside cell 0, OUTSIDE its label (30..70).
        router.cursor_moved(PointerId::MOUSE, 10.0, 20.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(
            router.captured_target(PointerId::MOUSE),
            Some("main_slider#0"),
            "the press captured the cell",
        );
        // Slide onto the label and release there — still the same cell.
        router.cursor_moved(PointerId::MOUSE, 50.0, 20.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read(&events);
        assert!(
            log.contains(&"0:PointerUp".to_string()),
            "a release on the widget's own label activates: {log:?}"
        );
        assert!(
            !log.contains(&"0:PointerLeave".to_string()),
            "and is not a slide-off cancel: {log:?}"
        );
    }

    /// R1497 — the drop fallback resolves the same address. The opted-in
    /// (`LayoutStyle::drop_target`) leg already skipped decoration, because it
    /// demands a marker a label does not carry; the FALLBACK leg took the deepest
    /// tag and so could hand a coordinator a label it cannot interpret. One
    /// address, every method.
    #[test]
    fn r1497_a_drop_climbs_past_decoration_to_the_widget() {
        let mut router = InputRouter::new();
        let (mut state, _log) = state_with_dnd();
        router.update_paint_scene(paint_with_labelled_cells("dnd"), &mut state);
        // No node here opts into `drop_target`, so this is the fallback leg.
        assert_eq!(
            resolve_drop_target_tag(router.last_paint_scene.as_ref().unwrap(), 150.0, 20.0),
            None
        );
        let dp = router
            .resolve_drop_point(150.0, 20.0)
            .expect("over a tagged region");
        assert_eq!(
            dp.tag, "dnd#1",
            "the drop names the cell, not the label painted over it",
        );
        // Normalised over the CELL rect (100 wide at x=100), not the label's:
        // (150 - 100) / 100 = 0.5.
        assert!(
            (dp.x_rel - 0.5).abs() < 1e-6,
            "normalised over the cell rect: {}",
            dp.x_rel,
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

#[cfg(test)]
mod pointer_reach_tests {
    use super::{PointerShadow, pointer_reach};
    use pinion_core::Scene;
    use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, Rect, ScrollNode};
    use pinion_core::style::{BoxStyle, LayoutStyle};
    use pinion_core::widgets::button::ButtonExternal;

    /// A state scene holding one real widget under `tag`.
    fn state_with(tag: &'static str) -> Scene {
        Scene::External(ExternalNode::new(Box::new(ButtonExternal::new())).with_tag(tag))
    }

    fn tagged_box(tag: &'static str) -> Scene {
        Scene::Box(BoxNode::new(Rect::new(0, 0, 40, 20), BoxStyle::default()).with_tag(tag))
    }

    /// The R1649.1 incident, reduced: a tagged card painted inside the window's
    /// one widget-backed root. Every press lands on the card, the card's tag
    /// resolves to nothing, and the router drops it — while `scene/click` and
    /// `send` keep working, because they never ask the router.
    #[test]
    fn r1650_a_tagged_child_over_a_widget_is_reported_as_a_shadow() {
        let paint = Scene::Container(
            ContainerNode::new(vec![tagged_box("card.alpha")]).with_tag("shell.root"),
        );
        let reach = pointer_reach(&paint, &state_with("shell.root"));
        assert_eq!(
            reach.shadows,
            vec![PointerShadow {
                tag: "card.alpha".to_string(),
                path: vec!["card.alpha".to_string()],
                shadowed: "shell.root".to_string(),
            }],
            "the card takes the root widget's input and the report names both"
        );
        assert_eq!(reach.deliverable, 1, "the root itself is still deliverable");
        assert_eq!(reach.inert, 0);
    }

    /// The declaration that repairs it — the same one CSS and the reference
    /// toolkit use — makes the surface report clean. Without this the check
    /// would flag a correctly-built screen and be worthless as a gate.
    #[test]
    fn r1650_declaring_the_child_transparent_clears_the_shadow() {
        let card = match tagged_box("card.alpha") {
            Scene::Box(n) => {
                Scene::Box(n.with_layout(LayoutStyle::new().with_pointer_transparent(true)))
            }
            other => other,
        };
        let paint = Scene::Container(ContainerNode::new(vec![card]).with_tag("shell.root"));
        let reach = pointer_reach(&paint, &state_with("shell.root"));
        assert!(reach.shadows.is_empty(), "{:?}", reach.shadows);
        assert_eq!(reach.deliverable, 1);
    }

    /// A tag over dead space is an ADDRESS, not a defect: nothing above it is a
    /// widget, so nothing loses input. Counted, so the report's numbers account
    /// for every tag it looked at instead of leaving a silent remainder.
    #[test]
    fn r1650_a_tag_with_no_widget_above_it_is_inert_not_a_shadow() {
        let paint = Scene::Container(ContainerNode::new(vec![
            tagged_box("legend.row"),
            tagged_box("legend.swatch"),
        ]));
        let reach = pointer_reach(&paint, &state_with("elsewhere"));
        assert!(reach.shadows.is_empty(), "{:?}", reach.shadows);
        assert_eq!(reach.inert, 2, "both tags counted, neither a defect");
        assert_eq!(reach.deliverable, 0);
    }

    /// The R1497 incident: a header cell IS the widget-backed target and its
    /// own centred label shadows it, which is why the clicks that landed on the
    /// text were lost while the ones beside it worked. The composite `#n` half
    /// is split off before the lookup, exactly as dispatch does it.
    #[test]
    fn r1650_a_composite_tag_resolves_on_its_primary_half() {
        let paint = Scene::Container(ContainerNode::new(vec![Scene::Container(
            ContainerNode::new(vec![tagged_box("colhdr_label#3")]).with_tag("colhdr#3"),
        )]));
        let reach = pointer_reach(&paint, &state_with("colhdr"));
        assert_eq!(reach.deliverable, 1, "`colhdr#3` resolves via `colhdr`");
        assert_eq!(
            reach
                .shadows
                .iter()
                .map(|s| s.tag.as_str())
                .collect::<Vec<_>>(),
            ["colhdr_label#3"],
            "and its own label is what swallowed the press"
        );
        assert_eq!(reach.shadows[0].shadowed, "colhdr#3");
    }

    /// A shadow inside a scroll is still a shadow, and the address reported is
    /// the one the wire accepts — the case a hand-written walker gets wrong by
    /// not descending, and the case the path collapse makes subtle.
    #[test]
    fn r1650_the_report_descends_a_scroll_and_uses_the_wire_address() {
        let paint = Scene::Container(
            ContainerNode::new(vec![Scene::Scroll(ScrollNode::new(
                Rect::new(0, 0, 100, 100),
                Scene::Container(ContainerNode::new(vec![tagged_box("row.7")])),
            ))])
            .with_tag("shell.root"),
        );
        let reach = pointer_reach(&paint, &state_with("shell.root"));
        assert_eq!(
            reach
                .shadows
                .iter()
                .map(|s| s.path.join("/"))
                .collect::<Vec<_>>(),
            ["0/row.7"],
            "the scroll contributes one segment and its content none"
        );
    }

    /// Everything under a transparent ancestor is unreachable, so nothing there
    /// can shadow. Without this the repair would have to be applied to every
    /// descendant instead of once at the overlay's root.
    #[test]
    fn r1650_a_transparent_ancestor_hides_its_whole_subtree() {
        let overlay = Scene::Container(
            ContainerNode::new(vec![tagged_box("ring.label")])
                .with_tag("ring")
                .with_layout(LayoutStyle::new().with_pointer_transparent(true)),
        );
        let paint = Scene::Container(ContainerNode::new(vec![overlay]).with_tag("shell.root"));
        let reach = pointer_reach(&paint, &state_with("shell.root"));
        assert!(reach.shadows.is_empty(), "{:?}", reach.shadows);
        assert_eq!(reach.inert, 0, "an unreachable tag is not even counted");
    }

    /// The verdict half, on the shape that was measured dead: a card painted
    /// over the whole board leaves the board's own centre resolving to the
    /// card, so the widget cannot be pressed anywhere a person would press it.
    #[test]
    fn r1650_a_widget_covered_at_its_centre_is_unreachable() {
        let mut board = ContainerNode::new(vec![Scene::Box(
            BoxNode::new(Rect::new(0, 0, 100, 100), BoxStyle::default()).with_tag("card.alarms"),
        )])
        .with_tag("dashboard");
        board.rect = Rect::new(0, 0, 100, 100);
        let paint = Scene::Container(board);
        let reach = pointer_reach(&paint, &state_with("dashboard"));
        assert_eq!(
            reach.unreachable,
            vec![super::PointerUnreachable {
                tag: "dashboard".to_string(),
                path: Vec::new(),
                blocked_by: Some("card.alarms".to_string()),
            }],
            "the board is not pressable at its own centre"
        );
    }

    /// …and the shape that is NOT a defect, which is what keeps the verdict
    /// usable as a gate: a tagged region covering part of a widget swallows the
    /// gaps and nothing else, because the widget's own composite cell still
    /// answers at the centre. Measured on `hello-data-grid`, whose header and
    /// scroll regions are tagged exactly this way.
    #[test]
    fn r1650_a_partial_shadow_that_leaves_the_centre_alone_is_not_a_defect() {
        let mut grid = ContainerNode::new(vec![
            Scene::Box(
                BoxNode::new(Rect::new(0, 0, 100, 20), BoxStyle::default()).with_tag("grid_header"),
            ),
            Scene::Box(
                BoxNode::new(Rect::new(0, 20, 100, 80), BoxStyle::default()).with_tag("grid#0_1"),
            ),
        ])
        .with_tag("grid");
        grid.rect = Rect::new(0, 0, 100, 100);
        let paint = Scene::Container(grid);
        let reach = pointer_reach(&paint, &state_with("grid"));
        assert_eq!(
            reach
                .shadows
                .iter()
                .map(|s| s.tag.as_str())
                .collect::<Vec<_>>(),
            ["grid_header"],
            "the header still intercepts its own band, and the census says so"
        );
        assert!(
            reach.unreachable.is_empty(),
            "but the grid answers at its centre through its own cell: {:?}",
            reach.unreachable
        );
    }

    /// The row names the widget that ACTUALLY loses the press, which is the
    /// nearest one — and until this fixture existed nothing said so: every
    /// other case here has one widget ancestor, so walking the chain from
    /// either end gave the same answer and a counterfactual reversing the walk
    /// passed. Two nested widgets is the smallest tree that can tell them
    /// apart, and it is the common one (a panel inside a shell).
    #[test]
    fn r1650_a_shadow_names_the_nearest_widget_above_it() {
        let mut panel = ContainerNode::new(vec![tagged_box("panel.caption")]).with_tag("panel");
        panel.rect = Rect::new(0, 0, 100, 100);
        let mut shell = ContainerNode::new(vec![Scene::Container(panel)]).with_tag("shell");
        shell.rect = Rect::new(0, 0, 100, 100);
        let state = Scene::Container(ContainerNode::new(vec![
            state_with("shell"),
            state_with("panel"),
        ]));
        let reach = pointer_reach(&Scene::Container(shell), &state);
        assert_eq!(
            reach
                .shadows
                .iter()
                .map(|s| s.shadowed.as_str())
                .collect::<Vec<_>>(),
            ["panel"],
            "the caption takes the PANEL's press, not the shell's"
        );
    }

    /// A widget painted over another widget is ordinary nesting — a row over
    /// its tree, a cell over its grid — and reporting it as a defect is what
    /// the first draft did. The sweep found four containers flagged that way
    /// on its first run, so the bar is "does a widget answer here", not "does
    /// THIS widget answer here".
    #[test]
    fn r1650_a_widget_covered_by_another_widget_is_not_a_defect() {
        let mut tree = ContainerNode::new(vec![Scene::Box(
            BoxNode::new(Rect::new(0, 0, 100, 100), BoxStyle::default()).with_tag("row#4"),
        )])
        .with_tag("tree");
        tree.rect = Rect::new(0, 0, 100, 100);
        let paint = Scene::Container(tree);
        let state = Scene::Container(ContainerNode::new(vec![
            state_with("tree"),
            state_with("row"),
        ]));
        let reach = pointer_reach(&paint, &state);
        assert!(
            reach.unreachable.is_empty(),
            "the press reaches `row`, which is the point: {:?}",
            reach.unreachable
        );
        assert_eq!(reach.deliverable, 2, "both are widget-backed tags");
    }

    /// A widget inside a scroll is pressable, and saying otherwise is the
    /// error this test pins. A node's own `rect` inside a [`Scene::Scroll`] is
    /// content-intrinsic, not window-absolute; probing the centre with it
    /// aimed at the wrong pixel and reported **59 false defects** on the first
    /// real surface with a scroll region. The probe goes through the one
    /// coordinate-translation authority instead.
    #[test]
    fn r1650_a_widget_inside_a_scroll_is_probed_in_window_coordinates() {
        let mut row = ContainerNode::new(vec![]).with_tag("grid#0_0");
        row.rect = Rect::new(0, 200, 100, 40);
        let mut content = ContainerNode::new(vec![Scene::Container(row)]);
        content.rect = Rect::new(0, 0, 100, 4000);
        let mut root = ContainerNode::new(vec![Scene::Scroll(
            ScrollNode::new(Rect::new(0, 0, 100, 300), Scene::Container(content))
                .with_offset(0, 180),
        )]);
        root.rect = Rect::new(0, 0, 100, 300);
        let paint = Scene::Container(root);
        let reach = pointer_reach(&paint, &state_with("grid"));
        assert_eq!(reach.deliverable, 1);
        assert!(
            reach.unreachable.is_empty(),
            "the row sits at window y=20..60 once the scroll offset is applied: {:?}",
            reach.unreachable
        );
    }

    /// A disabled region absorbs the press by design (R1554), so reporting it
    /// would train readers to ignore the report.
    #[test]
    fn r1650_a_disabled_region_absorbs_by_design_and_is_not_a_defect() {
        let veil = match tagged_box("panel.veil") {
            Scene::Box(n) => Scene::Box(n.with_layout(LayoutStyle::new().with_disabled(true))),
            other => other,
        };
        let paint = Scene::Container(ContainerNode::new(vec![veil]).with_tag("shell.root"));
        let reach = pointer_reach(&paint, &state_with("shell.root"));
        assert!(reach.shadows.is_empty(), "{:?}", reach.shadows);
    }

    /// Two widgets registered under one root, the way a screen that keeps its
    /// model in a second `External` is built.
    fn state_with_two(a: &'static str, b: &'static str) -> Scene {
        Scene::Container(ContainerNode::new(vec![state_with(a), state_with(b)]))
    }

    /// ★ R1664 — the R1663 incident, reduced: the screen registers `packet_view`
    /// and paints its surface under `pv.root`, a name nothing answers to.
    ///
    /// The point of this test is the FIRST four assertions. Every field the
    /// report had before this round reads exactly as it reads for a healthy
    /// screen with no widgets on it — no shadow (there is no victim to shadow),
    /// nothing unreachable (that population is the tags that *did* resolve, and
    /// none did), and a count of inert decoration. A person pressing this window
    /// gets nothing, anywhere, and the report said so in no field.
    #[test]
    fn r1664_a_screen_whose_paint_tag_matches_no_widget_reports_clean_except_here() {
        let paint = Scene::Container(
            ContainerNode::new(vec![tagged_box("pv.list"), tagged_box("pv.tree")])
                .with_tag("pv.root"),
        );
        let reach = pointer_reach(&paint, &state_with("packet_view"));

        assert!(reach.shadows.is_empty(), "{:?}", reach.shadows);
        assert!(reach.unreachable.is_empty(), "{:?}", reach.unreachable);
        assert_eq!(reach.deliverable, 0);
        assert_eq!(reach.inert, 3, "three painted tags, all decorative to it");

        // …and the one field that can tell this apart from an empty screen.
        assert!(
            reach.is_dead_to_a_pointer(),
            "every press in this window is dropped: {reach:?}"
        );
        assert_eq!(
            reach
                .externals
                .iter()
                .map(|e| (e.tag.as_str(), e.routed_by.clone()))
                .collect::<Vec<_>>(),
            [("packet_view", None)],
            "and it names the widget nothing on screen routes to"
        );
    }

    /// The repair, and the reason the assertion above is not merely "this screen
    /// is unusual": painting the root under the registered name makes the same
    /// screen live, with nothing else changed.
    #[test]
    fn r1664_painting_the_root_under_the_registered_tag_makes_it_reachable() {
        let paint = Scene::Container(
            ContainerNode::new(vec![tagged_box("pv.list"), tagged_box("pv.tree")])
                .with_tag("packet_view"),
        );
        let reach = pointer_reach(&paint, &state_with("packet_view"));
        assert!(!reach.is_dead_to_a_pointer(), "{reach:?}");
        assert_eq!(
            reach.externals,
            vec![super::ExternalRouting {
                tag: "packet_view".to_string(),
                routed_by: Some("packet_view".to_string()),
            }]
        );
    }

    /// ★ A single unrouted widget is NOT the defect, and this is what keeps the
    /// verdict from crying wolf: a model registered so `scene/query` can read it
    /// has no surface by design and can never be pressed. The screen is live.
    #[test]
    fn r1664_a_data_only_widget_with_no_surface_does_not_make_a_screen_dead() {
        let paint = Scene::Container(ContainerNode::new(vec![]).with_tag("packet_view"));
        let reach = pointer_reach(&paint, &state_with_two("packet_view", "pv.map"));
        assert!(
            !reach.is_dead_to_a_pointer(),
            "one widget is reachable, so the screen is drivable: {reach:?}"
        );
        assert_eq!(
            reach
                .externals
                .iter()
                .map(|e| (e.tag.as_str(), e.routed_by.as_deref()))
                .collect::<Vec<_>>(),
            [("packet_view", Some("packet_view")), ("pv.map", None)],
            "and the census still says which one has no surface"
        );
    }

    /// A screen with nothing registered has nothing to fail to reach. Asserted
    /// because the alternative — treating an empty roster as dead — would make
    /// "no widgets yet" and "the wiring broke" one answer again, which is the
    /// collapse this whole read exists to undo.
    #[test]
    fn r1664_an_empty_roster_is_not_a_dead_screen() {
        let paint = Scene::Container(ContainerNode::new(vec![tagged_box("splash.logo")]));
        let reach = pointer_reach(&paint, &Scene::Container(ContainerNode::new(vec![])));
        assert!(reach.externals.is_empty());
        assert!(!reach.is_dead_to_a_pointer());
    }

    /// The region a press LANDS IN is the outermost node carrying the address,
    /// so that is what the census names — the repair moves the container, not
    /// the leaf that happens to share its primary half.
    #[test]
    fn r1664_the_routing_names_the_shallowest_tag_that_resolves() {
        let paint = Scene::Container(
            ContainerNode::new(vec![Scene::Container(
                ContainerNode::new(vec![tagged_box("grid#7")]).with_tag("grid#3"),
            )])
            .with_tag("grid"),
        );
        let reach = pointer_reach(&paint, &state_with("grid"));
        assert_eq!(reach.externals[0].routed_by.as_deref(), Some("grid"));
    }

    /// ★ The two sides of the join cannot disagree. `deliverable` counts it from
    /// the paint tree and `externals` walks it from the state tree; they are
    /// separate code reading separate inputs, so a drift between them is a real
    /// possibility and this is what forbids it.
    ///
    /// Driven over every fixture in this module rather than one, because a
    /// cross-check asserted on a single shape is a cross-check that has only
    /// been asked one question.
    #[test]
    fn r1664_deliverable_and_the_external_census_agree_on_every_fixture() {
        let transparent = match tagged_box("card.alpha") {
            Scene::Box(n) => {
                Scene::Box(n.with_layout(LayoutStyle::new().with_pointer_transparent(true)))
            }
            other => other,
        };
        let cases: Vec<(&str, Scene, Scene)> = vec![
            (
                "the shadowed shell",
                Scene::Container(
                    ContainerNode::new(vec![tagged_box("card.alpha")]).with_tag("shell.root"),
                ),
                state_with("shell.root"),
            ),
            (
                "the repaired shell",
                Scene::Container(ContainerNode::new(vec![transparent]).with_tag("shell.root")),
                state_with("shell.root"),
            ),
            (
                "decoration only",
                Scene::Container(ContainerNode::new(vec![tagged_box("legend.row")])),
                state_with("elsewhere"),
            ),
            (
                "the composite header",
                Scene::Container(ContainerNode::new(vec![Scene::Container(
                    ContainerNode::new(vec![tagged_box("colhdr_label#3")]).with_tag("colhdr#3"),
                )])),
                state_with("colhdr"),
            ),
            (
                "the packet view",
                Scene::Container(
                    ContainerNode::new(vec![tagged_box("pv.list")]).with_tag("pv.root"),
                ),
                state_with("packet_view"),
            ),
        ];
        for (what, paint, state) in cases {
            let reach = pointer_reach(&paint, &state);
            let routed = reach
                .externals
                .iter()
                .filter(|e| e.routed_by.is_some())
                .count();
            assert_eq!(
                reach.deliverable > 0,
                routed > 0,
                "{what}: paint side says deliverable={}, state side says routed={routed}",
                reach.deliverable,
            );
            assert_eq!(
                reach.is_dead_to_a_pointer(),
                !reach.externals.is_empty() && reach.deliverable == 0,
                "{what}: the verdict must follow from the counts it summarises"
            );
        }
    }
}

/// The per-surface record of what each `External` was last told, keyed by tag.
///
/// A named type rather than a bare map because it crosses a crate boundary: the
/// shell holds one of these per window and hands it back every frame, and a
/// signature that spells the map out invites a caller to build a different one.
pub type ExternalSizes = std::collections::HashMap<String, (u32, u32)>;

/// R1656 §5.15 §5.35 — tell every painted [`Scene::External`] how big it is,
/// whenever that changes.
///
/// # The half-fact this closes
///
/// [`External::pointer_move`](pinion_core::external::External::pointer_move)
/// hands a **fraction** of the widget's post-layout rect and does not hand the
/// rect. A consumer that wants pixels — which is every consumer that draws its
/// own content — has to find the basis somewhere else, and there is no scope
/// inside a pointer callback from which the reactive viewport hook answers. So
/// the standard mistake is to multiply by a constant, and the standard mistake
/// is invisible until the window is resized.
///
/// It shipped here. A person reported that the analysis-tool canvas stops
/// responding to a real mouse after a maximise; measured, the application was
/// being told a cursor scaled by opening-size over current-size — 0.5775x
/// horizontally after a maximise from 1440 to 2494 — so every press landed
/// somewhere else, and further off the further right it was aimed.
///
/// [`External::on_resize`](pinion_core::external::External::on_resize) is a
/// §5.15 lifecycle item and has been declared since the contract was written.
/// **Nothing called it.** A declared arm with no implementation is worse than
/// an absent one (R1654): a consumer reads the trait, implements the arm, and
/// waits forever for a call. This is the call.
///
/// # Why the size comes from the paint scene and the handle from the state one
///
/// The same split [`pointer_reach`] and
/// `InputRouter::forward_pointer_move` work in: the paint scene carries the
/// post-layout geometry and its handles are rebuilt every frame, while the
/// state scene owns the handle whose fields survive between frames. Announcing
/// to the paint scene's handle would tell a value that is about to be dropped.
///
/// Only a size CHANGE is announced, so a still window costs one walk and no
/// calls — and a consumer can treat the callback as an event rather than
/// having to debounce it.
///
/// Returns how many widgets were told, which is what lets a test distinguish
/// "nothing changed" from "nothing was wired".
pub fn announce_external_sizes(
    paint_scene: &Scene,
    state_scene: &mut Scene,
    known: &mut ExternalSizes,
) -> usize {
    // The tags to ask about come from the STATE scene, because that is where
    // the `External` handles live. The paint scene often has no `Scene::External`
    // node at all — a `WidgetCore` binding's `view` replaces its primary
    // surface with an ordinary container tree carrying the same tag, and the
    // first draft of this function walked the paint scene for `External` nodes,
    // found none on exactly the screen it was written for, and never fired.
    let mut tags: Vec<String> = Vec::new();
    state_scene.for_each_node(&mut |visit| {
        if let Scene::External(node) = visit.node {
            if let Some(tag) = node.tag.as_deref() {
                tags.push(tag.to_owned());
            }
        }
    });
    // ★★★★★ R1736 — what each surface DREW, from the same scene and the same
    // rectangles as the sizes below.
    //
    // Beside the size deliberately: a screen that hit-tests itself needs both
    // halves of the same fact, and taking them from two passes is how they
    // would come to disagree about which frame they are describing. See
    // `pinion_core::painted` for what this closes.
    let painted: Vec<(String, Rect)> = tags
        .iter()
        .filter_map(|tag| {
            rect_for_tag(paint_scene, tag)
                .filter(|r| r.w > 0 && r.h > 0)
                .map(|rect| (tag.clone(), rect))
        })
        .collect();
    record_painted_marks(paint_scene, &painted);
    let mut told = 0;
    for tag in tags {
        // ★ `rect_for_tag` on the PAINT scene, which is the same resolution
        // `capture_rel_coords` divides by — so the size announced and the size
        // the fraction is a fraction of are one derivation. Two derivations of
        // one geometry is the defect this whole function exists to remove; it
        // must not reappear inside it.
        let Some(rect) = rect_for_tag(paint_scene, &tag) else {
            // Not painted this frame (a torn-off surface, a hidden pane). Drop
            // the memory so its size is announced again when it returns.
            known.remove(&tag);
            pinion_core::external::forget_surface_size(&tag);
            pinion_core::painted::forget_painted_regions(&tag);
            continue;
        };
        if rect.w == 0 || rect.h == 0 {
            continue; // a degenerate layout is not a size worth acting on
        }
        // ★★ R1684.4 — the size is recorded for readers as well as announced to
        // the widget: `on_resize` is an EVENT and is debounced by `known`,
        // while `surface_size` is a QUESTION that must answer on every frame
        // the surface is painted.
        //
        // ★ Placed above the debounce rather than below it, and the honest
        // reason is not the one first written here. A counterfactual that moved
        // it below PASSED, which is the measurement: the store is a map that
        // persists across frames, so a debounced frame changes nothing either
        // way, and a frame that DOES change is never debounced. The two
        // orderings are equivalent today. It sits here because this is the
        // point at which the rectangle is known to be real — after the
        // degenerate-layout guard and before any early return — so the record
        // cannot come to depend on what the announcement decides.
        pinion_core::external::record_surface_size(&tag, rect.w, rect.h);
        if known.get(&tag) == Some(&(rect.w, rect.h)) {
            continue;
        }
        let Some(external) = state_scene.find_external_with_tag_mut(&tag) else {
            known.remove(&tag);
            continue;
        };
        external.handle.on_resize(rect.w, rect.h);
        known.insert(tag, (rect.w, rect.h));
        told += 1;
    }
    told
}

/// ★★★★★ R1736 — record what ONE surface painted, for a caller that has the
/// paint scene and not the state one.
///
/// The windowed path goes through [`announce_external_sizes`], which does this
/// for every surface on the same pass as the sizes. The in-process sweeps have
/// no state scene to walk for `External` handles — they run `view()` and the
/// layout pass and nothing else — so they say which surface they painted.
///
/// ★ It exists so those sweeps take the SAME path the window does. A fixture
/// that skipped this would leave a screen resolving presses from its model
/// while the running app resolves them from its paint, which is two behaviours
/// under one name and the exact shape R1700 recorded: "the in-process sweeps
/// paint and hit-test inside one owner scope, where the size axis is void by
/// construction".
///
/// Returns whether the surface was painted at all.
pub fn record_painted_surface(paint_scene: &Scene, tag: &str) -> bool {
    let Some(rect) = rect_for_tag(paint_scene, tag).filter(|r| r.w > 0 && r.h > 0) else {
        pinion_core::painted::forget_painted_regions(tag);
        return false;
    };
    record_painted_marks(paint_scene, &[(tag.to_owned(), rect)]);
    true
}

/// ★★★★★ R1736 — record, for each painted surface, the tagged rectangles drawn
/// inside it, in paint order and in that surface's own coordinates.
///
/// One walk for every surface rather than one per surface, because paint order
/// is what the store is FOR and a per-surface walk would have to re-establish
/// it each time.
///
/// A mark belongs to the **smallest** surface whose rectangle contains its
/// centre — the same attribution `scene/pointer_target` makes, stated once here
/// rather than left to walk order. Without it a screen nested inside another
/// would find the host's marks in its own stack and resolve presses to things
/// it does not own.
fn record_painted_marks(paint_scene: &Scene, surfaces: &[(String, Rect)]) {
    let mut marks: BTreeMap<&str, Vec<(String, Rect)>> = surfaces
        .iter()
        .map(|(tag, _)| (tag.as_str(), Vec::new()))
        .collect();
    paint_scene.for_each_node(&mut |visit| {
        let Some(tag) = visit.node.tag() else { return };
        let Some(rect) = visit.absolute_rect() else {
            return; // clipped entirely away: painted nowhere, so not painted
        };
        let (cx, cy) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
        let Some((surface_tag, surface_rect)) = surfaces
            .iter()
            .filter(|(_, r)| cx >= r.x && cy >= r.y && cx < r.x + r.w && cy < r.y + r.h)
            .min_by_key(|(_, r)| u64::from(r.w) * u64::from(r.h))
        else {
            return;
        };
        if surface_tag == tag {
            return; // a surface is not a thing painted inside itself
        }
        if let Some(into) = marks.get_mut(surface_tag.as_str()) {
            into.push((
                tag.to_owned(),
                Rect::new(
                    rect.x.saturating_sub(surface_rect.x),
                    rect.y.saturating_sub(surface_rect.y),
                    rect.w,
                    rect.h,
                ),
            ));
        }
    });
    for (tag, into) in marks {
        pinion_core::painted::record_painted_regions(
            tag,
            pinion_core::painted::PaintedRegions::from_marks(into),
        );
    }
}
