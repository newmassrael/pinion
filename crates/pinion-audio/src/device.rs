//! Real audio-device output via **cpal** — the `cpal-backend` feature
//! (§5.54, R1274/R1282).
//!
//! This is the device consumer the [`crate::rt`] command queue was shaped
//! for: [`CpalOutput::start_default`] opens the system's default output (or
//! [`CpalOutput::start_on`] a specific one, chosen from
//! [`CpalOutput::output_device_names`]), moves an [`AudioRenderer`] into cpal's
//! audio-thread callback, and hands back an [`AudioController`] for the game/UI
//! thread. The callback body is just
//! `renderer.render(...)` — the same real-time-safe drain-and-render the
//! headless tests exercise (no lock, no shared mutable engine, no allocation,
//! and no free on the callback: retired voices go back over the
//! resource-return queue, freed on the control thread) — so the device path
//! and the tested path share the mixing/RT core, not two implementations. The
//! device callback additionally maps the stereo mix to the device's sample
//! format / channel layout below, which the headless path does not exercise.
//!
//! cpal reports the device's native config, so the adapter is
//! format/rate/channel-general: it renders the engine's interleaved stereo
//! `f32` and maps it to the device's sample format (`f32`/`i16`/`u16`), sample
//! rate (the engine is created *at* the device rate; per-voice resampling then
//! handles clips authored at any rate), and channel count (mono downmix,
//! stereo direct, or stereo-into-the-first-two-of-N).
//!
//! ## Choosing the device
//!
//! The host default is not always the wanted output, so the seam is
//! *enumerate + open-by-name*: [`CpalOutput::output_device_names`] lists the
//! outputs, [`CpalOutput::start_on`] opens one by its exact name, and
//! [`CpalOutput::device_name`] reports which one a live stream got. That is the
//! shape a settings panel needs (list, pick, persist the name, reopen it next
//! launch — falling back to the default when the saved device is gone), and it
//! is equally what lets a test open a *silent* virtual card, so a real device
//! callback can be exercised without making a sound.
//!
//! Matching is exact, and a miss is [`CpalError::DeviceNotFound`] — never a
//! silent fall back to the default, which would quietly send audio somewhere the
//! caller did not ask for. Fuzzy matching (a substring of a saved name, say) is
//! the *caller's* policy to apply over the enumerated list, not this seam's.
//!
//! Optional and Linux-gated by `libasound2-dev` (see the crate manifest): the
//! core audio crate builds everywhere; this backend is opt-in, so depending on
//! `pinion-audio` without `cpal-backend` needs no ALSA headers. Note the
//! *workspace* does build it: `examples/hello-audio-device` turns the feature on
//! (it is the wire proof that a real device callback runs concurrently with RPC),
//! so `cargo build --workspace` on Linux wants `libasound2-dev` installed.

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
    /// No output device is named this (the name came from a saved setting or
    /// an agent, and the device is absent now). Carries the requested name;
    /// [`CpalOutput::output_device_names`] lists what *is* present.
    DeviceNotFound(String),
    /// Enumerating the host's output devices failed.
    Enumerate(cpal::DevicesError),
    /// Reading a device's name failed.
    Name(cpal::DeviceNameError),
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
            Self::DeviceNotFound(name) => write!(f, "no output device named {name:?}"),
            Self::Enumerate(e) => write!(f, "audio device enumeration error: {e}"),
            Self::Name(e) => write!(f, "audio device name error: {e}"),
            Self::UnsupportedFormat(fmt) => write!(f, "unsupported device sample format {fmt:?}"),
            Self::Config(e) => write!(f, "audio device config error: {e}"),
            Self::Build(e) => write!(f, "audio stream build error: {e}"),
            Self::Play(e) => write!(f, "audio stream play error: {e}"),
        }
    }
}

impl std::error::Error for CpalError {}

/// A live output stream on one device. Playback stops when this is dropped, so
/// the caller keeps it alive for as long as sound is wanted.
pub struct CpalOutput {
    // Kept solely to own the stream; dropping it stops the device.
    _stream: cpal::Stream,
    device_name: String,
    sample_rate: u32,
    channels: u16,
}

impl CpalOutput {
    /// The names of every output device the host offers, in host order.
    ///
    /// This is the *enumerate* half of device selection: a settings panel lists
    /// these, the player picks one, and the choice is reopened later by name via
    /// [`CpalOutput::start_on`]. Names are matched exactly there, so they must
    /// come from here (or from a setting previously saved from here) rather than
    /// be guessed.
    ///
    /// # Errors
    ///
    /// [`CpalError::Enumerate`] if the host cannot list its devices, or
    /// [`CpalError::Name`] if one of them has no readable name.
    pub fn output_device_names() -> Result<Vec<String>, CpalError> {
        cpal::default_host()
            .output_devices()
            .map_err(CpalError::Enumerate)?
            .map(|device| device.name().map_err(CpalError::Name))
            .collect()
    }

    /// Open the host's **default** output device and start it. See
    /// [`CpalOutput::start_on`] for the argument and error contract.
    ///
    /// # Errors
    ///
    /// [`CpalError::NoDevice`] if the host has no default output, else as
    /// [`CpalOutput::start_on`].
    pub fn start_default(
        command_capacity: usize,
        max_voices: usize,
    ) -> Result<(AudioController, Self), CpalError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(CpalError::NoDevice)?;
        Self::start_on_device(&device, command_capacity, max_voices)
    }

    /// Open the output device with exactly this `name` (one of
    /// [`CpalOutput::output_device_names`]) and start it.
    ///
    /// Selecting a device by name — not just taking the host default — is what
    /// lets a player route audio to a chosen output, and what lets a test open a
    /// silent virtual card (an ALSA `snd-dummy`) so a real device callback runs
    /// with no audible output.
    ///
    /// Returns the control-thread [`AudioController`] paired with the live
    /// stream. `command_capacity` is the depth of the lock-free command ring;
    /// `max_voices` bounds the pre-reserved voice pool so the audio thread never
    /// reallocates (plays past it are rejected — a voice budget).
    ///
    /// # Errors
    ///
    /// [`CpalError::DeviceNotFound`] if no output device carries that name (a
    /// saved setting naming a now-absent device — recover by falling back to
    /// [`CpalOutput::start_default`]); otherwise [`CpalError`] if the device's
    /// config cannot be read, its sample format is unsupported, or the stream
    /// cannot be built / started.
    pub fn start_on(
        name: &str,
        command_capacity: usize,
        max_voices: usize,
    ) -> Result<(AudioController, Self), CpalError> {
        let device = cpal::default_host()
            .output_devices()
            .map_err(CpalError::Enumerate)?
            .find(|device| device.name().is_ok_and(|n| n == name))
            .ok_or_else(|| CpalError::DeviceNotFound(name.to_owned()))?;
        Self::start_on_device(&device, command_capacity, max_voices)
    }

    /// Open + start `device`: the one place a stream is built, so the default
    /// and by-name entry points cannot drift apart.
    fn start_on_device(
        device: &cpal::Device,
        command_capacity: usize,
        max_voices: usize,
    ) -> Result<(AudioController, Self), CpalError> {
        let device_name = device.name().map_err(CpalError::Name)?;
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
            SampleFormat::F32 => build_stream::<f32>(device, &config, renderer, channels_usize),
            SampleFormat::I16 => build_stream::<i16>(device, &config, renderer, channels_usize),
            SampleFormat::U16 => build_stream::<u16>(device, &config, renderer, channels_usize),
            other => return Err(CpalError::UnsupportedFormat(other)),
        }
        .map_err(CpalError::Build)?;
        stream.play().map_err(CpalError::Play)?;

        Ok((
            controller,
            Self {
                _stream: stream,
                device_name,
                sample_rate,
                channels,
            },
        ))
    }

    /// The name of the device this stream opened — the same string
    /// [`CpalOutput::output_device_names`] listed. Worth surfacing: it is how a
    /// caller (or an agent reading the audio surface) confirms *which* output the
    /// sound actually went to.
    #[must_use]
    pub fn device_name(&self) -> &str {
        &self.device_name
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The name lookup misses loudly. This is the guard that matters: a silent
    /// fall back to the default device would send a test's (or a game's) audio
    /// to whatever output happened to be default — including real speakers when
    /// a *silent* virtual card was the whole point of naming one.
    ///
    /// Needs no audio device: on a host with none, enumeration yields nothing
    /// and the lookup misses just the same.
    #[test]
    fn opening_an_absent_device_by_name_is_device_not_found() {
        const ABSENT: &str = "pinion::no-such-output-device";
        let Err(err) = CpalOutput::start_on(ABSENT, 8, 4) else {
            panic!("an absent device name must not open a stream");
        };
        assert!(
            matches!(&err, CpalError::DeviceNotFound(name) if name == ABSENT),
            "expected DeviceNotFound({ABSENT:?}), got {err:?}"
        );
    }

    /// Enumeration works, and every name it reports is usable as a `start_on`
    /// key — empty names would be unaddressable, so the list would be a lie.
    #[test]
    fn enumerated_device_names_are_addressable() {
        let names = CpalOutput::output_device_names().expect("enumerate output devices");
        assert!(
            names.iter().all(|name| !name.is_empty()),
            "device names must be non-empty to be openable by name: {names:?}"
        );
    }
}
