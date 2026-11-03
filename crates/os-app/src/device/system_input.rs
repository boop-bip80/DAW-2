// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 Samrat Dutta
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The input device (a microphone, a line input) for the mixer tracks, when the output is not an ASIO driver (spec:
//! audio/track-io.md §2.2): the one chosen in Settings > Audio, else the system's default (`audio_cfg::input_device`).
//! Its stream opens with the output, only while a track hears its input or is armed
//! (`super::want_inputs`), so the microphone is not taken (and the system does not show it in use) the rest of the
//! time. It runs at the output's rate (resampled when the device will not), and its callback pushes into the same ring
//! an ASIO driver's inputs go into (`super::input_ring`), which the streams read at their own place.
//!
//! What the device is (its name and channels) is asked off the UI thread and kept a few seconds (`info`): asking the
//! sound server takes a while, and the mixer asks on every repaint.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{BufferSize, FromSample, SampleFormat, SizedSample, StreamConfig, SupportedBufferSize};

use super::{input_ring, INPUT_CHANNELS};
use crate::audio_cfg::{self, AudioCfg};

/// The input device as the mixer offers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub name: String,
    /// Its inputs (at most `INPUT_CHANNELS`).
    pub channels: usize,
}

/// How long what `info` found is kept before the device is asked again.
const KEEP: Duration = Duration::from_secs(3);

struct Cache {
    found: Option<Info>,
    /// When it was asked last (`None`: never), and whether an ask is under way.
    at: Option<Instant>,
    asking: bool,
    /// Counts `forget`s: an ask begun before one is not kept.
    generation: u64,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { found: None, at: None, asking: false, generation: 0 });

/// Set by `forget`: an input that failed to open is tried again (`OutputDevice::sync_inputs`).
static RETRY: AtomicBool = AtomicBool::new(false);

/// Additional input delay from the causal sample-rate converter, in output frames.
static CONVERSION_DELAY: AtomicU64 = AtomicU64::new(0);

pub(crate) fn conversion_delay() -> u64 {
    CONVERSION_DELAY.load(Ordering::Acquire)
}

/// Another input device was chosen: what was found is dropped and the device is asked again.
pub fn forget() {
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    (c.found, c.at, c.asking) = (None, None, false);
    c.generation += 1;
    RETRY.store(true, Ordering::Release);
}

/// Whether another input device was chosen since this was last asked (`forget`).
pub fn take_retry() -> bool {
    RETRY.swap(false, Ordering::AcqRel)
}

/// The input device for the audio settings in use (`None`: none, or not asked yet). It never waits:
/// a device asked about more than `KEEP` ago is asked again on a thread of its own.
pub fn info() -> Option<Info> {
    if cfg!(test) {
        return None;
    }
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if !c.asking && c.at.is_none_or(|t| t.elapsed() >= KEEP) {
        c.asking = true;
        let generation = c.generation;
        let spawned = std::thread::Builder::new().name("input device".into()).spawn(move || {
            let found = ask(&AudioCfg::current());
            let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if c.generation == generation {
                (c.found, c.at, c.asking) = (found, Some(Instant::now()), false);
            }
        });
        if spawned.is_err() {
            c.asking = false;
        }
    }
    c.found.clone()
}

/// Asks the host `cfg` plays on for the input device `cfg` records from.
fn ask(cfg: &AudioCfg) -> Option<Info> {
    let device = audio_cfg::input_device(cfg)?;
    let channels = usize::from(input_config(&device).ok()?.channels()).min(INPUT_CHANNELS);
    (channels > 0).then(|| Info { name: audio_cfg::device_name(&device), channels })
}

/// The format an input device records in: its default, else the first it lists at its highest rate (a device that
/// reports no default format is still offered and opened, rather than left out).
pub fn input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    device.default_input_config().or_else(|e| {
        let listed = device.supported_input_configs().ok().and_then(|mut cs| cs.next()).map(|c| c.with_max_sample_rate());
        listed.ok_or_else(|| format!("{}: {e}", audio_cfg::device_name(device)))
    })
}

/// The open input stream. Dropped, it stops and the ring has no inputs again.
pub struct SystemInput {
    stream: Option<cpal::Stream>,
    /// The device it opened (`audio_cfg::device_name`): another one chosen, or another default, opens anew.
    pub name: String,
    /// Its error callback said it does not play on (the device went away).
    failed: Arc<AtomicBool>,
    alsa: bool,
}

impl Drop for SystemInput {
    fn drop(&mut self) {
        drop(self.stream.take());
        input_ring().open(0, 0);
        CONVERSION_DELAY.store(0, Ordering::Release);
        if self.alsa {
            super::heal_sound_server();
        }
    }
}

impl SystemInput {
    /// The stream stopped for good (it is opened again with the output).
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}

/// Opens the input device of `cfg` (`audio_cfg::input_device`) at `rate` (the output's), with blocks of about `block`
/// frames.
pub fn open(cfg: &AudioCfg, rate: u32, block: Option<u32>) -> Result<SystemInput, String> {
    let host = audio_cfg::host(cfg);
    let device = audio_cfg::input_device(cfg).ok_or("no audio input device")?;
    let name = audio_cfg::device_name(&device);
    let own = input_config(&device)?;
    // The output's rate when the device takes it, else its own (resampled in the callback).
    let supported = if own.sample_rate() == rate {
        own
    } else {
        device
            .supported_input_configs()
            .ok()
            .and_then(|mut cs| cs.find(|c| c.channels() == own.channels() && c.sample_format() == own.sample_format() && c.min_sample_rate() <= rate && rate <= c.max_sample_rate()))
            .map(|c| c.with_sample_rate(rate))
            .unwrap_or(own)
    };
    let format = supported.sample_format();
    let range = supported.buffer_size().clone();
    let mut config: StreamConfig = supported.into();
    // Through the sound server a stream that asks for no length is buffered for seconds: it asks for the output's.
    let b = block.unwrap_or(512);
    if matches!(range, SupportedBufferSize::Range { min, max } if (min..=max).contains(&b)) {
        config.buffer_size = BufferSize::Fixed(b);
    }
    let channels = usize::from(config.channels).clamp(1, INPUT_CHANNELS);
    input_ring().open(channels, b as usize);
    let failed = Arc::new(AtomicBool::new(false));
    let mut stream = build_any(&device, format, &config, rate, failed.clone());
    if stream.is_err() && config.buffer_size != BufferSize::Default {
        config.buffer_size = BufferSize::Default;
        stream = build_any(&device, format, &config, rate, failed.clone());
    }
    let stream = stream.inspect_err(|_| {
        input_ring().open(0, 0);
        CONVERSION_DELAY.store(0, Ordering::Release);
    })?;
    stream.play().map_err(|e| {
        input_ring().open(0, 0);
        CONVERSION_DELAY.store(0, Ordering::Release);
        e.to_string()
    })?;
    Ok(SystemInput { stream: Some(stream), name, failed, alsa: host.id().name() == "ALSA" })
}

fn build_any(device: &cpal::Device, format: SampleFormat, config: &StreamConfig, rate: u32, failed: Arc<AtomicBool>) -> Result<cpal::Stream, String> {
    match format {
        SampleFormat::F32 => build::<f32>(device, config, rate, failed),
        SampleFormat::F64 => build::<f64>(device, config, rate, failed),
        SampleFormat::I16 => build::<i16>(device, config, rate, failed),
        SampleFormat::I24 => build::<cpal::I24>(device, config, rate, failed),
        SampleFormat::I32 => build::<i32>(device, config, rate, failed),
        SampleFormat::U16 => build::<u16>(device, config, rate, failed),
        other => Err(format!("unsupported input sample format {other:?}")),
    }
}

fn build<T>(device: &cpal::Device, config: &StreamConfig, rate: u32, failed: Arc<AtomicBool>) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let device_ch = usize::from(config.channels).max(1);
    let ch = device_ch.min(INPUT_CHANNELS);
    let mut resample = Resampler::new(config.sample_rate, rate, ch)?;
    CONVERSION_DELAY.store(resample.as_ref().map_or(0, Resampler::delay), Ordering::Release);
    device
        .build_input_stream::<T, _, _>(
            config.clone(),
            move |data: &[T], _| {
                let frames = data.len() / device_ch;
                let at = |c: usize, f: usize| data[f * device_ch + c].to_sample::<f32>();
                match resample.as_mut() {
                    None => input_ring().push(frames, at),
                    Some(r) => {
                        // Chunked publication must still report the complete callback's burst to the ring reader.
                        let burst = (frames as u128 * u128::from(r.to)).div_ceil(u128::from(r.from)).min(usize::MAX as u128) as usize;
                        input_ring().block.fetch_max(burst, Ordering::Relaxed);
                        r.run(frames, at, |out| input_ring().push(out.len() / ch, |c, f| out[f * ch + c]));
                    }
                }
            },
            move |e| {
                use cpal::ErrorKind::*;
                match e.kind() {
                    Xrun | DeviceChanged | RealtimeDenied => {}
                    _ => {
                        eprintln!("audio input stream error: {e}");
                        failed.store(true, Ordering::Release);
                    }
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}

/// Streaming low-pass, windowed-sinc conversion. The kernel is calculated off the callback, widened for
/// downsampling, and delayed until its future input exists. Fixed history/output storage bounds callback memory
/// regardless of callback size. Rational phase accumulation avoids drift and makes chunk boundaries irrelevant.
/// Qualified rates are 8..384 kHz, with a pass band through 80% of the lower Nyquist frequency; the transition band
/// is intentional. At equal rates the converter is bypassed without adding delay.
struct Resampler {
    from: u32,
    to: u32,
    /// Integer and fractional input-frame position of the next output sample (fraction in units of `to`).
    next: u64,
    fraction: u64,
    seen: u64,
    half: usize,
    ch: usize,
    history: Vec<f32>,
    kernels: Vec<f32>,
    out: Vec<f32>,
}

const PHASES: usize = 1024;
const OUTPUT_CHUNK: usize = 256;

impl Resampler {
    fn new(from: u32, to: u32, ch: usize) -> Result<Option<Resampler>, String> {
        if !(8000..=384000).contains(&from) || !(8000..=384000).contains(&to) || !(1..=INPUT_CHANNELS).contains(&ch) {
            return Err("input conversion supports 8–384 kHz and 1–8 channels".into());
        }
        if from == to {
            return Ok(None);
        }
        // Reserve a transition band below Nyquist, rather than aliasing frequencies at the destination's edge.
        let ratio = (f64::from(from) / f64::from(to)).max(1.0) / 0.90;
        let half = (64.0 * ratio).ceil() as usize;
        let taps = 2 * half;
        let mut kernels = vec![0.0; (PHASES + 1) * taps];
        for phase in 0..=PHASES {
            let row = &mut kernels[phase * taps..(phase + 1) * taps];
            os_dsp::resample::weights(phase as f64 / PHASES as f64, ratio, half, os_dsp::resample::Quality::Exact, row);
            let sum: f64 = row.iter().map(|&v| f64::from(v)).sum();
            for v in row {
                *v = (f64::from(*v) / sum) as f32;
            }
        }
        Ok(Some(Resampler { from, to, next: 0, fraction: 0, seen: 0, half, ch,
            history: vec![0.0; (taps + 1) * ch], kernels, out: vec![0.0; OUTPUT_CHUNK * ch] }))
    }

    fn delay(&self) -> u64 {
        (self.half as u64 * u64::from(self.to)).div_ceil(u64::from(self.from))
    }

    /// Emit interleaved chunks without allocating or resizing. Empty and arbitrary-sized input blocks are valid.
    fn run(&mut self, frames: usize, at: impl Fn(usize, usize) -> f32, mut emit: impl FnMut(&[f32])) {
        let taps = 2 * self.half;
        let history_frames = taps + 1;
        let mut filled = 0;
        for f in 0..frames {
            let base = self.seen as usize % history_frames * self.ch;
            for c in 0..self.ch {
                self.history[base + c] = at(c, f);
            }
            self.seen += 1;
            while self.next + (self.half as u64) < self.seen {
                let phase = self.fraction as f64 * PHASES as f64 / f64::from(self.to);
                let index = (phase as usize).min(PHASES - 1);
                let blend = (phase - index as f64) as f32;
                let a = &self.kernels[index * taps..(index + 1) * taps];
                let b = &self.kernels[(index + 1) * taps..(index + 2) * taps];
                let first = self.next as i64 - self.half as i64 + 1;
                self.out[filled..filled + self.ch].fill(0.0);
                for k in 0..taps {
                    let source = first + k as i64;
                    if source < 0 {
                        continue; // Silence before the stream began.
                    }
                    let weight = a[k] + (b[k] - a[k]) * blend;
                    let base = source as usize % history_frames * self.ch;
                    for c in 0..self.ch {
                        self.out[filled + c] += self.history[base + c] * weight;
                    }
                }
                filled += self.ch;
                if filled == self.out.len() {
                    emit(&self.out);
                    filled = 0;
                }
                self.fraction += u64::from(self.from);
                self.next += self.fraction / u64::from(self.to);
                self.fraction %= u64::from(self.to);
            }
        }
        if filled > 0 {
            emit(&self.out[..filled]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(from: u32, to: u32, frames: usize, blocks: &[usize], at: impl Fn(usize, usize) -> f32) -> Vec<f32> {
        let mut r = Resampler::new(from, to, 2).unwrap().unwrap();
        let storage = (r.history.as_ptr(), r.out.as_ptr(), r.kernels.as_ptr());
        let capacity = (r.history.capacity(), r.out.capacity(), r.kernels.capacity());
        let mut got = Vec::new();
        let mut start = 0;
        for &block in blocks.iter().cycle() {
            let n = block.min(frames - start);
            r.run(n, |c, f| at(c, start + f), |out| {
                assert!(out.len() <= 2 * OUTPUT_CHUNK);
                got.extend_from_slice(out);
            });
            start += n;
            if start == frames {
                break;
            }
        }
        assert_eq!((r.history.as_ptr(), r.out.as_ptr(), r.kernels.as_ptr()), storage);
        assert_eq!((r.history.capacity(), r.out.capacity(), r.kernels.capacity()), capacity);
        let expected = ((frames.saturating_sub(r.half) as u64 * u64::from(to)).div_ceil(u64::from(from))) as usize;
        assert_eq!(got.len() / 2, expected, "the filter's stated delay accounts for the unreturned tail");
        got
    }

    #[test]
    fn resampling_is_independent_of_callback_boundaries_and_bounded() {
        for (from, to) in [(44100, 48000), (96000, 48000), (48000, 44100)] {
            let signal = |c, f: usize| ((f as f64 * 0.013 + c as f64).sin() * 0.8) as f32;
            let whole = convert(from, to, 40000, &[40000], signal);
            let split = convert(from, to, 40000, &[1, 0, 17, 441, 100, 512, 300], signal);
            assert_eq!(whole, split, "{from} -> {to}");
        }
        assert!(Resampler::new(48000, 48000, 2).unwrap().is_none());
        assert!(Resampler::new(0, 48000, 2).is_err());
        assert!(Resampler::new(384001, 48000, 2).is_err());
    }

    #[test]
    fn downsampling_rejects_aliases_and_preserves_the_pass_band() {
        for (from, to) in [(96000, 48000), (192000, 44100), (48000, 44100)] {
            let frequency = f64::from(to) * 0.40; // 80% of destination Nyquist.
            let alias_frequency = (f64::from(to) * 0.625).min(f64::from(from) * 0.49);
            let got = convert(from, to, from as usize / 5, &[73, 511, 1, 2048], |c, f| {
                let hz = if c == 0 { frequency } else { alias_frequency };
                (2.0 * std::f64::consts::PI * hz * f as f64 / f64::from(from)).sin() as f32
            });
            let valid = &got[2048..]; // Exclude the finite zero-padded start transient.
            let rms = |channel| (valid.chunks_exact(2).map(|v| f64::from(v[channel]).powi(2)).sum::<f64>() / (valid.len() / 2) as f64).sqrt();
            let pass = rms(0);
            let rejected = rms(1);
            assert!((pass - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.004, "{from} -> {to}: pass-band RMS {pass}");
            assert!(rejected < 0.0007, "{from} -> {to}: alias RMS {rejected}, expected more than 60 dB rejection");
        }
    }

    #[test]
    fn a_constant_input_has_unity_gain_and_the_delay_is_reported() {
        let r = Resampler::new(44100, 48000, 2).unwrap().unwrap();
        assert_eq!(r.delay(), (r.half as u64 * 48000).div_ceil(44100));
        let got = convert(44100, 48000, 10000, &[7, 441, 1], |c, _| if c == 0 { 0.25 } else { -0.75 });
        for sample in got[1024..].chunks_exact(2) {
            assert!((sample[0] - 0.25).abs() < 2e-6);
            assert!((sample[1] + 0.75).abs() < 2e-6);
        }
    }
}
