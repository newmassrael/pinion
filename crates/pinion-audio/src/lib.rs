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
//! azimuth pan) + a real cpal device backend (`cpal-backend` feature) + the
//! introspection surface. Deferred (same voice model): higher-order
//! resampling, richer spatialisation (HRTF / surround / doppler / per-source
//! rolloff tuning), and real-time voice-pool / resource-return hardening.
//!
//! ## Threading & the real-time control model
//!
//! [`AudioEngine::render`] takes a caller-provided buffer and does not
//! allocate in the render itself, so it is fit to run inside a real-time
//! audio callback (the callback's remaining real-time hardening is scoped in
//! [`rt`]). The single-thread engine is shared with [`AudioEngineExternal`] via
//! `Rc<RefCell<..>>` — fine for a headless test or a UI-thread pull, but not
//! what a sound card wants.
//!
//! [`rt`] is the standard game-audio control model: the mixer **owned by the
//! audio thread** ([`AudioRenderer`]), fed a **lock-free SPSC command queue**
//! (play/stop/set-param) from the control thread ([`AudioController`]), with a
//! lock-free [`AudioSnapshot`] published back for polling — no lock and no
//! shared mutable engine on the callback. [`AudioRenderer::render`] has the
//! exact `&mut [f32]` shape of a cpal output callback, and the real device
//! backend `device` (the `cpal-backend` feature) drives it from cpal on real
//! hardware. That backend is feature-gated (its Linux path needs
//! `libasound2-dev`) so a checkout without the ALSA headers still builds the
//! core crate; it is verified by `--example device_out`.

pub mod backend;
pub mod clip;
pub mod decode;
#[cfg(feature = "cpal-backend")]
pub mod device;
pub mod engine;
pub mod external;
pub mod mixer;
pub mod rt;
pub mod spatial;
pub mod wav;

pub use backend::{AudioBackend, InMemoryAudioBackend, peak, pump};
pub use clip::AudioClip;
pub use decode::{DecodeError, decode_compressed};
#[cfg(feature = "cpal-backend")]
pub use device::{CpalError, CpalOutput};
pub use engine::{AudioEngine, PlayOptions, ResolvedOutput, VoiceId};
pub use external::AudioEngineExternal;
pub use mixer::Voice;
pub use rt::{AudioCommand, AudioController, AudioRenderer, AudioSnapshot, realtime_channel};
pub use spatial::{Attenuation, Listener, Spatialization, Vec3, spatialize};
pub use wav::{WavError, decode_wav};
