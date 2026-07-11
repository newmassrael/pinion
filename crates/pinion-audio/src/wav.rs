//! Minimal WAV (RIFF/PCM) decoder — the in-scope asset boundary.
//!
//! A WAV container holds **uncompressed** PCM (or IEEE float): reading it is
//! a parse, not a codec. Per §3 / §5.54 this is exactly the boundary line —
//! WAV/PCM decode is in-engine, first-class; a *compressed* format
//! (OGG/FLAC) is the codec layer that arrives later (still in-engine per
//! §5.54, just a bigger decoder). This decoder is pure safe Rust with no
//! dependency.
//!
//! Supported sample formats: PCM 8/16/24-bit and IEEE float 32-bit — the
//! set game WAV assets actually use. Unknown chunks are skipped.

use std::fmt;

use crate::clip::AudioClip;

/// Failure decoding a WAV byte stream.
#[derive(Debug, PartialEq, Eq)]
pub enum WavError {
    /// The stream is shorter than a minimal RIFF/WAVE header.
    TooShort,
    /// Missing the `RIFF` / `WAVE` magic.
    NotWave,
    /// No `fmt ` chunk was found before the data.
    MissingFmt,
    /// No `data` chunk was found.
    MissingData,
    /// A `(format_tag, bits_per_sample)` combination this decoder does not
    /// handle.
    Unsupported { format: u16, bits: u16 },
}

impl fmt::Display for WavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "WAV stream too short"),
            Self::NotWave => write!(f, "not a RIFF/WAVE stream"),
            Self::MissingFmt => write!(f, "WAV missing fmt chunk"),
            Self::MissingData => write!(f, "WAV missing data chunk"),
            Self::Unsupported { format, bits } => {
                write!(f, "unsupported WAV format tag {format} at {bits}-bit")
            }
        }
    }
}

impl std::error::Error for WavError {}

fn u16_le(b: &[u8], off: usize) -> Option<u16> {
    <[u8; 2]>::try_from(b.get(off..off + 2)?)
        .ok()
        .map(u16::from_le_bytes)
}

fn u32_le(b: &[u8], off: usize) -> Option<u32> {
    <[u8; 4]>::try_from(b.get(off..off + 4)?)
        .ok()
        .map(u32::from_le_bytes)
}

/// Decode a WAV byte stream into a PCM [`AudioClip`].
///
/// # Errors
///
/// Returns a [`WavError`] when the stream is truncated, is not RIFF/WAVE,
/// lacks a `fmt `/`data` chunk, or uses a sample format this decoder does
/// not support.
pub fn decode_wav(bytes: &[u8]) -> Result<AudioClip, WavError> {
    if bytes.len() < 12 {
        return Err(WavError::TooShort);
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(WavError::NotWave);
    }

    let mut format_tag = 0u16;
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits = 0u16;
    let mut have_fmt = false;
    let mut data: Option<&[u8]> = None;

    // Walk the chunk list after the 12-byte RIFF/WAVE header.
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32_le(bytes, pos + 4).ok_or(WavError::TooShort)? as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(size).min(bytes.len());
        let body = &bytes[body_start..body_end];

        if id == b"fmt " {
            format_tag = u16_le(body, 0).ok_or(WavError::MissingFmt)?;
            channels = u16_le(body, 2).ok_or(WavError::MissingFmt)?;
            sample_rate = u32_le(body, 4).ok_or(WavError::MissingFmt)?;
            bits = u16_le(body, 14).ok_or(WavError::MissingFmt)?;
            have_fmt = true;
        } else if id == b"data" {
            data = Some(body);
        }

        // Chunks are word-aligned: an odd size carries a pad byte.
        pos = body_start + size + (size & 1);
    }

    if !have_fmt {
        return Err(WavError::MissingFmt);
    }
    let data = data.ok_or(WavError::MissingData)?;
    let samples = decode_samples(data, format_tag, bits)?;
    Ok(AudioClip::new(sample_rate, channels, samples))
}

/// WAVE format tag: uncompressed integer PCM.
const FORMAT_PCM: u16 = 1;
/// WAVE format tag: IEEE 32-bit float.
const FORMAT_FLOAT: u16 = 3;

fn decode_samples(data: &[u8], format: u16, bits: u16) -> Result<Vec<f32>, WavError> {
    match (format, bits) {
        (FORMAT_PCM, 8) => Ok(data
            .iter()
            .map(|&b| (f32::from(b) - 128.0) / 128.0)
            .collect()),
        (FORMAT_PCM, 16) => Ok(data
            .chunks_exact(2)
            .filter_map(|c| <[u8; 2]>::try_from(c).ok())
            .map(|c| f32::from(i16::from_le_bytes(c)) / 32_768.0)
            .collect()),
        (FORMAT_PCM, 24) => Ok(data
            .chunks_exact(3)
            .map(|c| {
                // Sign-extend the 24-bit little-endian sample into i32.
                let raw = i32::from(c[0]) | (i32::from(c[1]) << 8) | (i32::from(c[2]) << 16);
                let signed = (raw << 8) >> 8;
                // 24-bit range fits exactly in f32's 24-bit mantissa.
                #[allow(clippy::cast_precision_loss)]
                {
                    signed as f32 / 8_388_608.0
                }
            })
            .collect()),
        (FORMAT_FLOAT, 32) => Ok(data
            .chunks_exact(4)
            .filter_map(|c| <[u8; 4]>::try_from(c).ok())
            .map(f32::from_le_bytes)
            .collect()),
        _ => Err(WavError::Unsupported { format, bits }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal 16-bit PCM WAV with the given interleaved i16 samples.
    fn wav16(channels: u16, sample_rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let data_len = u32::try_from(data.len()).unwrap();
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&FORMAT_PCM.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&sample_rate.to_le_bytes());
        let byte_rate = sample_rate * u32::from(channels) * 2;
        b.extend_from_slice(&byte_rate.to_le_bytes());
        b.extend_from_slice(&(channels * 2).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        b.extend_from_slice(&data);
        b
    }

    #[test]
    fn decodes_16bit_pcm() {
        let bytes = wav16(1, 44_100, &[0, 16_384, -16_384, i16::MAX]);
        let clip = decode_wav(&bytes).expect("decodes");
        assert_eq!(clip.sample_rate(), 44_100);
        assert_eq!(clip.channels(), 1);
        assert_eq!(clip.frame_count(), 4);
        assert!((clip.samples()[1] - 0.5).abs() < 1e-3);
        assert!((clip.samples()[2] + 0.5).abs() < 1e-3);
    }

    #[test]
    fn decodes_with_unknown_chunk_skipped() {
        // Insert a LIST chunk before data; decoder must skip it.
        let mut bytes = wav16(2, 22_050, &[1, 2, 3, 4]);
        // Splice a fake "LIST" chunk right after the fmt chunk (offset 36).
        let mut spliced = bytes[..36].to_vec();
        spliced.extend_from_slice(b"LIST");
        spliced.extend_from_slice(&2u32.to_le_bytes());
        spliced.extend_from_slice(&[0xAA, 0xBB]); // even size, no pad
        spliced.extend_from_slice(&bytes[36..]);
        bytes = spliced;
        let clip = decode_wav(&bytes).expect("decodes past LIST");
        assert_eq!(clip.channels(), 2);
        assert_eq!(clip.frame_count(), 2);
    }

    #[test]
    fn rejects_non_wave() {
        assert_eq!(
            decode_wav(b"not a wav at all!!!!").unwrap_err(),
            WavError::NotWave
        );
        assert_eq!(decode_wav(b"tiny").unwrap_err(), WavError::TooShort);
    }
}
