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
            Self::UnsupportedFormat(fmt) => write!(f, "unsupported device sample format {fmt:?}"),
            Self::Config(e) => write!(f, "audio device config error: {e}"),
            Self::Build(e) => write!(f, "audio stream build error: {e}"),
            Self::Play(e) => write!(f, "audio stream play error: {e}"),
        }
    }
}

impl std::error::Error for CpalError {}

/// Reported by [`CpalOutput::device_name`] when the host cannot tell us what the
/// open device is called. Not addressable by [`CpalOutput::start_on`] — such a
/// device is reachable only as the host default.
const UNNAMED_DEVICE: &str = "<unnamed>";

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
    /// A device whose name cannot be read is **skipped**, not fatal: it is
    /// unaddressable by name anyway, so listing it would be useless, and failing
    /// the whole call over it would hide every *good* device from the panel.
    /// [`CpalOutput::start_on`] skips such a device for the same reason, so the
    /// two halves of the seam agree.
    ///
    /// # Errors
    ///
    /// [`CpalError::Enumerate`] if the host cannot list its devices at all.
    pub fn output_device_names() -> Result<Vec<String>, CpalError> {
        Ok(cpal::default_host()
            .output_devices()
            .map_err(CpalError::Enumerate)?
            .filter_map(|device| device.name().ok())
            .collect())
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
        // The name is *reporting only* ([`CpalOutput::device_name`]), so an
        // unreadable one must never fail the open — a host whose default output
        // has no readable name must still make sound. (It was fatal when this
        // body was first extracted, which silently turned a cosmetic field into a
        // precondition for any audio at all.)
        let device_name = device.name().unwrap_or_else(|_| UNNAMED_DEVICE.to_owned());
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
            write_frames(&stereo[..needed], data, channels);
        },
        |err| eprintln!("pinion-audio: cpal stream error: {err}"),
        None,
    )
}

/// Map the engine's interleaved stereo `f32` mix into the device's sample format
/// and channel layout: mono is a downmix, stereo is direct, and >2 channels get
/// the mix in the first two with the rest silenced.
///
/// This is the last thing that happens to a sample before the sound card sees it,
/// and it is the *only* step the headless render path never exercises — so it is
/// pulled out of the cpal callback as a pure function of `(stereo, data,
/// channels)` and unit-tested below. Nothing about it needs a device, and the
/// real-time snapshot's `peak` is measured on the mix *before* this runs, so a
/// bug here (silence, wrong channel, bad scaling) is invisible to every
/// introspection read: tests are the only thing that can catch it.
///
/// `channels` must be >= 1. A trailing partial frame — which cpal is not expected
/// to hand us, but which would index out of bounds if it ever did, on the
/// real-time thread — is silenced rather than assumed away.
fn write_frames<T>(stereo: &[f32], data: &mut [T], channels: usize)
where
    T: SizedSample + FromSample<f32>,
{
    let channels = channels.max(1);
    let mut frames = data.chunks_exact_mut(channels);
    for (frame, out) in frames.by_ref().enumerate() {
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
    for tail in frames.into_remainder() {
        *tail = T::from_sample(0.0f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name lookup misses loudly. This is the guard that matters: a silent
    /// fall back to the default device would send a test's (or a game's) audio
    /// to whatever output happened to be default — including real speakers when
    /// a *silent* virtual card was the whole point of naming one.
    ///
    /// Opens no device: it needs only that *enumeration* succeeds. On a host with
    /// no output at all the list is empty and the lookup misses identically — but
    /// if enumeration itself fails, the error is `Enumerate`, not
    /// `DeviceNotFound`, and this test says so rather than pretending otherwise.
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

    /// Enumeration must not error, and no name it reports may be empty (an empty
    /// name is unaddressable by `start_on`, so listing it would be a lie).
    ///
    /// Honest about its own reach: on a host with no output devices this is
    /// vacuous. What actually proves an enumerated name *opens* is the
    /// `hello-audio-device` demo, which opens the silent card by an enumerated
    /// name over the wire — that needs a device, so it lives in CI's sweep job.
    #[test]
    fn enumerated_device_names_are_non_empty() {
        let names = CpalOutput::output_device_names().expect("enumerate output devices");
        assert!(
            names.iter().all(|name| !name.is_empty()),
            "device names must be non-empty to be openable by name: {names:?}"
        );
    }

    // ── the format / channel-layout conversion (`write_frames`) ───────────────
    //
    // This is the shipping device path's last step and the one the headless
    // renderer never runs. `AudioSnapshot::peak` is measured on the stereo mix
    // BEFORE this conversion, so no introspection read — and therefore no RPC
    // demo — can observe a bug in here. These tests are the only guard, and they
    // need no sound card.

    /// Sample-wise comparison with a tolerance — the values here are all exactly
    /// representable, but an exact `==` on floats is not a habit worth forming.
    #[track_caller]
    fn assert_samples(got: &[f32], want: &[f32]) {
        assert_eq!(got.len(), want.len(), "frame count");
        for (i, (g, w)) in got.iter().zip(want).enumerate() {
            assert!(
                (g - w).abs() < 1e-6,
                "sample {i}: got {g}, want {w} (whole buffer {got:?})"
            );
        }
    }

    /// Stereo out: the mix passes through, sample for sample.
    #[test]
    fn stereo_passes_the_mix_through_unchanged() {
        let stereo = [0.5f32, -0.25, 1.0, -1.0];
        let mut out = [0.0f32; 4];
        write_frames(&stereo, &mut out, 2);
        assert_samples(&out, &[0.5, -0.25, 1.0, -1.0]);
    }

    /// Mono out: the two channels are averaged, not silently truncated to the
    /// left (which would lose every hard-right sound on a mono device).
    #[test]
    fn mono_downmixes_both_channels() {
        let stereo = [1.0f32, 0.0, 0.0, 1.0];
        let mut out = [0.0f32; 2];
        write_frames(&stereo, &mut out, 1);
        assert_samples(&out, &[0.5, 0.5]); // each frame is (l + r) / 2
    }

    /// Surround out: the mix lands in the first two channels and every extra one
    /// is SILENCED — never left carrying whatever the driver's buffer held.
    #[test]
    fn extra_channels_are_silenced_not_left_as_garbage() {
        let stereo = [0.5f32, -0.5];
        let mut out = [9.0f32; 6]; // 5.1, pre-filled with garbage
        write_frames(&stereo, &mut out, 6);
        assert_samples(&out, &[0.5, -0.5, 0.0, 0.0, 0.0, 0.0]);
    }

    /// The integer formats scale correctly. `i16` is the format most real cards
    /// hand us, and a wrong scale here is inaudible to `peak` (which reads the
    /// f32 mix) — it would ship as distortion nobody's test caught.
    #[test]
    fn i16_and_u16_conversions_scale_to_full_range() {
        let stereo = [1.0f32, -1.0];
        let mut i16_out = [0i16; 2];
        write_frames(&stereo, &mut i16_out, 2);
        assert_eq!(
            i16_out,
            [i16::MAX, i16::MIN],
            "full-scale maps to full-scale"
        );

        let mut u16_out = [0u16; 2];
        write_frames(&stereo, &mut u16_out, 2);
        assert_eq!(u16_out, [u16::MAX, u16::MIN], "u16 is offset-binary");

        // Silence must be the format's zero, not its numeric 0.
        let mut mid = [0u16; 2];
        write_frames(&[0.0, 0.0], &mut mid, 2);
        assert_eq!(mid, [32768, 32768], "u16 silence is mid-scale, not 0");
    }

    /// A trailing partial frame is silenced, not indexed out of bounds. cpal is
    /// not expected to hand us one — but this runs on the real-time thread, where
    /// an assumption that turns out false is a panic in the audio callback.
    #[test]
    fn a_partial_trailing_frame_is_silenced_not_a_panic() {
        let stereo = [1.0f32, 1.0];
        // 3 samples on a 2-channel device: one whole frame plus a stray sample.
        let mut out = [9.0f32; 3];
        write_frames(&stereo, &mut out, 2);
        assert_samples(&out, &[1.0, 1.0, 0.0]); // the odd tail sample is silenced
    }
}
