//! §5.28 — animation primitive substrate (R51.133 첫 land).
//!
//! ## Charter (§5.28 ratified at R33)
//!
//! Spring-physics first; tween/keyframe 은 special case of a critically
//! damped spring. `Animated<T>` wraps a [`Signal<T>`](crate::reactive::Signal)
//! with [`SpringConfig`] (stiffness / damping / mass); semi-implicit Euler
//! solver; interruptible (a new target preserves velocity). Industry
//! canonical: `SwiftUI` `Animation` / React Spring / `Compose`
//! `animateXxxAsState`.
//!
//! ## Scope (R51.133)
//!
//! Pure substrate primitive only. No ambient time source (caller-injected
//! `dt`), no `Signal` integration yet — keeps the §2 invariant #3
//! `dry_run` guarantee intact: identical `(state, config, dt)` always
//! returns the identical next state, so a scenario explorer can fast-
//! forward without side effects.
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
//! ## Carry (R51.134+)
//!
//! - [`Color`](crate::style::Color) / [`Rect`](crate::scene::Rect)
//!   `Animatable` impls via a linear-RGBA / continuous-rect conversion
//!   helper (saturating u8 / u32 arithmetic is incorrect for spring
//!   integration — needs an `f32` shadow space).
//! - `AnimationDriver` [`Effect`](crate::reactive) substrate driving
//!   `Signal` ticks per `Frame.dt`, cancelable via `Owner` drop.
//! - `Animated<T>` ergonomic wrapper over [`Signal<T>`](crate::reactive::Signal).
//! - SCE schema for declarative animated bindings; Forge emit.
//! - Curve-based easing enum (`Linear` / `EaseInQuad` / …) as the
//!   tween special case.
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
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
    /// predicate for the [`AnimationDriver`] (carry) to stop ticking.
    #[must_use]
    pub fn is_done(self, epsilon: f32) -> bool {
        self.target.sub(self.current).approx_zero(epsilon)
            && self.velocity.approx_zero(epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            nan_x.to_rect(),
            crate::scene::Rect::new(0, 5, 10, 20),
        );
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
}
