//! §5.28 — animation primitive substrate (R51.133 첫 land, R51.138 Signal wrap).
//!
//! ## Charter (§5.28 ratified at R33)
//!
//! Spring-physics first; tween/keyframe 은 special case of a critically
//! damped spring. [`Animation<T>`] wraps a [`Signal<T>`](crate::reactive::Signal)
//! with [`SpringConfig`] (stiffness / damping / mass); semi-implicit Euler
//! solver; interruptible (a new target preserves velocity). Industry
//! canonical: `SwiftUI` `Animation` / React Spring / `Compose`
//! `animateXxxAsState`.
//!
//! ## Scope (R51.133 substrate + R51.138 wrap)
//!
//! Pure substrate primitive plus the [`Signal`]-
//! bound wrapper. `Animation::tick` is the only mutating entry; it remains
//! caller-driven (driver injects `dt` from `Frame.dt`, §6.3) so the §2
//! invariant #3 `dry_run` guarantee survives: identical `(state, config, dt)`
//! sequences always yield identical [`Signal`]
//! evolution, so a scenario explorer can fast-forward without side effects.
//!
//! - [`Animatable`] trait: vector-arithmetic interface
//!   (`zero` / `add` / `sub` / `scale` / `approx_zero`). Default
//!   [`Animatable::lerp`] is derived from the arithmetic.
//! - Impls: [`f32`], [`AnimVec2`], [`AnimVec4`] (continuous-space
//!   vectors). [`Color`](crate::style::Color) / [`Rect`](crate::scene::Rect)
//!   impls deferred to a later round (linear-space conversion is a
//!   separate quality decision).
//! - [`SpringConfig`] with four presets ([`SpringConfig::DEFAULT`],
//!   [`SpringConfig::GENTLE`], [`SpringConfig::STIFF`],
//!   [`SpringConfig::WOBBLY`]) — numerics match `SwiftUI` / React Spring
//!   for cross-ecosystem familiarity.
//! - [`SpringState`] generic over `T: Animatable`: current / velocity /
//!   target triple, plus a pure [`SpringState::step`] (semi-implicit
//!   Euler) and [`SpringState::is_done`] (epsilon-based settle).
//!
//! ## Carry (R51.139+)
//!
//! - Framework runtime integration: paint-loop call to
//!   [`Owner::tick_animations`](crate::reactive::Owner::tick_animations)
//!   inside an [`Effect`](crate::reactive::Effect) driven by `Frame.dt`
//!   (§6.3). The substrate already supports the call; only the wiring
//!   from the runtime crate is outstanding.
//! - `hello-button` hover transition (1st application / visual evidence).
//! - SCE schema for declarative animated bindings; Forge emit.
//! - Additional easings (`EaseInQuart` / `EaseInBack` / `EaseInElastic`),
//!   gated on evidence-first carry.
//!
//! ## Interruptibility
//!
//! Re-targeting is a single field write: `state.target = new_target`.
//! The next [`SpringState::step`] continues with the existing velocity,
//! so the value evolves continuously through the change with no
//! discontinuity — exactly the `SwiftUI` / `Compose` guarantee.

/// Vector-arithmetic surface required by the spring solver.
///
/// Implementors define five primitive ops; [`Animatable::lerp`] is
/// derived automatically. Implementations live in *continuous* numeric
/// space (e.g. `f32`, [`AnimVec2`]) — quantized types like
/// [`Rect`](crate::scene::Rect) need an explicit `f32` shadow space
/// because saturating `u32` arithmetic violates the additive identities
/// the solver relies on.
pub trait Animatable: Copy {
    /// Additive identity. `x.add(Self::zero()) == x` must hold.
    #[must_use]
    fn zero() -> Self;

    /// Component-wise addition.
    #[must_use]
    fn add(self, other: Self) -> Self;

    /// Component-wise subtraction (`self - other`).
    #[must_use]
    fn sub(self, other: Self) -> Self;

    /// Scalar multiplication.
    #[must_use]
    fn scale(self, factor: f32) -> Self;

    /// Component-wise "all components within `epsilon` of zero".
    /// Used by [`SpringState::is_done`] for the rest predicate.
    #[must_use]
    fn approx_zero(self, epsilon: f32) -> bool;

    /// Linear interpolation. `t = 0` returns `a`, `t = 1` returns `b`;
    /// values outside `[0, 1]` extrapolate. Derived from the arithmetic
    /// as `a + (b - a) * t`; implementors override only when a
    /// type-specific path is more numerically stable.
    #[must_use]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a.add(b.sub(a).scale(t))
    }
}

impl Animatable for f32 {
    fn zero() -> Self {
        0.0
    }
    fn add(self, other: Self) -> Self {
        self + other
    }
    fn sub(self, other: Self) -> Self {
        self - other
    }
    fn scale(self, factor: f32) -> Self {
        self * factor
    }
    fn approx_zero(self, epsilon: f32) -> bool {
        self.abs() < epsilon
    }
}

/// 2-D continuous-space vector for position-like animations.
///
/// Runtime quantizes to [`Rect`](crate::scene::Rect) at frame paint
/// time. Use `f32` here (not `u32`) so the spring solver can produce
/// sub-pixel intermediate values during integration — quantization
/// happens at the `paint_adapter` boundary, not inside the animation.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnimVec2 {
    pub x: f32,
    pub y: f32,
}

impl AnimVec2 {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl Animatable for AnimVec2 {
    fn zero() -> Self {
        Self::new(0.0, 0.0)
    }
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
    fn scale(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }
    fn approx_zero(self, epsilon: f32) -> bool {
        self.x.abs() < epsilon && self.y.abs() < epsilon
    }
}

/// 4-D continuous-space vector — the linear-space carrier for RGBA
/// color animation among other uses.
///
/// [`Color`](crate::style::Color) animation requires this f32 shadow
/// because u8 saturating arithmetic on channels destroys the
/// additive identities the spring solver assumes. The future
/// [`Color`](crate::style::Color) `Animatable` impl converts to
/// `AnimVec4` (linear sRGB or premultiplied-linear, TBD at land time)
/// before the integration step.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnimVec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl AnimVec4 {
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }
}

impl Animatable for AnimVec4 {
    fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }
    fn add(self, other: Self) -> Self {
        Self::new(
            self.x + other.x,
            self.y + other.y,
            self.z + other.z,
            self.w + other.w,
        )
    }
    fn sub(self, other: Self) -> Self {
        Self::new(
            self.x - other.x,
            self.y - other.y,
            self.z - other.z,
            self.w - other.w,
        )
    }
    fn scale(self, factor: f32) -> Self {
        Self::new(
            self.x * factor,
            self.y * factor,
            self.z * factor,
            self.w * factor,
        )
    }
    fn approx_zero(self, epsilon: f32) -> bool {
        self.x.abs() < epsilon
            && self.y.abs() < epsilon
            && self.z.abs() < epsilon
            && self.w.abs() < epsilon
    }
}

/// 4-component continuous-space rectangle for animation.
///
/// Mirrors [`Rect`](crate::scene::Rect) (u32 grid coordinates) in
/// `f32` space so the spring solver can produce fractional
/// intermediate positions during integration. Quantize back at
/// paint-adapter boundary via [`AnimRect::to_rect`].
///
/// Use this (not [`AnimVec4`]) when the four components are
/// semantically `x` / `y` / `w` / `h` — the named fields are the
/// only thing distinguishing it from a generic 4-vector.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnimRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl AnimRect {
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Lift a quantized [`Rect`](crate::scene::Rect) into continuous
    /// space. `u32 → f32` widens, so values up to about 2²⁴ are
    /// exact; larger values lose lower-bit precision but stay within
    /// `f32` magnitude range.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn from_rect(r: crate::scene::Rect) -> Self {
        Self::new(r.x as f32, r.y as f32, r.w as f32, r.h as f32)
    }

    /// Quantize back to a [`Rect`](crate::scene::Rect). Components are
    /// rounded; negative, NaN, or out-of-`u32`-range values saturate
    /// to `0` / [`u32::MAX`] rather than wrapping.
    #[must_use]
    pub fn to_rect(self) -> crate::scene::Rect {
        crate::scene::Rect::new(
            saturate_u32(self.x),
            saturate_u32(self.y),
            saturate_u32(self.w),
            saturate_u32(self.h),
        )
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn saturate_u32(v: f32) -> u32 {
    if v.is_nan() || v <= 0.0 {
        0
    } else if v >= u32::MAX as f32 {
        u32::MAX
    } else {
        v.round() as u32
    }
}

impl Animatable for AnimRect {
    fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }
    fn add(self, other: Self) -> Self {
        Self::new(
            self.x + other.x,
            self.y + other.y,
            self.w + other.w,
            self.h + other.h,
        )
    }
    fn sub(self, other: Self) -> Self {
        Self::new(
            self.x - other.x,
            self.y - other.y,
            self.w - other.w,
            self.h - other.h,
        )
    }
    fn scale(self, factor: f32) -> Self {
        Self::new(
            self.x * factor,
            self.y * factor,
            self.w * factor,
            self.h * factor,
        )
    }
    fn approx_zero(self, epsilon: f32) -> bool {
        self.x.abs() < epsilon
            && self.y.abs() < epsilon
            && self.w.abs() < epsilon
            && self.h.abs() < epsilon
    }
}

/// Spring physics tuning: stiffness, damping, mass.
///
/// The damping ratio `ζ = damping / (2 * sqrt(stiffness * mass))`
/// determines the qualitative behaviour:
///
/// - `ζ == 1.0` → critically damped: fastest settle without overshoot
/// - `ζ < 1.0` → underdamped: overshoot then ring down
/// - `ζ > 1.0` → overdamped: slow asymptotic approach
///
/// The four presets ([`SpringConfig::DEFAULT`], [`SpringConfig::GENTLE`],
/// [`SpringConfig::STIFF`], [`SpringConfig::WOBBLY`]) mirror the numbers
/// used by `SwiftUI` `Animation` and React Spring so animations port
/// directly across ecosystems.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringConfig {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl SpringConfig {
    /// Construct a custom config. `#[non_exhaustive]` on the struct
    /// forbids cross-crate struct expressions, so this is the canonical
    /// builder for non-preset tunings.
    #[must_use]
    pub const fn new(stiffness: f32, damping: f32, mass: f32) -> Self {
        Self {
            stiffness,
            damping,
            mass,
        }
    }

    /// Matches `SwiftUI` `Animation.default` and React Spring
    /// `config.default` — the everyday choice.
    pub const DEFAULT: Self = Self::new(170.0, 26.0, 1.0);

    /// Softer, slower spring; `config.gentle` equivalent.
    pub const GENTLE: Self = Self::new(120.0, 14.0, 1.0);

    /// Tighter, faster settle; `config.stiff` equivalent.
    pub const STIFF: Self = Self::new(210.0, 20.0, 1.0);

    /// Bouncier, lower damping; `config.wobbly` equivalent.
    pub const WOBBLY: Self = Self::new(180.0, 12.0, 1.0);
}

/// Mutable spring-solver triple: `current`, `velocity`, `target`.
///
/// All three live in the same [`Animatable`] type. Re-targeting (the
/// canonical interruption pattern) is a single field write: assign a
/// new `target` and the next [`SpringState::step`] continues with the
/// existing velocity. No discontinuity, no per-target restart — that
/// is exactly what gives spring physics its "natural" feel under
/// rapid input.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpringState<T: Animatable> {
    pub current: T,
    pub velocity: T,
    pub target: T,
}

/// Curve-based easing functions for tween animation.
///
/// Tween animation is the spring's special case — a critically-damped
/// spring with a fixed duration approximates `EaseOutCubic` closely.
/// Use [`Tween`] for cases where deterministic finish-time matters
/// (UI route transitions, splash sequencing); use [`SpringState`] for
/// physical / interruptible interactions (drag-release, gesture
/// follow).
///
/// Each variant accepts `t ∈ [0, 1]` and returns an eased `t' ∈
/// [0, 1]` (endpoints exact). Out-of-range input extrapolates; callers
/// that need clamping wrap with `t.clamp(0.0, 1.0)` before calling.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Easing {
    /// Identity — `t` returned unchanged.
    #[default]
    Linear,
    /// `t²` — accelerating, "slow start".
    EaseInQuad,
    /// `1 - (1 - t)²` — decelerating, "soft stop".
    EaseOutQuad,
    /// Quad acceleration in the first half, decel in the second.
    EaseInOutQuad,
    /// `t³` — sharper acceleration than [`Easing::EaseInQuad`].
    EaseInCubic,
    /// `1 - (1 - t)³` — sharper deceleration.
    EaseOutCubic,
    /// Cubic accel in the first half, decel in the second — the
    /// closest curve match to a critically-damped spring of the
    /// same duration.
    EaseInOutCubic,
}

impl Easing {
    /// Map input `t` through the curve. `t = 0` → `0`, `t = 1` → `1`
    /// for every variant. Const-evaluable so callers can precompute
    /// curve points for static layout work.
    #[must_use]
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Self::Linear => t,
            Self::EaseInQuad => t * t,
            Self::EaseOutQuad => {
                let inv = 1.0 - t;
                1.0 - inv * inv
            }
            Self::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    let inv = -2.0 * t + 2.0;
                    1.0 - inv * inv / 2.0
                }
            }
            Self::EaseInCubic => t * t * t,
            Self::EaseOutCubic => {
                let inv = 1.0 - t;
                1.0 - inv * inv * inv
            }
            Self::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let inv = -2.0 * t + 2.0;
                    1.0 - inv * inv * inv / 2.0
                }
            }
        }
    }
}

/// Caller-driven tween animation — fixed duration, deterministic
/// finish time, curve-shaped progress.
///
/// Contrast with [`SpringState`]: a tween locks both end points and
/// the time taken, so it cannot be naturally interrupted mid-flight
/// without a discontinuity. Use this when the *when* matters as much
/// as the *where* (modal slide-in, list reorder choreography); use
/// [`SpringState`] when responsiveness under interruption is the
/// primary concern.
///
/// Pure: same `(from, to, duration, easing, elapsed)` always yields
/// the same `current()`. The §2 invariant #3 `dry_run` guarantee
/// extends here without further ceremony.
#[derive(Debug, Clone, Copy)]
pub struct Tween<T: Animatable> {
    pub from: T,
    pub to: T,
    pub duration: f32,
    pub easing: Easing,
    pub elapsed: f32,
}

impl<T: Animatable> Tween<T> {
    /// Construct a tween at `elapsed = 0`. `duration` must be `> 0`
    /// for a sensible progression — a zero-duration tween produces
    /// `current() == from` for any `elapsed < ε` and `to` afterward
    /// (the saturating fallback below).
    #[must_use]
    pub fn new(from: T, to: T, duration: f32, easing: Easing) -> Self {
        Self {
            from,
            to,
            duration,
            easing,
            elapsed: 0.0,
        }
    }

    /// Compute the current value. Clamps internal progress to
    /// `[0, 1]` so callers don't have to police `elapsed`.
    #[must_use]
    pub fn current(self) -> T {
        if self.duration <= 0.0 {
            return self.to;
        }
        let raw = (self.elapsed / self.duration).clamp(0.0, 1.0);
        let eased = self.easing.apply(raw);
        T::lerp(self.from, self.to, eased)
    }

    /// Advance `elapsed` by `dt` seconds. Caller-injected time
    /// keeps the dry-run guarantee intact.
    pub fn tick(&mut self, dt: f32) {
        self.elapsed += dt;
    }

    /// `true` once `elapsed >= duration` — the tween has reached
    /// its `to` value and further `tick` calls are no-ops with
    /// respect to [`Tween::current`].
    #[must_use]
    pub fn is_done(self) -> bool {
        self.elapsed >= self.duration
    }
}

impl<T: Animatable> SpringState<T> {
    /// Construct a state at rest at `value` — `current == target ==
    /// value` and `velocity == zero`. The natural starting point
    /// before animation begins.
    #[must_use]
    pub fn at_rest(value: T) -> Self {
        Self {
            current: value,
            velocity: T::zero(),
            target: value,
        }
    }

    /// Advance the spring by `dt` seconds under `config`.
    ///
    /// Pure function — no mutation, no ambient state, no allocation.
    /// Given identical `(self, config, dt)` always returns the
    /// identical next state, preserving the §2 invariant #3 `dry_run`
    /// guarantee. Uses semi-implicit Euler: integrate velocity from
    /// acceleration first, then integrate position from the new
    /// velocity. More stable than explicit Euler at the same `dt`,
    /// equivalent to symplectic integration at first order.
    ///
    /// Force law: `F = -stiffness * (current - target) - damping *
    /// velocity`, acceleration `a = F / mass`.
    #[must_use]
    pub fn step(self, config: SpringConfig, dt: f32) -> Self {
        let displacement = self.current.sub(self.target);
        let spring_force = displacement.scale(-config.stiffness);
        let damping_force = self.velocity.scale(-config.damping);
        let force = spring_force.add(damping_force);
        let accel = force.scale(1.0 / config.mass);

        let new_velocity = self.velocity.add(accel.scale(dt));
        let new_current = self.current.add(new_velocity.scale(dt));

        Self {
            current: new_current,
            velocity: new_velocity,
            target: self.target,
        }
    }

    /// `true` when both displacement (`target - current`) and velocity
    /// fall component-wise below `epsilon` — the natural rest
    /// predicate for [`Animation::is_at_rest`] / driver tick-skip.
    #[must_use]
    pub fn is_done(self, epsilon: f32) -> bool {
        self.target.sub(self.current).approx_zero(epsilon) && self.velocity.approx_zero(epsilon)
    }
}

// ────────────────────────────────────────────────────────────────────────
// R51.138 — `Animation<T>` Signal wrapper + `Tickable` trait
// ────────────────────────────────────────────────────────────────────────

use std::cell::RefCell;
use std::rc::Rc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::reactive::{Owner, Signal};

/// Sub-pixel rest threshold — an [`Animation`] is considered settled
/// when both displacement-from-target and velocity drop below this
/// component-wise. R601 §5.28 §5.7 lifts this to a non-generic
/// module-level const so consumers outside the `Animation<T>` impl
/// block can name it without specifying the carrier type. The
/// [`Animation::DEFAULT_REST_EPSILON`] associated const remains as
/// an in-impl alias for discovery; both paths return the same
/// value.
///
/// Value rationale: `0.01` matches the visual noise floor at typical
/// `HiDPI` device pixel ratios (a sub-pixel quarter-displacement
/// stops being perceptible past the panel's rendering precision)
/// and is the canonical Material 3 / `SwiftUI` spring-solver
/// settlement threshold.
pub const DEFAULT_REST_EPSILON: f32 = 0.01;

/// Object-safe surface for the [`Owner`] tick dispatch.
///
/// [`Owner::tick_animations`] walks the registry as `&dyn Tickable` so all
/// concrete `T`s share one storage type. Both methods take `&self` — the
/// implementor handles interior mutation (typically via `RefCell` /
/// `Signal::set`).
pub trait Tickable {
    /// Advance internal state by `dt` seconds. Implementations are expected
    /// to be no-ops once the underlying state has settled
    /// ([`Tickable::is_at_rest`] returns `true`).
    fn tick(&self, dt: f32);

    /// Whether further `tick` calls would meaningfully change observable
    /// state. The framework driver uses this to short-circuit settled
    /// animations and avoid re-firing every subscribed effect once per
    /// frame for the rest of the program's lifetime.
    fn is_at_rest(&self, epsilon: f32) -> bool;

    /// R629 §5.28 — object-safe form of [`Animation::settle`]: jump to
    /// the current internal target with zero velocity. After the call
    /// [`Tickable::is_at_rest`] returns `true` for any non-NaN
    /// `epsilon`. No-op if the implementation has no settable target
    /// (default: no-op).
    ///
    /// Caller does not need to know the underlying `T`; the
    /// implementation reads its own target and writes back. Enables
    /// bulk wire control (`scene/animate_settle`) over an
    /// owner's full registry without per-type dispatch.
    fn settle(&self) {}

    /// R629 §5.28 — object-safe form of [`Animation::cancel`]: freeze
    /// at the current internal value with zero velocity. After the
    /// call [`Tickable::is_at_rest`] returns `true` for any non-NaN
    /// `epsilon`. No-op if the implementation has no cancellable
    /// motion (default: no-op).
    ///
    /// Caller does not need to know the underlying `T`; the
    /// implementation reads its own current value and writes back.
    /// Enables bulk wire control (`scene/animate_cancel`) over an
    /// owner's full registry without per-type dispatch.
    fn cancel(&self) {}
}

/// Spring-animated value bound to a [`Signal<T>`](crate::reactive::Signal).
///
/// Construct with [`Animation::new`] (binds the animation to an [`Owner`]
/// for tick dispatch). Move the target with [`Animation::set_target`] —
/// the existing velocity carries through, so the value evolves
/// continuously through interruptions (the canonical `SwiftUI` /
/// `Compose` interrupt semantics).
///
/// Reads via [`Animation::value`] — or, more usefully, subscribe to the
/// underlying [`Signal`] via [`Animation::signal`]
/// so a [`Computed`](crate::reactive::Computed) /
/// [`Effect`](crate::reactive::Effect) automatically tracks frame
/// updates.
///
/// Cloning yields a shared handle — both observe the same
/// [`Signal`] and the same spring state.
///
/// # Type bounds
///
/// Mirrors [`Signal<T>`](crate::reactive::Signal): `T` must be
/// [`Animatable`] (for the solver) and `Clone + PartialEq + Serialize +
/// DeserializeOwned + 'static` (for the [`Signal`]
/// host).
pub struct Animation<T>
where
    T: Animatable + Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    inner: Rc<AnimationInner<T>>,
}

struct AnimationInner<T>
where
    T: Animatable + Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    state: RefCell<SpringState<T>>,
    config: SpringConfig,
    signal: Signal<T>,
    /// Rest threshold — component-wise displacement + velocity epsilon below
    /// which the spring is considered settled. The default
    /// ([`Animation::DEFAULT_REST_EPSILON`]) matches the visual noise floor
    /// for screen-scale animations (sub-pixel motion).
    rest_epsilon: f32,
}

impl<T> Tickable for AnimationInner<T>
where
    T: Animatable + Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    fn tick(&self, dt: f32) {
        // Snapshot under borrow_mut, step out-of-borrow, write back, then
        // call `Signal::set` — Signal's own equality check skips the
        // notification cascade when the value did not actually move.
        let stepped = {
            let state = *self.state.borrow();
            state.step(self.config, dt)
        };
        *self.state.borrow_mut() = stepped;
        self.signal.set(stepped.current);
    }

    fn is_at_rest(&self, epsilon: f32) -> bool {
        self.state.borrow().is_done(epsilon)
    }

    fn settle(&self) {
        let target = self.state.borrow().target;
        *self.state.borrow_mut() = SpringState::at_rest(target);
        self.signal.set(target);
    }

    fn cancel(&self) {
        let current = self.state.borrow().current;
        *self.state.borrow_mut() = SpringState::at_rest(current);
        self.signal.set(current);
    }
}

impl<T> Animation<T>
where
    T: Animatable + Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    /// In-impl alias for the module-level
    /// [`DEFAULT_REST_EPSILON`]
    /// const (R601 §5.28 §5.7). Kept for discoverability — when the
    /// caller already has the [`Animation`] type in scope, the
    /// associated-const path is more ergonomic than the module path.
    /// Both forms return the same `0.01` threshold.
    pub const DEFAULT_REST_EPSILON: f32 = crate::animation::DEFAULT_REST_EPSILON;

    /// Construct a spring-animated value at rest at `initial`, registered
    /// for tick dispatch on `owner`. Re-target via [`Animation::set_target`].
    ///
    /// The animation lives as long as either (a) the caller's handle or
    /// (b) `owner`'s tick registry references it. Dropping `owner` removes
    /// the registry entry so subsequent
    /// [`Owner::tick_animations`](crate::reactive::Owner::tick_animations)
    /// calls no longer step this animation — but the caller's handle can
    /// still read [`Animation::value`] for the last computed value.
    #[must_use]
    pub fn new(owner: &Owner, initial: T, config: SpringConfig) -> Self {
        let inner = Rc::new(AnimationInner {
            state: RefCell::new(SpringState::at_rest(initial)),
            config,
            signal: Signal::new(initial),
            rest_epsilon: Self::DEFAULT_REST_EPSILON,
        });
        let as_tickable: Rc<dyn Tickable> = Rc::clone(&inner) as Rc<dyn Tickable>;
        owner.register_animation(as_tickable);
        Self { inner }
    }

    /// Re-target the spring. Velocity carries through, so the animation
    /// continues smoothly across the target change — no discontinuity.
    pub fn set_target(&self, target: T) {
        self.inner.state.borrow_mut().target = target;
    }

    /// Current target value.
    #[must_use]
    pub fn target(&self) -> T {
        self.inner.state.borrow().target
    }

    /// Current displayed value — the most recently
    /// [`Signal::set`](crate::reactive::Signal::set) value, mirroring the
    /// spring's `current`. Auto-subscribes the active reactive scope (same
    /// rules as [`Signal::get`](crate::reactive::Signal::get)).
    #[must_use]
    pub fn value(&self) -> T {
        self.inner.signal.get()
    }

    /// Underlying [`Signal`]. Subscribe through
    /// this when building [`Computed`](crate::reactive::Computed) /
    /// [`Effect`](crate::reactive::Effect) chains that depend on the
    /// animated value.
    #[must_use]
    pub fn signal(&self) -> &Signal<T> {
        &self.inner.signal
    }

    /// Spring tuning (read-only).
    #[must_use]
    pub fn config(&self) -> SpringConfig {
        self.inner.config
    }

    /// Whether the spring has settled under the wrapper's epsilon
    /// (configured at construction; defaults to
    /// [`Animation::DEFAULT_REST_EPSILON`]).
    #[must_use]
    pub fn is_at_rest(&self) -> bool {
        self.inner.is_at_rest(self.inner.rest_epsilon)
    }

    /// Snapshot of the internal spring triple — exposes velocity for
    /// interrupt-aware re-targeting strategies and diagnostics. The
    /// `current` field mirrors [`Animation::value`]; the `velocity` field
    /// is the one no `Signal` snapshot can supply.
    #[must_use]
    pub fn spring_state(&self) -> SpringState<T> {
        *self.inner.state.borrow()
    }

    // ────────────────────────────────────────────────────────────────
    // R623 §5.28 — control surface
    // ────────────────────────────────────────────────────────────────

    /// R623 §5.28 — hard-reset the spring to `value` (and target) with
    /// **zero velocity**. Equivalent to `SpringState::at_rest(value)`:
    /// the spring jumps to `value`, the next [`Self::is_at_rest`] read
    /// returns `true`, and the next [`Owner::tick_animations`] step is
    /// a no-op until something writes a new [`Self::set_target`].
    ///
    /// Differs from [`Self::set_target`] in three ways:
    /// 1. `current` is overwritten (no smooth fade from the existing
    ///    position).
    /// 2. `velocity` is dropped (no carry-through from any in-flight
    ///    re-target).
    /// 3. `target` is set to the same `value` (so the spring is at
    ///    rest, not animating toward).
    ///
    /// Common use cases:
    /// - "Jump to" a known state without animation (e.g. a settings
    ///   "Reset to defaults" button that should land instantly).
    /// - Mid-animation interrupt that needs the velocity dropped (a
    ///   `set_target` mid-flight would carry the velocity through —
    ///   visually fine for most cases, but `reset` is the explicit
    ///   "stop AND jump" surface).
    /// - Test fixtures that need a deterministic starting state.
    ///
    /// Writes the wrapper's [`Signal`] so
    /// subscribers re-run on the next reactive tick (equality-skip
    /// applies if `value` already matches the current `Signal::get`).
    /// Industry analogues: Framer Motion's `set` (no-animation snap),
    /// `React Spring`'s `set`, `SwiftUI`'s `.animation(nil) { ... }`.
    pub fn reset(&self, value: T) {
        *self.inner.state.borrow_mut() = SpringState::at_rest(value);
        self.inner.signal.set(value);
    }

    /// R623 §5.28 — settle at the current `target` with zero velocity.
    /// Equivalent to `reset(target())` — the spring jumps to wherever
    /// it was heading and stops, no more steps required.
    ///
    /// Common use cases:
    /// - AI agent or user action: "skip the animation, finish now"
    ///   (e.g. a "Done" / "Skip animation" UI affordance).
    /// - Frame-budget eviction: if the application detects it is
    ///   dropping frames, fast-forwarding non-essential animations
    ///   to rest can reclaim tick-time.
    /// - Deterministic snapshot setup: assert the post-animation
    ///   state without simulating frames.
    ///
    /// Industry analogues: Framer Motion's `.stop({immediate: true})`,
    /// CSS `animation-play-state: paused` + jump-to-end pseudo.
    pub fn settle(&self) {
        <AnimationInner<T> as Tickable>::settle(&self.inner);
    }

    /// R623 §5.28 — cancel: settle at the current `value` (not the
    /// target) with zero velocity, dropping any in-flight motion.
    /// Equivalent to `reset(value())`.
    ///
    /// The key distinction from [`Self::settle`]: cancel stops at
    /// the *current visible position*, not at the target. Useful
    /// when the application wants the animation to halt visibly
    /// where it is rather than complete the transition.
    ///
    /// Common use cases:
    /// - User aborts a transition mid-flight (e.g. a draggable that
    ///   was returning to its rest pose; the user grabs it again —
    ///   cancel the return, lock in the current position as the new
    ///   rest).
    /// - Out-of-budget eviction without forcing the visual jump to
    ///   the target.
    /// - Spring re-tune mid-animation: cancel the existing motion,
    ///   then issue a fresh `set_target` with the new value.
    ///
    /// Industry analogues: Framer Motion's `.stop()` (without
    /// `immediate: true`), Web Animations API
    /// `Animation.cancel()` (different semantics — that one resets
    /// to the start; pinion's matches the "stop where you are" form
    /// from React Spring `.pause()`).
    pub fn cancel(&self) {
        <AnimationInner<T> as Tickable>::cancel(&self.inner);
    }
}

impl<T> Clone for Animation<T>
where
    T: Animatable + Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> std::fmt::Debug for Animation<T>
where
    T: Animatable + Clone + PartialEq + Serialize + DeserializeOwned + 'static + std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Animation")
            .field("state", &*self.inner.state.borrow())
            .field("config", &self.inner.config)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─────────────────────────────────────────────────────────────────
    // R601 §5.28 — DEFAULT_REST_EPSILON substrate lift
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r601_default_rest_epsilon_module_const_matches_animation_alias() {
        // The Animation<T>::DEFAULT_REST_EPSILON associated const is
        // an alias for the module-level DEFAULT_REST_EPSILON since
        // R601. Both names must resolve to the same bit pattern —
        // a future refactor that drifts the two would surface here.
        // bit-pattern equality is the right comparison for an alias
        // contract (both sides are the same compile-time literal, no
        // arithmetic).
        assert_eq!(
            DEFAULT_REST_EPSILON.to_bits(),
            Animation::<f32>::DEFAULT_REST_EPSILON.to_bits(),
            "module const must equal the in-impl alias",
        );
        assert_eq!(
            DEFAULT_REST_EPSILON.to_bits(),
            Animation::<AnimVec4>::DEFAULT_REST_EPSILON.to_bits(),
            "the alias resolves identically regardless of carrier T",
        );
    }

    #[test]
    fn r601_default_rest_epsilon_value_is_canonical_0_01() {
        // The numeric value itself is pinned by every spring-driven
        // widget cascade (theme-fade, scroll fling, caret blink) so a
        // change is a cross-cutting visual regression risk.
        assert!(
            (DEFAULT_REST_EPSILON - 0.01).abs() < f32::EPSILON,
            "DEFAULT_REST_EPSILON must equal the canonical 0.01 threshold",
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R623 §5.28 — Animation control surface (reset / settle / cancel)
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r623_reset_jumps_to_value_with_zero_velocity() {
        let owner = Owner::new();
        let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
        // Set a target so the spring would normally animate toward it.
        a.set_target(10.0);
        // Step once via tick_animations so velocity > 0.
        owner.tick_animations(0.016);
        assert!(!a.is_at_rest(), "precondition: spring should be in motion");
        // Reset to a different value.
        a.reset(5.0);
        // Spring lands at value, target = value, velocity = 0.
        assert!((a.value() - 5.0).abs() < f32::EPSILON);
        assert!((a.target() - 5.0).abs() < f32::EPSILON);
        assert!(a.is_at_rest(), "reset must drop velocity → at rest");
    }

    #[test]
    fn r623_reset_signal_writes_for_subscribers() {
        let owner = Owner::new();
        let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
        a.reset(7.0);
        // Signal::get reflects the new value.
        assert!((a.signal().get() - 7.0).abs() < f32::EPSILON);
    }

    #[test]
    fn r623_settle_jumps_to_target() {
        let owner = Owner::new();
        let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
        a.set_target(42.0);
        owner.tick_animations(0.016);
        // Mid-flight: current < target.
        let before = a.value();
        assert!(before < 42.0);
        a.settle();
        assert!((a.value() - 42.0).abs() < f32::EPSILON);
        assert!(a.is_at_rest());
    }

    #[test]
    fn r623_cancel_holds_at_current_position() {
        let owner = Owner::new();
        let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
        a.set_target(100.0);
        // Step several times so current is somewhere mid-flight.
        for _ in 0..5 {
            owner.tick_animations(0.016);
        }
        let mid = a.value();
        assert!(mid > 0.0 && mid < 100.0, "mid-flight precondition");
        a.cancel();
        // Current stays where it was; target now also equals current.
        assert!((a.value() - mid).abs() < f32::EPSILON);
        assert!((a.target() - mid).abs() < f32::EPSILON);
        assert!(a.is_at_rest());
    }

    #[test]
    fn r623_cancel_then_settle_is_idempotent() {
        // After cancel, settle should be a no-op (current == target).
        let owner = Owner::new();
        let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
        a.set_target(10.0);
        owner.tick_animations(0.016);
        a.cancel();
        let after_cancel = a.value();
        a.settle();
        assert!((a.value() - after_cancel).abs() < f32::EPSILON);
    }

    #[test]
    fn r623_reset_subsequent_tick_is_noop() {
        let owner = Owner::new();
        let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
        a.reset(5.0);
        owner.tick_animations(0.016);
        // At-rest spring + tick = no value change.
        assert!((a.value() - 5.0).abs() < f32::EPSILON);
    }

    // R629 §5.28 — Owner::settle_animations / cancel_animations
    // bulk walks land animation control over real Animation<T>
    // springs (object-safe Tickable extension). The ProgrammableRest
    // fixture in `reactive::owner::tests::animation_active` covers
    // the default no-op + count contract; the cases below pin that
    // the substrate actually flips spring state on real Animation<T>
    // registrations (no-op default would silently break theme fades
    // + every R612 axis-adjacent animation).
    #[test]
    fn r629_owner_settle_animations_lands_animation_at_target() {
        let owner = Owner::new();
        let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
        a.set_target(7.0);
        owner.tick_animations(0.016);
        assert!(a.value() < 7.0, "mid-flight precondition");
        let visited = owner.settle_animations();
        assert_eq!(visited, 1);
        assert!((a.value() - 7.0).abs() < f32::EPSILON);
        assert!(a.is_at_rest());
    }

    #[test]
    fn r629_owner_cancel_animations_freezes_animation_at_current() {
        let owner = Owner::new();
        let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
        a.set_target(100.0);
        for _ in 0..5 {
            owner.tick_animations(0.016);
        }
        let mid = a.value();
        assert!(mid > 0.0 && mid < 100.0, "mid-flight precondition");
        let visited = owner.cancel_animations();
        assert_eq!(visited, 1);
        assert!((a.value() - mid).abs() < f32::EPSILON);
        assert!((a.target() - mid).abs() < f32::EPSILON);
        assert!(a.is_at_rest());
    }

    #[test]
    fn r629_owner_settle_walk_lands_descendant_animations() {
        let parent = Owner::new();
        let child = Owner::new_child(&parent);
        let a = Animation::new(&parent, 0.0_f32, SpringConfig::DEFAULT);
        let b = Animation::new(&child, 0.0_f32, SpringConfig::DEFAULT);
        a.set_target(5.0);
        b.set_target(10.0);
        parent.tick_animations(0.016);
        // Walk from parent must land both registrations.
        let visited = parent.settle_animations();
        assert_eq!(visited, 2);
        assert!(a.is_at_rest());
        assert!(b.is_at_rest());
        assert!((a.value() - 5.0).abs() < f32::EPSILON);
        assert!((b.value() - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn r623_animvec4_carrier_supports_control_surface() {
        // Sanity: the control surface is generic over T: Animatable,
        // works on AnimVec4 the same as on f32. Pinned because
        // ThemeLinear's theme-fade animation rides on AnimVec4 and
        // a future cancel() call from theme code must compile.
        let owner = Owner::new();
        let initial = AnimVec4::new(0.0, 0.0, 0.0, 0.0);
        let target = AnimVec4::new(1.0, 2.0, 3.0, 4.0);
        let a = Animation::new(&owner, initial, SpringConfig::DEFAULT);
        a.set_target(target);
        owner.tick_animations(0.016);
        a.settle();
        let v = a.value();
        assert!((v.x - 1.0).abs() < f32::EPSILON);
        assert!((v.w - 4.0).abs() < f32::EPSILON);
        assert!(a.is_at_rest());
    }

    #[test]
    fn f32_lerp_endpoints() {
        assert!((f32::lerp(0.0, 10.0, 0.0) - 0.0).abs() < 1e-6);
        assert!((f32::lerp(0.0, 10.0, 1.0) - 10.0).abs() < 1e-6);
        assert!((f32::lerp(0.0, 10.0, 0.5) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn f32_lerp_extrapolation() {
        assert!((f32::lerp(0.0, 10.0, 2.0) - 20.0).abs() < 1e-6);
        assert!((f32::lerp(0.0, 10.0, -1.0) - -10.0).abs() < 1e-6);
    }

    #[test]
    fn anim_vec2_arithmetic() {
        let a = AnimVec2::new(1.0, 2.0);
        let b = AnimVec2::new(3.0, 4.0);
        assert_eq!(a.add(b), AnimVec2::new(4.0, 6.0));
        assert_eq!(b.sub(a), AnimVec2::new(2.0, 2.0));
        assert_eq!(a.scale(2.0), AnimVec2::new(2.0, 4.0));
        assert!(AnimVec2::zero().approx_zero(1e-6));
    }

    #[test]
    fn anim_vec4_arithmetic() {
        let a = AnimVec4::new(1.0, 2.0, 3.0, 4.0);
        let b = AnimVec4::new(5.0, 6.0, 7.0, 8.0);
        assert_eq!(a.add(b), AnimVec4::new(6.0, 8.0, 10.0, 12.0));
        assert_eq!(b.sub(a), AnimVec4::new(4.0, 4.0, 4.0, 4.0));
        assert_eq!(a.scale(0.5), AnimVec4::new(0.5, 1.0, 1.5, 2.0));
    }

    #[test]
    fn spring_config_presets_distinct() {
        assert_ne!(SpringConfig::DEFAULT, SpringConfig::GENTLE);
        assert_ne!(SpringConfig::DEFAULT, SpringConfig::STIFF);
        assert_ne!(SpringConfig::DEFAULT, SpringConfig::WOBBLY);
        assert_eq!(SpringConfig::default(), SpringConfig::DEFAULT);
    }

    #[test]
    fn spring_at_rest_is_done() {
        let s = SpringState::at_rest(0.0_f32);
        assert!(s.is_done(0.001));
    }

    #[test]
    fn spring_converges_to_target() {
        // Start at 0, target 100, integrate at 60 fps for 5 seconds.
        // DEFAULT preset settles well before that.
        let mut s = SpringState::at_rest(0.0_f32);
        s.target = 100.0;
        let dt = 1.0 / 60.0;
        for _ in 0..300 {
            s = s.step(SpringConfig::DEFAULT, dt);
        }
        assert!(
            (s.current - 100.0).abs() < 0.5,
            "expected near 100.0, got {}",
            s.current
        );
        assert!(
            s.velocity.abs() < 0.5,
            "expected near-zero velocity, got {}",
            s.velocity
        );
        assert!(s.is_done(1.0));
    }

    #[test]
    fn spring_step_is_pure() {
        // Same input → same output, no hidden state.
        let s = SpringState::<f32> {
            current: 5.0,
            velocity: 1.0,
            target: 10.0,
        };
        let a = s.step(SpringConfig::DEFAULT, 1.0 / 60.0);
        let b = s.step(SpringConfig::DEFAULT, 1.0 / 60.0);
        assert_eq!(a, b);
    }

    #[test]
    fn spring_interrupt_preserves_velocity() {
        // Drive toward target_a for a few steps, then re-target —
        // velocity must carry through.
        let mut s = SpringState::at_rest(0.0_f32);
        s.target = 100.0;
        let dt = 1.0 / 60.0;
        for _ in 0..10 {
            s = s.step(SpringConfig::DEFAULT, dt);
        }
        let velocity_before_retarget = s.velocity;
        s.target = 50.0;
        // The very next state still observes the velocity from before.
        assert!((s.velocity - velocity_before_retarget).abs() < 1e-6);
    }

    #[test]
    fn spring_vec2_converges() {
        let mut s = SpringState::at_rest(AnimVec2::new(0.0, 0.0));
        s.target = AnimVec2::new(100.0, -50.0);
        let dt = 1.0 / 60.0;
        for _ in 0..300 {
            s = s.step(SpringConfig::DEFAULT, dt);
        }
        assert!((s.current.x - 100.0).abs() < 0.5);
        assert!((s.current.y - -50.0).abs() < 0.5);
    }

    #[test]
    fn animatable_lerp_default_via_arithmetic() {
        // Verify the default Animatable::lerp on AnimVec2 matches
        // hand-computed values.
        let a = AnimVec2::new(0.0, 0.0);
        let b = AnimVec2::new(10.0, 20.0);
        let mid = <AnimVec2 as Animatable>::lerp(a, b, 0.5);
        assert_eq!(mid, AnimVec2::new(5.0, 10.0));
    }

    #[test]
    fn anim_rect_arithmetic() {
        let a = AnimRect::new(1.0, 2.0, 3.0, 4.0);
        let b = AnimRect::new(5.0, 6.0, 7.0, 8.0);
        assert_eq!(a.add(b), AnimRect::new(6.0, 8.0, 10.0, 12.0));
        assert_eq!(b.sub(a), AnimRect::new(4.0, 4.0, 4.0, 4.0));
        assert_eq!(a.scale(0.5), AnimRect::new(0.5, 1.0, 1.5, 2.0));
        assert!(AnimRect::zero().approx_zero(1e-6));
    }

    #[test]
    fn anim_rect_round_trip_small_values() {
        let r = crate::scene::Rect::new(10, 20, 100, 50);
        let lifted = AnimRect::from_rect(r);
        assert_eq!(lifted, AnimRect::new(10.0, 20.0, 100.0, 50.0));
        assert_eq!(lifted.to_rect(), r);
    }

    #[test]
    fn anim_rect_to_rect_saturates() {
        let neg = AnimRect::new(-5.0, -1.0, -100.0, -0.5);
        assert_eq!(neg.to_rect(), crate::scene::Rect::new(0, 0, 0, 0));
        let nan_x = AnimRect::new(f32::NAN, 5.0, 10.0, 20.0);
        assert_eq!(nan_x.to_rect(), crate::scene::Rect::new(0, 5, 10, 20),);
    }

    #[test]
    fn anim_rect_to_rect_rounds_half_up() {
        let r = AnimRect::new(10.4, 10.5, 10.6, 11.5);
        assert_eq!(r.to_rect(), crate::scene::Rect::new(10, 11, 11, 12));
    }

    #[test]
    fn anim_rect_converges_to_target() {
        // Resize+move a rect under spring; check all four channels
        // settle near target.
        let mut s = SpringState::at_rest(AnimRect::new(0.0, 0.0, 50.0, 30.0));
        s.target = AnimRect::new(100.0, 80.0, 200.0, 120.0);
        let dt = 1.0 / 60.0;
        for _ in 0..300 {
            s = s.step(SpringConfig::DEFAULT, dt);
        }
        assert!((s.current.x - 100.0).abs() < 0.5);
        assert!((s.current.y - 80.0).abs() < 0.5);
        assert!((s.current.w - 200.0).abs() < 0.5);
        assert!((s.current.h - 120.0).abs() < 0.5);
        assert!(s.is_done(1.0));
    }

    #[test]
    fn easing_endpoints_exact() {
        // Every variant must map 0 → 0 and 1 → 1 exactly.
        for e in [
            Easing::Linear,
            Easing::EaseInQuad,
            Easing::EaseOutQuad,
            Easing::EaseInOutQuad,
            Easing::EaseInCubic,
            Easing::EaseOutCubic,
            Easing::EaseInOutCubic,
        ] {
            assert_eq!(e.apply(0.0).to_bits(), 0.0_f32.to_bits());
            assert_eq!(e.apply(1.0).to_bits(), 1.0_f32.to_bits());
        }
    }

    #[test]
    fn easing_linear_is_identity() {
        assert!((Easing::Linear.apply(0.25) - 0.25).abs() < 1e-6);
        assert!((Easing::Linear.apply(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn easing_quad_curve_shape() {
        // EaseInQuad: t=0.5 → 0.25 (below linear)
        assert!((Easing::EaseInQuad.apply(0.5) - 0.25).abs() < 1e-6);
        // EaseOutQuad: t=0.5 → 0.75 (above linear)
        assert!((Easing::EaseOutQuad.apply(0.5) - 0.75).abs() < 1e-6);
        // EaseInOutQuad: midpoint = 0.5
        assert!((Easing::EaseInOutQuad.apply(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn easing_monotonic_on_unit_interval() {
        // All curves should be monotonically non-decreasing on
        // [0, 1] — sample 0.0, 0.1, …, 1.0 and check.
        // (precision_loss allow: i ∈ 0..=10 fits exactly in f32.)
        for e in [
            Easing::EaseInQuad,
            Easing::EaseOutQuad,
            Easing::EaseInOutQuad,
            Easing::EaseInCubic,
            Easing::EaseOutCubic,
            Easing::EaseInOutCubic,
        ] {
            let mut prev = f32::NEG_INFINITY;
            for i in 0..=10u32 {
                let t = i as f32 / 10.0;
                let v = e.apply(t);
                assert!(
                    v >= prev,
                    "easing {e:?} not monotonic at t={t}: {v} < {prev}",
                );
                prev = v;
            }
        }
    }

    #[test]
    fn tween_at_start_returns_from() {
        let t = Tween::new(0.0_f32, 100.0_f32, 1.0, Easing::Linear);
        assert!((t.current() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn tween_after_full_duration_returns_to() {
        let mut t = Tween::new(0.0_f32, 100.0_f32, 1.0, Easing::Linear);
        t.tick(1.0);
        assert!((t.current() - 100.0).abs() < 1e-6);
        assert!(t.is_done());
    }

    #[test]
    fn tween_linear_midpoint() {
        let mut t = Tween::new(0.0_f32, 100.0_f32, 1.0, Easing::Linear);
        t.tick(0.5);
        assert!((t.current() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn tween_easing_affects_midpoint() {
        let mut t = Tween::new(0.0_f32, 100.0_f32, 1.0, Easing::EaseInQuad);
        t.tick(0.5);
        // EaseInQuad at t=0.5 = 0.25 → value 25.0
        assert!((t.current() - 25.0).abs() < 1e-6);
    }

    #[test]
    fn tween_zero_duration_returns_to() {
        let t = Tween::new(0.0_f32, 50.0_f32, 0.0, Easing::Linear);
        // Saturating fallback: zero-duration tween is "instant".
        assert!((t.current() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn tween_extrapolation_clamps() {
        // Ticking past duration must not overshoot.
        let mut t = Tween::new(0.0_f32, 100.0_f32, 1.0, Easing::Linear);
        t.tick(2.0);
        assert!((t.current() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn tween_pure_function() {
        let t = Tween::new(0.0_f32, 100.0_f32, 1.0, Easing::EaseInOutCubic);
        let a = t.current();
        let b = t.current();
        assert!((a - b).abs() < 1e-6);
    }

    // ──────────────────────────────────────────────────────────────────
    // R51.138 — Animation<T> wrapper tests
    // ──────────────────────────────────────────────────────────────────

    mod animation {
        use super::super::{AnimVec2, Animation, SpringConfig};
        use crate::reactive::{Effect, Owner};
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        #[test]
        fn new_starts_at_rest_at_initial_value() {
            let owner = Owner::new();
            let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            assert!((a.value() - 0.0).abs() < 1e-6);
            assert!((a.target() - 0.0).abs() < 1e-6);
            assert!(a.is_at_rest());
        }

        #[test]
        fn set_target_advances_value_over_ticks() {
            let owner = Owner::new();
            let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            a.set_target(100.0);
            assert!((a.target() - 100.0).abs() < 1e-6);
            // Initially still at rest position; first tick begins motion.
            let dt = 1.0 / 60.0;
            for _ in 0..300 {
                owner.tick_animations(dt);
            }
            assert!((a.value() - 100.0).abs() < 0.5, "got {}", a.value());
            assert!(a.is_at_rest());
        }

        #[test]
        fn tick_via_owner_drives_all_registered_animations() {
            let owner = Owner::new();
            let first = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            let second = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            first.set_target(50.0);
            second.set_target(-50.0);
            assert_eq!(owner.registered_animation_count(), 2);
            let dt = 1.0 / 60.0;
            for _ in 0..300 {
                owner.tick_animations(dt);
            }
            assert!((first.value() - 50.0).abs() < 0.5);
            assert!((second.value() - -50.0).abs() < 0.5);
        }

        #[test]
        fn interrupt_preserves_velocity() {
            let owner = Owner::new();
            let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            a.set_target(100.0);
            let dt = 1.0 / 60.0;
            for _ in 0..10 {
                owner.tick_animations(dt);
            }
            let mid_velocity = a.spring_state().velocity;
            assert!(mid_velocity.abs() > 0.0, "expected motion before retarget");
            a.set_target(50.0);
            // Velocity carries through the retarget unchanged.
            assert!((a.spring_state().velocity - mid_velocity).abs() < 1e-6);
        }

        #[test]
        fn signal_notifies_effect_on_each_value_change() {
            let owner = Owner::new();
            let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            let observed: Rc<RefCell<Vec<f32>>> = Rc::new(RefCell::new(Vec::new()));
            let a_for_effect = a.clone();
            let observed_for_effect = Rc::clone(&observed);
            let _e = Effect::new(&owner, move || {
                observed_for_effect.borrow_mut().push(a_for_effect.value());
            });
            // Eager run captures 0.0.
            assert_eq!(*observed.borrow(), vec![0.0]);
            a.set_target(100.0);
            // Re-targeting alone does not fire the effect — only Signal::set does.
            assert_eq!(observed.borrow().len(), 1);
            owner.tick_animations(1.0 / 60.0);
            // First tick moved the value → effect re-ran exactly once.
            assert_eq!(observed.borrow().len(), 2);
        }

        #[test]
        fn tick_batches_writes_so_multi_animation_effect_fires_once_per_frame() {
            let owner = Owner::new();
            let first = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            let second = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            first.set_target(100.0);
            second.set_target(200.0);

            let runs = Rc::new(Cell::new(0_u32));
            let first_for_effect = first.clone();
            let second_for_effect = second.clone();
            let runs_for_effect = Rc::clone(&runs);
            let _e = Effect::new(&owner, move || {
                runs_for_effect.set(runs_for_effect.get() + 1);
                let _ = first_for_effect.value();
                let _ = second_for_effect.value();
            });
            assert_eq!(runs.get(), 1, "eager initial run");

            owner.tick_animations(1.0 / 60.0);
            // Both animations stepped during the same batch — the effect
            // subscribed to both should fire exactly once for the frame.
            assert_eq!(runs.get(), 2, "frame coalesces to single rerun");
        }

        #[test]
        fn equality_skipping_tick_does_not_fire_effect() {
            let owner = Owner::new();
            let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            // No target change → at rest → spring step is a no-op → Signal::set
            // hits the equality skip → effect does not re-fire.
            let runs = Rc::new(Cell::new(0_u32));
            let a_for_effect = a.clone();
            let runs_for_effect = Rc::clone(&runs);
            let _e = Effect::new(&owner, move || {
                runs_for_effect.set(runs_for_effect.get() + 1);
                let _ = a_for_effect.value();
            });
            assert_eq!(runs.get(), 1);
            for _ in 0..10 {
                owner.tick_animations(1.0 / 60.0);
            }
            assert_eq!(runs.get(), 1, "settled animation must not re-fire effect");
        }

        #[test]
        fn dropping_owner_unregisters_animation_from_tick() {
            let owner = Owner::new();
            let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            a.set_target(100.0);
            owner.tick_animations(1.0 / 60.0);
            let after_one_tick = a.value();
            assert!(after_one_tick > 0.0, "animation moved during first tick");
            drop(owner);
            // The caller's handle survives, but no driver is calling tick.
            let still_same = a.value();
            assert!(
                (still_same - after_one_tick).abs() < 1e-6,
                "value frozen after owner drop"
            );
        }

        // Hand-rolled Tickable witness used by the depth-first ordering test
        // below — `Animation<T>` itself is order-agnostic so we use a probe
        // instead. Defined at module scope so `items_after_statements`
        // (clippy::pedantic) does not fire on a function-local item.
        use super::super::Tickable;
        struct Probe(&'static str, Rc<RefCell<Vec<&'static str>>>);
        impl Tickable for Probe {
            fn tick(&self, _dt: f32) {
                self.1.borrow_mut().push(self.0);
            }
            fn is_at_rest(&self, _epsilon: f32) -> bool {
                true
            }
        }

        #[test]
        fn child_owner_animations_tick_before_parent_animations() {
            let parent = Owner::new();
            let child = Owner::new_child(&parent);
            let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
            parent.register_animation(Rc::new(Probe("parent", Rc::clone(&log))));
            child.register_animation(Rc::new(Probe("child", Rc::clone(&log))));
            parent.tick_animations(1.0 / 60.0);
            assert_eq!(*log.borrow(), vec!["child", "parent"]);
        }

        #[test]
        fn anim_vec2_animation_converges() {
            let owner = Owner::new();
            let a = Animation::new(&owner, AnimVec2::new(0.0, 0.0), SpringConfig::DEFAULT);
            a.set_target(AnimVec2::new(50.0, -30.0));
            let dt = 1.0 / 60.0;
            for _ in 0..300 {
                owner.tick_animations(dt);
            }
            let v = a.value();
            assert!((v.x - 50.0).abs() < 0.5);
            assert!((v.y - -30.0).abs() < 0.5);
            assert!(a.is_at_rest());
        }

        #[test]
        fn clone_handles_share_state() {
            let owner = Owner::new();
            let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            let alias = a.clone();
            a.set_target(100.0);
            assert!((alias.target() - 100.0).abs() < 1e-6);
            owner.tick_animations(1.0 / 60.0);
            assert!((alias.value() - a.value()).abs() < 1e-6);
        }

        #[test]
        fn registry_count_grows_with_construction() {
            let owner = Owner::new();
            assert_eq!(owner.registered_animation_count(), 0);
            let _a1 = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
            assert_eq!(owner.registered_animation_count(), 1);
            let _a2 = Animation::new(&owner, 0.0_f32, SpringConfig::GENTLE);
            assert_eq!(owner.registered_animation_count(), 2);
        }

        #[test]
        fn config_accessor_returns_construction_value() {
            let owner = Owner::new();
            let a = Animation::new(&owner, 0.0_f32, SpringConfig::WOBBLY);
            assert_eq!(a.config(), SpringConfig::WOBBLY);
        }
    }
}
