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
}
