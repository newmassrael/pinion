//! Compressed game-audio decode — OGG Vorbis and FLAC (§5.54, R1274).
//!
//! WAV/PCM is a parse, so [`crate::wav`] hand-rolls it dependency-free. A
//! *compressed* format is a genuine codec (Vorbis: MDCT + codebooks + floor /
//! residue; FLAC: Rice residual + LPC prediction), so this layer bridges to
//! the canonical pure-Rust audio decoder — **symphonia** — rather than
//! hand-rolling thousands of lines of error-prone bit-twiddling. That is the
//! same "use the canonical platform substrate, do not reinvent it" call the
//! clipboard (arboard), tray (ksni), and font (skrifa) layers make.
//!
//! [`decode_compressed`] decodes straight to the engine's currency — a PCM
//! [`AudioClip`] of interleaved `f32` — so the mixer still never sees a codec
//! (`clip.rs`'s boundary holds). symphonia is an implementation detail: its
//! types never appear in this module's public surface, and its errors are
//! mapped onto [`DecodeError`], so the decoder could be swapped without
//! breaking callers.
//!
//! ## One content-sniffing entry point (not per-format)
//!
//! There is a single [`decode_compressed`], not a `decode_ogg` /
//! `decode_flac` pair, because symphonia detects the container from the
//! stream's magic bytes — a format hint is only advice it overrides. Per-name
//! functions would either lie (a `decode_ogg` that happily decodes FLAC) or
//! need an extra format-assertion to stop lying; a single sniffing decoder is
//! both honest and what real engines do (assets are detected by content, not
//! trusted by extension). WAV is *not* handled here — it is uncompressed, has
//! its own dependency-free [`crate::wav::decode_wav`], and symphonia's WAV
//! reader is deliberately not compiled in.
//!
//! ## Boundary (why not all of symphonia)
//!
//! symphonia can also demux MP4/MKV and decode AAC/MP3/ALAC. Those are
//! enabled only by opt-in features; this crate compiles it with
//! `default-features = false` and just `ogg` / `vorbis` / `flac`, so the
//! dependency stays *audio*, inside the §3 / §5.54 line, and does not drag in
//! the video-multimedia stack that remains out of scope.

use std::fmt;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::clip::AudioClip;

/// Failure decoding a compressed audio stream.
#[derive(Debug)]
pub enum DecodeError {
    /// The container held no decodable audio track.
    NoTrack,
    /// The container or codec is not one this build was compiled to support
    /// (e.g. an MP3 stream, which the audio-only feature set excludes).
    Unsupported,
    /// The decoder rejected the stream as corrupt or truncated.
    Corrupt(String),
    /// Reading the in-memory stream failed.
    Io(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTrack => write!(f, "no decodable audio track"),
            Self::Unsupported => write!(f, "unsupported container or codec"),
            Self::Corrupt(msg) => write!(f, "corrupt audio stream: {msg}"),
            Self::Io(msg) => write!(f, "audio stream I/O error: {msg}"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<SymphoniaError> for DecodeError {
    fn from(err: SymphoniaError) -> Self {
        match err {
            SymphoniaError::IoError(e) => Self::Io(e.to_string()),
            SymphoniaError::Unsupported(_) => Self::Unsupported,
            other => Self::Corrupt(other.to_string()),
        }
    }
}

/// Decode a compressed audio stream (OGG Vorbis or FLAC) into a PCM
/// [`AudioClip`]. The container is detected from the stream's content, so the
/// caller need not know which of the two it holds.
///
/// FLAC is lossless: its samples reconstruct the original PCM exactly (modulo
/// the source bit depth), so a decoded FLAC agrees sample-for-sample with the
/// same audio decoded from WAV. Vorbis is lossy.
///
/// # Errors
///
/// Returns a [`DecodeError`] when the bytes are not a supported compressed
/// format, are truncated or corrupt, or carry no audio track. (WAV/PCM is not
/// a compressed format — use [`crate::wav::decode_wav`].)
pub fn decode_compressed(bytes: &[u8]) -> Result<AudioClip, DecodeError> {
    // symphonia reads from an owned in-memory stream; the byte slice is copied
    // once into the cursor (decode is not on the audio hot path).
    let source = std::io::Cursor::new(bytes.to_vec());
    let stream = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());

    // No format hint: symphonia detects OGG vs FLAC from the magic bytes.
    let probed = symphonia::default::get_probe().format(
        &Hint::new(),
        stream,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.sample_rate.is_some())
        .ok_or(DecodeError::NoTrack)?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.ok_or(DecodeError::NoTrack)?;
    // The frame count the container declares (FLAC STREAMINFO total-samples,
    // Vorbis last-granule). Used below to catch a stream cut short mid-decode,
    // which symphonia otherwise surfaces as an ordinary end-of-stream.
    let declared_frames = track.codec_params.n_frames;

    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut samples: Vec<f32> = Vec::new();
    let mut channels: u16 = 0;
    let mut buffer: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // A clean end-of-stream surfaces as an IoError(UnexpectedEof).
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let pcm = decoder.decode(&packet)?;
        // The first decoded packet fixes the channel count and buffer size.
        if buffer.is_none() {
            let spec = *pcm.spec();
            // Channel counts are tiny (1..=8); >u16 is impossible for audio.
            channels = u16::try_from(spec.channels.count()).unwrap_or(0);
            buffer = Some(SampleBuffer::new(pcm.capacity() as u64, spec));
        }
        if let Some(buf) = &mut buffer {
            buf.copy_interleaved_ref(pcm);
            samples.extend_from_slice(buf.samples());
        }
    }

    if channels == 0 {
        return Err(DecodeError::NoTrack);
    }
    let clip = AudioClip::new(sample_rate, channels, samples);

    // symphonia reports both a clean container end and a stream cut short
    // mid-decode as the same `UnexpectedEof` from `next_packet`. Cross-check
    // the frames actually decoded against the count the container declared: a
    // shortfall means the stream was truncated (the common "asset copied /
    // downloaded incompletely" case), which the public contract promises to
    // reject rather than return as a silently-clipped clip. Only a shortfall
    // is an error — encoder padding can make a valid stream overshoot.
    if let Some(declared) = declared_frames
        && (clip.frame_count() as u64) < declared
    {
        return Err(DecodeError::Corrupt(format!(
            "truncated stream: decoded {} of {declared} declared frames",
            clip.frame_count()
        )));
    }
    Ok(clip)
}
