//! R56.1.f.0 §5.13 — Input-event modifier primitives shared between
//! the runtime (winit ↔ shell bridge) and the widget catalog
//! (`WidgetCore::apply_key`). Lives in `pinion-core` so widget code
//! can name [`Modifiers`] without a `pinion-runtime` dependency
//! (which would invert the crate graph — `pinion-runtime` depends on
//! `pinion-core`, not the reverse).
//!
//! Originally defined in `pinion-runtime/src/input.rs` (R51.108 §5.41
//! winit-free abstract surface); R56.1.f.0 lifted the type so the
//! `WidgetCore::apply_key` signature can carry the four-bit modifier
//! state directly into widget keystroke handling. The W3C
//! `KeyboardEvent` modifier surface (`shiftKey` / `ctrlKey` /
//! `altKey` / `metaKey`) is the industry-portable vocabulary every
//! desktop toolkit (winit, GTK, the toolkit, Cocoa) and every browser exposes
//! as independent booleans — refactoring to a bitflag here would
//! diverge from that substrate.
//!
//! The §5.41 TUI bridge (`pinion-tui::input::modifiers_from_crossterm`)
//! and the §5.35 GUI bridge (`pinion-runtime::input::InputRouter` via
//! `pinion-shell::app::modifiers_from_winit`) both construct
//! [`Modifiers`] from their respective platform vocabularies and
//! forward through the [`WidgetCore::apply_key`](crate::widget_core::WidgetCore::apply_key)
//! dispatch path — the substrate stays platform-agnostic.
//!
//! R56.2.a §5.13 §5.38 — extends the surface with [`CompositionEvent`],
//! the W3C `CompositionEvent` mirror that platform IME bridges feed
//! into [`WidgetCore::apply_composition`](crate::widget_core::WidgetCore::apply_composition).
//! The four phases (`Start` / `Update` / `Commit` / `Cancel`) map 1:1
//! to the [`TextFieldExternal::apply_composition_*`](crate::widgets::text_field::TextFieldExternal)
//! substrate landed in R56.1.g. winit 0.30's
//! [`WindowEvent::Ime`](https://docs.rs/winit/0.30/winit/event/enum.WindowEvent.html#variant.Ime)
//! cross-platform abstraction is the canonical desktop bridge (Wayland
//! `text-input-v3` + X11 XIM + macOS `NSTextInputContext` + Windows
//! TSF all funnel through winit's four-variant `Ime` enum); the
//! pinion-shell `app.rs` Ime arm performs the
//! `winit::Ime → CompositionEvent` mapping with `was_composing` state
//! tracking so empty `Preedit` triggers `Cancel` and `Disabled`
//! cancels an in-flight session.

use core::cell::Cell;

use crate::cell_value::CellKind;
use crate::external::IntrospectValue;
use crate::scene::Scene;

/// R876 §5.49 §5.51 / R879 — the press-to-drag distance (logical px,
/// Euclidean): a press that strays past this from its origin *became a drag*
/// (the toolkit `startDragDistance`, the DOM no-`click`-after-drag rule). The runtime's `InputRouter` is the
/// primary judge (its `became_drag` latch gates the `DnD` trailing click and the
/// double-click detector), but the determination is a framework *contract*: a
/// capture-path External that must distinguish its own click from its own drag
/// (the node-graph release-select suppression + drag dead zone) measures
/// against the SAME constant, so the two paths can never disagree on what a
/// click is. Lives in `pinion-core` (the contract crate), not the runtime that happens
/// to apply it ([[helper-crate-home-ssot-axis]] — the R877.1 `LINE_HEIGHT_PX` precedent).
pub const DRAG_CLICK_THRESHOLD_PX: f64 = 4.0;

/// R880 — the press-to-drag **latch** over [`DRAG_CLICK_THRESHOLD_PX`]: a
/// press origin plus the sticky "became a drag" determination. The metric
/// (Euclidean logical-px distance from the origin, strictly greater than
/// the threshold) was re-derived at three sites — the runtime router's
/// press tracker, the node-graph drag dead zone (R879.1) and its marquee
/// twin — so the *predicate* joins the *constant* in the contract crate
/// ([[helper-crate-home-ssot-axis]]); every consumer advances the same
/// latch and can never disagree on what a click is.
///
/// Once latched the gesture stays a drag for its lifetime (W3C: a drag
/// cancels the click/double-click cycle even if the cursor returns to the
/// origin) — the latch is dropped with the press, never reset.
#[derive(Clone, Copy, Debug)]
pub struct DragLatch {
    origin: (f64, f64),
    live: bool,
}

impl DragLatch {
    /// Open the latch at the press origin (logical px).
    #[must_use]
    pub const fn new(origin: (f64, f64)) -> Self {
        Self {
            origin,
            live: false,
        }
    }

    /// Advance with the current cursor (logical px, same space as the
    /// origin); latches once the press strays past
    /// [`DRAG_CLICK_THRESHOLD_PX`]. Returns the post-advance [`Self::live`].
    pub fn advance(&mut self, cursor: (f64, f64)) -> bool {
        if !self.live {
            let dx = cursor.0 - self.origin.0;
            let dy = cursor.1 - self.origin.1;
            self.live = dx.hypot(dy) > DRAG_CLICK_THRESHOLD_PX;
        }
        self.live
    }

    /// Whether this press became a drag.
    #[must_use]
    pub const fn live(&self) -> bool {
        self.live
    }
}

/// R1549 §5.35 §5.38 — press-and-hold **auto-repeat** cadence: how long a
/// held press waits before it starts repeating, and how fast it repeats
/// after that. The toolkit `autoRepeatDelay` /
/// `autoRepeatInterval` pair, plus the `accelerated`
/// axis the toolkit keeps on a different class, expressed as one closed-form
/// declaration.
///
/// A widget declares one through
/// [`External::auto_repeat`](crate::external::External::auto_repeat); the
/// runtime router supplies the clock. Holding an increment arrow, a
/// pagination chevron or a scroll step then keeps stepping — the desktop
/// behaviour every professional tool has and which no pinion widget had
/// before this type existed.
///
/// # Cadence
///
/// A hold fires at
///
/// ```text
/// delay, delay + i(0), delay + i(0) + i(1), …
/// where i(n) = max(min_interval, interval * accel^n)
/// ```
///
/// so `accel == 1.0` (the default) is the toolkit's fixed-interval repeat and
/// `accel < 1.0` is an accelerating one that bottoms out at
/// `min_interval`. Closed-form in the fire ordinal — the ratified
/// closed-form-primitive axis — so
/// [`Self::interval_after`] answers any point of the ramp without
/// replaying the hold, which is what lets the cadence be *published*
/// rather than only observed.
///
/// # Defaults
///
/// [`Self::DEFAULT_DELAY_SECS`] = 300 ms and
/// [`Self::DEFAULT_INTERVAL_SECS`] = 100 ms are the toolkit's
/// `AUTO_REPEAT_DELAY` / `AUTO_REPEAT_INTERVAL` (`qabstractbutton.cpp`) —
/// the widest-deployed desktop pair, and the toolkit-parity floor this
/// framework measures against. Platform *keyboard* repeat (the OS
/// `repeat` flag pinion already forwards on key presses) is a separate,
/// user-configurable channel and is deliberately NOT read here: a
/// pointer hold on a widget is the widget's cadence, not the keyboard's.
///
/// # Validation
///
/// Every constructor clamps into a sane domain, so a malformed
/// declaration can never hang a frame: intervals are held at or above
/// [`Self::MIN_INTERVAL_FLOOR_SECS`] (a non-finite or non-positive
/// interval would make the router's catch-up loop unbounded for a large
/// `dt`), the delay saturates at `0.0`, and `accel` is confined to
/// `(0.0, 1.0]` (an `accel > 1.0` would *decelerate* without bound —
/// expressible as a longer `interval` instead, so admitting it would be
/// two spellings of one cadence).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoRepeat {
    delay_secs: f32,
    interval_secs: f32,
    accel: f32,
    min_interval_secs: f32,
}

impl AutoRepeat {
    /// The toolkit `AUTO_REPEAT_DELAY` — the hold a press must survive before the first repeat
    /// fires. Long enough that an ordinary click never repeats.
    pub const DEFAULT_DELAY_SECS: f32 = 0.300;

    /// The toolkit `AUTO_REPEAT_INTERVAL` — the steady-state gap between repeats once the delay
    /// has elapsed (10 Hz).
    pub const DEFAULT_INTERVAL_SECS: f32 = 0.100;

    /// Hard floor on any interval (1 ms = 1000 Hz). Bounds the router's
    /// per-frame catch-up loop for any `dt`, so a pathological
    /// declaration costs frames rather than hanging one.
    pub const MIN_INTERVAL_FLOOR_SECS: f32 = 0.001;

    /// The desktop default cadence — [`Self::DEFAULT_DELAY_SECS`] then
    /// [`Self::DEFAULT_INTERVAL_SECS`] forever, the toolkit's
    /// `setAutoRepeat(true)` with both properties left
    /// alone.
    #[must_use]
    pub fn desktop() -> Self {
        Self::new(Self::DEFAULT_DELAY_SECS, Self::DEFAULT_INTERVAL_SECS)
    }

    /// A fixed-interval cadence: wait `delay_secs`, then fire every
    /// `interval_secs`. Both are clamped (see the type's validation
    /// section).
    #[must_use]
    pub fn new(delay_secs: f32, interval_secs: f32) -> Self {
        let interval = clamp_interval(interval_secs);
        Self {
            delay_secs: if delay_secs.is_finite() {
                delay_secs.max(0.0)
            } else {
                0.0
            },
            interval_secs: interval,
            accel: 1.0,
            min_interval_secs: interval,
        }
    }

    /// Add acceleration: each successive interval is `accel` times the previous
    /// one, never dropping below `min_interval_secs`. The toolkit spells this `setAccelerated(true)` — a bare
    /// on/off with no reachable curve; here the curve is the declaration, so
    /// two widgets can ramp differently and each can say how.
    ///
    /// `accel` is clamped into `(0.0, 1.0]`; a non-finite value disables
    /// acceleration rather than poisoning the cadence.
    #[must_use]
    pub fn accelerating(mut self, accel: f32, min_interval_secs: f32) -> Self {
        self.accel = if accel.is_finite() {
            accel.clamp(f32::MIN_POSITIVE, 1.0)
        } else {
            1.0
        };
        self.min_interval_secs = clamp_interval(min_interval_secs).min(self.interval_secs);
        self
    }

    /// Seconds a press must be held before the FIRST repeat fires.
    #[must_use]
    pub const fn delay_secs(&self) -> f32 {
        self.delay_secs
    }

    /// The un-accelerated (first) repeat interval in seconds.
    #[must_use]
    pub const fn interval_secs(&self) -> f32 {
        self.interval_secs
    }

    /// Per-fire interval multiplier; `1.0` = no acceleration.
    #[must_use]
    pub const fn accel(&self) -> f32 {
        self.accel
    }

    /// Floor the accelerating interval bottoms out at, in seconds. Equal
    /// to [`Self::interval_secs`] when acceleration is off.
    #[must_use]
    pub const fn min_interval_secs(&self) -> f32 {
        self.min_interval_secs
    }

    /// The gap that follows the `fires`-th repeat (0-based), i.e. how
    /// long after fire *n* the next one lands. Closed form in `n`:
    /// `max(min_interval, interval * accel^n)`.
    ///
    /// `powi` saturates to `0.0` for a long enough hold with `accel < 1`,
    /// at which point the `max` pins the answer to the floor — so the
    /// result is always at least [`Self::MIN_INTERVAL_FLOOR_SECS`],
    /// however long the hold ran.
    #[must_use]
    pub fn interval_after(&self, fires: u32) -> f32 {
        let ramped = self.interval_secs * self.accel.powi(clamp_exp(fires));
        ramped.max(self.min_interval_secs)
    }

    /// Seconds from the press until the `n`-th repeat (1-based) fires —
    /// `delay + Σ interval_after(k)` for `k < n - 1`. `0` answers the
    /// delay itself (no repeat has fired yet), so the sequence reads
    /// `elapsed_at_fire(1) == delay_secs`.
    ///
    /// The ramp is a geometric series, but summing it in closed form
    /// would disagree with the router once the `min_interval` floor
    /// truncates it; this walks the same [`Self::interval_after`] the
    /// router steps, so the published schedule and the fired schedule
    /// are one derivation. Bounded by `n`, which callers hold.
    #[must_use]
    pub fn elapsed_at_fire(&self, n: u32) -> f32 {
        let mut t = self.delay_secs;
        for k in 1..n {
            t += self.interval_after(k - 1);
        }
        t
    }
}

/// Clamp a declared interval into `[MIN_INTERVAL_FLOOR_SECS, ∞)`,
/// mapping non-finite input onto the floor. Shared by
/// [`AutoRepeat::new`] and [`AutoRepeat::accelerating`] so the two
/// entry points cannot disagree about what a legal interval is.
fn clamp_interval(secs: f32) -> f32 {
    if secs.is_finite() {
        secs.max(AutoRepeat::MIN_INTERVAL_FLOOR_SECS)
    } else {
        AutoRepeat::MIN_INTERVAL_FLOOR_SECS
    }
}

/// `powi` takes an `i32`; a fire count past `i32::MAX` is unreachable in
/// a hold (it would need ~10^9 frames) but the cast must still be total.
/// Saturating keeps the ramp monotone at the extreme instead of wrapping
/// it back to a *longer* interval.
#[allow(clippy::cast_possible_wrap)]
const fn clamp_exp(fires: u32) -> i32 {
    if fires > i32::MAX as u32 {
        i32::MAX
    } else {
        fires as i32
    }
}

/// R882 §5.39 §5.35 — held-key absolute state for the **non-modifier
/// chord vocabulary**: the [`Modifiers`] out-of-band cache pattern
/// generalised past `ModifiersState`. Windowing systems deliver
/// per-key press/release edges but no queryable "is this key held"
/// fact, so the shell tracks it — and the *vocabulary* (which keys are
/// chords at all) is a single cross-backend policy: a GUI that pans on
/// held-`Space` while the TUI's cache forgot the key would be a §2 #6
/// dual-invariant bug, so the decode lives once here in the contract
/// crate (the [`DRAG_CLICK_THRESHOLD_PX`] /
/// [[helper-crate-home-ssot-axis]] discipline), not per shell.
///
/// Tracked today: `Space` — the design tool / the raster editor / Krita hand-tool
/// pan chord (left-drag pans while held). Closed-form like [`Modifiers`]: future chord
/// keys (an `H` hand tool, a `Z` zoom chord) extend the struct by a `SemVer` minor
/// bump.
///
/// Key strings: pinion's named-key boundaries emit `"Space"` (the
/// winit `NamedKey` / crossterm-bridge spelling — both backends
/// normalise to it), while the W3C `KeyboardEvent.key` value for the
/// spacebar is the `" "` character — [`Self::note`] accepts BOTH, the
/// same dual-spelling tolerance the listbox typeahead /
/// `virtual_select` keystroke decoders already apply, so an RPC
/// client speaking strict W3C cannot silently arm nothing. Both
/// producers — the winit `KeyboardInput` edges and the
/// `scene/key state:"down"/"up"` RPC peer — feed [`Self::note`], so
/// native and AI-driven chords can never diverge.
///
/// Lifetime: the GUI shell clears the cache on window blur (the
/// browser missed-keyup convention — the keyup goes to whichever
/// window stole focus). The TUI has no blur event on the baseline
/// crossterm protocol, so its cache is RPC-owned and persists until
/// the client releases it (`state:"up"`) — a documented §2 #6
/// divergence carry, the same class as the TUI paste axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeldKeys {
    space: bool,
}

impl HeldKeys {
    /// Record a key edge: `pressed = true` on key-down (auto-repeat
    /// re-sends are idempotent), `false` on key-up. Keys outside the
    /// chord vocabulary are ignored.
    pub fn note(&mut self, key: &str, pressed: bool) {
        if key == "Space" || key == " " {
            self.space = pressed;
        }
    }

    /// Whether `Space` — the pan chord — is currently held.
    #[must_use]
    pub const fn space(self) -> bool {
        self.space
    }

    /// Forget every held key — the window-blur arm (a keyup that
    /// raced the focus loss never arrives; a stranded chord would
    /// turn every later left drag into a pan).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// R885 §5.49 — enumerate the currently-held chord keys by their
    /// canonical *named* wire spelling (the `scene/key` vocabulary the
    /// winit boundary emits — `"Space"`, never the W3C `" "`
    /// character [`Self::note`] also tolerates on input). One home
    /// for the held-set enumeration: the `scene/input_state` READ
    /// peer serializes exactly this list, so the read is the inverse
    /// of the `scene/key state:"down"` writes by construction.
    #[must_use]
    pub fn held_names(self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.space {
            names.push("Space");
        }
        names
    }
}

/// R885 §5.49 — by-value snapshot of the shell's out-of-band input
/// state, resolved by the embedder before each RPC dispatch and
/// consumed by the `scene/input_state` READ method (the
/// `fragment_cache_stats` resolution pattern). Lives next to
/// [`Modifiers`] / [`HeldKeys`] because it is pure contract data both
/// backends produce and `pinion-rpc` serializes
/// ([[helper-crate-home-ssot-axis]]).
///
/// Field shapes mirror their write peers (read = inverse of write):
///
/// * [`Self::modifiers`] — the `scene/modifiers` absolute cache.
///   `None` = the backend keeps no absolute modifier state (the TUI:
///   crossterm delivers modifiers per-key-event only; its
///   `scene/modifiers` gap is the documented §2 #6 carry). The wire
///   surfaces `null` so an AI client can tell "axis unavailable"
///   from "no modifier held".
/// * [`Self::held_keys`] — [`HeldKeys::held_names`], the
///   `scene/key state:"down"/"up"` chord cache (both backends).
/// * [`Self::cursor`] — the dispatch-scoped window's last mouse
///   cursor position, the state every `scene/click` / `scene/hover` /
///   `scene/drag` `x`/`y` writes. `None` until the first cursor
///   event lands in that window.
/// * [`Self::key_dispatch`] — R1074 §5.39 §5.16, the multi-window
///   keyboard-dispatch gate state ([`KeyDispatchFocus`]). `None` on a
///   single-OS-window backend (the TUI), the "axis unavailable" honesty
///   of [`Self::modifiers`]; `Some` on the GUI shell whose key routing
///   is gated per OS window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InputStateSnapshot {
    /// Absolute modifier cache (`None` = backend tracks none).
    pub modifiers: Option<Modifiers>,
    /// Canonical named spellings of the held chord keys.
    pub held_keys: Vec<&'static str>,
    /// R1619 §5.35 §5.16 — canonical wire names of the pointer buttons held
    /// right now ([`PointerButtons::held_names`]), for the dispatch-scoped
    /// window's mouse pointer.
    ///
    /// The READ peer of the `scene/pointer_button` writes, exactly as
    /// [`held_keys`](Self::held_keys) is of `scene/key`: an AI that drives a
    /// drag-select over the wire can confirm the press it sent is still held
    /// before sending the moves that depend on it. An empty list means no
    /// button is held — a backend that cannot answer the axis at all does not
    /// arise here, because the framework itself owns the state (unlike
    /// [`modifiers`](Self::modifiers), which mirrors a platform cache).
    pub held_pointer_buttons: Vec<&'static str>,
    /// Last cursor position in the dispatch-scoped window.
    pub cursor: Option<(f64, f64)>,
    /// R1074 §5.39 §5.16 — multi-window key-dispatch gate state, or
    /// `None` on a single-OS-window backend. See [`KeyDispatchFocus`].
    pub key_dispatch: Option<KeyDispatchFocus>,
    /// R1620 §5.45 §5.16 — what the mouse pointer's **auto-scroll** is doing,
    /// or `None` when no gesture is holding a scroll region.
    ///
    /// A view that moves on its own is the one thing an agent watching
    /// `scene/scroll_state` cannot explain: the offset changes with no call
    /// having been made. This says which gesture is doing it and at what
    /// speed — and, because the declared band travels with it, also answers
    /// the harder question of why a drag near an edge is NOT scrolling.
    pub auto_scroll: Option<AutoScrollState>,
}

/// R1620 §5.45 §5.16 — the live auto-scroll of one held pointer: the region's
/// declared ramp and the velocity that ramp is currently asking for.
///
/// Present only while a button is held over a pinned scroll region, which is
/// the same gate the behaviour itself has — so "absent" means "no gesture owns
/// a scroll region", never "the axis is unavailable".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoScrollState {
    /// Logical px/s along x; negative scrolls toward the origin. `0.0` when the
    /// pointer is outside the band on this axis.
    pub velocity_x: f64,
    /// Logical px/s along y.
    pub velocity_y: f64,
    /// The region's declared edge band, in logical px (`0.0` = auto-scroll off).
    pub margin: f64,
    /// The region's declared top speed, in logical px/s.
    pub max_speed: f64,
}

/// R1074 §5.39 §5.16 — the multi-window keyboard-dispatch gate state,
/// the READ peer of the R1071/R1073 `os_focused_window` +
/// `key_press_owner` writes (the GUI shell's per-OS-window key routing
/// gate). Present (`Some`) only on a backend that dispatches keys
/// across multiple OS windows; a single-OS-window backend (the TUI: one
/// process = one alternate screen, no `WindowId`) surfaces the whole
/// axis as `None` — the same "axis unavailable, not empty" honesty as
/// [`InputStateSnapshot::modifiers`].
///
/// Distinct from §5.39 *widget* focus (`FocusManager::focused_tag`,
/// which widget receives keys): this is which *OS window* may dispatch
/// keys at all ([[routing-and-focus-are-separate-axes]]). Exposing it
/// makes the close-during-dispatch gate (R1073) — whose decision an AI
/// otherwise cannot observe — introspectable: the gate admits a
/// continuation press iff its window both owns the press
/// ([`Self::key_press_owners`]) and is the OS-focused window
/// ([`Self::os_focused_window`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyDispatchFocus {
    /// The OS-focused window id — the key-dispatch gate target — or
    /// `None` when no window currently holds OS keyboard focus (the
    /// gate then fails OPEN). Mirrors the shell's `os_focused_window`.
    pub os_focused_window: Option<String>,
    /// `(key, owning_window)` for every currently-held key, pinned at
    /// the press rising edge and cleared on its release edge, **sorted
    /// by key** for a deterministic snapshot. Mirrors the shell's
    /// `key_press_owner`.
    pub key_press_owners: Vec<(String, String)>,
    /// R1428 §5.39 §5.16 §5.41 — the **fails-open focus verdict for the
    /// dispatch-scoped window** (the request's `{window: "<id>"}` scope),
    /// derived at snapshot time from [`Self::os_focused_window`], never
    /// stored: `true` when this window holds OS focus OR when no window's
    /// focus is known (the gate fails open). It is the SAME predicate the
    /// GUI shell gates key admission on (`is_key_dispatch_window`) AND the
    /// R1427 terminal-cursor render on, so an AI reads the exact bit that
    /// predicts the cursor's filled-vs-hollow state (`true` → filled,
    /// `false` → hollow) in ONE `scene/input_state {window}` call —
    /// instead of correlating a snapshot with a client-side compare of
    /// [`Self::os_focused_window`] against a hard-coded window id.
    ///
    /// Distinct from [`Self::os_focused_window`] (the global "who is
    /// focused") — `focused` is that fact PROJECTED onto this dispatch's
    /// window through the fails-open gate. On the single-OS-window backend
    /// (the TUI) the whole axis is `None` (this field is unreachable), so
    /// hollow-on-blur stays GUI-only exactly like R1427.
    pub focused: bool,
}

/// R880.1 — the multi-select pointer-chord policy, decoded ONCE for every
/// set-mutating click / marquee consumer: a command chord
/// ([`Modifiers::command_key`] — `Ctrl`, or `Cmd` on macOS) **toggles**
/// membership; else `Shift` **extends**; else a plain interaction
/// **replaces**. The chord→verb *precedence* is one policy — before this
/// lift the identical if-chain lived in the list/grid coordinator
/// (`VirtualSelectExternal`), the node-graph click, and the marquee
/// release, and a divergence (one widget testing `Shift` first, one
/// reading `Ctrl` without `Meta`) would be a cross-widget UX bug.
///
/// What *extend* means is the consumer's model decision and is deliberately
/// not encoded here: an ordered list extends the range from its anchor (the
/// W3C listbox / the toolkit `ExtendedSelection` convention), an unordered canvas unions the
/// swept set in (the engine graph convention).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionChord {
    /// No chord — replace the selection with the interaction's target.
    Replace,
    /// `Ctrl` / `Cmd` — toggle the target's membership.
    Toggle,
    /// `Shift` — extend (range on ordered models, union on unordered).
    Extend,
}

impl SelectionChord {
    /// Decode the held modifiers into the selection verb.
    #[must_use]
    pub const fn from_modifiers(mods: Modifiers) -> Self {
        if mods.command_key() {
            Self::Toggle
        } else if mods.shift_key() {
            Self::Extend
        } else {
            Self::Replace
        }
    }
}

/// R902.1 — the **non-navigation** multi-select keyboard set-op a key+modifier
/// chord maps to, decoded ONCE for every multi-select keyboard consumer (the
/// list/grid [`nav_select_key`](crate::widgets::virtual_select::nav_select_key)
/// and the tree-grid outliner). Unlike [`SelectionChord`] (which decodes the
/// modifiers on a *navigation* key into replace / toggle / extend), this
/// classifies the keys that are **not** navigation — `Ctrl+A` (select all) and
/// `Ctrl+Space` (toggle the active row) — so the consumer swallows them without
/// computing a navigation target. The chord→op mapping is one policy: a
/// divergence (one widget binding `Ctrl+A`, another `Cmd+A` only) would be a
/// cross-widget keyboard bug, so it lives here, not re-typed per consumer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiSelectKeyOp {
    /// `Ctrl`/`Cmd`+A — select every (visible) row.
    SelectAll,
    /// `Ctrl`/`Cmd`+Space — toggle the active (cursor) row's membership.
    ToggleCursor,
}

impl MultiSelectKeyOp {
    /// Classify a key + held modifiers into a non-navigation set-op, or `None`
    /// when the chord is not a multi-select set-op (the caller then treats the
    /// key as navigation / type-ahead). The command gate is
    /// [`Modifiers::command_key`] (Ctrl, or Cmd on macOS), matching the
    /// text-field / list select-all chord; `Space` is spelled both `" "` and
    /// `"Space"` (the character-key vs named-key wire forms).
    #[must_use]
    pub fn classify(key: &str, mods: Modifiers) -> Option<Self> {
        if !mods.command_key() {
            return None;
        }
        if key.eq_ignore_ascii_case("a") {
            Some(Self::SelectAll)
        } else if key == " " || key == "Space" {
            Some(Self::ToggleCursor)
        } else {
            None
        }
    }
}

/// R1658 §5.13 §5.39 — which platform delivery a keystroke came out of.
///
/// Opaque and comparable, not arithmetic: the only question a consumer may
/// ask is whether two keystrokes carry the *same* batch, so the value is not
/// a number anyone can do sums on. Ids are allocated per binding and never
/// reused within a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBatch(u64);

impl KeyBatch {
    /// The batch a freshly-built shell starts in, before any platform
    /// delivery has opened one. Distinct from every batch
    /// [`next`](Self::next) produces, so a key dispatched before the first
    /// delivery does not claim to have arrived with one.
    #[must_use]
    pub const fn initial() -> Self {
        Self(0)
    }

    /// The batch after this one. The runtime owns the allocation; a binding
    /// never calls this.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// R1658 §5.13 §5.39 — when a keystroke reached the runtime, and which
/// platform delivery it arrived in.
///
/// # Why an event has to carry this
///
/// Without it the only clock an embedder can read is `Instant::now()` inside
/// its own handler, and that is *when this app got round to the key*, not
/// *when the key arrived*. The two differ by however long the previous
/// keystroke's handler took — so any window judged against the handler's
/// clock (a repeat window, a double-tap, a chord timeout) silently shrinks
/// under load, by exactly the amount of work the app is doing.
///
/// That is a measured user-facing defect, not a hypothetical: an embedder
/// with a blocking round trip per keystroke lost two presses out of three
/// from a 500 ms repeat window under 2x CPU oversubscription, and the
/// dropped presses went through to the terminal underneath as raw escapes.
/// The same product's terminal frontend had no such defect, because its
/// input layer hands over the read a key came out of.
///
/// # The two halves, and why both
///
/// [`at`](Self::at) is *when*, [`batch`](Self::batch) is *with what*. They
/// answer different questions and neither derives the other: two keys the
/// platform handed over in one delivery share a batch **and** an instant,
/// but two keys with instants a microsecond apart may still be separate
/// deliveries. A gesture is a statement about the second — three presses out
/// of one platform read are one gesture however long the app spends between
/// them — which is why [`arrived_with`](Self::arrived_with) is published as
/// a predicate rather than left to be re-derived from an id.
///
/// # What it is not
///
/// It is **not** when the human pressed the key. No portable windowing layer
/// offers that: winit exposes no platform event timestamp, so this is stamped
/// by the runtime as the delivery is opened — before any binding handler runs,
/// which is the whole of what the defect above needs. Naming it `arrival`
/// rather than `time` is the honest shape.
///
/// Carrying it on the event also makes a keystroke **replayable**: a handler
/// that reads `Instant::now()` cannot be replayed at all, where one that
/// reads the arrival it was given can be handed a recorded one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyArrival {
    at: std::time::Instant,
    batch: KeyBatch,
}

impl KeyArrival {
    /// Build an arrival. The runtime calls this as it opens a delivery;
    /// a test calls it to hand a binding a known arrival.
    #[must_use]
    pub const fn new(at: std::time::Instant, batch: KeyBatch) -> Self {
        Self { at, batch }
    }

    /// When the runtime opened the delivery this keystroke came in.
    ///
    /// Stamped before any binding handler ran — see the type's own doc for
    /// what that buys and what it does not claim.
    #[must_use]
    pub const fn at(self) -> std::time::Instant {
        self.at
    }

    /// Which platform delivery this keystroke came in.
    #[must_use]
    pub const fn batch(self) -> KeyBatch {
        self.batch
    }

    /// Whether `other` came out of the same platform delivery as this one —
    /// "these arrived together".
    ///
    /// The predicate rather than the id, because this is the whole of what a
    /// consumer wants to know and a consumer that compares ids itself is a
    /// second author of the rule.
    #[must_use]
    pub fn arrived_with(self, other: Self) -> bool {
        self.batch == other.batch
    }
}

/// R1658 §5.13 §5.39 — one keystroke, with everything the runtime knows
/// about it at dispatch time.
///
/// Exists so that the next fact a keystroke needs to carry is a **field**
/// rather than a fourth hook. The keyboard entry point has already grown
/// once this way — `apply_key` → `apply_key_repeat` added the platform
/// auto-repeat flag as a whole new method, and every impl of the old one had
/// to keep working — and a hook per fact does not scale past two.
///
/// [`WidgetCore::apply_key_press`](crate::WidgetCore::apply_key_press) is
/// the hook that takes it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct KeyPress<'a> {
    /// W3C `KeyboardEvent.key` name — the same string
    /// [`apply_key`](crate::WidgetCore::apply_key) receives.
    pub key: &'a str,
    /// Modifier state at dispatch time.
    pub modifiers: Modifiers,
    /// `true` for an OS auto-repeat re-send of a held key, `false` for the
    /// leading press. Synthesised keystrokes are never repeats.
    pub repeat: bool,
    /// When this keystroke reached the runtime, and what it arrived with.
    pub arrival: KeyArrival,
}

impl<'a> KeyPress<'a> {
    /// Build a keystroke. The runtime calls this on every dispatch path;
    /// a test calls it to drive a binding directly.
    #[must_use]
    pub const fn new(
        key: &'a str,
        modifiers: Modifiers,
        repeat: bool,
        arrival: KeyArrival,
    ) -> Self {
        Self {
            key,
            modifiers,
            repeat,
            arrival,
        }
    }
}

/// R56.1.f.0 §5.13 — abstract modifier-key state, mirroring
/// `winit::keyboard::ModifiersState` and W3C DOM Level 3
/// `getModifierState` without the winit dependency. Four modifier
/// bits cover the desktop-portable baseline (Shift / Control / Alt /
/// Meta). Closed-form: future modifiers (`CapsLock` / `NumLock` /
/// Hyper) are rare enough that a `SemVer` minor bump is the
/// textbook extension path (rather than the §5.13
/// `#[non_exhaustive]`-style hedge which only applies cleanly to enum
/// variants where a wildcard arm has a meaningful default).
///
/// `clippy::struct_excessive_bools` lint is intentionally suppressed:
/// the four-bool shape mirrors the W3C `KeyboardEvent` modifier
/// surface (`shiftKey` / `ctrlKey` / `altKey` / `metaKey`), which
/// every browser and every desktop windowing toolkit (winit, GTK,
/// the toolkit, Cocoa) exposes as independent booleans — refactoring to a
/// bitflag or state-machine here would diverge from the industry
/// vocabulary substrate callers expect.
// R1569 §5.39 — `Hash` because a modifier state is half of a
// [`Chord`](crate::accelerator::Chord), and a chord is the natural key of a
// keymap. Free on a four-`bool` POD that already derives `Eq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Modifiers {
    /// Shift key (left or right) currently held.
    pub shift: bool,
    /// Control key (left or right) currently held.
    pub ctrl: bool,
    /// Alt / Option key (left or right) currently held.
    pub alt: bool,
    /// Meta / Cmd / Super / Windows key currently held.
    pub meta: bool,
}

impl Modifiers {
    /// Zero-modifier state, matching `winit::keyboard::ModifiersState::empty`.
    /// Used by the substrate's `ShellCore::new` to initialise the
    /// modifier cache before the first `ModifiersChanged` event, and
    /// by RPC dispatch paths that surface a no-modifier keystroke
    /// (the `IntrospectValue::Text(key)` variant of
    /// `invoke("key", ...)` — see R56.1.d).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        }
    }

    /// Shift-bit accessor matching the winit `ModifiersState::shift_key`
    /// method shape. Substrate callers read this for Tab-reverse focus
    /// traversal (R51.83 §5.40) and R56.1.f Shift-Arrow text selection.
    #[must_use]
    pub const fn shift_key(self) -> bool {
        self.shift
    }

    /// Control-bit accessor mirroring `winit::keyboard::ModifiersState::control_key`.
    #[must_use]
    pub const fn control_key(self) -> bool {
        self.ctrl
    }

    /// Alt-bit accessor mirroring `winit::keyboard::ModifiersState::alt_key`.
    #[must_use]
    pub const fn alt_key(self) -> bool {
        self.alt
    }

    /// Meta-bit accessor mirroring `winit::keyboard::ModifiersState::super_key`.
    #[must_use]
    pub const fn meta_key(self) -> bool {
        self.meta
    }

    /// R880.1 — the **command-chord predicate**: `Ctrl` or `Meta` (the
    /// macOS `Cmd` key reaches winit as `super`/meta). R879.1 ratified
    /// `control_key() || meta_key()` as the gate for editor command chords
    /// (`Ctrl/Cmd+A/C/X/V/Z`); the predicate was then re-derived inline at
    /// every chord site (text field, field keymap, select-all, undo) —
    /// this accessor is its one home, so no site can drift to a
    /// Ctrl-only decode that leaves `Cmd` dead on macOS.
    #[must_use]
    pub const fn command_key(self) -> bool {
        self.ctrl || self.meta
    }

    /// `true` iff no modifier is held — convenience for the canonical
    /// "plain keystroke" branch in `apply_key` implementations
    /// (Shift+Arrow extends a text selection; plain Arrow moves the
    /// caret without selection).
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !self.shift && !self.ctrl && !self.alt && !self.meta
    }

    /// R781 §5.35 §5.41 — encode the held modifiers into the compact wire
    /// token the R51.42 composite-send payload carries (`"<key>:<Event>:<token>"`).
    ///
    /// The token is the canonical-order subset of `scam` (shift, ctrl, alt,
    /// meta), so `Modifiers { shift: true, ctrl: true, .. }` → `"sc"`. An
    /// empty modifier state yields `""`, and the router omits the trailing
    /// `":<token>"` segment entirely (the two-segment back-compat wire every
    /// pre-R781 composite consumer already parses). Inverse of
    /// [`from_wire_token`](Self::from_wire_token) — the R773 encode↔decode
    /// SSOT discipline applied to the pointer modifier axis.
    #[must_use]
    pub fn as_wire_token(self) -> String {
        let mut token = String::new();
        if self.shift {
            token.push('s');
        }
        if self.ctrl {
            token.push('c');
        }
        if self.alt {
            token.push('a');
        }
        if self.meta {
            token.push('m');
        }
        token
    }

    /// R781 §5.35 §5.41 — decode a wire modifier token (any order of the
    /// `scam` letters) back into [`Modifiers`]. Inverse of
    /// [`as_wire_token`](Self::as_wire_token). Returns `None` on any letter
    /// outside `scam` so a malformed token is rejected rather than silently
    /// dropping bits (a stale wire from an older protocol revision surfaces
    /// as "no modifiers handled" at the decode site, not a misparse).
    #[must_use]
    pub fn from_wire_token(token: &str) -> Option<Self> {
        let mut m = Self::empty();
        for ch in token.chars() {
            match ch {
                's' => m.shift = true,
                'c' => m.ctrl = true,
                'a' => m.alt = true,
                'm' => m.meta = true,
                _ => return None,
            }
        }
        Some(m)
    }
}

/// R56.2.a §5.13 §5.38 — abstract IME composition phase event,
/// mirroring W3C UI Events `CompositionEvent` without a winit
/// dependency. Carries one of four phases that map 1:1 to the
/// [`TextFieldExternal::apply_composition_*`](crate::widgets::text_field::TextFieldExternal)
/// substrate landed in R56.1.g:
///
/// - [`CompositionEvent::Start`]: begin composition. Mirrors the W3C
///   `compositionstart` event (data is empty / not yet known). Callers
///   should fire this once per composition session before any
///   `Update`; the substrate is defensive against missing-start
///   (`Update` without a prior `Start` is a no-op at the
///   [`TextEditState`](crate::widgets::text_edit::TextEditState)
///   layer), but the SCXML transition that gates the caret-blink
///   posture only fires through `Start`.
/// - [`CompositionEvent::Update`]: replace the active preedit with
///   `text`. Mirrors W3C `compositionupdate` (the `data` field carries
///   the new preedit). Empty `text` is canonically just an empty
///   preedit (the user has deleted all in-flight characters but
///   composition stays open); use [`CompositionEvent::Commit`] or
///   [`CompositionEvent::Cancel`] to end the session.
/// - [`CompositionEvent::Commit`]: end composition by inserting `text`
///   at the caret. Mirrors W3C `compositionend` with non-empty `data`.
///   Empty `text` is the canonical "no-data compositionend" shape and
///   the substrate routes it through `preedit_cancel` (matches the
///   Wayland `text-input-v3` cancel-via-empty-commit behaviour).
/// - [`CompositionEvent::Cancel`]: end composition without inserting
///   any text. Mirrors IME cancel (Escape during preedit, blur with
///   discarded composition, `WindowEvent::Ime::Disabled` mid-flight).
///
/// `#[non_exhaustive]` reserves room for future Wayland-style
/// `delete_surrounding` (text replacement) and explicit
/// `set_surrounding` (context-aware IME) variants without a `SemVer`
/// break — winit 0.30's `Ime` enum stays at the four-variant shape
/// the cross-platform LCD supports today, so the substrate matches.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompositionEvent {
    /// Begin a composition session. Substrate seeds the
    /// preedit buffer via [`TextEditState::preedit_start`](crate::widgets::text_edit::TextEditState::preedit_start)
    /// AND drives [`TextFieldEvent::BeginEdit`](crate::widgets::text_field::TextFieldEvent::BeginEdit)
    /// through the SCXML.
    Start,
    /// Replace the active preedit with the carried `String`. Updates
    /// the reactive preedit sidecar; the SCXML stays in `Editing`.
    Update(String),
    /// End the composition by inserting the carried `String` at the
    /// caret position. Empty string is the no-data compositionend
    /// shape and routes through cancel.
    Commit(String),
    /// End the composition without inserting any text.
    Cancel,
}

/// R773 §5.35 §5.13 — the W3C pointer-event name subset that the
/// composite-tag input router emits over the `send` wire to
/// command-class [`External`](crate::external::External) widgets.
///
/// This is the **wire vocabulary** for the `invoke("send", "<name>")`
/// channel: the `InputRouter` rewrites a
/// paint hit-target into a bare event name (or a `"<sub>:<name>"`
/// composite, see [`composite_tag`](crate::composite_tag)) and forwards
/// it; the receiving widget decodes the `<name>` half. Lifting the five
/// names into one enum makes the **encode** site (the router, via
/// [`as_wire_name`](Self::as_wire_name)) and every **decode** site (via
/// [`from_wire_name`](Self::from_wire_name)) reference a single
/// vocabulary instead of independent string literals — a divergence
/// between producer and consumer would be a silent wire bug (the router
/// emits a name no decoder recognises and the event vanishes), not a
/// style choice, so the pair lives once here (`decode == inverse(encode)`,
/// the R743.1 / R745 / R770.1 SSOT class).
///
/// Lives in `pinion-core::input` alongside [`Modifiers`] and
/// [`CompositionEvent`] — the shared input-event primitives both the
/// `pinion-runtime` router (producer) and the `pinion-core` /
/// `pinion-widget-paint` widget catalog (consumers) name without
/// inverting the crate graph.
///
/// Scope boundary: the per-widget SCE-emitted event enums
/// (`ButtonEvent`, `CheckboxEvent`, …) carry the *same* five pointer
/// names but derive them from the variant ident string via the SCE-002
/// [`WidgetEventName`](crate::WidgetEventName) derive — a self-consistent,
/// SCXML-canonical vocabulary owned by each statechart, a *different*
/// decision (wire name → SCXML transition) that this enum does not fold.
/// The two vocabularies are pinned together by a cross-vocab test in
/// `widgets::button` so a rename on either side is caught at test time.
/// The keyboard-side `"KeyboardActivate"` token is a separate wire
/// vocabulary (not a pointer event) and is left to its callers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum PointerWireEvent {
    /// `"PointerEnter"` — cursor entered the target (hover begins).
    Enter,
    /// `"PointerDown"` — primary button pressed over the target.
    Down,
    /// `"PointerUp"` — primary button released (the activate edge).
    Up,
    /// `"PointerLeave"` — cursor left the target (hover ends, or a
    /// mid-press stray under capture).
    Leave,
    /// `"PointerCancel"` — the pointer interaction was aborted.
    Cancel,
}

impl PointerWireEvent {
    /// Every pointer wire event, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; Self::ARMS] =
        [Self::Enter, Self::Down, Self::Up, Self::Leave, Self::Cancel];

    /// R1643 — the closed vocabulary an argument's `ArgDomain::OneOf` points at,
    /// projected from [`ALL`](Self::ALL) through
    /// [`as_wire_name`](Self::as_wire_name) rather than written out, so the
    /// published set and the set [`from_wire_name`](Self::from_wire_name) admits
    /// cannot disagree — and `ALL`'s length is the `VariantCensus` arm count, so
    /// a sixth event is a build failure here instead of a vocabulary quietly one
    /// short on the wire. R1630's ratchet; see
    /// `ArrangePass::WIRE_NAMES` for the same shape.
    ///
    /// It exists because `SplitterExternal`'s `send` accepts exactly these five
    /// and had no way to say so: the surface declared the name on the READ
    /// channel and published no vocabulary at all, which the widened catalog
    /// walk found on its first run (R1643).
    pub const WIRE_NAMES: [&'static str; Self::ARMS] = {
        let mut out = [""; Self::ARMS];
        let mut i = 0;
        while i < Self::ARMS {
            out[i] = Self::ALL[i].as_wire_name_const();
            i += 1;
        }
        out
    };

    /// The `const` half of [`as_wire_name`](Self::as_wire_name), which
    /// [`WIRE_NAMES`](Self::WIRE_NAMES) is folded out of.
    ///
    /// Two spellings of one match because a `const` cannot call a non-`const`
    /// method and `as_wire_name` is public API with callers that do not need
    /// const-ness; `r1643_the_pointer_vocabulary_has_one_spelling` holds the two
    /// to each other over every arm, so the duplication cannot drift. The same
    /// shape R1639 recorded for `WidgetEventName`, where a derive macro emits
    /// the arms twice for the same reason.
    #[must_use]
    pub const fn as_wire_name_const(self) -> &'static str {
        match self {
            Self::Enter => "PointerEnter",
            Self::Down => "PointerDown",
            Self::Up => "PointerUp",
            Self::Leave => "PointerLeave",
            Self::Cancel => "PointerCancel",
        }
    }

    /// Encode `self` into its canonical W3C wire name — the single
    /// source the router emits. Inverse of [`from_wire_name`](Self::from_wire_name).
    #[must_use]
    pub fn as_wire_name(self) -> &'static str {
        match self {
            PointerWireEvent::Enter => "PointerEnter",
            PointerWireEvent::Down => "PointerDown",
            PointerWireEvent::Up => "PointerUp",
            PointerWireEvent::Leave => "PointerLeave",
            PointerWireEvent::Cancel => "PointerCancel",
        }
    }

    /// Decode a W3C pointer-event name into a [`PointerWireEvent`];
    /// `None` for any other name (the caller rejects the `send` payload
    /// or treats it as out-of-vocabulary). Inverse of
    /// [`as_wire_name`](Self::as_wire_name).
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "PointerEnter" => Some(PointerWireEvent::Enter),
            "PointerDown" => Some(PointerWireEvent::Down),
            "PointerUp" => Some(PointerWireEvent::Up),
            "PointerLeave" => Some(PointerWireEvent::Leave),
            "PointerCancel" => Some(PointerWireEvent::Cancel),
            _ => None,
        }
    }
}

/// R1416 §5.35 §5.15 — which mouse button a pointer edge belongs to, for the
/// **raw multi-button pointer stream** an [`External`](crate::external::External)
/// receives when it opts into
/// [`wants_raw_pointer_buttons`](crate::external::External::wants_raw_pointer_buttons).
///
/// The full W3C `PointerEvent` primary / auxiliary / secondary set, named (not
/// numbered) in the lower-case string-vocab convention the RPC transport shares.
/// The transport's `DragButton` (`left` / `middle`, the R881 gesture arc) and
/// `ClickButton` (`left` / `right`, the R887 click arc) are button SUBSETS that
/// each predate this stream and answer a *different* question (which gesture /
/// click arc to run); this is the one an `External` sees on the typed
/// [`RawPointerButton`] carrier. It lives in core — where the trait method that
/// takes it lives — rather than being folded into either transport subset, the
/// [[wire-vocab-canon-pin-not-fold]] discipline: a shared vocabulary is pinned
/// at the boundary, not merged across it.
///
/// Not `#[non_exhaustive]`: a closed three-button mouse set, matching the
/// [`PointerWireEvent`] / `DragButton` / `ClickButton` precedent (a future
/// `Back` / `Forward` is a deliberate spec expansion, not a silent wildcard).
// `Hash` (R1422): keyed in the router's per-(pointer, button) raw double-click
// tracker, so a left double-click and a right double-click count independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// The primary button (`"left"`) — focus / select / activate by default.
    Left,
    /// The auxiliary button (`"middle"`) — pan / PRIMARY paste by default.
    Middle,
    /// The secondary button (`"right"`) — the context-menu button by default.
    Right,
}

impl PointerButton {
    /// Canonical lower-case wire name. Inverse of [`from_wire_name`](Self::from_wire_name).
    #[must_use]
    pub fn as_wire_name(self) -> &'static str {
        match self {
            PointerButton::Left => "left",
            PointerButton::Middle => "middle",
            PointerButton::Right => "right",
        }
    }

    /// Decode a wire button name; `None` for anything outside the vocabulary so
    /// a typo surfaces at the call site. Inverse of
    /// [`as_wire_name`](Self::as_wire_name).
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "left" => Some(PointerButton::Left),
            "middle" => Some(PointerButton::Middle),
            "right" => Some(PointerButton::Right),
            _ => None,
        }
    }
}

/// R1431 §5.35 — the device that produced a pointer event, the W3C `PointerEvent.pointerType` / the
/// toolkit `pointerType()` peer. `Pen` and `Eraser` distinguish the two ends of a stylus (the DCC
/// "flip to erase" gesture, a toolkit distinction that W3C folds into `"pen"`);
/// `Mouse` and `Touch` are the non-tablet devices. Not `#[non_exhaustive]`: a closed set matching
/// [`PointerButton`]'s precedent.
///
/// The wire vocabulary is the W3C set plus `"eraser"`: `"mouse"` / `"pen"` /
/// `"eraser"` / `"touch"`. `Mouse` is the default — what a plain pointer reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PointerKind {
    /// A mouse or trackpad (`"mouse"`) — the default device.
    #[default]
    Mouse,
    /// A stylus tip (`"pen"`) — the W3C `"pen"` type.
    Pen,
    /// A stylus's ERASER end (`"eraser"`) — the toolkit `Eraser` pointer type, which W3C
    /// folds into `"pen"`; kept distinct so an eraser-aware surface flips to erase
    /// without a device query.
    Eraser,
    /// A finger / touch contact (`"touch"`).
    Touch,
}

impl PointerKind {
    /// Canonical lower-case wire name. Inverse of [`from_wire_name`](Self::from_wire_name).
    #[must_use]
    pub fn as_wire_name(self) -> &'static str {
        match self {
            PointerKind::Mouse => "mouse",
            PointerKind::Pen => "pen",
            PointerKind::Eraser => "eraser",
            PointerKind::Touch => "touch",
        }
    }

    /// Decode a wire pointer-type name; `None` for anything outside the
    /// vocabulary so a typo surfaces at the call site. Inverse of
    /// [`as_wire_name`](Self::as_wire_name).
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "mouse" => Some(PointerKind::Mouse),
            "pen" => Some(PointerKind::Pen),
            "eraser" => Some(PointerKind::Eraser),
            "touch" => Some(PointerKind::Touch),
            _ => None,
        }
    }
}

/// R1432 §5.35 — the lifecycle phase of a continuous native gesture (a
/// trackpad pinch / rotate), the winit `TouchPhase` / the toolkit native gesture event
/// `NativeGestureType` phase peer. A gesture is not a single event but an arc: it `Begin`s when
/// the fingers land, streams `Update`s as they move, and `End`s when they lift — or
/// `Cancel`s if the platform aborts the recognition. A magnification-aware surface
/// accumulates the per-`Update` delta between `Begin` and `End`, and discards the
/// accumulator on `Cancel` (the fingers lifted without committing), so it needs the
/// phase to bracket the interaction rather than treat each delta in isolation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GesturePhase {
    /// The gesture started — the fingers landed (`"begin"`). winit
    /// `TouchPhase::Started` / the toolkit `GestureStarted`.
    #[default]
    Begin,
    /// The gesture is updating — the fingers moved and a fresh delta arrived
    /// (`"update"`). winit `TouchPhase::Moved` / the toolkit `GestureUpdated`. This is the phase that carries the
    /// meaningful magnification / rotation change.
    Update,
    /// The gesture finished — the fingers lifted (`"end"`). winit
    /// `TouchPhase::Ended` / the toolkit `GestureFinished`.
    End,
    /// The gesture was cancelled — the platform aborted recognition without a
    /// clean finish (`"cancel"`). winit `TouchPhase::Cancelled` / the toolkit
    /// `GestureCanceled`. A surface accumulating a preview drops it rather
    /// than committing.
    Cancel,
}

impl GesturePhase {
    /// Canonical lower-case wire name. Inverse of [`from_wire_name`](Self::from_wire_name).
    #[must_use]
    pub fn as_wire_name(self) -> &'static str {
        match self {
            GesturePhase::Begin => "begin",
            GesturePhase::Update => "update",
            GesturePhase::End => "end",
            GesturePhase::Cancel => "cancel",
        }
    }

    /// Decode a wire gesture-phase name; `None` for anything outside the
    /// vocabulary so a typo surfaces at the call site. Inverse of
    /// [`as_wire_name`](Self::as_wire_name).
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "begin" => Some(GesturePhase::Begin),
            "update" => Some(GesturePhase::Update),
            "end" => Some(GesturePhase::End),
            "cancel" => Some(GesturePhase::Cancel),
            _ => None,
        }
    }
}

/// R1416 §5.35 — a raw pointer-button transition: the press or the release
/// edge. The winit `ElementState::Pressed` / `Released` mirror, named in core
/// so the [`External`](crate::external::External) trait and the RPC
/// `scene/pointer_button` decode share one vocabulary without a winit
/// dependency (`pinion-core` sits below the platform bridges).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEdge {
    /// The button was pressed (`"down"`).
    Down,
    /// The button was released (`"up"`).
    Up,
}

impl PointerEdge {
    /// Canonical lower-case wire name. Inverse of [`from_wire_name`](Self::from_wire_name).
    #[must_use]
    pub fn as_wire_name(self) -> &'static str {
        match self {
            PointerEdge::Down => "down",
            PointerEdge::Up => "up",
        }
    }

    /// Decode a wire edge name; `None` outside the vocabulary. Inverse of
    /// [`as_wire_name`](Self::as_wire_name).
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "down" => Some(PointerEdge::Down),
            "up" => Some(PointerEdge::Up),
            _ => None,
        }
    }

    /// `true` for the press edge ([`Down`](Self::Down)).
    #[must_use]
    pub fn is_down(self) -> bool {
        matches!(self, PointerEdge::Down)
    }
}

/// R1418 §5.35 §5.15 — the set of mouse buttons currently held, the pinion
/// peer of the toolkit `buttons()` and the DOM `MouseEvent.buttons` bitmask.
///
/// A **state**, not an edge. [`PointerButton`] + [`PointerEdge`] say *what just
/// happened*; this says *what is currently down*. Following the DOM / the
/// toolkit convention, the set reflects the state **after** the transition: a
/// press INCLUDES the pressed button, a release EXCLUDES the released one.
///
/// Carried on [`RawPointerButton`] so a raw sink reads WHICH buttons are down at each edge,
/// not only the single [`button`](RawPointerButton::button) that just changed — a
/// chord (press left, then right) reports `{left, right}` on the right-down edge, and the
/// state an xterm SGR motion report or a toolkit drag-with-buttons gesture
/// needs.
///
/// **R1619 — it is no longer only the raw channel's.** The router keeps one
/// per-pointer set and stamps it onto *every* dispatched pointer event, so a
/// widget that never opted into the raw stream still learns that a
/// [`PointerEnter`](PointerWireEvent::Enter) arrived with the primary button
/// held — which is the inner step of every drag-select and, before R1619, was
/// byte-identical on the wire to a plain hover. The set travels as the send
/// payload's fourth segment ([`as_wire_token`](Self::as_wire_token)) and is
/// published for reading on `scene/input_state`
/// ([`InputStateSnapshot::held_pointer_buttons`]).
///
/// Against the reference: the toolkit carries the held set on its
/// **single-point event base**, so its mouse, hover and **enter** events all
/// answer it — but its *leave* is not a pointing event at all. That handler
/// takes the framework's plain BASE event type, which has no position, no
/// modifiers and no buttons, so "did the pointer leave me mid-drag?" can only
/// be answered there by consulting global state at an unrelated moment. Here
/// [`Leave`](PointerWireEvent::Leave) is stamped like every other arm.
/// (Read from the reference source; the exact class and handler names are in
/// the round's memory note rather than here, so this file cites the capability
/// and not another project's namespace.)
///
/// A `u8` bitmask over the three [`PointerButton`]s (no external `bitflags`
/// dependency, and `unsafe_code` is forbidden workspace-wide), exposed through
/// set operations so callers never touch the raw bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PointerButtons(u8);

impl PointerButtons {
    const fn bit(button: PointerButton) -> u8 {
        match button {
            PointerButton::Left => 1 << 0,
            PointerButton::Middle => 1 << 1,
            PointerButton::Right => 1 << 2,
        }
    }

    /// No buttons held.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// `true` when no button is held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// `true` when `button` is in the held set.
    #[must_use]
    pub const fn contains(self, button: PointerButton) -> bool {
        self.0 & Self::bit(button) != 0
    }

    /// The set with `button` added (a press).
    #[must_use]
    pub const fn with(self, button: PointerButton) -> Self {
        Self(self.0 | Self::bit(button))
    }

    /// The set with `button` removed (a release).
    #[must_use]
    pub const fn without(self, button: PointerButton) -> Self {
        Self(self.0 & !Self::bit(button))
    }

    /// The compact wire token for the held set — the `lmr` letters in canonical
    /// order (`{left, right}` → `"lr"`), the [`Modifiers::as_wire_token`] pattern
    /// applied to the button axis. Empty set yields `""`. Used by an
    /// introspection surface so an AI client reads the held buttons as data.
    /// Inverse of [`from_wire_token`](Self::from_wire_token).
    #[must_use]
    pub fn as_wire_token(self) -> String {
        let mut token = String::new();
        if self.contains(PointerButton::Left) {
            token.push('l');
        }
        if self.contains(PointerButton::Middle) {
            token.push('m');
        }
        if self.contains(PointerButton::Right) {
            token.push('r');
        }
        token
    }

    /// R1619 §5.35 §5.16 — enumerate the held buttons by their canonical wire
    /// spelling ([`PointerButton::as_wire_name`]) in the closed set's
    /// declaration order.
    ///
    /// One home for the enumeration, so the `scene/input_state` READ peer
    /// serializes exactly the set the button-edge WRITES built — the
    /// [`HeldKeys::held_names`] discipline, where the read is the inverse of
    /// the writes by construction rather than by a second spelling. Distinct
    /// from [`as_wire_token`](Self::as_wire_token), which packs the same set
    /// into ONE payload segment: a JSON reader wants names, a `:`-separated
    /// grammar with no room for a separator wants letters.
    #[must_use]
    pub fn held_names(self) -> Vec<&'static str> {
        [
            PointerButton::Left,
            PointerButton::Middle,
            PointerButton::Right,
        ]
        .into_iter()
        .filter(|&b| self.contains(b))
        .map(PointerButton::as_wire_name)
        .collect()
    }

    /// Decode a held-set wire token (any order of the `lmr` letters) back into
    /// [`PointerButtons`]; `None` on any letter outside `lmr` so a malformed
    /// token is rejected rather than silently dropped. Inverse of
    /// [`as_wire_token`](Self::as_wire_token) — the R773 encode↔decode SSOT
    /// discipline applied to the held-button axis.
    #[must_use]
    pub fn from_wire_token(token: &str) -> Option<Self> {
        let mut set = Self::empty();
        for ch in token.chars() {
            set = match ch {
                'l' => set.with(PointerButton::Left),
                'm' => set.with(PointerButton::Middle),
                'r' => set.with(PointerButton::Right),
                _ => return None,
            };
        }
        Some(set)
    }
}

/// R1416 §5.35 §5.15 — one raw mouse-button edge delivered to an
/// [`External`](crate::external::External) that owns the multi-button pointer
/// stream (opts in via
/// [`wants_raw_pointer_buttons`](crate::external::External::wants_raw_pointer_buttons)).
/// Carries the `button`, the press/release `edge`, and the `modifiers` held AT
/// THAT EDGE.
///
/// **Position is deliberately absent.** The widget correlates the edge with the
/// cursor position it already tracks through
/// [`pointer_move`](crate::external::External::pointer_move) — a raw sink also
/// sets [`wants_hover_move`](crate::external::External::wants_hover_move) (or
/// [`wants_pointer_capture`](crate::external::External::wants_pointer_capture)) —
/// exactly as a pre-R1416 consumer paired the `send`-wire `PointerDown` with the
/// last forwarded move. Splitting position out keeps this carrier the pure
/// *button* channel; the move channel already exists.
///
/// **Both edges carry modifiers.** This closes the press-edge-drops-modifiers
/// gap the legacy `PointerDown` send wire has (that path routed the press
/// through the zero-modifier `dispatch_send`, only the release through
/// `dispatch_send_mods`), so a raw sink reads a consistent modifier state on
/// down and up — the shape a terminal mouse report or a marquee gesture needs.
///
/// **[`buttons`](Self::buttons) carries the full held set** (R1418), the toolkit `buttons()`
/// peer, so a chord or a motion-with-buttons is expressible: `button` names the ONE
/// that changed, `buttons` names ALL held after the change.
///
/// **[`click_count`](Self::click_count) carries the consecutive-click ordinal** (R1422),
/// the toolkit `MouseButtonDblClick` / DOM `MouseEvent.detail` peer: the router synthesises `2` on a press that
/// repeats the same button on the same spot within the double-click window,
/// and echoes that count onto the matching release, so a raw sink reads a
/// double-click without re-implementing the timing itself. See
/// [`click_count`](Self::click_count) for the exact rule.
///
/// Not `#[non_exhaustive]`: the router (pinion-runtime) constructs it with a
/// struct literal across the crate boundary, the
/// [`DragUpdate`](crate::external::DragUpdate) / [`DropPoint`](crate::external::DropPoint)
/// cross-crate-carrier precedent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawPointerButton {
    /// Which mouse button transitioned (the toolkit `button()` peer).
    pub button: PointerButton,
    /// Whether the button was pressed or released.
    pub edge: PointerEdge,
    /// The keyboard modifiers held at this edge.
    pub modifiers: Modifiers,
    /// The full set of buttons held AFTER this edge (the toolkit
    /// `buttons()` / DOM `MouseEvent.buttons` peer): a press
    /// includes the pressed button, a release excludes the released one.
    pub buttons: PointerButtons,
    /// R1422 §5.35 — the consecutive-click ordinal of `button` at this edge, the
    /// toolkit `MouseButtonDblClick` / DOM `MouseEvent.detail` peer. `1` on a first press (the toolkit `MouseButtonPress`); `2`
    /// on a second press of the SAME button, at the same spot (within the
    /// framework double-click time + distance window shared with the `DoubleClick`
    /// send-wire path so the two rules cannot drift), i.e. the toolkit `MouseButtonDblClick`. It
    /// caps at `2` — pinion stops at binary single/double, matching the
    /// send-wire `DoubleClick` (no rolling triple-click). A release ([`PointerEdge::Up`]) echoes the
    /// count of the press it releases, so a press/release pair reads one
    /// consistent ordinal (the DOM `detail` model, which the toolkit drops on
    /// release). A lone release with no matching tracked press reports `1`.
    pub click_count: u8,
}

/// The keyboard-side activation token: the `send`-payload event name a focused
/// command widget receives on keyboard activation (Enter / Space), the
/// keyboard peer of the pointer-release activation edge ([`PointerWireEvent::Up`]).
/// One home for the literal on the **decode** side (the `*Event::KeyboardActivate`
/// SCE enums on the *emit* side own their own `stringify!` form — a separate,
/// statechart-bound vocabulary, per [`PointerWireEvent`]'s scope note).
pub const KEYBOARD_ACTIVATE_EVENT: &str = "KeyboardActivate";

/// R778 §5.35 — does this `send`-payload event name denote a command widget's
/// **activation edge**? True for the pointer release ([`PointerWireEvent::Up`])
/// and the keyboard activation token ([`KEYBOARD_ACTIVATE_EVENT`]).
///
/// The shared decode predicate for the command-widget `handle_send` decoders
/// that have no per-item SCE statechart — [`VirtualSelectExternal`](crate::widgets::virtual_select),
/// [`ViewSortFilterExternal`](crate::widgets::view_order), and
/// [`GridSortExternal`](crate::widgets::grid_sort) — lifted on the third
/// consumer (R778) so the set of events that count as "activate" cannot drift
/// between them (a divergence would be a routing bug, not a style choice). The
/// per-widget statecharts decode their own activation through the SCE-002
/// [`WidgetEventName`](crate::WidgetEventName) derive + `detect`, a different
/// vocabulary this predicate does not fold.
#[must_use]
pub fn is_activation_event(event_name: &str) -> bool {
    event_name == KEYBOARD_ACTIVATE_EVENT || event_name == PointerWireEvent::Up.as_wire_name()
}

/// The press-time snapshot a [`DragCalibration`] holds between the first
/// captured move and the release: the per-consumer `payload`, the cursor's
/// `press_x_rel` anchor fraction across the basis rect, and the largest cursor
/// fraction the drag has strayed from the press so far (the click-vs-drag
/// discriminator — see [`DragCalibration::traveled_beyond`]).
#[derive(Clone, Copy, Debug)]
struct DragAnchor<T: Copy> {
    payload: T,
    press_x_rel: f64,
    max_abs_delta: f64,
}

/// R914 §5.27 §5.35 — the capture-drag **calibration** substrate: the
/// "first captured move calibrates, every later move yields travel" idiom
/// shared by the standalone 1-D drag-calibration coordinators — the column
/// resize ([`ColumnResizeExternal`](crate::widgets::column_widths), R786), the
/// property-grid numeric scrub (R875), the data-grid cell scrub (R914), and the
/// splitter ratio drag (`SplitterExternal` in `pinion-widget-paint`, R683).
/// These all own the drag micro-lifecycle entirely in `pointer_move`
/// (calibrate → apply) + a `PointerUp` teardown.
///
/// Two adjacent isomorphic families are deliberately *not* routed here, for
/// shape reasons rather than effort:
/// * the **scroll bar** ([`ScrollBarExternal`](crate::widgets::scrollbar), R660)
///   is 1-D isomorphic, but its calibration snapshot is gated by + embedded in
///   its `{Idle, Hover, Dragging, Disabled}` SCXML gesture statechart (the
///   `Dragging` state owns the snapshot's lifecycle; `scroll_max` is pinned at
///   press for mid-drag re-measure stability), so routing it would couple a
///   separate primitive's lifetime to the statechart transitions — its
///   `DragStart` stays statechart-local;
/// * the **dock tear-off** (`dock.rs` in `pinion-widget-paint`) and the
///   **node-graph node drag** (hello-node-editor) are *2-D* (they snapshot
///   an `(x, y)` grab offset and apply a 2-D delta), a genuinely different
///   shape than this 1-D primitive — a future `DragCalibration2D` (two
///   consumers exist, so its own justified lift, not forced into this one).
///
/// A capture-lock pointer drag
/// ([`wants_pointer_capture`](crate::external::External::wants_pointer_capture))
/// delivers a stream of
/// [`pointer_move(x_rel, _)`](crate::external::External::pointer_move) where
/// `x_rel` is the cursor's fraction across a *stable-width* basis rect
/// ([`capture_normalize`](crate::external::External::capture_normalize)). The
/// drag is anchored on the **press**, not on the first move, so:
///
/// * the **first** move after a press is *calibration only* — it snapshots the
///   payload (the dragged thing's value at press) and the cursor's anchor
///   fraction, and mutates nothing (the user has not dragged yet, exactly the
///   column-resize first-move rule);
/// * every **later** move yields the cursor's fraction delta since the press
///   (`x_rel − press_x_rel`); the caller scales it by its basis width to recover
///   true pixel travel and applies it on top of the snapshotted base, so an
///   intermediate clamp un-clamps cleanly when the cursor returns.
///
/// `T` is the per-consumer payload that genuinely diverges — a column's
/// `width_at_press: u32` for the resize, a `(source, kind, base)` triple for a
/// scrub — and the basis width and the value application stay with the caller
/// (a const grid width here, a measured viewport there; whole int units vs a
/// continuous float). What is *invariant*, and so lifted here so it cannot
/// drift between the three, is the press-anchored calibration and the "did a
/// drag actually run?" teardown signal that lets a release suppress the
/// trailing click.
///
/// `Copy` payload so the snapshot lives in a [`Cell`] (the `ResizeDragStart` /
/// `ScrubDrag` idiom this generalises).
pub struct DragCalibration<T: Copy> {
    anchor: Cell<Option<DragAnchor<T>>>,
}

impl<T: Copy> DragCalibration<T> {
    /// An idle calibration — no drag in flight.
    #[must_use]
    pub fn new() -> Self {
        Self {
            anchor: Cell::new(None),
        }
    }

    /// Drive one captured `pointer_move` at cursor fraction `x_rel`.
    ///
    /// On the **first** move the calibration is empty: `seed()` snapshots the
    /// payload (returning [`None`] *declines* the drag — a press that did not
    /// land on a draggable target, e.g. a non-numeric scrub row), and the move
    /// returns [`None`] (calibration only, no mutation). Every **later** move
    /// returns `Some((payload, delta))` where `delta = x_rel − press_x_rel`;
    /// multiply `delta` by the basis width for the cursor's pixel travel since
    /// the press.
    pub fn drive(&self, x_rel: f64, seed: impl FnOnce() -> Option<T>) -> Option<(T, f64)> {
        match self.anchor.get() {
            None => {
                if let Some(payload) = seed() {
                    self.anchor.set(Some(DragAnchor {
                        payload,
                        press_x_rel: x_rel,
                        max_abs_delta: 0.0,
                    }));
                }
                None
            }
            Some(mut anchor) => {
                let delta = x_rel - anchor.press_x_rel;
                anchor.max_abs_delta = anchor.max_abs_delta.max(delta.abs());
                self.anchor.set(Some(anchor));
                Some((anchor.payload, delta))
            }
        }
    }

    /// Has the drag strayed far enough from the press to be a **drag**, not a
    /// **click**? `true` once the cursor's largest pixel travel from the press
    /// (`max fraction-delta · basis`) reaches `threshold_px` — pass [`DRAG_CLICK_THRESHOLD_PX`] for the framework's click-vs-drag SSOT
    /// (the toolkit `startDragDistance`, the DOM no-`click`-after-drag rule). `false` while idle or
    /// still within the dead zone.
    ///
    /// The opt-in discriminator for calibration consumers that ALSO have a
    /// click action on the same press (a scrub cell that a plain click should
    /// instead *focus*): gate both the live mutation and the trailing-click
    /// suppression on this so a sub-threshold press stays a click. Consumers
    /// with no click action (column resize, splitter) ignore it.
    ///
    /// R1346 — "ignore it" is about the *click-vs-drag* question only, and it
    /// needs a **pixel basis** the caller must supply. A consumer whose
    /// `pointer_move` sees normalised fractions and never learns its pixel
    /// width (the splitter: its basis is implicitly `1.0`) cannot call this
    /// meaningfully. Such a consumer answering "did this gesture settle on a
    /// new value?" for a drag-end commit channel should instead compare the
    /// released value against the press snapshot [`Self::end_payload`] hands
    /// back — see that method. Do NOT reach for [`Self::end`]'s bool for that
    /// question: under the real router it is `true` for a bare click.
    #[must_use]
    pub fn traveled_beyond(&self, basis: f64, threshold_px: f64) -> bool {
        self.anchor
            .get()
            .is_some_and(|a| a.max_abs_delta * basis >= threshold_px)
    }

    /// Tear the drag down at release (`PointerUp` / `PointerCancel`). Returns
    /// whether a drag had **calibrated** — so the caller can suppress the
    /// trailing click (a scrub must not also open the inline editor or move
    /// the cursor as a plain click would).
    ///
    /// ## What `true` does and does not mean (R1346)
    ///
    /// `true` means *an anchor existed*, i.e. at least one [`Self::drive`]
    /// arrived and its `seed` accepted — **not** that the cursor travelled or
    /// that any value changed. The distinction is load-bearing for capture
    /// widgets: on a press over a widget that opts into
    /// `wants_pointer_capture()`, `InputRouter::pointer_down` forwards the
    /// press-time cursor to *that captured widget* as an initial `pointer_move`
    /// (R51.35 click-to-position, pinned by `pinion-runtime`
    /// `input.rs::pointer_down_forwards_initial_cursor`), so under the real
    /// router a **bare click arms the anchor** and this returns `true` with
    /// zero travel. Only a consumer the router never forwards a press-time move
    /// to sees `false` here for a click.
    ///
    /// So: `true` is the right gate for *trailing-click suppression* (its
    /// original purpose — a calibration ran, so the press was pointer-owned).
    /// It is the **wrong** gate for a *drag-end commit* ("persist the settled
    /// value"), which must additionally establish that something moved — via
    /// [`Self::traveled_beyond`] where a pixel basis exists, or by comparing
    /// the released value against the press snapshot [`Self::end_payload`]
    /// returns.
    pub fn end(&self) -> bool {
        self.end_payload().is_some()
    }

    /// R1346 — tear the drag down and return the **press-time payload** the
    /// calibration snapshotted (`None` when no drag had calibrated).
    ///
    /// The [`Self::end`] peer that keeps what `end` discards. A drag-end
    /// *commit* channel needs the press snapshot to answer "did this gesture
    /// actually settle on a new value, or did the user just click?" — compare
    /// the payload against the released value and stay silent when they agree.
    /// `SplitterExternal`'s `"ratio_committed"` is the first consumer: its
    /// payload is the `ratio_at_press`, so a click (which calibrates but
    /// mutates nothing) compares equal and correctly emits nothing.
    ///
    /// Value-typed like the rest of the primitive — `T: Copy` — so the caller
    /// owns the snapshot after the teardown.
    pub fn end_payload(&self) -> Option<T> {
        self.anchor.take().map(|anchor| anchor.payload)
    }

    /// Whether a drag is live (calibrated and not yet released) — the AI-first
    /// `dragging` / `scrubbing` query slot and a future drag-highlight surface.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.anchor.get().is_some()
    }
}

impl<T: Copy> Default for DragCalibration<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy> core::fmt::Debug for DragCalibration<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DragCalibration")
            .field("is_active", &self.is_active())
            .finish_non_exhaustive()
    }
}

/// R764.1 §5.38 §5.22 — forward a W3C `KeyboardEvent.key` to the
/// `TextField`-class External tagged `tag`, the SSOT every `TextField`
/// binding's `WidgetCore::apply_key` routes a recognised key through.
/// Pre-R764.1 this `find_external_with_tag_mut` then `introspect_mut`
/// then `invoke("key", …)` then match-`Bool` block was hand-rolled in 5
/// sites across 4 bindings (hello-textfield, hello-textarea, todomvc
/// `TF_TAG` and `EDIT_TF_TAG`, hello-combobox-editable `INPUT_TAG`).
///
/// R804 relocated this from `pinion-widget-paint::text_field` (its R764.1
/// birthplace) into `pinion-core::input`: the body is pure `Scene` /
/// `External` introspection with zero paint, so its GUI-crate home forced
/// the TUI binding (which cannot depend on the vello paint crate) to keep
/// a third hand-rolled copy. One core home lets every backend share it.
///
/// Empty `modifiers` sends the bare [`IntrospectValue::Text`] wire shape
/// (the R56.1.d single-keystroke path); any held modifier sends the
/// R56.1.f.0 Json shape carrying the four W3C bits so `Shift+Arrow` /
/// `Ctrl+A` reach the substrate's modifier-aware selection arms.
///
/// Returns the External's recognition result (the W3C `defaultPrevented`
/// semantic the binding propagates from `apply_key`): `true` only on
/// `Ok(Bool(true))`, `false` on an unrecognised key or any non-`Bool`
/// shape (a substrate misconfiguration defers to the shell fallback
/// chain rather than silently swallowing the key). The binding keeps its
/// own `focused == Some(<my tag>)` roving-tabindex guard before calling.
#[must_use]
pub fn forward_key_to_field(scene: &mut Scene, tag: &str, key: &str, modifiers: Modifiers) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(tag) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let args = if modifiers == Modifiers::empty() {
        IntrospectValue::Text(key.to_owned())
    } else {
        IntrospectValue::Json(serde_json::json!({
            "key": key,
            "shift": modifiers.shift_key(),
            "ctrl": modifiers.control_key(),
            "alt": modifiers.alt_key(),
            "meta": modifiers.meta_key(),
        }))
    };
    matches!(intro.invoke("key", args), Ok(IntrospectValue::Bool(true)))
}

/// R878 §5.38 §5.22 — the shared **typed inline-editor keymap** over a
/// `TextField`-class External: the key dispatch a binding runs while its
/// one shared inline editor owns focus. Pre-R878 this match was hand-rolled
/// byte-identically in `hello-data-grid` (R837) and `hello-property-grid`
/// (R836/R875); the node-rename editor (R878) is the third consumer, so the
/// decision the block encodes is lifted here once:
///
/// * `Enter` runs the binding's `commit` policy, `Escape` its `cancel`
///   policy (both are per-binding closures — *what* a commit writes and
///   where focus returns stay binding decisions; W3C `aria-grid` /
///   styled item delegate edit-mode convention).
/// * The caret / deletion keys (`ArrowLeft` / `ArrowRight` / `Home` /
///   `End` / `Backspace` / `Delete`) always reach the field via
///   [`forward_key_to_field`] — editing motion never depends on the cell
///   type.
/// * Any other key passes the [`CellKind::accepts_keystroke`] gate first,
///   so an int / float editor rejects letters at the keystroke edge (the
///   R836 typed-editor contract) while a text editor accepts every
///   printable. Rejected keys return `false` (defer to the shell fallback
///   chain) — with the editor focused no sibling keymap consumes them, so
///   a stray `ArrowUp` is inert rather than smuggled into the field.
///
/// The forward-all editors (todomvc `EDIT_TF_TAG`, `hello-file-manager`
/// `RENAME_TF_TAG`) intentionally do NOT route here: their policy forwards
/// *every* non-Enter/Escape key to the field (no whitelist, no kind gate),
/// which is a different decision, not a missed consumer of this one.
pub fn edit_field_keymap(
    scene: &mut Scene,
    tag: &str,
    key: &str,
    modifiers: Modifiers,
    kind: CellKind,
    commit: impl FnOnce(),
    cancel: impl FnOnce(),
) -> bool {
    match key {
        "Enter" => {
            commit();
            true
        }
        "Escape" => {
            cancel();
            true
        }
        "ArrowLeft" | "ArrowRight" | "Home" | "End" | "Backspace" | "Delete" => {
            forward_key_to_field(scene, tag, key, modifiers)
        }
        other => {
            // R879 audit fix — a Ctrl/Meta chord is a *command* (select-all,
            // clipboard), not text input: it bypasses the per-kind keystroke
            // gate so `Ctrl+A` / `Ctrl+C` reach the field's modifier-aware
            // arms even in an int / float editor. The pre-lift data-grid /
            // property-grid copies gated chords out (a latent defect the
            // R878 lift had faithfully preserved).
            let is_command_chord = modifiers.command_key();
            if is_command_chord || kind.accepts_keystroke(other) {
                forward_key_to_field(scene, tag, other, modifiers)
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! R56.1.f.0 §5.13 — `Modifiers` regression battery. Covers the
    //! W3C `KeyboardEvent` accessor surface, `Default == empty()`
    //! identity, and the `is_empty` predicate used by `apply_key`
    //! plain-keystroke branches.

    use super::{
        AutoRepeat, DragLatch, HeldKeys, KEYBOARD_ACTIVATE_EVENT, Modifiers, PointerWireEvent,
        is_activation_event,
    };

    // ─────────────────────────────────────────────────────────────
    // R1549 §5.35 §5.38 — `AutoRepeat` cadence battery.
    // ─────────────────────────────────────────────────────────────

    /// The declared defaults ARE the toolkit's `qabstractbutton.cpp` constants; a silent drift
    /// here would silently change every held button in the catalogue, so the
    /// pair is pinned rather than merely documented.
    #[test]
    fn desktop_defaults_are_the_qt_pair() {
        let r = AutoRepeat::desktop();
        assert!((r.delay_secs() - 0.300).abs() < 1e-6, "AUTO_REPEAT_DELAY");
        assert!(
            (r.interval_secs() - 0.100).abs() < 1e-6,
            "AUTO_REPEAT_INTERVAL",
        );
        assert!(
            (r.accel() - 1.0).abs() < 1e-6,
            "a plain the toolkit button does not accelerate",
        );
    }

    /// Without acceleration every interval is the same one, forever — the
    /// `powi` ramp must not drift the fixed cadence.
    #[test]
    fn fixed_cadence_never_ramps() {
        let r = AutoRepeat::new(0.5, 0.25);
        for n in [0_u32, 1, 7, 100, 10_000] {
            assert!(
                (r.interval_after(n) - 0.25).abs() < 1e-6,
                "fire {n} keeps the declared interval",
            );
        }
    }

    /// An accelerating cadence shortens monotonically and stops at its
    /// declared floor — the peer of `setAccelerated`,
    /// which the toolkit offers only as an on/off with no reachable curve.
    #[test]
    fn accelerating_cadence_ramps_down_to_its_floor() {
        let r = AutoRepeat::new(0.3, 0.100).accelerating(0.5, 0.020);
        assert!((r.interval_after(0) - 0.100).abs() < 1e-6);
        assert!((r.interval_after(1) - 0.050).abs() < 1e-6);
        assert!((r.interval_after(2) - 0.025).abs() < 1e-6);
        assert!(
            (r.interval_after(3) - 0.020).abs() < 1e-6,
            "0.0125 would undercut the floor, so the floor answers",
        );
        assert!(
            (r.interval_after(u32::MAX) - 0.020).abs() < 1e-6,
            "and it still answers once `powi` has saturated to zero",
        );
    }

    /// Every interval any declaration can produce stays at or above the
    /// floor. This is what bounds the router's per-frame catch-up loop:
    /// were it reachable-zero, one large `scene/tick` would spin forever.
    #[test]
    fn no_declaration_can_produce_a_zero_interval() {
        for (interval, accel, floor) in [
            (0.0_f32, 1.0_f32, 0.0_f32),
            (-5.0, 1.0, -5.0),
            (f32::NAN, 1.0, f32::NAN),
            (f32::INFINITY, f32::NAN, 0.0),
            (0.1, 0.0, 0.0),
            (0.1, -1.0, 0.0),
        ] {
            let r = AutoRepeat::new(0.1, interval).accelerating(accel, floor);
            for n in [0_u32, 1, 50] {
                let i = r.interval_after(n);
                assert!(
                    i >= AutoRepeat::MIN_INTERVAL_FLOOR_SECS,
                    "interval_after({n}) = {i} for ({interval}, {accel}, {floor})",
                );
            }
        }
    }

    /// `accel > 1.0` would decelerate without bound. It is refused (pinned
    /// to `1.0`) rather than admitted, because a slower cadence is already
    /// spellable as a longer `interval_secs` — admitting both would be two
    /// spellings of one cadence, and the published `accel` would stop
    /// meaning "at or faster than `interval_secs`".
    #[test]
    fn deceleration_is_not_a_second_spelling_of_a_slow_cadence() {
        let r = AutoRepeat::new(0.3, 0.1).accelerating(4.0, 0.1);
        assert!((r.accel() - 1.0).abs() < 1e-6);
        assert!((r.interval_after(3) - 0.1).abs() < 1e-6, "no growth");
    }

    /// A malformed delay saturates at `0.0` rather than poisoning the
    /// schedule — the first repeat then lands immediately, which is a
    /// legible cadence, where a `NaN` threshold would compare false
    /// forever and silently disable the repeat.
    #[test]
    fn malformed_delay_saturates_instead_of_disabling() {
        assert!((AutoRepeat::new(-1.0, 0.1).delay_secs() - 0.0).abs() < 1e-6);
        assert!((AutoRepeat::new(f32::NAN, 0.1).delay_secs() - 0.0).abs() < 1e-6);
        assert!(AutoRepeat::new(f32::NAN, 0.1).delay_secs().is_finite());
    }

    /// The published schedule is the schedule the router walks: the first
    /// repeat lands at the delay, and each later one adds the interval
    /// that followed its predecessor.
    #[test]
    fn published_schedule_matches_the_stepped_one() {
        let r = AutoRepeat::new(0.3, 0.100).accelerating(0.5, 0.020);
        assert!((r.elapsed_at_fire(1) - 0.300).abs() < 1e-6);
        assert!((r.elapsed_at_fire(2) - 0.400).abs() < 1e-6);
        assert!((r.elapsed_at_fire(3) - 0.450).abs() < 1e-6);
        // Replay the router's own stepping and land on the same instant.
        let mut t = r.delay_secs();
        for k in 0..2 {
            t += r.interval_after(k);
        }
        assert!((t - r.elapsed_at_fire(3)).abs() < 1e-6);
    }

    /// A floor above the declared interval is clamped down to it, so
    /// `min_interval_secs <= interval_secs` always holds and the ramp is
    /// never inverted (a floor ABOVE the start would make fire 0 the
    /// fastest and every later one slower — deceleration by the back door).
    #[test]
    fn floor_cannot_exceed_the_interval_it_floors() {
        let r = AutoRepeat::new(0.3, 0.05).accelerating(0.5, 0.5);
        assert!(r.min_interval_secs() <= r.interval_secs());
        assert!((r.interval_after(0) - 0.05).abs() < 1e-6);
    }

    #[test]
    fn r885_held_names_enumerates_canonical_spellings() {
        // The `scene/input_state` READ enumeration: empty when idle,
        // the canonical *named* spelling ("Space") even when the
        // chord was armed via the W3C `" "` character — the read is
        // the inverse of the write vocabulary, not an echo of it.
        let mut held = HeldKeys::default();
        assert!(held.held_names().is_empty());
        held.note(" ", true);
        assert_eq!(held.held_names(), vec!["Space"]);
        held.note("Space", false);
        assert!(held.held_names().is_empty());
    }

    #[test]
    fn r882_held_keys_tracks_the_space_chord_only() {
        // The chord-vocabulary SSOT: `Space` (the pinion named-key
        // spelling) and `" "` (the strict W3C `KeyboardEvent.key`
        // value) BOTH arm the pan chord — the dual-spelling tolerance
        // the keystroke decoders already apply; any other key is
        // ignored, auto-repeat is idempotent, and `clear` (the
        // window-blur arm) forgets everything.
        let mut held = HeldKeys::default();
        assert!(!held.space());
        held.note("a", true);
        held.note("Enter", true);
        assert!(!held.space(), "non-space keys never arm the chord");
        held.note(" ", true);
        assert!(held.space(), "the W3C \" \" spelling arms the chord too");
        held.note(" ", false);
        assert!(!held.space());
        held.note("Space", true);
        assert!(held.space());
        held.note("Space", true);
        assert!(held.space(), "auto-repeat is idempotent");
        held.note("Space", false);
        assert!(!held.space());
        held.note("Space", true);
        held.clear();
        assert!(!held.space(), "clear() forgets the held chord");
    }

    #[test]
    fn r880_drag_latch_is_sticky_past_threshold() {
        // The contract predicate over DRAG_CLICK_THRESHOLD_PX (4 logical
        // px, Euclidean, strictly greater): a wobble inside the dead zone
        // stays a click; once past, the gesture is a drag for its lifetime
        // — even back at the origin (the toolkit startDragDistance latch).
        let mut latch = DragLatch::new((10.0, 10.0));
        assert!(
            !latch.advance((12.0, 10.0)),
            "2px wobble: inside the dead zone"
        );
        assert!(
            !latch.advance((10.0, 14.0)),
            "exactly 4px: not yet a drag (strict)"
        );
        assert!(!latch.live());
        assert!(latch.advance((15.0, 10.0)), "5px: past the threshold");
        assert!(
            latch.advance((10.0, 10.0)),
            "returning to the origin stays a drag"
        );
        assert!(latch.live());
    }

    #[test]
    fn r880_1_selection_chord_and_command_key_decode() {
        use super::SelectionChord;
        let m = |shift, ctrl, meta| Modifiers {
            shift,
            ctrl,
            alt: false,
            meta,
        };
        assert_eq!(
            SelectionChord::from_modifiers(m(false, false, false)),
            SelectionChord::Replace
        );
        assert_eq!(
            SelectionChord::from_modifiers(m(false, true, false)),
            SelectionChord::Toggle
        );
        assert_eq!(
            SelectionChord::from_modifiers(m(true, false, false)),
            SelectionChord::Extend
        );
        // The command chord wins over Shift — ONE precedence for every
        // selection consumer (list, grid, node canvas, marquee).
        assert_eq!(
            SelectionChord::from_modifiers(m(true, true, false)),
            SelectionChord::Toggle
        );
        // Cmd (meta) is a command chord too — the macOS convention the
        // command_key predicate encodes (R879.1-ratified ctrl-or-meta).
        assert_eq!(
            SelectionChord::from_modifiers(m(false, false, true)),
            SelectionChord::Toggle
        );
        assert!(m(false, false, true).command_key());
        assert!(m(false, true, false).command_key());
        assert!(!m(true, false, false).command_key());
    }

    #[test]
    fn r902_1_multiselect_key_op_classifies_the_non_nav_chords() {
        use super::MultiSelectKeyOp;
        let cmd = Modifiers {
            shift: false,
            ctrl: true,
            alt: false,
            meta: false,
        };
        let meta = Modifiers {
            shift: false,
            ctrl: false,
            alt: false,
            meta: true,
        };
        let none = Modifiers::empty();
        // Ctrl/Cmd+A -> select-all; case-insensitive on the character key.
        assert_eq!(
            MultiSelectKeyOp::classify("a", cmd),
            Some(MultiSelectKeyOp::SelectAll)
        );
        assert_eq!(
            MultiSelectKeyOp::classify("A", meta),
            Some(MultiSelectKeyOp::SelectAll)
        );
        // Ctrl/Cmd+Space (both wire spellings) -> toggle the cursor.
        assert_eq!(
            MultiSelectKeyOp::classify(" ", cmd),
            Some(MultiSelectKeyOp::ToggleCursor)
        );
        assert_eq!(
            MultiSelectKeyOp::classify("Space", cmd),
            Some(MultiSelectKeyOp::ToggleCursor)
        );
        // Without the command modifier, these are NOT set-ops (plain 'a' =
        // type-ahead, plain Space = expand-toggle) — the caller navigates.
        assert_eq!(MultiSelectKeyOp::classify("a", none), None);
        assert_eq!(MultiSelectKeyOp::classify(" ", none), None);
        // A command chord on an unrelated key is not a set-op either.
        assert_eq!(MultiSelectKeyOp::classify("b", cmd), None);
        assert_eq!(MultiSelectKeyOp::classify("ArrowDown", cmd), None);
    }

    #[test]
    fn r778_activation_edge_is_pointer_up_or_keyboard_activate() {
        // The lifted command-widget activation predicate (R778): the two
        // events that count as "activate", and nothing else.
        assert!(is_activation_event(PointerWireEvent::Up.as_wire_name()));
        assert!(is_activation_event(KEYBOARD_ACTIVATE_EVENT));
        for name in [
            "PointerDown",
            "PointerEnter",
            "PointerLeave",
            "PointerCancel",
            "",
        ] {
            assert!(
                !is_activation_event(name),
                "{name} is not an activation edge"
            );
        }
    }

    #[test]
    fn r56_1_f_0_empty_has_no_bits_set() {
        let m = Modifiers::empty();
        assert!(!m.shift);
        assert!(!m.ctrl);
        assert!(!m.alt);
        assert!(!m.meta);
        assert!(m.is_empty());
    }

    #[test]
    fn r56_1_f_0_default_equals_empty() {
        assert_eq!(Modifiers::default(), Modifiers::empty());
    }

    #[test]
    fn r56_1_f_0_accessors_mirror_w3c_surface() {
        let m = Modifiers {
            shift: true,
            ctrl: false,
            alt: true,
            meta: false,
        };
        assert!(m.shift_key());
        assert!(!m.control_key());
        assert!(m.alt_key());
        assert!(!m.meta_key());
        assert!(!m.is_empty());
    }

    #[test]
    fn r56_1_f_0_any_bit_breaks_is_empty() {
        for m in [
            Modifiers {
                shift: true,
                ctrl: false,
                alt: false,
                meta: false,
            },
            Modifiers {
                shift: false,
                ctrl: true,
                alt: false,
                meta: false,
            },
            Modifiers {
                shift: false,
                ctrl: false,
                alt: true,
                meta: false,
            },
            Modifiers {
                shift: false,
                ctrl: false,
                alt: false,
                meta: true,
            },
        ] {
            assert!(!m.is_empty(), "any single bit must break is_empty");
        }
    }

    #[test]
    fn r781_wire_token_round_trips_every_combination() {
        // Encode ↔ decode are inverses for all 16 combinations (the
        // divergence-is-a-bug guard for the pointer modifier wire).
        for bits in 0u8..16 {
            let m = Modifiers {
                shift: bits & 1 != 0,
                ctrl: bits & 2 != 0,
                alt: bits & 4 != 0,
                meta: bits & 8 != 0,
            };
            let token = m.as_wire_token();
            assert_eq!(
                Modifiers::from_wire_token(&token),
                Some(m),
                "round-trip {m:?}"
            );
        }
        // Canonical order + empty-state contract.
        assert_eq!(Modifiers::empty().as_wire_token(), "");
        assert_eq!(
            Modifiers {
                shift: true,
                ctrl: true,
                alt: false,
                meta: false
            }
            .as_wire_token(),
            "sc",
        );
        // Decode is order-tolerant; a non-scam letter rejects the whole token.
        assert_eq!(
            Modifiers::from_wire_token("cs"),
            Some(Modifiers {
                shift: true,
                ctrl: true,
                alt: false,
                meta: false
            }),
        );
        assert_eq!(
            Modifiers::from_wire_token("sx"),
            None,
            "unknown letter rejects"
        );
        assert_eq!(Modifiers::from_wire_token(""), Some(Modifiers::empty()));
    }

    #[test]
    fn r56_1_f_0_clone_copy_eq_round_trip() {
        let m = Modifiers {
            shift: true,
            ctrl: true,
            alt: false,
            meta: false,
        };
        let n = m;
        assert_eq!(m, n);
        let o = m;
        assert_eq!(m, o);
    }
}

#[cfg(test)]
mod r56_2_a_composition_event_tests {
    //! R56.2.a §5.13 §5.38 — [`CompositionEvent`] enum surface tests.
    //! Pins the four W3C-mirrored variants + `Debug` + `PartialEq` +
    //! `Clone` derives so downstream pattern-matching call sites
    //! (`WidgetCore::apply_composition` dispatch in widget bindings,
    //! pinion-shell `WindowEvent::Ime` arm) stay stable.

    use super::CompositionEvent;
    use super::PointerWireEvent;

    #[test]
    fn r56_2_a_four_variants_construct_and_compare() {
        assert_eq!(CompositionEvent::Start, CompositionEvent::Start);
        assert_eq!(
            CompositionEvent::Update("ha".to_owned()),
            CompositionEvent::Update("ha".to_owned()),
        );
        assert_eq!(
            CompositionEvent::Commit("han".to_owned()),
            CompositionEvent::Commit("han".to_owned()),
        );
        assert_eq!(CompositionEvent::Cancel, CompositionEvent::Cancel);
    }

    #[test]
    fn r56_2_a_variants_are_distinct() {
        assert_ne!(CompositionEvent::Start, CompositionEvent::Cancel);
        assert_ne!(
            CompositionEvent::Update("ha".to_owned()),
            CompositionEvent::Commit("ha".to_owned()),
        );
        assert_ne!(
            CompositionEvent::Update("ha".to_owned()),
            CompositionEvent::Update("han".to_owned()),
        );
    }

    #[test]
    fn r56_2_a_clone_round_trip_preserves_data() {
        let original = CompositionEvent::Commit("\u{D55C}".to_owned()); // Korean syllable "한"
        let cloned = original.clone();
        assert_eq!(original, cloned);
        if let CompositionEvent::Commit(text) = cloned {
            assert_eq!(text.len(), 3, "Korean syllable is 3 UTF-8 bytes");
        } else {
            panic!("Clone must preserve variant tag");
        }
    }

    #[test]
    fn r56_2_a_empty_update_is_distinct_from_cancel() {
        // Empty Update is a "preedit cleared but composition still open"
        // signal — distinct from explicit Cancel which ends the session.
        // The substrate routes them through different `apply_composition_*`
        // methods on the External (Update("") → preedit_update("") vs
        // Cancel → preedit_cancel + SCXML CancelEdit).
        assert_ne!(
            CompositionEvent::Update(String::new()),
            CompositionEvent::Cancel,
        );
    }

    #[test]
    fn r56_2_a_four_known_variants_pattern_match() {
        // The `#[non_exhaustive]` attribute matters at the crate
        // boundary (external crates must include a wildcard arm);
        // inside `pinion-core` the four-variant match stays exhaustive.
        // This test pins the in-crate matchable surface so a future
        // variant addition is caught here at compile time and the
        // author updates the per-arm dispatch (and adds the
        // downstream wildcard-arm regression in the relevant
        // consumer crate).
        let events = [
            CompositionEvent::Start,
            CompositionEvent::Update("x".to_owned()),
            CompositionEvent::Commit("x".to_owned()),
            CompositionEvent::Cancel,
        ];
        for e in &events {
            let label = match e {
                CompositionEvent::Start => "start",
                CompositionEvent::Update(_) => "update",
                CompositionEvent::Commit(_) => "commit",
                CompositionEvent::Cancel => "cancel",
            };
            assert!(!label.is_empty());
        }
    }

    // R1643 — the crate's own `ALL`, which is held to the variant count by
    // `#[variant_census(all)]`. This was a fourth hand-written copy of the arm
    // list, in a test, so a sixth event would have left it silently short in the
    // one place that checks the round trip.
    use super::PointerWireEvent as PWE;
    const ALL_POINTER_WIRE_EVENTS: [PointerWireEvent; PWE::ARMS] = PWE::ALL;

    #[test]
    fn r773_pointer_wire_event_encode_decode_round_trips() {
        // decode(encode(e)) == e for every variant — the SSOT pairing
        // guard: a name added to one direction but not the other fails
        // here at compile/test time.
        for e in ALL_POINTER_WIRE_EVENTS {
            assert_eq!(PointerWireEvent::from_wire_name(e.as_wire_name()), Some(e));
        }
    }

    /// R1643 — the two spellings of the wire name agree on every arm.
    ///
    /// `WIRE_NAMES` is a `const` folded out of `as_wire_name_const`, and
    /// `as_wire_name` is the public non-const accessor its callers use. A `const`
    /// cannot call a non-`const` method, so the match exists twice — the shape
    /// R1639 recorded for the `WidgetEventName` derive, where a macro emits the
    /// arms twice for the same reason. A generator's duplication is invisible to
    /// its author; a hand-written one is not, so it is pinned here.
    /// # The cardinality check alone is not enough, and a counterfactual said so
    ///
    /// `#[variant_census(all)]` holds `ALL`'s LENGTH to the arm count, which
    /// catches a list that is short. It cannot catch a list that is the right
    /// length with an arm written twice — and that is not a hypothetical: a
    /// counterfactual replacing `Cancel` with a second `Leave` left this test,
    /// the round trip, and the whole workspace suite green while
    /// `WIRE_NAMES` published `PointerLeave` twice and `PointerCancel` not at
    /// all. R1630 recorded the argument that closes it (total + surjective +
    /// equal cardinality implies injective) and this test had only the third
    /// term, so the set below supplies the second.
    #[test]
    fn r1643_the_pointer_vocabulary_has_one_spelling() {
        assert_eq!(PWE::WIRE_NAMES.len(), PWE::ALL.len());
        let distinct: std::collections::BTreeSet<&str> = PWE::WIRE_NAMES.into_iter().collect();
        assert_eq!(
            distinct.len(),
            PWE::ARMS,
            "every arm must publish its OWN name: {:?}",
            PWE::WIRE_NAMES,
        );
        for (i, event) in PWE::ALL.into_iter().enumerate() {
            assert_eq!(
                event.as_wire_name(),
                event.as_wire_name_const(),
                "{event:?} is spelled twice and they must agree",
            );
            assert_eq!(
                PWE::WIRE_NAMES[i],
                event.as_wire_name(),
                "the published list IS the names, in declaration order",
            );
            assert_eq!(
                PWE::from_wire_name(PWE::WIRE_NAMES[i]),
                Some(event),
                "every published name is one the parser admits",
            );
        }
        assert_eq!(PWE::from_wire_name("PointerHover"), None, "and only those");
    }

    #[test]
    fn r773_pointer_wire_event_names_are_canonical() {
        assert_eq!(PointerWireEvent::Enter.as_wire_name(), "PointerEnter");
        assert_eq!(PointerWireEvent::Down.as_wire_name(), "PointerDown");
        assert_eq!(PointerWireEvent::Up.as_wire_name(), "PointerUp");
        assert_eq!(PointerWireEvent::Leave.as_wire_name(), "PointerLeave");
        assert_eq!(PointerWireEvent::Cancel.as_wire_name(), "PointerCancel");
    }

    #[test]
    fn r773_pointer_wire_event_rejects_unknown_names() {
        // Names outside the pointer vocabulary (a different wire
        // vocabulary, or a typo) decode to None so callers can fall
        // through to their own handling or reject the payload.
        assert_eq!(PointerWireEvent::from_wire_name("PointerWheel"), None);
        assert_eq!(PointerWireEvent::from_wire_name("PointerMove"), None);
        assert_eq!(PointerWireEvent::from_wire_name("KeyboardActivate"), None);
        assert_eq!(PointerWireEvent::from_wire_name("DoubleClick"), None);
        assert_eq!(PointerWireEvent::from_wire_name(""), None);
    }
}

#[cfg(test)]
mod edit_field_keymap_tests {
    //! R878 §5.38 — the lifted typed inline-editor keymap. The decision
    //! surface (Enter / Escape interception, caret-key whitelist, the
    //! `CellKind` keystroke gate) is covered here; the forwarding
    //! plumbing itself is [`super::forward_key_to_field`], exercised
    //! end-to-end by the data-grid / property-grid binding tests.

    use std::cell::Cell;

    use super::{Modifiers, edit_field_keymap};
    use crate::cell_value::CellKind;
    use crate::scene::{ContainerNode, Scene};

    fn empty_scene() -> Scene {
        Scene::Container(ContainerNode::new(Vec::new()))
    }

    #[test]
    fn enter_runs_commit_only_and_consumes() {
        let mut scene = empty_scene();
        let committed = Cell::new(false);
        let cancelled = Cell::new(false);
        let handled = edit_field_keymap(
            &mut scene,
            "tf",
            "Enter",
            Modifiers::empty(),
            CellKind::Text,
            || committed.set(true),
            || cancelled.set(true),
        );
        assert!(handled, "Enter is consumed by the keymap");
        assert!(committed.get(), "Enter runs the commit policy");
        assert!(!cancelled.get(), "Enter never cancels");
    }

    #[test]
    fn escape_runs_cancel_only_and_consumes() {
        let mut scene = empty_scene();
        let committed = Cell::new(false);
        let cancelled = Cell::new(false);
        let handled = edit_field_keymap(
            &mut scene,
            "tf",
            "Escape",
            Modifiers::empty(),
            CellKind::Int,
            || committed.set(true),
            || cancelled.set(true),
        );
        assert!(handled, "Escape is consumed by the keymap");
        assert!(cancelled.get(), "Escape runs the cancel policy");
        assert!(!committed.get(), "Escape never commits");
    }

    #[test]
    fn command_chords_bypass_the_kind_gate() {
        // R879 audit fix — Ctrl+A in an int editor is select-all, not the
        // letter "a": the chord reaches the field's modifier-aware arm and
        // is RECOGNISED, where the plain letter dies at the gate. A live
        // `TextFieldExternal` in the scene makes the two outcomes
        // distinguishable (both would read `false` against a missing field).
        use crate::reactive::Owner;
        use crate::scene::ExternalNode;
        use crate::widgets::text_edit::use_text_edit_state;
        use crate::widgets::text_field::TextFieldExternal;
        Owner::new().run(|| {
            let editor = use_text_edit_state("tf");
            editor.set_text("42".to_owned());
            let mut scene = Scene::External(
                ExternalNode::new(Box::new(
                    TextFieldExternal::new().attach_state(editor.clone()),
                ))
                .with_tag("tf"),
            );
            let touched = Cell::new(false);
            let chord = Modifiers {
                ctrl: true,
                ..Modifiers::empty()
            };
            let handled = edit_field_keymap(
                &mut scene,
                "tf",
                "a",
                chord,
                CellKind::Int,
                || touched.set(true),
                || touched.set(true),
            );
            assert!(
                handled,
                "Ctrl+A reaches the field's select-all arm in an int editor"
            );
            assert!(!touched.get(), "a chord is never commit/cancel");
            assert_eq!(
                editor.selection_range(),
                Some((0, 2)),
                "and it actually selected all"
            );
            // The bare letter still dies at the int gate.
            let bare = edit_field_keymap(
                &mut scene,
                "tf",
                "a",
                Modifiers::empty(),
                CellKind::Int,
                || touched.set(true),
                || touched.set(true),
            );
            assert!(!bare, "the plain letter is still gated out");
        });
    }

    #[test]
    fn gate_rejects_keystrokes_outside_the_kind() {
        // An int editor rejects a letter at the keystroke edge — no
        // closure fires and the key defers to the shell fallback chain.
        let mut scene = empty_scene();
        let touched = Cell::new(false);
        let handled = edit_field_keymap(
            &mut scene,
            "tf",
            "a",
            Modifiers::empty(),
            CellKind::Int,
            || touched.set(true),
            || touched.set(true),
        );
        assert!(!handled, "an int editor rejects a letter");
        assert!(!touched.get(), "a rejected keystroke runs no policy");
    }

    #[test]
    fn named_keys_outside_the_whitelist_defer() {
        // `ArrowUp` is neither commit/cancel, whitelist, nor a single
        // printable — it bubbles (inert while the editor owns focus).
        let mut scene = empty_scene();
        let handled = edit_field_keymap(
            &mut scene,
            "tf",
            "ArrowUp",
            Modifiers::empty(),
            CellKind::Text,
            || {},
            || {},
        );
        assert!(
            !handled,
            "non-whitelist named keys defer to the fallback chain"
        );
    }

    #[test]
    fn whitelist_keys_forward_even_when_the_field_is_missing() {
        // Caret / deletion keys route into `forward_key_to_field`; with
        // no External under the tag the forward reports `false` (the
        // shell fallback), never a panic.
        let mut scene = empty_scene();
        for key in [
            "ArrowLeft",
            "ArrowRight",
            "Home",
            "End",
            "Backspace",
            "Delete",
        ] {
            let handled = edit_field_keymap(
                &mut scene,
                "tf",
                key,
                Modifiers::empty(),
                CellKind::Float,
                || {},
                || {},
            );
            assert!(
                !handled,
                "{key} forwards; a missing field reports unhandled"
            );
        }
    }
}

#[cfg(test)]
mod drag_calibration_tests {
    use super::DragCalibration;

    /// The first move snapshots the base and the anchor fraction but yields no
    /// travel (calibration only) — the press-anchored, non-mutating first frame
    /// the column resize and the scrub both depend on.
    #[test]
    fn first_move_calibrates_and_yields_nothing() {
        let cal = DragCalibration::<f64>::new();
        assert!(!cal.is_active(), "idle before the first move");
        let first = cal.drive(0.5, || Some(10.0));
        assert_eq!(first, None, "the calibration frame yields no travel");
        assert!(cal.is_active(), "the snapshot is now armed");
    }

    /// Every later move reports the payload and the cursor's fraction delta
    /// since the press — the caller scales `delta` by its basis width.
    #[test]
    fn later_moves_yield_payload_and_fraction_delta() {
        /// The payload rides through untouched; the delta is float arithmetic,
        /// so compare it against an epsilon rather than for bit-equality.
        fn assert_drag(got: Option<(f64, f64)>, base: f64, delta: f64) {
            let (p, d) = got.expect("a later move yields travel");
            assert!(
                (p - base).abs() < f64::EPSILON,
                "payload {p} != base {base}"
            );
            assert!((d - delta).abs() < 1e-9, "delta {d} != expected {delta}");
        }
        let cal = DragCalibration::<f64>::new();
        cal.drive(0.5, || Some(10.0)); // calibrate at fraction 0.5, base 10.0
        assert_drag(cal.drive(0.7, || unreachable!("seed runs once")), 10.0, 0.2);
        // Delta is always measured from the PRESS, not the previous move — so a
        // clamp that floored an intermediate value un-clamps cleanly on return.
        assert_drag(
            cal.drive(0.4, || unreachable!("seed runs once")),
            10.0,
            -0.1,
        );
        assert_drag(cal.drive(0.5, || unreachable!("seed runs once")), 10.0, 0.0);
    }

    /// A `seed` that returns `None` declines the drag — the snapshot stays
    /// empty (a press on a non-draggable target, e.g. a non-numeric scrub row).
    #[test]
    fn declined_seed_does_not_arm() {
        let cal = DragCalibration::<u32>::new();
        assert_eq!(cal.drive(0.3, || None), None);
        assert!(!cal.is_active(), "a declined seed never arms the drag");
        // A later move with a live seed still calibrates (the arm is per-press).
        assert_eq!(cal.drive(0.3, || Some(7)), None);
        assert!(cal.is_active());
    }

    /// `end` reports whether a drag had calibrated, so a release can suppress
    /// the trailing click only when a real drag ran.
    #[test]
    fn end_reports_whether_a_drag_ran() {
        let cal = DragCalibration::<u32>::new();
        assert!(
            !cal.end(),
            "a press that never moved is a click, not a drag"
        );

        cal.drive(0.5, || Some(7));
        assert!(cal.end(), "a calibrated drag is reported on teardown");
        assert!(!cal.is_active(), "teardown clears the snapshot");
        assert!(!cal.end(), "a second teardown reports no drag (idempotent)");
    }

    /// `traveled_beyond` discriminates a click (within the dead zone) from a
    /// drag (strayed past `threshold_px` of pixel travel) using the consumer's
    /// basis width — the click-to-focus vs drag-to-scrub split.
    #[test]
    fn traveled_beyond_discriminates_click_from_drag() {
        let cal = DragCalibration::<u32>::new();
        // basis 100px, threshold 4px => a 0.04 fraction delta is the boundary.
        assert!(!cal.traveled_beyond(100.0, 4.0), "idle: no travel");

        cal.drive(0.5, || Some(0)); // calibrate at the press (the R51.35 forward)
        assert!(
            !cal.traveled_beyond(100.0, 4.0),
            "the calibration frame is a click so far"
        );

        cal.drive(0.52, || unreachable!("seeded")); // 0.02 * 100 = 2px < 4px
        assert!(
            !cal.traveled_beyond(100.0, 4.0),
            "a 2px stray is still within the click dead zone"
        );

        cal.drive(0.56, || unreachable!("seeded")); // 0.06 * 100 = 6px >= 4px
        assert!(
            cal.traveled_beyond(100.0, 4.0),
            "a 6px stray crossed into a drag"
        );

        // The max is sticky: returning toward the press stays a drag.
        cal.drive(0.5, || unreachable!("seeded"));
        assert!(
            cal.traveled_beyond(100.0, 4.0),
            "max travel is sticky — still a drag"
        );

        cal.end();
        assert!(
            !cal.traveled_beyond(100.0, 4.0),
            "teardown resets the discriminator"
        );
    }

    #[test]
    fn r1418_pointer_buttons_set_ops_and_wire_round_trip() {
        use super::{PointerButton, PointerButtons};
        let empty = PointerButtons::empty();
        assert!(empty.is_empty());
        assert_eq!(empty.as_wire_token(), "");
        // A chord: press left, then right → held {left, right}.
        let chord = empty.with(PointerButton::Left).with(PointerButton::Right);
        assert!(chord.contains(PointerButton::Left));
        assert!(chord.contains(PointerButton::Right));
        assert!(!chord.contains(PointerButton::Middle));
        assert!(!chord.is_empty());
        // Canonical `lmr` order regardless of insertion order.
        assert_eq!(chord.as_wire_token(), "lr");
        // Release left → {right}.
        assert_eq!(chord.without(PointerButton::Left).as_wire_token(), "r");
        // Decode is the inverse of encode; a bad letter rejects.
        for token in ["", "l", "m", "r", "lm", "lr", "mr", "lmr"] {
            let set = PointerButtons::from_wire_token(token).expect("valid token");
            assert_eq!(set.as_wire_token(), token, "round-trip {token:?}");
        }
        assert_eq!(PointerButtons::from_wire_token("x"), None);
        assert_eq!(PointerButtons::from_wire_token("ls"), None);
    }

    #[test]
    fn input_state_snapshot_default_has_no_key_dispatch_axis() {
        // A single-OS-window backend leaves the whole axis `None` — the
        // "axis unavailable" honesty, distinct from "available but empty".
        let snap = super::InputStateSnapshot::default();
        assert_eq!(snap.key_dispatch, None, "default = axis unavailable");
    }

    #[test]
    fn key_dispatch_focus_default_is_unfocused_and_unowned() {
        let kd = super::KeyDispatchFocus::default();
        assert_eq!(kd.os_focused_window, None, "no window holds OS focus");
        assert!(kd.key_press_owners.is_empty(), "no key is held / owned");
        // R1428 — the default per-window verdict is unfocused; the shell
        // derives the real value from `is_key_dispatch_window(wid)`.
        assert!(!kd.focused, "default per-window verdict is unfocused");
    }

    #[test]
    fn input_state_snapshot_carries_dispatch_focus() {
        // The GUI shell populates the axis; the snapshot round-trips it
        // verbatim (owners are the producer's responsibility to sort).
        let kd = super::KeyDispatchFocus {
            os_focused_window: Some("pane-1".to_owned()),
            key_press_owners: vec![
                ("Enter".to_owned(), "pane-1".to_owned()),
                ("Space".to_owned(), "main".to_owned()),
            ],
            // R1428 — this dispatch is scoped to "pane-1", which holds
            // focus, so the derived per-window verdict is `true`.
            focused: true,
        };
        let snap = super::InputStateSnapshot {
            key_dispatch: Some(kd.clone()),
            ..Default::default()
        };
        assert_eq!(snap.key_dispatch.as_ref(), Some(&kd));
        let got = snap.key_dispatch.unwrap();
        assert_eq!(got.os_focused_window.as_deref(), Some("pane-1"));
        assert_eq!(got.key_press_owners.len(), 2);
        assert!(
            got.focused,
            "the focused window's per-window verdict is true"
        );
    }

    #[test]
    fn r1432_gesture_phase_wire_round_trips_and_rejects_typos() {
        use super::GesturePhase;
        // Every phase encodes and decodes to itself — the RPC decode
        // (`from_wire_name`) and the introspect encode (`as_wire_name`) share
        // one vocabulary, so a `scene/pinch_gesture {phase}` and the field it
        // surfaces can never drift.
        for phase in [
            GesturePhase::Begin,
            GesturePhase::Update,
            GesturePhase::End,
            GesturePhase::Cancel,
        ] {
            assert_eq!(
                GesturePhase::from_wire_name(phase.as_wire_name()),
                Some(phase)
            );
        }
        assert_eq!(GesturePhase::Begin.as_wire_name(), "begin");
        assert_eq!(GesturePhase::Cancel.as_wire_name(), "cancel");
        // A typo / out-of-vocabulary name decodes to `None` so it surfaces at
        // the call site as an `invalid_params`, never a silent default.
        assert_eq!(GesturePhase::from_wire_name("started"), None);
        assert_eq!(GesturePhase::from_wire_name(""), None);
        // The default is `Begin` — a gesture arc's first phase.
        assert_eq!(GesturePhase::default(), GesturePhase::Begin);
    }
}

/// R1658 §5.13 §5.39 — a keystroke says when it arrived.
#[cfg(test)]
mod key_arrival_tests {
    use super::{KeyArrival, KeyBatch, KeyPress, Modifiers};

    #[test]
    fn r1658_arrived_together_is_the_batch_and_not_the_instant() {
        // THE AXIS, PINNED. Two keystrokes of one delivery share both halves,
        // so a shell-level test cannot tell an implementation that compares
        // batches from one that compares instants. These two can: they hold
        // the halves apart deliberately.
        let t0 = std::time::Instant::now();
        let t1 = std::time::Instant::now();

        let one = KeyArrival::new(t0, KeyBatch::initial().next());
        let same_batch_later_instant = KeyArrival::new(t1, KeyBatch::initial().next());
        assert!(
            one.arrived_with(same_batch_later_instant),
            "one delivery is one gesture however its instants were taken"
        );

        let same_instant_next_batch = KeyArrival::new(t0, KeyBatch::initial().next().next());
        assert!(
            !one.arrived_with(same_instant_next_batch),
            "two deliveries are two gestures even if the clock could not \
             separate them — which is exactly why the batch exists and the \
             instant is not the answer"
        );
    }

    #[test]
    fn r1658_the_initial_batch_is_nobodys_delivery() {
        // A keystroke dispatched before any backend opened a delivery must not
        // claim to have arrived with the first one that opens.
        let initial = KeyBatch::initial();
        assert_ne!(initial, initial.next());
        assert_ne!(initial.next(), initial.next().next());
    }

    #[test]
    fn r1658_a_press_carries_its_arrival_unchanged() {
        let arrival = KeyArrival::new(std::time::Instant::now(), KeyBatch::initial().next());
        let press = KeyPress::new("a", Modifiers::empty(), true, arrival);
        assert_eq!(press.key, "a");
        assert!(press.repeat);
        assert_eq!(press.arrival, arrival);
        assert_eq!(press.arrival.at(), arrival.at());
        assert_eq!(press.arrival.batch(), arrival.batch());
    }
}
