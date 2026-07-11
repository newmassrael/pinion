//! Real audio-device output via **cpal** — the `cpal-backend` feature
//! (§5.54, R1274/R1282).
//!
//! This is the device consumer the [`crate::rt`] command queue was shaped
//! for: [`CpalOutput::start_default`] opens the system's default output, moves
//! an [`AudioRenderer`] into cpal's audio-thread callback, and hands back an
//! [`AudioController`] for the game/UI thread. The callback body is just
//! `renderer.render(...)` — the same real-time-safe drain-and-render the
//! headless tests exercise (no lock, no shared mutable engine, no allocation,
//! and no free: retired voices go back over the resource-return queue) — so
//! the device path and the tested path are the *same* code, not two
//! implementations.
//!
//! cpal reports the device's native config, so the adapter is
//! format/rate/channel-general: it renders the engine's interleaved stereo
//! `f32` and maps it to the device's sample format (`f32`/`i16`/`u16`), sample
//! rate (the engine is created *at* the device rate; per-voice resampling then
//! handles clips authored at any rate), and channel count (mono downmix,
//! stereo direct, or stereo-into-the-first-two-of-N).
//!
//! Optional and Linux-gated by `libasound2-dev` (see the crate manifest): the
//! core audio crate builds everywhere; this backend is opt-in so a checkout
//! without the ALSA headers still compiles.

use std::fmt;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::engine::AudioEngine;
use crate::rt::{AudioController, AudioRenderer, realtime_channel};

/// Failure opening or starting a cpal output stream.
#[derive(Debug)]
pub enum CpalError {
    /// The host has no default output device.
    NoDevice,
    /// The device's default sample format is not one this adapter writes.
    UnsupportedFormat(SampleFormat),
    /// Querying the device's default configuration failed.
    Config(cpal::DefaultStreamConfigError),
    /// Building the output stream failed.
    Build(cpal::BuildStreamError),
    /// Starting playback failed.
    Play(cpal::PlayStreamError),
}

impl fmt::Display for CpalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => write!(f, "no default audio output device"),
            Self::UnsupportedFormat(fmt) => write!(f, "unsupported device sample format {fmt:?}"),
            Self::Config(e) => write!(f, "audio device config error: {e}"),
            Self::Build(e) => write!(f, "audio stream build error: {e}"),
            Self::Play(e) => write!(f, "audio stream play error: {e}"),
        }
    }
}

impl std::error::Error for CpalError {}

/// A live output stream on the default device. Playback stops when this is
/// dropped, so the caller keeps it alive for as long as sound is wanted.
pub struct CpalOutput {
    // Kept solely to own the stream; dropping it stops the device.
    _stream: cpal::Stream,
    sample_rate: u32,
    channels: u16,
}

impl CpalOutput {
    /// Open the default output device, start it, and return the control-thread
    /// [`AudioController`] paired with the live stream. `command_capacity` is
    /// the depth of the lock-free command ring; `max_voices` bounds the
    /// pre-reserved voice pool so the audio thread never reallocates (plays
    /// past it are rejected — a voice budget).
    ///
    /// # Errors
    ///
    /// [`CpalError`] if there is no output device, its config cannot be read,
    /// its sample format is unsupported, or the stream cannot be built/started.
    pub fn start_default(
        command_capacity: usize,
        max_voices: usize,
    ) -> Result<(AudioController, Self), CpalError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(CpalError::NoDevice)?;
        let supported = device.default_output_config().map_err(CpalError::Config)?;
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();

        // The engine renders at the device's own rate; per-voice resampling
        // pitches any clip to match, so no separate resample stage is needed.
        let engine = AudioEngine::new(sample_rate);
        let (controller, renderer) = realtime_channel(engine, command_capacity, max_voices);

        let channels_usize = channels as usize;
        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(&device, &config, renderer, channels_usize),
            SampleFormat::I16 => build_stream::<i16>(&device, &config, renderer, channels_usize),
            SampleFormat::U16 => build_stream::<u16>(&device, &config, renderer, channels_usize),
            other => return Err(CpalError::UnsupportedFormat(other)),
        }
        .map_err(CpalError::Build)?;
        stream.play().map_err(CpalError::Play)?;

        Ok((
            controller,
            Self {
                _stream: stream,
                sample_rate,
                channels,
            },
        ))
    }

    /// The device's output sample rate (the rate the engine renders at).
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The device's output channel count.
    #[must_use]
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

/// Build a device stream for sample type `T`, mapping the engine's stereo
/// `f32` mix into the device's format and channel layout each callback.
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut renderer: AudioRenderer,
    channels: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    // Reused across callbacks; grows to the largest block cpal asks for, then
    // stops allocating. Together with the pre-reserved voice pool and the
    // resource-return queue in `rt`, the steady-state callback neither
    // allocates nor frees.
    let mut stereo: Vec<f32> = Vec::new();
    let channels = channels.max(1);

    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let frames = data.len() / channels;
            let needed = frames * 2;
            if stereo.len() < needed {
                stereo.resize(needed, 0.0);
            }
            renderer.render(&mut stereo[..needed]);

            for (frame, out) in data.chunks_mut(channels).enumerate() {
                let l = stereo[frame * 2];
                let r = stereo[frame * 2 + 1];
                if channels == 1 {
                    out[0] = T::from_sample((l + r) * 0.5);
                } else {
                    out[0] = T::from_sample(l);
                    out[1] = T::from_sample(r);
                    for extra in out.iter_mut().skip(2) {
                        *extra = T::from_sample(0.0f32);
                    }
                }
            }
        },
        |err| eprintln!("pinion-audio: cpal stream error: {err}"),
        None,
    )
}
