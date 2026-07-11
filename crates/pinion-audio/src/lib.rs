//! First-class introspectable engine audio (§5.54, ratified R1274).
//!
//! Per the F4 spec amendment, audio is a **first-class in-engine
//! subsystem**, not the §3 "codec embed" exclusion and not an opaque
//! `External`. This crate is the substrate of that decision:
//!
//! - [`AudioClip`] — decoded PCM, the currency the mixer sums.
//! - [`wav`] — a pure-Rust WAV/PCM decoder (the in-scope asset boundary;
//!   uncompressed PCM is a parse, not a codec).
//! - [`Voice`] / mixing — the deterministic stereo mix (§2 #3).
//! - [`AudioEngine`] — the live voice graph: play / stop / master gain /
//!   render, with the whole graph readable as data.
//! - [`AudioBackend`] / [`InMemoryAudioBackend`] — the output-device seam,
//!   headlessly verifiable; a real cpal device backend drops in behind the
//!   same trait (deferred: a headless box has no device to verify it).
//! - [`AudioEngineExternal`] — the §5.15 introspection surface: an AI reads
//!   what is playing and drives play/stop over RPC (§2 #2 / #7). This is the
//!   "not hidden behind opaque External" half of §5.54 made concrete.
//!
//! ## Scope of this increment
//!
//! WAV/PCM decode + a stereo mixer (mono pan-pot / stereo balance) +
//! one-shot & looping voices + master gain + the introspection surface.
//! Deferred (same voice model): compressed decode (OGG/FLAC), resampling,
//! richer 3D spatialisation, and the real cpal device backend.

pub mod backend;
pub mod clip;
pub mod engine;
pub mod external;
pub mod mixer;
pub mod wav;

pub use backend::{AudioBackend, InMemoryAudioBackend, pump};
pub use clip::AudioClip;
pub use engine::{AudioEngine, PlayOptions, VoiceId};
pub use external::AudioEngineExternal;
pub use mixer::Voice;
pub use wav::{WavError, decode_wav};
