//! Proof that the real-time control model works across a real thread
//! boundary: the [`AudioRenderer`] moves to a spawned "audio thread", the
//! controller stays on the main thread, and commands + snapshot cross the
//! lock-free queue. Deterministic (no sleeps): `thread::spawn` establishes the
//! happens-before that makes the pre-queued command visible to the renderer,
//! so this is ZERO-FLAKE.

use std::sync::Arc;
use std::thread;

use pinion_audio::{AudioClip, AudioEngine, Listener, PlayOptions, peak, realtime_channel};

fn tone(frames: usize) -> Arc<AudioClip> {
    AudioClip::new(48_000, 1, vec![1.0; frames]).shared()
}

#[test]
fn renderer_runs_on_a_separate_thread() {
    let (mut controller, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 16);

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
    let (mut controller, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 16);
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

#[test]
fn control_thread_drives_the_listener_across_the_boundary() {
    // Two identical spatial plays; one channel also gets a control-thread
    // listener move onto the source before the audio thread renders. The moved
    // channel renders at full gain, the unmoved at the attenuated 0.25 — proof
    // the SetListener command crossed the lock-free queue and changed the mix.
    // Deterministic: everything is queued before spawn, so spawn's
    // happens-before makes it visible to the renderer (ZERO-FLAKE, no sleeps).
    fn render_peak(move_listener: bool) -> (f32, Listener) {
        let (mut controller, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 8);
        // A voice 4 units to the world-right → default listener gives gain
        // 1/(1+3) = 0.25.
        let opts = PlayOptions {
            position: Some([4.0, 0.0, 0.0]),
            ..PlayOptions::one_shot()
        };
        controller.play(tone(512), "src", opts).expect("queued");
        if move_listener {
            // Move the ears onto the source → distance 1 → full gain.
            controller.set_listener(Listener::new(
                [3.0, 0.0, 0.0],
                [0.0, 0.0, -1.0],
                [0.0, 1.0, 0.0],
            ));
        }
        let peak_of = thread::spawn(move || {
            let mut out = vec![0.0f32; 256];
            renderer.render(&mut out);
            peak(&out)
        })
        .join()
        .expect("audio thread ok");
        (peak_of, controller.listener())
    }

    let (moved_peak, moved_listener) = render_peak(true);
    let (still_peak, still_listener) = render_peak(false);

    assert!(
        (still_peak - 0.25).abs() < 1e-2,
        "unmoved: attenuated at distance 4: {still_peak}"
    );
    assert!(
        moved_peak > 0.9,
        "moved: full gain on top of the source: {moved_peak}"
    );
    assert!(
        moved_peak > still_peak + 0.5,
        "the control-thread listener drive changed the audio-thread mix"
    );
    // The control-thread mirror reflects the move (and the origin seed).
    // Component-wise (`clippy::float_cmp` forbids `==` on float arrays).
    let approx3 = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
    assert!(approx3(moved_listener.position, [3.0, 0.0, 0.0]));
    assert!(approx3(still_listener.position, [0.0, 0.0, 0.0]));
}

#[test]
fn per_voice_snapshot_crosses_the_thread_boundary() {
    // The audio thread publishes the per-voice slots; the control thread reads
    // them back and joins the label — all across the real thread boundary.
    // Deterministic: the play is queued before spawn (spawn's happens-before),
    // and the join establishes the happens-before for reading the published
    // slots (ZERO-FLAKE, no sleeps).
    let (mut controller, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 8);
    let opts = PlayOptions {
        position: Some([4.0, 0.0, 0.0]),
        ..PlayOptions::one_shot()
    };
    controller.play(tone(512), "src", opts).expect("queued");

    let renderer = thread::spawn(move || {
        let mut out = vec![0.0f32; 256];
        renderer.render(&mut out);
        renderer // moved back so the engine + slots stay alive
    })
    .join()
    .expect("audio thread ok");
    assert_eq!(renderer.engine().voice_count(), 1);

    // The slots the audio thread published are visible here; the label joins
    // from the control-thread map.
    let voices = controller.snapshot().voices();
    assert_eq!(voices.len(), 1);
    assert_eq!(controller.label_of(voices[0].id), Some("src"));
    assert!((voices[0].distance.expect("3D voice") - 4.0).abs() < 1e-3);
    assert!(
        (voices[0].effective_gain - 0.25).abs() < 1e-3,
        "effective gain at distance 4"
    );
}

#[test]
fn retired_voices_cross_the_return_queue_and_are_freed_on_the_control_thread() {
    // The audio thread must never free a clip. A one-shot that finishes on the
    // spawned "audio thread" is shipped back over the resource-return queue;
    // the control (main) thread frees it via reclaim(). Deterministic: the
    // spawn/join happens-before makes the returned voice visible here.
    let (mut controller, mut renderer) = realtime_channel(AudioEngine::new(48_000), 16, 16);
    let clip = tone(2); // finishes within one 128-frame render
    controller
        .play(Arc::clone(&clip), "blip", PlayOptions::one_shot())
        .expect("queued");
    assert_eq!(Arc::strong_count(&clip), 2);

    let renderer = thread::spawn(move || {
        let mut out = vec![0.0f32; 256];
        renderer.render(&mut out); // reaps the finished voice into the return queue
        renderer // moved back so the returned voice stays alive in its ring
    })
    .join()
    .expect("audio thread ok");

    // The audio thread rendered and reaped, but did NOT free the clip.
    assert_eq!(renderer.engine().voice_count(), 0, "one-shot reaped");
    assert_eq!(
        Arc::strong_count(&clip),
        2,
        "clip returned over the queue, not freed on the audio thread"
    );

    // The control thread reclaims and frees it.
    assert_eq!(controller.reclaim(), 1);
    assert_eq!(
        Arc::strong_count(&clip),
        1,
        "reclaim freed the returned voice on the control thread"
    );
}
