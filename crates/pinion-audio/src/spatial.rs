//! 3D spatialisation — turning an emitter position + a listener into the
//! gain and pan a voice is heard with (§5.54, R1274).
//!
//! This is the deterministic geometry half of positional audio: given where a
//! sound is in the world and where the listener's ears are, it derives a
//! **distance attenuation** (how loud) and an **azimuth pan** (how far left /
//! right), which the mixer then applies through the exact same constant-power
//! pan pot a hand-panned voice uses. No HRTF, no reverb, no doppler — those
//! are refinements on this same seam; the substance here is the listener-
//! relative geometry, and like the whole mixer it is pure and RNG-free so a
//! headless render and a live device agree sample-for-sample (§2 #3).
//!
//! Positions are plain `[f32; 3]` ([`Vec3`]) in a right-handed metric space.
//! A dedicated engine math type (shared with the Phase-C scene graph /
//! physics) does not exist yet; when it does this module adopts it rather
//! than the reverse — inventing the engine's `Vec3` inside the audio crate
//! before that crate exists would be the premature-abstraction mistake.

use serde::Serialize;

/// A point or direction in the engine's right-handed 3D space (metres).
pub type Vec3 = [f32; 3];

/// Distances below this (metres) are treated as "at the listener": no
/// meaningful direction, so the pan collapses to centre.
const NEAR_EPSILON: f32 = 1e-4;

/// The listener — the ears the mix is rendered for.
///
/// `forward` and `up` are unit vectors defining the listener's orientation;
/// their cross product is the listener's "right", the axis azimuth pan is
/// measured along. The default faces `-Z` with `+Y` up (the common
/// right-handed convention), so an emitter at `+X` is heard on the right.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Listener {
    /// World position of the listener.
    pub position: Vec3,
    /// Unit facing direction.
    pub forward: Vec3,
    /// Unit up direction.
    pub up: Vec3,
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
        }
    }
}

impl Listener {
    /// A listener at `position` facing `forward` with `up`. The two direction
    /// vectors are normalised defensively; a degenerate (zero-length) vector
    /// falls back to the default axis so `right` is always well defined.
    #[must_use]
    pub fn new(position: Vec3, forward: Vec3, up: Vec3) -> Self {
        Self {
            position,
            forward: normalize_or(forward, [0.0, 0.0, -1.0]),
            up: normalize_or(up, [0.0, 1.0, 0.0]),
        }
    }

    /// The listener's right axis, `forward × up`, normalised. If `forward`
    /// and `up` are parallel (a degenerate orientation the caller should not
    /// supply), the cross product vanishes and this falls back to world `+X` —
    /// well defined, but no longer orientation-aware; pass a non-parallel
    /// `forward`/`up` for a meaningful azimuth.
    #[must_use]
    pub fn right(&self) -> Vec3 {
        normalize_or(cross(self.forward, self.up), [1.0, 0.0, 0.0])
    }
}

/// Distance-attenuation parameters — the OpenAL inverse-distance-clamped
/// model, the game-audio default.
///
/// `gain = reference_distance / (reference_distance + rolloff * (d -
/// reference_distance))`, with `d` clamped to
/// `[reference_distance, max_distance]`. So at (or inside) the reference
/// distance the gain is `1.0`; at twice it with unit rolloff, `0.5`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Attenuation {
    /// Distance at and within which the sound is at full gain.
    pub reference_distance: f32,
    /// Distance past which no further attenuation is applied.
    pub max_distance: f32,
    /// How steeply gain falls between reference and max (0 = no rolloff).
    pub rolloff: f32,
}

impl Default for Attenuation {
    fn default() -> Self {
        Self {
            reference_distance: 1.0,
            max_distance: f32::MAX,
            rolloff: 1.0,
        }
    }
}

impl Attenuation {
    /// The gain multiplier for a source `distance` metres away, in `[0, 1]`.
    #[must_use]
    pub fn gain(&self, distance: f32) -> f32 {
        // A non-finite distance (a NaN/inf emitter position from the in-process
        // API) is silenced rather than let a NaN through to the mix, where the
        // `f32::max` peak probe would hide it and a real device would get it.
        if !distance.is_finite() {
            return 0.0;
        }
        // A non-positive reference distance is meaningless; treat it as the
        // smallest positive distance so the formula stays finite.
        let reference = self.reference_distance.max(NEAR_EPSILON);
        let clamped = distance.clamp(reference, self.max_distance.max(reference));
        let denom = reference + self.rolloff.max(0.0) * (clamped - reference);
        if denom <= 0.0 {
            1.0
        } else {
            (reference / denom).clamp(0.0, 1.0)
        }
    }
}

/// The resolved spatialisation of one emitter: how far, how loud, how panned.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Spatialization {
    /// Distance from the listener (metres).
    pub distance: f32,
    /// Distance-attenuation gain multiplier, `[0, 1]`.
    pub gain: f32,
    /// Derived stereo pan, `-1.0` left … `1.0` right.
    pub pan: f32,
}

/// Resolve an emitter at `emitter` heard by `listener` under `attenuation`.
#[must_use]
pub fn spatialize(listener: &Listener, emitter: Vec3, attenuation: &Attenuation) -> Spatialization {
    let to = sub(emitter, listener.position);
    let distance = length(to);
    let gain = attenuation.gain(distance);
    // Azimuth: project the (unit) direction to the emitter onto the listener's
    // right axis. In front → 0 (right ⊥ forward); to the right → +1; behind
    // → 0 (front/back is ambiguous for a stereo pair, by design).
    let pan = if distance > NEAR_EPSILON {
        dot(listener.right(), normalize_or(to, [0.0, 0.0, 0.0])).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    Spatialization {
        distance,
        gain,
        pan,
    }
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(v: Vec3) -> f32 {
    dot(v, v).sqrt()
}

/// Normalise `v`, or return `fallback` if `v` is (near) zero-length.
fn normalize_or(v: Vec3, fallback: Vec3) -> Vec3 {
    let len = length(v);
    if len > NEAR_EPSILON {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn default_listener_hears_plus_x_on_the_right() {
        let l = Listener::default();
        // Emitter to the world-right (+X) at the reference distance.
        let s = spatialize(&l, [1.0, 0.0, 0.0], &Attenuation::default());
        assert!(approx(s.pan, 1.0), "pan {} should be hard right", s.pan);
        assert!(approx(s.gain, 1.0), "at reference distance → full gain");
        assert!(approx(s.distance, 1.0));
    }

    #[test]
    fn minus_x_is_left_and_front_is_centre() {
        let l = Listener::default();
        let a = Attenuation::default();
        assert!(approx(spatialize(&l, [-1.0, 0.0, 0.0], &a).pan, -1.0));
        // Straight ahead (-Z) is dead centre.
        assert!(approx(spatialize(&l, [0.0, 0.0, -1.0], &a).pan, 0.0));
        // Directly behind (+Z) is also centre (front/back ambiguity).
        assert!(approx(spatialize(&l, [0.0, 0.0, 1.0], &a).pan, 0.0));
    }

    #[test]
    fn distance_halves_gain_at_double_reference() {
        let a = Attenuation::default(); // reference 1, rolloff 1
        assert!(approx(a.gain(1.0), 1.0));
        assert!(approx(a.gain(2.0), 0.5));
        assert!(approx(a.gain(3.0), 1.0 / 3.0));
        // Closer than reference is not boosted above full gain.
        assert!(approx(a.gain(0.25), 1.0));
    }

    #[test]
    fn max_distance_floors_the_attenuation() {
        let a = Attenuation {
            reference_distance: 1.0,
            max_distance: 10.0,
            rolloff: 1.0,
        };
        // Past max_distance the gain stops falling (clamped at the max value).
        assert!(approx(a.gain(100.0), a.gain(10.0)));
    }

    #[test]
    fn rotating_the_listener_swaps_the_sides() {
        // Face +Z instead of -Z: now +X is on the listener's *left*.
        let l = Listener::new([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
        assert!(approx(
            spatialize(&l, [1.0, 0.0, 0.0], &Attenuation::default()).pan,
            -1.0
        ));
    }

    #[test]
    fn emitter_at_the_listener_is_centred() {
        let l = Listener::default();
        let s = spatialize(&l, [0.0, 0.0, 0.0], &Attenuation::default());
        assert!(approx(s.pan, 0.0), "no direction → centre");
        assert!(approx(s.gain, 1.0), "at listener → full gain");
    }

    #[test]
    fn non_finite_distance_is_silenced_not_a_nan() {
        // A NaN/inf emitter position (only reachable from the in-process API)
        // must never leak a NaN into the mix — it is silenced.
        let a = Attenuation::default();
        assert!(a.gain(f32::NAN) < 1e-9, "NaN distance → silence");
        assert!(a.gain(f32::INFINITY) < 1e-9, "inf distance → silence");
        let l = Listener::default();
        let s = spatialize(&l, [f32::NAN, 0.0, 0.0], &a);
        assert!(s.gain.is_finite() && s.gain < 1e-9, "NaN position → 0 gain");
        assert!(s.pan.is_finite(), "pan stays finite");
    }
}
