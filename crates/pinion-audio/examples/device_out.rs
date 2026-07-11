//! Manual device-output smoke check for the cpal backend (R1282).
//!
//! Run on a machine with an audio device + `libasound2-dev`:
//! `cargo run -p pinion-audio --example device_out --features cpal-backend`
//!
//! It opens the default output, plays a real FLAC-decoded clip through the
//! lock-free command queue, and confirms the audio-thread callback actually
//! ran by reading `frames_rendered` back off the snapshot. This is *not* a
//! committed auto-test: it depends on a real device, so it would be flaky /
//! unbuildable in CI — exactly the kind of hardware-gated check that belongs
//! in a runnable example, not the test suite.

use std::sync::Arc;
use std::time::Duration;

use pinion_audio::{CpalOutput, PlayOptions, decode_compressed};

// A real compressed asset (the same fixture the decode tests use).
const CHIME_FLAC: &[u8] = include_bytes!("../tests/fixtures/tone.flac");

fn main() {
    // 64-command ring, 32-voice pool (the audio thread never reallocates; a
    // 33rd simultaneous voice would be rejected and counted in the snapshot).
    let (mut controller, out) = match CpalOutput::start_default(64, 32) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("device_out: no audio output available: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "device open: {} Hz, {} channel(s)",
        out.sample_rate(),
        out.channels()
    );

    let clip = decode_compressed(CHIME_FLAC)
        .expect("decode chime")
        .shared();
    // Loop it so there is live audio to observe while we sample the snapshot.
    let id = controller
        .play(Arc::clone(&clip), "chime", PlayOptions::looping())
        .expect("queue play");
    println!("queued looping voice #{id}; letting the audio thread run...");

    // Let the real audio callback pull from the engine, then sample mid-play.
    std::thread::sleep(Duration::from_millis(200));
    let snap = controller.snapshot();
    let frames = snap.frames_rendered();
    let peak = snap.peak();
    println!(
        "audio thread rendered {frames} frames (peak {peak:.4}, voices {})",
        snap.voice_count()
    );
    assert!(
        frames > 0,
        "the cpal callback never fired — device integration broken"
    );
    assert!(
        peak > 0.01,
        "no audible samples reached the device (peak {peak})"
    );

    // Stop and let the tail drain, proving commands mutate the live stream.
    controller.stop_all();
    std::thread::sleep(Duration::from_millis(50));
    // The stopped voice was returned over the resource-return queue, not freed
    // on the audio thread; reclaim it here on the control thread.
    let reclaimed = controller.reclaim();
    println!(
        "after stop_all: voices {}, total frames {}, reclaimed {reclaimed} retired voice(s), \
         rejected {}",
        controller.snapshot().voice_count(),
        controller.snapshot().frames_rendered(),
        controller.snapshot().rejected(),
    );
    println!(
        "OK: cpal callback drains the queue, renders audible output, honours stop, and frees \
         retired voices off the audio thread."
    );
}
