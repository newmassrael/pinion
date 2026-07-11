//! End-to-end proof that 3D positioning changes the rendered stereo output,
//! captured headlessly through the [`InMemoryAudioBackend`] seam. Unlike the
//! `spatial` unit tests (which check the geometry in isolation), these drive
//! a real engine → mixer → backend pump and assert on the captured samples.

use std::sync::Arc;

use pinion_audio::{AudioClip, AudioEngine, InMemoryAudioBackend, Listener, PlayOptions, pump};

fn tone() -> Arc<AudioClip> {
    // Full-scale mono so channel energy is easy to read.
    AudioClip::new(48_000, 1, vec![1.0; 256]).shared()
}

/// Summed squared energy of the (L, R) legs of an interleaved stereo buffer.
fn channel_energy(stereo: &[f32]) -> (f32, f32) {
    stereo
        .chunks_exact(2)
        .fold((0.0, 0.0), |(l, r), f| (l + f[0] * f[0], r + f[1] * f[1]))
}

fn render_positioned(position: [f32; 3]) -> (f32, f32) {
    let mut engine = AudioEngine::new(48_000);
    engine.play(
        tone(),
        "src",
        PlayOptions::one_shot().with_position(position),
    );
    let mut backend = InMemoryAudioBackend::new(48_000);
    pump(&mut engine, &mut backend, 256);
    channel_energy(backend.captured())
}

#[test]
fn left_positioned_voice_dominates_the_left_channel() {
    // Default listener at the origin facing -Z; emitter to the world-left.
    let (l, r) = render_positioned([-1.0, 0.0, 0.0]);
    assert!(
        l > r * 4.0,
        "left-positioned source should dominate the left channel (l={l}, r={r})"
    );
}

#[test]
fn right_positioned_voice_dominates_the_right_channel() {
    let (l, r) = render_positioned([1.0, 0.0, 0.0]);
    assert!(
        r > l * 4.0,
        "right-positioned source should dominate the right channel (l={l}, r={r})"
    );
}

#[test]
fn distant_voice_is_strongly_attenuated() {
    let peak_at = |dist: f32| {
        let mut engine = AudioEngine::new(48_000);
        engine.play(
            tone(),
            "src",
            PlayOptions::one_shot().with_position([0.0, 0.0, -dist]),
        );
        let mut backend = InMemoryAudioBackend::new(48_000);
        pump(&mut engine, &mut backend, 256);
        backend.peak()
    };
    let near = peak_at(1.0);
    let far = peak_at(100.0);
    assert!(near > 0.5, "near source audible (near={near})");
    assert!(
        far < near * 0.1,
        "far source strongly attenuated (near={near}, far={far})"
    );
}

#[test]
fn rotating_the_listener_flips_the_stereo_field() {
    // Emitter fixed at +X. Facing -Z the listener hears it on the right;
    // rotate to face +Z and the same emitter flips to the left.
    let render = |listener: Listener| {
        let mut engine = AudioEngine::new(48_000);
        engine.set_listener(listener);
        engine.play(
            tone(),
            "src",
            PlayOptions::one_shot().with_position([1.0, 0.0, 0.0]),
        );
        let mut backend = InMemoryAudioBackend::new(48_000);
        pump(&mut engine, &mut backend, 256);
        channel_energy(backend.captured())
    };
    let (l0, r0) = render(Listener::default());
    let (l1, r1) = render(Listener::new(
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0],
    ));
    assert!(r0 > l0, "facing -Z: +X emitter is on the right");
    assert!(l1 > r1, "facing +Z: +X emitter flips to the left");
}
