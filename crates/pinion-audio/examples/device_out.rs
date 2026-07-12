//! Manual device-output smoke check for the cpal backend (R1282, R1309).
//!
//! Run on a machine with an audio device + `libasound2-dev`:
//! `cargo run -p pinion-audio --example device_out --features cpal-backend`
//!
//! It opens an output, plays a real FLAC-decoded clip through the lock-free
//! command queue, and confirms the audio-thread callback actually ran by
//! reading `frames_rendered` back off the snapshot. This is *not* a committed
//! auto-test: it depends on a real device, so it would be flaky / unbuildable
//! in CI — exactly the kind of hardware-gated check that belongs in a runnable
//! example, not the test suite.
//!
//! With no argument it opens the **host default** — which is audible, so it
//! plays out of the real speakers. Pass a device name (R1309) to route it
//! elsewhere; with no name it also prints the devices it could have used:
//!
//! ```text
//! cargo run -p pinion-audio --example device_out --features cpal-backend -- "hw:CARD=Dummy,DEV=0"
//! ```
//!
//! An ALSA `snd-dummy` card (`sudo modprobe snd-dummy`) is a *silent* device
//! with a real timer-paced callback — the same trick the `hello-audio-device`
//! wire demo uses to run the real device path without making a sound.

use std::sync::Arc;
use std::time::Duration;

use pinion_audio::{CpalOutput, PlayOptions, decode_compressed};

// A real compressed asset (the same fixture the decode tests use).
const CHIME_FLAC: &[u8] = include_bytes!("../tests/fixtures/tone.flac");

fn main() {
    let requested = std::env::args().nth(1);

    match CpalOutput::output_device_names() {
        Ok(names) => println!("output devices: {names:?}"),
        Err(e) => eprintln!("device_out: could not enumerate devices: {e}"),
    }

    // 64-command ring, 32-voice pool (the audio thread never reallocates; a
    // 33rd simultaneous voice would be rejected and counted in the snapshot).
    let opened = match &requested {
        Some(name) => CpalOutput::start_on(name, 64, 32),
        None => CpalOutput::start_default(64, 32),
    };
    let (mut controller, out) = match opened {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("device_out: no audio output available: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "device open: {:?} — {} Hz, {} channel(s)",
        out.device_name(),
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
