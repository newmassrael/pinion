//! First-class introspectable engine audio (§5.54, ratified R1274).
//!
//! Per the F4 spec amendment, audio is a **first-class in-engine
//! subsystem**, not the §3 "codec embed" exclusion and not an opaque
//! `External`. This crate is the substrate of that decision:
//!
//! - [`AudioClip`] — decoded PCM, the currency the mixer sums.
//! - [`wav`] — a pure-Rust WAV/PCM decoder (the in-scope asset boundary;
//!   uncompressed PCM is a parse, not a codec).
//! - [`decode`] — compressed game-audio decode (OGG Vorbis / FLAC) bridged to
//!   the canonical pure-Rust codec (symphonia), still producing plain PCM.
//! - [`Voice`] / mixing — the deterministic stereo mix (§2 #3).
//! - [`spatial`] — 3D positional audio: a [`spatial::Listener`] plus a
//!   per-voice world position resolve to distance attenuation + azimuth pan
//!   through the very same pan pot a hand-panned voice uses.
//! - [`AudioEngine`] — the live voice graph: play / stop / master gain /
//!   listener / render, with the whole graph readable as data.
//! - [`AudioBackend`] / [`InMemoryAudioBackend`] — the output-device seam,
//!   headlessly verifiable; a real cpal device backend drops in behind the
//!   same trait (deferred: a headless box has no device to verify it).
//! - [`AudioEngineExternal`] — the §5.15 introspection surface: an AI reads
//!   what is playing and drives play/stop over RPC (§2 #2 / #7). This is the
//!   "not hidden behind opaque External" half of §5.54 made concrete.
//!
//! ## Scope of this increment
//!
//! WAV/PCM + compressed (OGG Vorbis / FLAC) decode, a stereo mixer (mono
//! pan-pot / stereo balance) with per-voice linear-interpolation
//! **resampling** to the engine rate + one-shot & looping voices + master
//! gain + 3D positional audio (listener-relative distance attenuation +
//! azimuth pan) + the introspection surface. Deferred (same voice model):
//! higher-order resampling, richer spatialisation (HRTF / surround / doppler /
//! per-source rolloff tuning), and the real cpal device backend.
//!
//! ## Threading & the real-time transition (deferred, backend-forced)
//!
//! [`AudioEngine::render`] takes a caller-provided buffer and allocates
//! nothing, so it is already fit to run inside a real-time audio callback.
//! What is **not** yet real-time is the *ownership*: today the engine is a
//! single-thread, on-demand pull graph, shared with
//! [`AudioEngineExternal`] via `Rc<RefCell<..>>`.
//!
//! The Unreal-class target is the standard game-audio control model — the
//! mixer owned by the **audio device thread**, fed a lock-free command
//! queue (play/stop/set-param) from the game/UI thread, publishing a state
//! snapshot back for introspection, with a no-alloc callback. That
//! transition replaces the `Rc<RefCell>` with an SPSC command channel +
//! published snapshot. It is deliberately **not** built here: its exact
//! shape is forced by the real cpal backend (the forcing consumer), and
//! guessing it before that consumer exists would be speculative
//! abstraction. The pieces that are non-speculative today — the alloc-free
//! buffer-in render, the deterministic mix — are already in place.

pub mod backend;
pub mod clip;
pub mod decode;
pub mod engine;
pub mod external;
pub mod mixer;
pub mod spatial;
pub mod wav;

pub use backend::{AudioBackend, InMemoryAudioBackend, pump};
pub use clip::AudioClip;
pub use decode::{DecodeError, decode_compressed};
pub use engine::{AudioEngine, PlayOptions, ResolvedOutput, VoiceId};
pub use external::AudioEngineExternal;
pub use mixer::Voice;
pub use spatial::{Attenuation, Listener, Spatialization, Vec3, spatialize};
pub use wav::{WavError, decode_wav};
