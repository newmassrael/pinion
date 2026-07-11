//! Headless verification of the compressed decoder against real, committed
//! fixtures (generated once with ffmpeg: a 440 Hz mono sine, 48 kHz, 0.1 s,
//! at the ffmpeg `sine` default level of about -18 dBFS).
//!
//! The strong assertion is a **cross-decoder parity oracle**: FLAC is
//! lossless, so `decode_compressed(tone.flac)` must reconstruct the exact PCM
//! our own dependency-free `decode_wav(tone.wav)` produces. OGG Vorbis is
//! lossy, so it gets structural + energy assertions (right rate/channels,
//! comparable length, and the same peak level as the source within a lossy
//! band) rather than sample equality.

use pinion_audio::{decode_compressed, decode_wav};

const TONE_WAV: &[u8] = include_bytes!("fixtures/tone.wav");
const TONE_FLAC: &[u8] = include_bytes!("fixtures/tone.flac");
const TONE_OGG: &[u8] = include_bytes!("fixtures/tone.ogg");

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()))
}

#[test]
fn flac_decodes_bit_identical_to_wav() {
    let wav = decode_wav(TONE_WAV).expect("wav decodes");
    let flac = decode_compressed(TONE_FLAC).expect("flac decodes");

    assert_eq!(flac.sample_rate(), wav.sample_rate(), "same rate");
    assert_eq!(flac.channels(), wav.channels(), "same channels");
    assert_eq!(
        flac.frame_count(),
        wav.frame_count(),
        "lossless: identical frame count"
    );

    // FLAC is lossless — every sample must match the WAV within one 16-bit
    // quantisation step (1/32768). This is the parity oracle: two independent
    // decoders (our WAV parser, symphonia's FLAC codec) agree exactly.
    let (ws, fs) = (wav.samples(), flac.samples());
    let max_delta = ws
        .iter()
        .zip(fs)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(
        max_delta <= 1.0 / 32768.0 + 1e-6,
        "FLAC != WAV: max sample delta {max_delta} exceeds one LSB"
    );
}

#[test]
fn ogg_decodes_to_matching_shape() {
    let wav = decode_wav(TONE_WAV).expect("wav decodes");
    let ogg = decode_compressed(TONE_OGG).expect("ogg decodes");

    assert_eq!(ogg.sample_rate(), 48_000, "rate preserved");
    assert_eq!(ogg.channels(), 1, "mono preserved");
    // Vorbis carries encoder delay/padding, so the length is close but not
    // exact — within a 20 ms band of the source.
    let slack = 48_000 / 50;
    let (a, b) = (ogg.frame_count(), wav.frame_count());
    assert!(
        a.abs_diff(b) <= slack,
        "ogg length {a} not within {slack} of wav length {b}"
    );

    // Lossy, but the tone's level must survive: the OGG peak matches the WAV
    // source peak within a generous lossy band (both ~0.125, i.e. -18 dBFS).
    let (po, pw) = (peak(ogg.samples()), peak(wav.samples()));
    assert!(pw > 0.1, "wav source should be audible, peak was {pw}");
    assert!(
        (po - pw).abs() < 0.03,
        "ogg peak {po} should track wav peak {pw} within the lossy band"
    );
}

#[test]
fn garbage_is_rejected_not_panicked() {
    // Non-audio bytes fail cleanly (no panic).
    assert!(decode_compressed(b"this is not audio at all").is_err());
    assert!(decode_compressed(&[]).is_err());
    // A truncated FLAC header (magic only) is corrupt, not decodable.
    assert!(decode_compressed(b"fLaC").is_err());
}

#[test]
fn format_is_detected_from_content_not_a_hint() {
    // The decoder sniffs the container, so both fixtures decode through the
    // one entry point with no per-format routing.
    assert!(decode_compressed(TONE_FLAC).is_ok(), "FLAC detected");
    assert!(decode_compressed(TONE_OGG).is_ok(), "OGG detected");
}

#[test]
fn truncated_stream_is_rejected_not_silently_clipped() {
    // A stream cut short mid-decode (valid header, missing trailing audio) is
    // the common "asset copied/downloaded incompletely" case. It must error,
    // not return a silently-shortened clip. The full FLAC decodes to a known
    // frame count; a prefix that still probes must not come back at full
    // length claiming success.
    let full = decode_compressed(TONE_FLAC).expect("full flac ok");
    let cut = &TONE_FLAC[..TONE_FLAC.len() * 3 / 4];
    match decode_compressed(cut) {
        Err(_) => {} // rejected — the contract holds.
        Ok(clip) => panic!(
            "truncated FLAC returned Ok with {} of {} frames (silent data loss)",
            clip.frame_count(),
            full.frame_count()
        ),
    }
}
