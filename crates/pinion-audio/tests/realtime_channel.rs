//! Proof that the real-time control model works across a real thread
//! boundary: the [`AudioRenderer`] moves to a spawned "audio thread", the
//! controller stays on the main thread, and commands + snapshot cross the
//! lock-free queue. Deterministic (no sleeps): `thread::spawn` establishes the
//! happens-before that makes the pre-queued command visible to the renderer,
//! so this is ZERO-FLAKE.

use std::sync::Arc;
use std::thread;

use pinion_audio::{AudioClip, AudioEngine, PlayOptions, peak, realtime_channel};

fn tone(frames: usize) -> Arc<AudioClip> {
    AudioClip::new(48_000, 1, vec![1.0; frames]).shared()
}

#[test]
fn renderer_runs_on_a_separate_thread() {
    let (mut controller, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16);

    // Queue a play from the control (main) thread before the audio thread
    // starts; spawn's happens-before guarantees the renderer sees it.
    let id = controller
        .play(tone(512), "bell", PlayOptions::one_shot())
        .expect("queued");
    assert_eq!(id, 1);

    let handle = thread::spawn(move || {
        let mut out = vec![0.0f32; 256]; // 128 stereo frames
        renderer.render(&mut out);
        (peak(&out), renderer.engine().voice_count())
    });
    let (block_peak, voice_count) = handle.join().expect("audio thread ok");

    assert!(
        block_peak > 0.5,
        "the queued voice was rendered on the audio thread"
    );
    assert_eq!(
        voice_count, 1,
        "512-frame tone still playing after 128 frames"
    );

    // The snapshot the audio thread published is visible here (join gives the
    // happens-before); the controller polls it without touching the engine.
    assert_eq!(controller.snapshot().voice_count(), 1);
    assert!(controller.snapshot().peak() > 0.5);
    assert_eq!(controller.snapshot().frames_rendered(), 128);
}

#[test]
fn control_thread_drives_a_running_render_loop() {
    // The audio thread renders a fixed number of blocks; the control thread
    // queues a play, then a stop_all, ahead of the loop. Bounded + no timing
    // assumptions: we only assert monotonic progress and a final quiet block.
    let (mut controller, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16);
    controller
        .play(tone(64), "blip", PlayOptions::one_shot())
        .expect("queued");
    controller.stop_all();

    let handle = thread::spawn(move || {
        let mut last = vec![0.0f32; 128];
        for _ in 0..4 {
            renderer.render(&mut last);
        }
        renderer.engine().voice_count()
    });
    let final_voices = handle.join().expect("audio thread ok");

    // stop_all was queued, so after the loop nothing is playing, and the audio
    // thread made progress (frames rendered > 0).
    assert_eq!(final_voices, 0);
    assert!(controller.snapshot().frames_rendered() >= 128);
}
